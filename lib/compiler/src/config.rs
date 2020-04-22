//! The configuration for the
use crate::compiler::Compiler;
use crate::std::boxed::Box;
use enumset::{EnumSet, EnumSetType};
use target_lexicon::Triple;
pub use wasm_common::Features;

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use raw_cpuid::CpuId;

/// The nomenclature is inspired by the [raw-cpuid crate].
/// The list of supported features was initially retrieved from
/// [cranelift-native].
///
/// The `CpuFeature` enum vaues are likely to grow closer to the
/// original cpuid. However, we prefer to start small and grow from there.
///
/// If you would like to use a flag that doesn't exist yet here, please
/// open a PR.
///
/// [cpuid crate]: https://docs.rs/cpuid/0.1.1/cpuid/enum.CpuFeature.html
/// [cranelift-native]: https://github.com/bytecodealliance/cranelift/blob/6988545fd20249b084c53f4761b8c861266f5d31/cranelift-native/src/lib.rs#L51-L92
#[allow(missing_docs)]
#[derive(EnumSetType, Debug, Hash)]
pub enum CpuFeature {
    // X86 features
    SSE2,
    SSE3,
    SSSE3,
    SSE41,
    SSE42,
    POPCNT,
    AVX,
    BMI1,
    BMI2,
    AVX2,
    AVX512DQ,
    AVX512VL,
    LZCNT,
    // ARM features
    // Risc-V features
}

impl CpuFeature {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    /// Retrieves the features for the current Host
    pub fn for_host() -> EnumSet<Self> {
        let mut features = EnumSet::new();
        let cpuid = CpuId::new();

        if let Some(info) = cpuid.get_feature_info() {
            if info.has_sse2() {
                features.insert(CpuFeature::SSE2);
            }
            if info.has_sse3() {
                features.insert(CpuFeature::SSE3);
            }
            if info.has_ssse3() {
                features.insert(CpuFeature::SSSE3);
            }
            if info.has_sse41() {
                features.insert(CpuFeature::SSE41);
            }
            if info.has_sse42() {
                features.insert(CpuFeature::SSE42);
            }
            if info.has_popcnt() {
                features.insert(CpuFeature::POPCNT);
            }
            if info.has_avx() {
                features.insert(CpuFeature::AVX);
            }
        }
        if let Some(info) = cpuid.get_extended_feature_info() {
            if info.has_bmi1() {
                features.insert(CpuFeature::BMI1);
            }
            if info.has_bmi2() {
                features.insert(CpuFeature::BMI2);
            }
            if info.has_avx2() {
                features.insert(CpuFeature::AVX2);
            }
            if info.has_avx512dq() {
                features.insert(CpuFeature::AVX512DQ);
            }
            if info.has_avx512vl() {
                features.insert(CpuFeature::AVX512VL);
            }
        }
        if let Some(info) = cpuid.get_extended_function_info() {
            if info.has_lzcnt() {
                features.insert(CpuFeature::LZCNT);
            }
        }
        features
    }
    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    /// Retrieves the features for the current Host
    pub fn for_host() -> EnumSet<Self> {
        // We default to an empty hash set
        EnumSet::new();
    }
}

/// This is the target that we will use for compiling
/// the WebAssembly Module, and then run it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Target {
    triple: Triple,
    cpu_features: EnumSet<CpuFeature>,
}

impl Target {
    /// Creates a new target given a triple
    pub fn new(triple: Triple, cpu_features: EnumSet<CpuFeature>) -> Target {
        Target {
            triple,
            cpu_features,
        }
    }

    /// The triple associated for the target.
    pub fn triple(&self) -> &Triple {
        &self.triple
    }

    /// The triple associated for the target.
    pub fn cpu_features(&self) -> &EnumSet<CpuFeature> {
        &self.cpu_features
    }
}

/// The default for the Target will use the HOST as the triple
impl Default for Target {
    fn default() -> Target {
        Target {
            triple: Triple::host(),
            cpu_features: CpuFeature::for_host(),
        }
    }
}

/// The compiler configuration options.
///
/// This options must have WebAssembly `Features` and a specific
/// `Target` to compile to.
pub trait CompilerConfig: Clone {
    /// Gets the WebAssembly features
    fn features(&self) -> &Features;

    /// Gets the WebAssembly features, mutable
    fn features_mut(&mut self) -> &mut Features;

    /// Gets the target that we will use for compiling
    /// the WebAssembly module
    fn target(&self) -> &Target;

    /// Gets the target that we will use for compiling
    /// the WebAssembly module, mutable
    fn target_mut(&mut self) -> &mut Target;

    /// Gets the custom compiler config
    fn compiler(&self) -> Box<dyn Compiler>;
}
