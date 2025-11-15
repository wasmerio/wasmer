use crate::compiler::CraneliftCompiler;
use cranelift_codegen::{
    CodegenResult,
    isa::{TargetIsa, lookup},
    settings::{self, Configurable},
};
use std::{
    collections::HashMap,
    fs::File,
    io::{self, Write},
    sync::Arc,
};
use std::{num::NonZero, path::PathBuf};
use wasmer_compiler::{
    Compiler, CompilerConfig, Engine, EngineBuilder, ModuleMiddleware,
    misc::{CompiledKind, function_kind_to_filename, save_assembly_to_file},
};
use wasmer_types::target::{Architecture, CpuFeature, Target};

/// Callbacks to the different Cranelift compilation phases.
#[derive(Debug, Clone)]
pub struct CraneliftCallbacks {
    debug_dir: PathBuf,
}

impl CraneliftCallbacks {
    /// Creates a new instance of `CraneliftCallbacks` with the specified debug directory.
    pub fn new(debug_dir: PathBuf) -> Result<Self, io::Error> {
        // Create the debug dir in case it doesn't exist
        std::fs::create_dir_all(&debug_dir)?;
        Ok(Self { debug_dir })
    }

    /// Writes the pre-optimization intermediate representation to a debug file.
    pub fn preopt_ir(&self, kind: &CompiledKind, mem_buffer: &[u8]) {
        let mut path = self.debug_dir.clone();
        path.push(format!("{}.preopt.clif", function_kind_to_filename(kind)));
        let mut file =
            File::create(path).expect("Error while creating debug file from Cranelift IR");
        file.write_all(mem_buffer).unwrap();
    }

    /// Writes the object file memory buffer to a debug file.
    pub fn obj_memory_buffer(&self, kind: &CompiledKind, mem_buffer: &[u8]) {
        let mut path = self.debug_dir.clone();
        path.push(format!("{}.o", function_kind_to_filename(kind)));
        let mut file =
            File::create(path).expect("Error while creating debug file from Cranelift object");
        file.write_all(mem_buffer).unwrap();
    }

    /// Writes the assembly memory buffer to a debug file.
    pub fn asm_memory_buffer(
        &self,
        kind: &CompiledKind,
        arch: Architecture,
        mem_buffer: &[u8],
    ) -> Result<(), wasmer_types::CompileError> {
        let mut path = self.debug_dir.clone();
        path.push(format!("{}.s", function_kind_to_filename(kind)));
        save_assembly_to_file(arch, path, mem_buffer, HashMap::<usize, String>::new())
    }
}

// Runtime Environment

/// Possible optimization levels for the Cranelift codegen backend.
#[non_exhaustive]
#[derive(Clone, Debug)]
pub enum CraneliftOptLevel {
    /// No optimizations performed, minimizes compilation time by disabling most
    /// optimizations.
    None,
    /// Generates the fastest possible code, but may take longer.
    Speed,
    /// Similar to `speed`, but also performs transformations aimed at reducing
    /// code size.
    SpeedAndSize,
}

/// Global configuration options used to create an
/// `wasmer_engine::Engine` and customize its behavior.
///
/// This structure exposes a builder-like interface and is primarily
/// consumed by `wasmer_engine::Engine::new`.
#[derive(Debug, Clone)]
pub struct Cranelift {
    enable_nan_canonicalization: bool,
    enable_verifier: bool,
    pub(crate) enable_perfmap: bool,
    enable_pic: bool,
    opt_level: CraneliftOptLevel,
    /// The number of threads to use for compilation.
    pub num_threads: NonZero<usize>,
    /// The middleware chain.
    pub(crate) middlewares: Vec<Arc<dyn ModuleMiddleware>>,
    pub(crate) callbacks: Option<CraneliftCallbacks>,
}

impl Cranelift {
    /// Creates a new configuration object with the default configuration
    /// specified.
    pub fn new() -> Self {
        Self {
            enable_nan_canonicalization: false,
            enable_verifier: false,
            opt_level: CraneliftOptLevel::Speed,
            enable_pic: false,
            num_threads: std::thread::available_parallelism().unwrap_or(NonZero::new(1).unwrap()),
            middlewares: vec![],
            enable_perfmap: false,
            callbacks: None,
        }
    }

    /// Enable NaN canonicalization.
    ///
    /// NaN canonicalization is useful when trying to run WebAssembly
    /// deterministically across different architectures.
    pub fn canonicalize_nans(&mut self, enable: bool) -> &mut Self {
        self.enable_nan_canonicalization = enable;
        self
    }

    /// Set the number of threads to use for compilation.
    pub fn num_threads(&mut self, num_threads: NonZero<usize>) -> &mut Self {
        self.num_threads = num_threads;
        self
    }

    /// The optimization levels when optimizing the IR.
    pub fn opt_level(&mut self, opt_level: CraneliftOptLevel) -> &mut Self {
        self.opt_level = opt_level;
        self
    }

    /// Generates the ISA for the provided target
    pub fn isa(&self, target: &Target) -> CodegenResult<Arc<dyn TargetIsa>> {
        let mut builder =
            lookup(target.triple().clone()).expect("construct Cranelift ISA for triple");
        // Cpu Features
        let cpu_features = target.cpu_features();
        if target.triple().architecture == Architecture::X86_64
            && !cpu_features.contains(CpuFeature::SSE2)
        {
            panic!("x86 support requires SSE2");
        }
        if cpu_features.contains(CpuFeature::SSE3) {
            builder.enable("has_sse3").expect("should be valid flag");
        }
        if cpu_features.contains(CpuFeature::SSSE3) {
            builder.enable("has_ssse3").expect("should be valid flag");
        }
        if cpu_features.contains(CpuFeature::SSE41) {
            builder.enable("has_sse41").expect("should be valid flag");
        }
        if cpu_features.contains(CpuFeature::SSE42) {
            builder.enable("has_sse42").expect("should be valid flag");
        }
        if cpu_features.contains(CpuFeature::POPCNT) {
            builder.enable("has_popcnt").expect("should be valid flag");
        }
        if cpu_features.contains(CpuFeature::AVX) {
            builder.enable("has_avx").expect("should be valid flag");
        }
        if cpu_features.contains(CpuFeature::BMI1) {
            builder.enable("has_bmi1").expect("should be valid flag");
        }
        if cpu_features.contains(CpuFeature::BMI2) {
            builder.enable("has_bmi2").expect("should be valid flag");
        }
        if cpu_features.contains(CpuFeature::AVX2) {
            builder.enable("has_avx2").expect("should be valid flag");
        }
        if cpu_features.contains(CpuFeature::AVX512DQ) {
            builder
                .enable("has_avx512dq")
                .expect("should be valid flag");
        }
        if cpu_features.contains(CpuFeature::AVX512VL) {
            builder
                .enable("has_avx512vl")
                .expect("should be valid flag");
        }
        if cpu_features.contains(CpuFeature::LZCNT) {
            builder.enable("has_lzcnt").expect("should be valid flag");
        }

        builder.finish(self.flags())
    }

    /// Generates the flags for the compiler
    pub fn flags(&self) -> settings::Flags {
        let mut flags = settings::builder();

        // Enable probestack
        flags
            .enable("enable_probestack")
            .expect("should be valid flag");

        // Always use inline stack probes (otherwise the call to Probestack needs to be relocated).
        flags
            .set("probestack_strategy", "inline")
            .expect("should be valid flag");

        if self.enable_pic {
            flags.enable("is_pic").expect("should be a valid flag");
        }

        // We set up libcall trampolines in engine-universal.
        // These trampolines are always reachable through short jumps.
        flags
            .enable("use_colocated_libcalls")
            .expect("should be a valid flag");

        // Allow Cranelift to implicitly spill multi-value returns via a hidden
        // StructReturn argument when register results are exhausted.
        flags
            .enable("enable_multi_ret_implicit_sret")
            .expect("should be a valid flag");

        // Invert cranelift's default-on verification to instead default off.
        let enable_verifier = if self.enable_verifier {
            "true"
        } else {
            "false"
        };
        flags
            .set("enable_verifier", enable_verifier)
            .expect("should be valid flag");
        flags
            .set("enable_safepoints", "true")
            .expect("should be valid flag");

        flags
            .set(
                "opt_level",
                match self.opt_level {
                    CraneliftOptLevel::None => "none",
                    CraneliftOptLevel::Speed => "speed",
                    CraneliftOptLevel::SpeedAndSize => "speed_and_size",
                },
            )
            .expect("should be valid flag");

        let enable_nan_canonicalization = if self.enable_nan_canonicalization {
            "true"
        } else {
            "false"
        };
        flags
            .set("enable_nan_canonicalization", enable_nan_canonicalization)
            .expect("should be valid flag");

        settings::Flags::new(flags)
    }

    /// Callbacks that will triggered in the different compilation
    /// phases in Cranelift.
    pub fn callbacks(&mut self, callbacks: Option<CraneliftCallbacks>) -> &mut Self {
        self.callbacks = callbacks;
        self
    }
}

impl CompilerConfig for Cranelift {
    fn enable_pic(&mut self) {
        self.enable_pic = true;
    }

    fn enable_verifier(&mut self) {
        self.enable_verifier = true;
    }

    fn enable_perfmap(&mut self) {
        self.enable_perfmap = true;
    }

    fn canonicalize_nans(&mut self, enable: bool) {
        self.enable_nan_canonicalization = enable;
    }

    /// Transform it into the compiler
    fn compiler(self: Box<Self>) -> Box<dyn Compiler> {
        Box::new(CraneliftCompiler::new(*self))
    }

    /// Pushes a middleware onto the back of the middleware chain.
    fn push_middleware(&mut self, middleware: Arc<dyn ModuleMiddleware>) {
        self.middlewares.push(middleware);
    }
}

impl Default for Cranelift {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Cranelift> for Engine {
    fn from(config: Cranelift) -> Self {
        EngineBuilder::new(config).engine()
    }
}
