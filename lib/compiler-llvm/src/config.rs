use crate::compiler::LLVMCompiler;
pub use inkwell::OptimizationLevel as LLVMOptLevel;
use inkwell::targets::{
    CodeModel, InitializationConfig, RelocMode, Target as InkwellTarget, TargetMachine,
    TargetMachineOptions, TargetTriple,
};
use itertools::Itertools;
use std::fs::File;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::{fmt::Debug, num::NonZero};
use target_lexicon::BinaryFormat;
use wasmer_compiler::misc::{CompiledKind, function_kind_to_filename};
use wasmer_compiler::{Compiler, CompilerConfig, Engine, EngineBuilder, ModuleMiddleware};
use wasmer_types::{
    Features,
    target::{Architecture, OperatingSystem, Target, Triple},
};

/// The InkWell ModuleInfo type
pub type InkwellModule<'ctx> = inkwell::module::Module<'ctx>;

/// The InkWell MemoryBuffer type
pub type InkwellMemoryBuffer = inkwell::memory_buffer::MemoryBuffer;

/// Callbacks to the different LLVM compilation phases.
#[derive(Debug, Clone)]
pub struct LLVMCallbacks {
    debug_dir: PathBuf,
}

impl LLVMCallbacks {
    pub fn new(debug_dir: PathBuf) -> Result<Self, io::Error> {
        // Create the debug dir in case it doesn't exist
        std::fs::create_dir_all(&debug_dir)?;
        Ok(Self { debug_dir })
    }

    pub fn preopt_ir(&self, kind: &CompiledKind, module: &InkwellModule) {
        let mut path = self.debug_dir.clone();
        path.push(function_kind_to_filename(kind, ".preopt.ll"));
        module
            .print_to_file(&path)
            .expect("Error while dumping pre optimized LLVM IR");
    }
    pub fn postopt_ir(&self, kind: &CompiledKind, module: &InkwellModule) {
        let mut path = self.debug_dir.clone();
        path.push(function_kind_to_filename(kind, ".postopt.ll"));
        module
            .print_to_file(&path)
            .expect("Error while dumping post optimized LLVM IR");
    }
    pub fn obj_memory_buffer(&self, kind: &CompiledKind, memory_buffer: &InkwellMemoryBuffer) {
        let mut path = self.debug_dir.clone();
        path.push(function_kind_to_filename(kind, ".o"));
        let mem_buf_slice = memory_buffer.as_slice();
        let mut file =
            File::create(path).expect("Error while creating debug object file from LLVM IR");
        file.write_all(mem_buf_slice).unwrap();
    }

    pub fn asm_memory_buffer(&self, kind: &CompiledKind, asm_memory_buffer: &InkwellMemoryBuffer) {
        let mut path = self.debug_dir.clone();
        path.push(function_kind_to_filename(kind, ".s"));
        let mem_buf_slice = asm_memory_buffer.as_slice();
        let mut file =
            File::create(path).expect("Error while creating debug assembly file from LLVM IR");
        file.write_all(mem_buf_slice).unwrap();
    }
}

#[derive(Debug, Clone)]
pub struct LLVM {
    pub(crate) enable_nan_canonicalization: bool,
    pub(crate) enable_g0m0_opt: bool,
    pub(crate) enable_verifier: bool,
    pub(crate) enable_perfmap: bool,
    pub(crate) opt_level: LLVMOptLevel,
    is_pic: bool,
    pub(crate) callbacks: Option<LLVMCallbacks>,
    /// The middleware chain.
    pub(crate) middlewares: Vec<Arc<dyn ModuleMiddleware>>,
    /// Number of threads to use when compiling a module.
    pub(crate) num_threads: NonZero<usize>,
}

impl LLVM {
    /// Creates a new configuration object with the default configuration
    /// specified.
    pub fn new() -> Self {
        Self {
            enable_nan_canonicalization: false,
            enable_verifier: false,
            enable_perfmap: false,
            opt_level: LLVMOptLevel::Aggressive,
            is_pic: false,
            callbacks: None,
            middlewares: vec![],
            enable_g0m0_opt: false,
            num_threads: std::thread::available_parallelism().unwrap_or(NonZero::new(1).unwrap()),
        }
    }

    /// The optimization levels when optimizing the IR.
    pub fn opt_level(&mut self, opt_level: LLVMOptLevel) -> &mut Self {
        self.opt_level = opt_level;
        self
    }

    /// (warning: experimental) Pass the value of the first (#0) global and the base pointer of the
    /// first (#0) memory as parameter between guest functions.
    pub fn enable_pass_params_opt(&mut self) -> &mut Self {
        // internally, the "pass_params" opt is known as g0m0 opt.
        self.enable_g0m0_opt = true;
        self
    }

    pub fn num_threads(&mut self, num_threads: NonZero<usize>) -> &mut Self {
        self.num_threads = num_threads;
        self
    }

    /// Callbacks that will triggered in the different compilation
    /// phases in LLVM.
    pub fn callbacks(&mut self, callbacks: Option<LLVMCallbacks>) -> &mut Self {
        self.callbacks = callbacks;
        self
    }

    fn reloc_mode(&self, binary_format: BinaryFormat) -> RelocMode {
        if matches!(binary_format, BinaryFormat::Macho) {
            return RelocMode::Static;
        }

        if self.is_pic {
            RelocMode::PIC
        } else {
            RelocMode::Static
        }
    }

    fn code_model(&self, binary_format: BinaryFormat) -> CodeModel {
        // We normally use the large code model, but when targeting shared
        // objects, we are required to use PIC. If we use PIC anyways, we lose
        // any benefit from large code model and there's some cost on all
        // platforms, plus some platforms (MachO) don't support PIC + large
        // at all.
        if matches!(binary_format, BinaryFormat::Macho) {
            return CodeModel::Default;
        }

        if self.is_pic {
            CodeModel::Small
        } else {
            CodeModel::Large
        }
    }

    pub(crate) fn target_operating_system(&self, target: &Target) -> OperatingSystem {
        match target.triple().operating_system {
            OperatingSystem::Darwin(deployment) if !self.is_pic => {
                // LLVM detects static relocation + darwin + 64-bit and
                // force-enables PIC because MachO doesn't support that
                // combination. They don't check whether they're targeting
                // MachO, they check whether the OS is set to Darwin.
                //
                // Since both linux and darwin use SysV ABI, this should work.
                //  but not in the case of Aarch64, there the ABI is slightly different
                #[allow(clippy::match_single_binding)]
                match target.triple().architecture {
                    Architecture::Aarch64(_) => OperatingSystem::Darwin(deployment),
                    _ => OperatingSystem::Linux,
                }
            }
            other => other,
        }
    }

    pub(crate) fn target_binary_format(&self, target: &Target) -> target_lexicon::BinaryFormat {
        if self.is_pic {
            target.triple().binary_format
        } else {
            match self.target_operating_system(target) {
                OperatingSystem::Darwin(_) => target_lexicon::BinaryFormat::Macho,
                _ => target_lexicon::BinaryFormat::Elf,
            }
        }
    }

    fn target_triple(&self, target: &Target) -> TargetTriple {
        let architecture = if target.triple().architecture
            == Architecture::Riscv64(target_lexicon::Riscv64Architecture::Riscv64gc)
        {
            target_lexicon::Architecture::Riscv64(target_lexicon::Riscv64Architecture::Riscv64)
        } else {
            target.triple().architecture
        };
        // Hack: we're using is_pic to determine whether this is a native
        // build or not.

        let operating_system = self.target_operating_system(target);
        let binary_format = self.target_binary_format(target);

        let triple = Triple {
            architecture,
            vendor: target.triple().vendor.clone(),
            operating_system,
            environment: target.triple().environment,
            binary_format,
        };
        TargetTriple::create(&triple.to_string())
    }

    /// Generates the target machine for the current target
    pub fn target_machine(&self, target: &Target) -> TargetMachine {
        self.target_machine_with_opt(target, true)
    }

    pub(crate) fn target_machine_with_opt(
        &self,
        target: &Target,
        enable_optimization: bool,
    ) -> TargetMachine {
        let triple = target.triple();
        let cpu_features = &target.cpu_features();

        match triple.architecture {
            Architecture::X86_64 | Architecture::X86_32(_) => {
                InkwellTarget::initialize_x86(&InitializationConfig {
                    asm_parser: true,
                    asm_printer: true,
                    base: true,
                    disassembler: true,
                    info: true,
                    machine_code: true,
                })
            }
            Architecture::Aarch64(_) => InkwellTarget::initialize_aarch64(&InitializationConfig {
                asm_parser: true,
                asm_printer: true,
                base: true,
                disassembler: true,
                info: true,
                machine_code: true,
            }),
            Architecture::Riscv64(_) | Architecture::Riscv32(_) => {
                InkwellTarget::initialize_riscv(&InitializationConfig {
                    asm_parser: true,
                    asm_printer: true,
                    base: true,
                    disassembler: true,
                    info: true,
                    machine_code: true,
                })
            }
            Architecture::LoongArch64 => {
                InkwellTarget::initialize_loongarch(&InitializationConfig {
                    asm_parser: true,
                    asm_printer: true,
                    base: true,
                    disassembler: true,
                    info: true,
                    machine_code: true,
                })
            }
            _ => unimplemented!("target {} not yet supported in Wasmer", triple),
        }

        // The CPU features formatted as LLVM strings
        // We can safely map to gcc-like features as the CPUFeatures
        // are compliant with the same string representations as gcc.
        let llvm_cpu_features = cpu_features
            .iter()
            .map(|feature| format!("+{feature}"))
            .join(",");

        let target_triple = self.target_triple(target);
        let llvm_target = InkwellTarget::from_triple(&target_triple).unwrap();
        let mut llvm_target_machine_options = TargetMachineOptions::new()
            .set_cpu(match triple.architecture {
                Architecture::Riscv64(_) => "generic-rv64",
                Architecture::Riscv32(_) => "generic-rv32",
                Architecture::LoongArch64 => "generic-la64",
                _ => "generic",
            })
            .set_features(match triple.architecture {
                Architecture::Riscv64(_) => "+m,+a,+c,+d,+f",
                Architecture::Riscv32(_) => "+m,+a,+c,+d,+f",
                Architecture::LoongArch64 => "+f,+d",
                _ => &llvm_cpu_features,
            })
            .set_level(if enable_optimization {
                self.opt_level
            } else {
                LLVMOptLevel::None
            })
            .set_reloc_mode(self.reloc_mode(self.target_binary_format(target)))
            .set_code_model(match triple.architecture {
                Architecture::LoongArch64 | Architecture::Riscv64(_) | Architecture::Riscv32(_) => {
                    CodeModel::Medium
                }
                _ => self.code_model(self.target_binary_format(target)),
            });
        if let Architecture::Riscv64(_) = triple.architecture {
            llvm_target_machine_options = llvm_target_machine_options.set_abi("lp64d");
        }
        llvm_target
            .create_target_machine_from_options(&target_triple, llvm_target_machine_options)
            .unwrap()
    }
}

impl CompilerConfig for LLVM {
    /// Emit code suitable for dlopen.
    fn enable_pic(&mut self) {
        // TODO: although we can emit PIC, the object file parser does not yet
        // support all the relocations.
        self.is_pic = true;
    }

    fn enable_perfmap(&mut self) {
        self.enable_perfmap = true
    }

    /// Whether to verify compiler IR.
    fn enable_verifier(&mut self) {
        self.enable_verifier = true;
    }

    fn canonicalize_nans(&mut self, enable: bool) {
        self.enable_nan_canonicalization = enable;
    }

    /// Transform it into the compiler.
    fn compiler(self: Box<Self>) -> Box<dyn Compiler> {
        Box::new(LLVMCompiler::new(*self))
    }

    /// Pushes a middleware onto the back of the middleware chain.
    fn push_middleware(&mut self, middleware: Arc<dyn ModuleMiddleware>) {
        self.middlewares.push(middleware);
    }

    fn supported_features_for_target(&self, _target: &Target) -> wasmer_types::Features {
        let mut feats = Features::default();
        feats.exceptions(true);
        feats
    }
}

impl Default for LLVM {
    fn default() -> LLVM {
        Self::new()
    }
}

impl From<LLVM> for Engine {
    fn from(config: LLVM) -> Self {
        EngineBuilder::new(config).engine()
    }
}
