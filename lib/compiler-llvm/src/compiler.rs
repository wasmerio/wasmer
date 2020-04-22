//! Support for compiling with LLVM.
// Allow unused imports while developing.
#![allow(unused_imports, dead_code)]

use crate::config::LLVMConfig;
use crate::translator::FuncTranslator;
use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};
use wasm_common::entity::{EntityRef, PrimaryMap};
use wasm_common::Features;
use wasm_common::{FuncIndex, FuncType, LocalFuncIndex, MemoryIndex, TableIndex};
use wasmer_compiler::FunctionBodyData;
use wasmer_compiler::TrapInformation;
use wasmer_compiler::{Compilation, CompileError, CompiledFunction, Compiler};
use wasmer_compiler::{CompilerConfig, ModuleTranslationState, Target};
use wasmer_runtime::{MemoryPlan, Module, TablePlan, TrapCode};

use inkwell::targets::{InitializationConfig, Target as InkwellTarget};

/// A compiler that compiles a WebAssembly module with LLVM, translating the Wasm to LLVM IR,
/// optimizing it and then translating to assembly.
pub struct LLVMCompiler {
    config: LLVMConfig,
}

impl LLVMCompiler {
    /// Creates a new LLVM compiler
    pub fn new(config: &LLVMConfig) -> LLVMCompiler {
        LLVMCompiler {
            config: config.clone(),
        }
    }

    /// Gets the WebAssembly features for this Compiler
    fn config(&self) -> &LLVMConfig {
        &self.config
    }
}

impl Compiler for LLVMCompiler {
    /// Gets the WebAssembly features for this Compiler
    fn features(&self) -> &Features {
        self.config.features()
    }

    /// Gets the target associated to this Compiler.
    fn target(&self) -> &Target {
        self.config.target()
    }

    /// Compile the module using LLVM, producing a compilation result with
    /// associated relocations.
    fn compile_module<'data, 'module>(
        &self,
        module: &'module Module,
        _module_translation: &ModuleTranslationState,
        function_body_inputs: PrimaryMap<LocalFuncIndex, FunctionBodyData<'data>>,
        memory_plans: PrimaryMap<MemoryIndex, MemoryPlan>,
        table_plans: PrimaryMap<TableIndex, TablePlan>,
    ) -> Result<Compilation, CompileError> {
        let functions = function_body_inputs
            .into_iter()
            .collect::<Vec<(LocalFuncIndex, &FunctionBodyData<'_>)>>()
            .par_iter()
            .map_init(FuncTranslator::new, |func_translator, (i, input)| {
                func_translator.translate(
                    module,
                    i,
                    input,
                    self.config(),
                    &memory_plans,
                    &table_plans,
                )
            })
            .collect::<Result<Vec<_>, CompileError>>()?
            .into_iter()
            .collect::<PrimaryMap<LocalFuncIndex, _>>();

        Ok(Compilation::new(functions))
    }

    fn compile_wasm_trampolines(
        &self,
        _signatures: &[FuncType],
    ) -> Result<Vec<CompiledFunction>, CompileError> {
        // Note: do not implement this yet
        unimplemented!("Trampoline compilation not yet implemented.")
    }
}
