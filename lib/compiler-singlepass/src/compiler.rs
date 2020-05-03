//! Support for compiling with Singlepass.
// Allow unused imports while developing.
#![allow(unused_imports, dead_code)]

use crate::config::SinglepassConfig;
use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};
use wasm_common::entity::{EntityRef, PrimaryMap};
use wasm_common::Features;
use wasm_common::{FuncIndex, FuncType, LocalFuncIndex, MemoryIndex, TableIndex};
use wasmer_compiler::FunctionBodyData;
use wasmer_compiler::TrapInformation;
use wasmer_compiler::{Compilation, CompileError, CompiledFunction, Compiler};
use wasmer_compiler::{CompilerConfig, ModuleTranslationState, Target};
use wasmer_runtime::Module;
use wasmer_runtime::TrapCode;
use wasmer_runtime::{MemoryPlan, TablePlan};
use crate::codegen_x64::{FuncGen, CodegenError};
use wasmer_compiler::wasmparser::{BinaryReader, BinaryReaderError};

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

    /// Compile the module using Singlepass, producing a compilation result with
    /// associated relocations.
    fn compile_module(
        &self,
        module: &Module,
        module_translation: &ModuleTranslationState,
        function_body_inputs: PrimaryMap<LocalFuncIndex, FunctionBodyData<'_>>,
        memory_plans: PrimaryMap<MemoryIndex, MemoryPlan>,
        table_plans: PrimaryMap<TableIndex, TablePlan>,
    ) -> Result<Compilation, CompileError> {
        let functions = function_body_inputs
            .into_iter()
            .collect::<Vec<(LocalFuncIndex, &FunctionBodyData<'_>)>>()
            .par_iter()
            .map(|(i, input)| {
                let mut reader = BinaryReader::new_with_offset(input.data, input.module_offset);

                // This local list excludes arguments.
                let mut locals = vec![];
                let num_locals = reader.read_local_count().map_err(to_compile_error)?;
                for _ in 0..num_locals {
                    let mut counter = 0;
                    let (_count, ty) = reader
                        .read_local_decl(&mut counter)
                        .map_err(to_compile_error)?;
                    for _ in 0.._count {
                        locals.push(ty);
                    }
                }

                let mut generator = FuncGen::new(
                    module,
                    &self.config,
                    &memory_plans,
                    &table_plans,
                    *i,
                    &locals,
                ).map_err(to_compile_error)?;

                while generator.has_control_frames() {
                    let op = reader.read_operator().map_err(to_compile_error)?;
                    generator.feed_operator(op).map_err(to_compile_error)?;
                }

                Ok(unimplemented!())
            })
            .collect::<Result<Vec<CompiledFunction>, CompileError>>()?
            .into_iter()
            .collect::<PrimaryMap<LocalFuncIndex, CompiledFunction>>();

        Ok(Compilation::new(functions))
    }

    fn compile_wasm_trampolines(
        &self,
        _signatures: &[FuncType],
    ) -> Result<Vec<CompiledFunction>, CompileError> {
        // Note: do not implement this yet
        Err(CompileError::Codegen(
            "Singlepass trampoline compilation not supported yet".to_owned(),
        ))
    }
}

trait ToCompileError {
    fn to_compile_error(self) -> CompileError;
}

impl ToCompileError for BinaryReaderError {
    fn to_compile_error(self) -> CompileError {
        CompileError::Codegen(
            self.message().into()
        )
    }
}

impl ToCompileError for CodegenError {
    fn to_compile_error(self) -> CompileError {
        CompileError::Codegen(
            self.message
        )
    }
}

fn to_compile_error<T: ToCompileError>(x: T) -> CompileError {
    x.to_compile_error()
}
