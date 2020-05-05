//! Support for compiling with Singlepass.
// Allow unused imports while developing.
#![allow(unused_imports, dead_code)]

use crate::config::SinglepassConfig;
use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};
use wasm_common::entity::{EntityRef, PrimaryMap};
use wasm_common::Features;
use wasm_common::{FunctionIndex, FunctionType, LocalFunctionIndex, MemoryIndex, TableIndex};
use wasmer_compiler::FunctionBodyData;
use wasmer_compiler::TrapInformation;
use wasmer_compiler::{Compilation, CompileError, Compiler, FunctionBody};
use wasmer_compiler::{CompilerConfig, ModuleTranslationState, Target};
use wasmer_runtime::Module;
use wasmer_runtime::TrapCode;
use wasmer_runtime::{MemoryPlan, TablePlan};

/// A compiler that compiles a WebAssembly module with Singlepass.
/// It does the compilation in one pass
pub struct SinglepassCompiler {
    config: SinglepassConfig,
}

impl SinglepassCompiler {
    /// Creates a new Singlepass compiler
    pub fn new(config: &SinglepassConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }

    /// Gets the WebAssembly features for this Compiler
    fn config(&self) -> &SinglepassConfig {
        &self.config
    }
}

impl Compiler for SinglepassCompiler {
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
    fn compile_module(
        &self,
        _module: &Module,
        _module_translation: &ModuleTranslationState,
        _function_body_inputs: PrimaryMap<LocalFunctionIndex, FunctionBodyData<'_>>,
        _memory_plans: PrimaryMap<MemoryIndex, MemoryPlan>,
        _table_plans: PrimaryMap<TableIndex, TablePlan>,
    ) -> Result<Compilation, CompileError> {
        // Note to implementors: please use rayon paralell iterator to generate
        // the machine code in parallel.
        // Here's an example on how Cranelift is doing it:
        // https://github.com/wasmerio/wasmer-reborn/blob/master/lib/compiler-cranelift/src/compiler.rs#L202-L267
        Err(CompileError::Codegen(
            "Singlepass compilation not supported yet".to_owned(),
        ))
    }

    fn compile_wasm_trampolines(
        &self,
        _signatures: &[FunctionType],
    ) -> Result<Vec<FunctionBody>, CompileError> {
        // Note: do not implement this yet
        Err(CompileError::Codegen(
            "Singlepass trampoline compilation not supported yet".to_owned(),
        ))
    }
}
