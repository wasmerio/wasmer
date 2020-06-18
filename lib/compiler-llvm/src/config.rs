use crate::compiler::LLVMCompiler;
use inkwell::targets::{
    CodeModel, InitializationConfig, RelocMode, Target as InkwellTarget, TargetMachine,
    TargetTriple,
};
use inkwell::OptimizationLevel;
use itertools::Itertools;
use std::sync::Arc;
use target_lexicon::Architecture;
use wasm_common::{FunctionType, LocalFunctionIndex};
use wasmer_compiler::{Compiler, CompilerConfig, FunctionMiddlewareGenerator, Target, Triple};

/// The InkWell ModuleInfo type
pub type InkwellModule<'ctx> = inkwell::module::Module<'ctx>;

/// The InkWell MemoryBuffer type
pub type InkwellMemoryBuffer = inkwell::memory_buffer::MemoryBuffer;

/// The compiled function kind, used for debugging in the `LLVMCallbacks`.
#[derive(Debug, Clone)]
pub enum CompiledFunctionKind {
    // A locally-defined function in the Wasm file
    Local(LocalFunctionIndex),
    // A function call trampoline for a given signature
    FunctionCallTrampoline(FunctionType),
    // A dynamic function trampoline for a given signature
    DynamicFunctionTrampoline(FunctionType),
}

/// Callbacks to the different LLVM compilation phases.
pub trait LLVMCallbacks: Send + Sync {
    fn preopt_ir(&self, function: &CompiledFunctionKind, module: &InkwellModule);
    fn postopt_ir(&self, function: &CompiledFunctionKind, module: &InkwellModule);
    fn obj_memory_buffer(
        &self,
        function: &CompiledFunctionKind,
        memory_buffer: &InkwellMemoryBuffer,
    );
}

#[derive(Clone)]
pub struct LLVMConfig {
    pub(crate) enable_nan_canonicalization: bool,
    pub(crate) enable_verifier: bool,
    pub(crate) opt_level: OptimizationLevel,
    is_pic: bool,
    pub(crate) callbacks: Option<Arc<dyn LLVMCallbacks>>,
    /// The middleware chain.
    pub(crate) middlewares: Vec<Arc<dyn FunctionMiddlewareGenerator>>,
}

impl LLVMConfig {
    /// Creates a new configuration object with the default configuration
    /// specified.
    pub fn new() -> Self {
        Self {
            enable_nan_canonicalization: false,
            enable_verifier: false,
            opt_level: OptimizationLevel::Aggressive,
            is_pic: false,
            callbacks: None,
            middlewares: vec![],
        }
    }

    /// Should the LLVM verifier be enabled.
    ///
    /// The verifier assures that the generated LLVM IR is valid.
    pub fn verify_ir(&mut self, enable: bool) -> &mut Self {
        self.enable_verifier = enable;
        self
    }

    /// Enable NaN canonicalization.
    ///
    /// NaN canonicalization is useful when trying to run WebAssembly
    /// deterministically across different architectures.
    pub fn canonicalize_nans(&mut self, enable: bool) -> &mut Self {
        self.enable_nan_canonicalization = enable;
        self
    }

    /// The optimization levels when optimizing the IR.
    pub fn opt_level(&mut self, opt_level: OptimizationLevel) -> &mut Self {
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
        CodeModel::Large
    }

    fn target_triple(&self, target: &Target) -> TargetTriple {
        let operating_system =
            if target.triple().operating_system == wasmer_compiler::OperatingSystem::Darwin {
                // LLVM detects static relocation + darwin + 64-bit and
                // force-enables PIC because MachO doesn't support that
                // combination. They don't check whether they're targeting
                // MachO, they check whether the OS is set to Darwin.
                //
                // Since both linux and darwin use SysV ABI, this should work.
                wasmer_compiler::OperatingSystem::Linux
            } else {
                target.triple().operating_system
            };
        let triple = Triple {
            architecture: target.triple().architecture,
            vendor: target.triple().vendor.clone(),
            operating_system,
            environment: target.triple().environment,
            binary_format: target_lexicon::BinaryFormat::Elf,
        };
        TargetTriple::create(&triple.to_string())
    }

    /// Generates the target machine for the current target
    pub fn target_machine(&self, target: &Target) -> TargetMachine {
        let triple = target.triple();
        let cpu_features = &target.cpu_features();

        match triple.architecture {
            Architecture::X86_64 => InkwellTarget::initialize_x86(&InitializationConfig {
                asm_parser: true,
                asm_printer: true,
                base: true,
                disassembler: true,
                info: true,
                machine_code: true,
            }),
            Architecture::Arm(_) => InkwellTarget::initialize_aarch64(&InitializationConfig {
                asm_parser: true,
                asm_printer: true,
                base: true,
                disassembler: true,
                info: true,
                machine_code: true,
            }),
            _ => unimplemented!("target {} not supported", triple),
        }

        // The CPU features formatted as LLVM strings
        // We can safely map to gcc-like features as the CPUFeatures
        // are compliant with the same string representations as gcc.
        let llvm_cpu_features = cpu_features
            .iter()
            .map(|feature| format!("+{}", feature.to_string()))
            .join(",");

        let target_triple = self.target_triple(&target);
        let llvm_target = InkwellTarget::from_triple(&target_triple).unwrap();
        llvm_target
            .create_target_machine(
                &target_triple,
                "generic",
                &llvm_cpu_features,
                self.opt_level,
                self.reloc_mode(),
                self.code_model(),
            )
            .unwrap()
    }
}

impl CompilerConfig for LLVMConfig {
    /// Emit code suitable for dlopen.
    fn enable_pic(&mut self) {
        // TODO: although we can emit PIC, the object file parser does not yet
        // support all the relocations.
        self.is_pic = true;
    }

    /// Transform it into the compiler.
    fn compiler(&self) -> Box<dyn Compiler + Send> {
        Box::new(LLVMCompiler::new(&self))
    }

    /// Pushes a middleware onto the back of the middleware chain.
    fn push_middleware(&mut self, middleware: Arc<dyn FunctionMiddlewareGenerator>) {
        self.middlewares.push(middleware);
    }
}

impl Default for LLVMConfig {
    fn default() -> LLVMConfig {
        Self::new()
    }
}
