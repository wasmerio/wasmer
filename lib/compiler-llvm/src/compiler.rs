use crate::config::LLVM;
use crate::config::OptimizationStyle;
use crate::translator::FuncTrampoline;
use crate::translator::FuncTranslator;
use rayon::ThreadPoolBuilder;
use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};
use std::sync::Arc;
use tempfile::NamedTempFile;
use tempfile::tempdir;
use wasmer_compiler::progress::ProgressContext;
use wasmer_compiler::types::module::CompileModuleInfo;
use wasmer_compiler::{
    CompiledObjects, WASM_LARGE_FUNCTION_THRESHOLD, WASM_TRAMPOLINE_ESTIMATED_BODY_SIZE,
    build_function_buckets, emit_metadata_and_link, translate_function_buckets,
};
use wasmer_compiler::{Compiler, FunctionBodyData, ModuleMiddleware, ModuleTranslationState};
use wasmer_types::ExportIndex;
use wasmer_types::entity::PrimaryMap;
use wasmer_types::target::Target;
use wasmer_types::{CompilationProgressCallback, CompileError, LocalFunctionIndex};

/// A compiler that compiles a WebAssembly module with LLVM, translating the Wasm to LLVM IR,
/// optimizing it and then translating to assembly.
#[derive(Debug)]
pub struct LLVMCompiler {
    config: LLVM,
}

impl LLVMCompiler {
    /// Creates a new LLVM compiler
    pub fn new(config: LLVM) -> LLVMCompiler {
        LLVMCompiler { config }
    }

    /// Gets the config for this Compiler
    fn config(&self) -> &LLVM {
        &self.config
    }
}

impl Compiler for LLVMCompiler {
    fn name(&self) -> &str {
        "llvm"
    }

    fn get_perfmap_enabled(&self) -> bool {
        self.config.enable_perfmap
    }

    fn deterministic_id(&self) -> String {
        format!(
            "llvm-{}",
            match self.config.opt_level {
                inkwell::OptimizationLevel::None => "opt0",
                inkwell::OptimizationLevel::Less => "optl",
                inkwell::OptimizationLevel::Default => "optd",
                inkwell::OptimizationLevel::Aggressive => "opta",
            }
        )
    }

    /// Get the middlewares for this compiler
    fn get_middlewares(&self) -> &[Arc<dyn ModuleMiddleware>] {
        &self.config.middlewares
    }

    fn enable_readonly_funcref_table(&self) -> bool {
        self.config.enable_readonly_funcref_table
    }

    /// Compile the module using LLVM, producing a compilation result with
    /// associated relocations.
    fn compile_module(
        &self,
        target: &Target,
        compile_info: &CompileModuleInfo,
        compile_info_blob: Vec<u8>,
        module_translation: &ModuleTranslationState,
        function_body_inputs: PrimaryMap<LocalFunctionIndex, FunctionBodyData<'_>>,
        progress_callback: Option<&CompilationProgressCallback>,
    ) -> Result<NamedTempFile, CompileError> {
        let module_file = tempfile::Builder::new()
            .prefix("wasmer-image")
            .suffix(".so")
            .tempfile()
            .map_err(|err| CompileError::Codegen(format!("cannot create temporary file: {err}")))?;
        tracing::trace!(path = ?module_file.path(), "compiling to module file");

        let module = &compile_info.module;
        let module_hash = module.hash_string();

        let total_function_call_trampolines = module.signatures.len();
        let total_dynamic_trampolines = module.num_imported_functions;
        let total_steps = WASM_TRAMPOLINE_ESTIMATED_BODY_SIZE
            * ((total_dynamic_trampolines + total_function_call_trampolines) as u64)
            + function_body_inputs
                .iter()
                .map(|(_, body)| body.data.len() as u64)
                .sum::<u64>();

        let progress = progress_callback
            .cloned()
            .map(|cb| ProgressContext::new(cb, total_steps, "Compiling functions"));

        let module = &compile_info.module;
        let memory_styles = &compile_info.memory_styles;
        let table_styles = &compile_info.table_styles;
        let signature_hashes = &module.signature_hashes;

        let build_directory = tempdir().map_err(|err| {
            CompileError::Codegen(format!("cannot create temporary build folder: {err}"))
        })?;

        let pool = ThreadPoolBuilder::new()
            .num_threads(self.config.num_threads.get())
            .build()
            .map_err(|e| CompileError::Resource(e.to_string()))?;

        let buckets =
            build_function_buckets(&function_body_inputs, WASM_LARGE_FUNCTION_THRESHOLD / 3);
        let largest_bucket = buckets.first().map(|b| b.size).unwrap_or_default();
        tracing::debug!(buckets = buckets.len(), largest_bucket, "buckets built");
        let object_files = translate_function_buckets(
            &pool,
            || {
                let compiler = &self;
                let target_machines = enum_iterator::all::<OptimizationStyle>()
                    .map(|style| {
                        (
                            style,
                            compiler.config().target_machine_with_opt(target, style),
                        )
                    })
                    .collect();
                let pointer_width = target.triple().pointer_width().unwrap().bytes();
                FuncTranslator::new(
                    target.triple().clone(),
                    target_machines,
                    pointer_width,
                    *target.cpu_features(),
                    self.config.enable_non_volatile_memops,
                    module
                        .exports
                        .get("__wasm_apply_data_relocs")
                        .and_then(|export| {
                            if let ExportIndex::Function(index) = export {
                                Some(*index)
                            } else {
                                None
                            }
                        }),
                )
                .unwrap()
            },
            |func_translator, i, input| {
                func_translator.translate(
                    module,
                    module_translation,
                    signature_hashes,
                    i,
                    input,
                    self.config(),
                    memory_styles,
                    table_styles,
                    target.triple(),
                    build_directory.path(),
                )
            },
            progress.clone(),
            &buckets,
        )?;

        let progress = progress.clone();
        let trampolines_objects = pool.install(|| {
            module
                .signatures
                .iter()
                .collect::<Vec<_>>()
                .par_iter()
                .map_init(
                    || {
                        let target_machine = self.config().target_machine(target);
                        FuncTrampoline::new(target_machine, target.triple().clone()).unwrap()
                    },
                    |func_trampoline, (index, sig)| {
                        let function_name = format!("t{}", index.as_u32());
                        let trampoline = func_trampoline.trampoline(
                            sig,
                            self.config(),
                            &function_name,
                            compile_info,
                            build_directory.path(),
                        );
                        if let Some(progress) = progress.as_ref() {
                            progress.notify_steps(WASM_TRAMPOLINE_ESTIMATED_BODY_SIZE)?;
                        }
                        trampoline
                    },
                )
                .collect::<Result<Vec<_>, CompileError>>()
        })?;

        // TODO: I removed the parallel processing of dynamic trampolines because we're passing
        // the sections bytes and relocations directly into the trampoline generation function.
        // We can move that logic out and re-enable parallel processing. Hopefully, there aren't
        // enough dynamic trampolines to actually cause a noticeable performance degradation.
        let dynamic_trampolines_objects = {
            let progress = progress.clone();
            let target_machine = self.config().target_machine(target);
            let func_trampoline =
                FuncTrampoline::new(target_machine, target.triple().clone()).unwrap();
            module
                .imported_function_types()
                .collect::<Vec<_>>()
                .into_iter()
                .enumerate()
                .map(|(index, func_type)| {
                    let function_name = format!("dt{}", index);
                    let trampoline = func_trampoline.dynamic_trampoline(
                        &func_type,
                        self.config(),
                        &function_name,
                        index as u32,
                        &module_hash,
                        build_directory.path(),
                    )?;
                    if let Some(progress) = progress.as_ref() {
                        progress.notify_steps(WASM_TRAMPOLINE_ESTIMATED_BODY_SIZE)?;
                    }
                    Ok(trampoline)
                })
                .collect::<Result<Vec<_>, CompileError>>()
        }?;

        let result = emit_metadata_and_link(
            target,
            compile_info_blob,
            build_directory.path(),
            module_file,
            &CompiledObjects {
                object_files: &object_files,
                trampoline_object_files: &trampolines_objects,
                dynamic_trampoline_object_files: &dynamic_trampolines_objects,
            },
            self.config
                .callbacks
                .as_ref()
                .map(|callbacks| callbacks.debug_dir.clone()),
            module.hash().map(|hash| hash.to_string()),
        )?;

        Ok(result)
    }

    fn with_opts(
        &mut self,
        _suggested_compiler_opts: &wasmer_types::target::UserCompilerOptimizations,
    ) -> Result<(), CompileError> {
        Ok(())
    }
}
