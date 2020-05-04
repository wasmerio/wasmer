//! Support for compiling with Singlepass.
// Allow unused imports while developing.
#![allow(unused_imports, dead_code)]

use crate::codegen_x64::{gen_import_call_trampoline, gen_std_trampoline, CodegenError, FuncGen};
use crate::config::SinglepassConfig;
use rayon::prelude::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};
use wasm_common::entity::{EntityRef, PrimaryMap};
use wasm_common::Features;
use wasm_common::{FunctionIndex, FunctionType, LocalFunctionIndex, MemoryIndex, TableIndex};
use wasmer_compiler::wasmparser::{BinaryReader, BinaryReaderError};
use wasmer_compiler::TrapInformation;
use wasmer_compiler::{Compilation, CompileError, CompiledFunction, Compiler, SectionIndex};
use wasmer_compiler::{CompilerConfig, ModuleTranslationState, Target};
use wasmer_compiler::{FunctionBody, FunctionBodyData};
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

    /// Compile the module using Singlepass, producing a compilation result with
    /// associated relocations.
    fn compile_module(
        &self,
        module: &Module,
        module_translation: &ModuleTranslationState,
        function_body_inputs: PrimaryMap<LocalFunctionIndex, FunctionBodyData<'_>>,
        memory_plans: PrimaryMap<MemoryIndex, MemoryPlan>,
        table_plans: PrimaryMap<TableIndex, TablePlan>,
    ) -> Result<Compilation, CompileError> {
        let import_trampolines: PrimaryMap<SectionIndex, _> = (0..module.num_imported_funcs)
            .map(FunctionIndex::new)
            .collect::<Vec<_>>()
            .into_par_iter()
            .map(|i| gen_import_call_trampoline(i, module.signatures[module.functions[i]].clone()))
            .collect::<Vec<_>>()
            .into_iter()
            .collect();
        let functions = function_body_inputs
            .into_iter()
            .collect::<Vec<(LocalFunctionIndex, &FunctionBodyData<'_>)>>()
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
                )
                .map_err(to_compile_error)?;

                while generator.has_control_frames() {
                    let op = reader.read_operator().map_err(to_compile_error)?;
                    generator.feed_operator(op).map_err(to_compile_error)?;
                }

                Ok(generator.finalize())
            })
            .collect::<Result<Vec<CompiledFunction>, CompileError>>()?
            .into_iter()
            .collect::<PrimaryMap<LocalFunctionIndex, CompiledFunction>>();

        Ok(Compilation::new(functions, import_trampolines))
    }

    fn compile_wasm_trampolines(
        &self,
        signatures: &[FunctionType],
    ) -> Result<Vec<FunctionBody>, CompileError> {
        Ok(signatures
            .par_iter()
            .cloned()
            .map(gen_std_trampoline)
            .collect())
    }
}

trait ToCompileError {
    fn to_compile_error(self) -> CompileError;
}

impl ToCompileError for BinaryReaderError {
    fn to_compile_error(self) -> CompileError {
        CompileError::Codegen(self.message().into())
    }
}

impl ToCompileError for CodegenError {
    fn to_compile_error(self) -> CompileError {
        CompileError::Codegen(self.message)
    }
}

fn to_compile_error<T: ToCompileError>(x: T) -> CompileError {
    x.to_compile_error()
}
