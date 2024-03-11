use crate::compiler::LLVMCompiler;
use inkwell::targets::{
    CodeModel, InitializationConfig, RelocMode, Target as InkwellTarget, TargetMachine,
    TargetTriple,
};
pub use inkwell::OptimizationLevel as LLVMOptLevel;
use itertools::Itertools;
use std::fmt::Debug;
use std::sync::Arc;
use target_lexicon::Architecture;
use wasmer_compiler::{Compiler, CompilerConfig, Engine, EngineBuilder, ModuleMiddleware};
use wasmer_types::{FunctionType, LocalFunctionIndex, Target, Triple};

/// The InkWell ModuleInfo type
pub type InkwellModule<'ctx> = inkwell::module::Module<'ctx>;

/// The InkWell MemoryBuffer type
pub type InkwellMemoryBuffer = inkwell::memory_buffer::MemoryBuffer;

/// The compiled function kind, used for debugging in the `LLVMCallbacks`.
#[derive(Debug, Clone)]
pub enum CompiledKind {
    // A locally-defined function in the Wasm file.
    Local(LocalFunctionIndex),
    // A function call trampoline for a given signature.
    FunctionCallTrampoline(FunctionType),
    // A dynamic function trampoline for a given signature.
    DynamicFunctionTrampoline(FunctionType),
    // An entire Wasm module.
    Module,
}

/// Callbacks to the different LLVM compilation phases.
pub trait LLVMCallbacks: Debug + Send + Sync {
    fn preopt_ir(&self, function: &CompiledKind, module: &InkwellModule);
    fn postopt_ir(&self, function: &CompiledKind, module: &InkwellModule);
    fn obj_memory_buffer(&self, function: &CompiledKind, memory_buffer: &InkwellMemoryBuffer);
}

#[derive(Debug, Clone)]
pub struct LLVM {
    pub(crate) enable_nan_canonicalization: bool,
    pub(crate) enable_verifier: bool,
    pub(crate) opt_level: LLVMOptLevel,
    is_pic: bool,
    pub(crate) callbacks: Option<Arc<dyn LLVMCallbacks>>,
    /// The middleware chain.
    pub(crate) middlewares: Vec<Arc<dyn ModuleMiddleware>>,
}

impl LLVM {
    /// Creates a new configuration object with the default configuration
    /// specified.
    pub fn new() -> Self {
        Self {
            enable_nan_canonicalization: false,
            enable_verifier: false,
            opt_level: LLVMOptLevel::Aggressive,
            is_pic: false,
            callbacks: None,
            middlewares: vec![],
        }
    }

    /// The optimization levels when optimizing the IR.
    pub fn opt_level(&mut self, opt_level: LLVMOptLevel) -> &mut Self {
        self.opt_level = opt_level;
        self
    }

    /// Callbacks that will triggered in the different compilation
    /// phases in LLVM.
    pub fn callbacks(&mut self, callbacks: Option<Arc<dyn LLVMCallbacks>>) -> &mut Self {
        self.callbacks = callbacks;
        self
    }

    fn reloc_mode(&self) -> RelocMode {
        if self.is_pic {
            RelocMode::PIC
        } else {
            RelocMode::Static
        }
    }

    fn code_model(&self) -> CodeModel {
        // We normally use the large code model, but when targeting shared
        // objects, we are required to use PIC. If we use PIC anyways, we lose
        // any benefit from large code model and there's some cost on all
        // platforms, plus some platforms (MachO) don't support PIC + large
        // at all.
        if self.is_pic {
            CodeModel::Small
        } else {
            CodeModel::Large
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
        let operating_system = if target.triple().operating_system
            == wasmer_types::OperatingSystem::Darwin
            && !self.is_pic
        {
            // LLVM detects static relocation + darwin + 64-bit and
            // force-enables PIC because MachO doesn't support that
            // combination. They don't check whether they're targeting
            // MachO, they check whether the OS is set to Darwin.
            //
            // Since both linux and darwin use SysV ABI, this should work.
            //  but not in the case of Aarch64, there the ABI is slightly different
            #[allow(clippy::match_single_binding)]
            match target.triple().architecture {
                _ => wasmer_types::OperatingSystem::Linux,
            }
        } else {
            target.triple().operating_system
        };
        let binary_format = if self.is_pic {
            target.triple().binary_format
        } else {
            target_lexicon::BinaryFormat::Elf
        };
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
            Architecture::Riscv64(_) => InkwellTarget::initialize_riscv(&InitializationConfig {
                asm_parser: true,
                asm_printer: true,
                base: true,
                disassembler: true,
                info: true,
                machine_code: true,
            }),
            // Architecture::Arm(_) => InkwellTarget::initialize_arm(&InitializationConfig {
            //     asm_parser: true,
            //     asm_printer: true,
            //     base: true,
            //     disassembler: true,
            //     info: true,
            //     machine_code: true,
            // }),
            _ => unimplemented!("target {} not yet supported in Wasmer", triple),
        }

        // The CPU features formatted as LLVM strings
        // We can safely map to gcc-like features as the CPUFeatures
        // are compliant with the same string representations as gcc.
        let llvm_cpu_features = cpu_features
            .iter()
            .map(|feature| format!("+{}", feature.to_string()))
            .join(",");

        let target_triple = self.target_triple(target);
        let llvm_target = InkwellTarget::from_triple(&target_triple).unwrap();
        let llvm_target_machine = llvm_target
            .create_target_machine(
                &target_triple,
                match triple.architecture {
                    Architecture::Riscv64(_) => "generic-rv64",
                    _ => "generic",
                },
                match triple.architecture {
                    Architecture::Riscv64(_) => "+m,+a,+c,+d,+f",
                    _ => &llvm_cpu_features,
                },
                self.opt_level,
                self.reloc_mode(),
                match triple.architecture {
                    Architecture::Riscv64(_) => CodeModel::Medium,
                    _ => self.code_model(),
                },
            )
            .unwrap();

        if let Architecture::Riscv64(_) = triple.architecture {
            // TODO: totally non-portable way to change ABI
            unsafe {
                // This structure mimic the internal structure from inkwell
                // that is defined as
                //  #[derive(Debug)]
                //  pub struct TargetMachine {
                //    pub(crate) target_machine: LLVMTargetMachineRef,
                //  }
                pub struct MyTargetMachine {
                    pub target_machine: *const u8,
                }
                // It is use to live patch the create LLVMTargetMachine
                // to hard change the ABI and force "-mabi=lp64d" ABI
                // instead of the default that don't use float registers
                // because there is no current way to do this change

                let my_target_machine: MyTargetMachine = std::mem::transmute(llvm_target_machine);

                *((my_target_machine.target_machine as *mut u8).offset(0x410) as *mut u64) = 5;
                std::ptr::copy_nonoverlapping(
                    "lp64d\0".as_ptr(),
                    (my_target_machine.target_machine as *mut u8).offset(0x418),
                    6,
                );

                std::mem::transmute(my_target_machine)
            }
        } else {
            llvm_target_machine
        }
    }
}

impl CompilerConfig for LLVM {
    /// Emit code suitable for dlopen.
    fn enable_pic(&mut self) {
        // TODO: although we can emit PIC, the object file parser does not yet
        // support all the relocations.
        self.is_pic = true;
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
