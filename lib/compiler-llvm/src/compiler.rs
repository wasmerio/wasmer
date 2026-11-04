use crate::config::LLVM;
use crate::config::OptimizationStyle;
use crate::translator::FuncTrampoline;
use crate::translator::FuncTranslator;
use crate::translator::enable_m0_optimization;
use itertools::Itertools;
use rayon::ThreadPoolBuilder;
use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};
use std::sync::Arc;
use wasmer_compiler::elf::CompileOutput;
use wasmer_compiler::progress::ProgressContext;
use wasmer_compiler::types::{function::Compilation, module::CompileModuleInfo};
use wasmer_compiler::{
    CompiledObjects, Compiler, FunctionBodyData, ModuleMiddleware, ModuleTranslationState,
    WasmSourceMap, emit_metadata_and_link,
};
use wasmer_compiler::{
    WASM_LARGE_FUNCTION_THRESHOLD, WASM_TRAMPOLINE_ESTIMATED_BODY_SIZE, build_function_buckets,
    translate_function_buckets,
};
use wasmer_types::ExportIndex;
use wasmer_types::MemoryIndex;
use wasmer_types::entity::PrimaryMap;
use wasmer_types::target::Target;
use wasmer_types::{
    CompilationProgressCallback, CompileError, FunctionIndex, LocalFunctionIndex,
};
use wasmer_vm::MemoryStyle;

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

    fn get_debugger(&self) -> Option<wasmer_compiler::Debugger> {
        self.config.debugger
    }

    fn deterministic_id(&self) -> String {
        use wasmer_compiler::DeterministicIdComponent as Component;

        let mut components = vec![Component::Llvm];
        components.push(match self.config.opt_level {
            inkwell::OptimizationLevel::None => Component::OptNone,
            inkwell::OptimizationLevel::Less => Component::OptLess,
            inkwell::OptimizationLevel::Default => Component::OptDefault,
            inkwell::OptimizationLevel::Aggressive => Component::OptAggressive,
        });
        if self.config.enable_nan_canonicalization {
            components.push(Component::NanCanonicalization);
        }
        if self.config.enable_non_volatile_memops {
            components.push(Component::NonVolatileMemops);
        }
        if self.config.is_pic {
            components.push(Component::Pic);
        }
        if self.config.enable_readonly_funcref_table {
            components.push(Component::ReadonlyFuncrefTable);
        }
        if self.config.enable_m0 {
            components.push(Component::EnableM0);
        }

        components
            .into_iter()
            .map(|component| component.to_string())
            .collect_vec()
            .join("-")
    }

    /// Get the middlewares for this compiler
    fn get_middlewares(&self) -> &[Arc<dyn ModuleMiddleware>] {
        &self.config.middlewares
    }

    fn m0_pass_param_enabled(&self, memory_styles: &PrimaryMap<MemoryIndex, MemoryStyle>) -> bool {
        enable_m0_optimization(&self.config, memory_styles)
    }

    fn enable_readonly_funcref_table(&self) -> bool {
        self.config.enable_readonly_funcref_table
    }

    /// Compile the module using LLVM, producing a compilation result with
    /// associated relocations.
    fn compile_module(
        &self,
        target: &Target,
        compile_info: &mut CompileModuleInfo,
        module_translation: &ModuleTranslationState,
        function_body_inputs: PrimaryMap<LocalFunctionIndex, FunctionBodyData<'_>>,
        progress_callback: Option<&CompilationProgressCallback>,
    ) -> Result<Compilation, CompileError> {
        wasmer_compiler::validate_module_fixed_table_size(
            &compile_info.module,
            self.config.max_table_elements,
        )?;
        let function_max_stack_usage = function_body_inputs.iter().map(|_| None).collect();
        let binary_format = self.config.target_binary_format(target);

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

        let pool = ThreadPoolBuilder::new()
            .num_threads(self.config.num_threads.get())
            .build()
            .map_err(|e| CompileError::Resource(e.to_string()))?;

        let source_map = Arc::new(
            WasmSourceMap::new(module, module_translation, &function_body_inputs)
                .map_err(CompileError::Codegen)?,
        );
        let buckets =
            build_function_buckets(&function_body_inputs, WASM_LARGE_FUNCTION_THRESHOLD / 3);
        let largest_bucket = buckets.first().map(|b| b.size).unwrap_or_default();
        tracing::debug!(buckets = buckets.len(), largest_bucket, "buckets built");

        let functions = translate_function_buckets(
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
                    binary_format,
                    pointer_width,
                    *target.cpu_features(),
                    self.config.enable_non_volatile_memops,
                    source_map.clone(),
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
                )
            },
            progress.clone(),
            &buckets,
        )?;

        let progress = progress.clone();
        let function_call_trampolines = pool.install(|| {
            module
                .signatures
                .iter()
                .collect::<Vec<_>>()
                .par_iter()
                .map_init(
                    || {
                        let target_machine = self.config().target_machine(target);
                        FuncTrampoline::new(target_machine, target.triple().clone(), binary_format)
                            .unwrap()
                    },
                    |func_trampoline, (sig_index, sig)| {
                        let kind = wasmer_compiler::misc::CompiledKind::FunctionCallTrampoline(
                            *sig_index,
                            (*sig).clone(),
                        );
                        let trampoline =
                            func_trampoline.trampoline(sig, self.config(), &kind, compile_info);
                        if let Some(progress) = progress.as_ref() {
                            progress.notify_steps(WASM_TRAMPOLINE_ESTIMATED_BODY_SIZE)?;
                        }
                        trampoline
                    },
                )
                .collect::<Result<Vec<_>, _>>()
        })?;

        // TODO: I removed the parallel processing of dynamic trampolines because we're passing
        // the sections bytes and relocations directly into the trampoline generation function.
        // We can move that logic out and re-enable parallel processing. Hopefully, there aren't
        // enough dynamic trampolines to actually cause a noticeable performance degradation.
        let dynamic_function_trampolines = {
            let progress = progress.clone();
            let target_machine = self.config().target_machine(target);
            let func_trampoline =
                FuncTrampoline::new(target_machine, target.triple().clone(), binary_format)
                    .unwrap();
            module
                .imported_function_types()
                .collect::<Vec<_>>()
                .into_iter()
                .enumerate()
                .map(|(index, func_type)| {
                    let kind = wasmer_compiler::misc::CompiledKind::DynamicFunctionTrampoline(
                        FunctionIndex::from_u32(index as u32),
                        func_type.clone(),
                    );
                    let trampoline = func_trampoline.dynamic_trampoline(
                        &func_type,
                        self.config(),
                        &kind,
                        index as u32,
                        &module_hash,
                    )?;
                    if let Some(progress) = progress.as_ref() {
                        progress.notify_steps(WASM_TRAMPOLINE_ESTIMATED_BODY_SIZE)?;
                    }
                    Ok(trampoline)
                })
                .collect::<Result<Vec<_>, CompileError>>()?
        };

        let object_files = functions
            .into_iter()
            .map(|object| CompileOutput {
                object,
                maximum_stack_usage: None,
            })
            .collect();
        let trampolines_objects = function_call_trampolines
            .into_iter()
            .map(|object| CompileOutput {
                object,
                maximum_stack_usage: None,
            })
            .collect();
        let dynamic_trampolines_objects = dynamic_function_trampolines
            .into_iter()
            .map(|object| CompileOutput {
                object,
                maximum_stack_usage: None,
            })
            .collect();

        let module_hash = module.hash_string();
        compile_info.function_max_stack_usage = function_max_stack_usage;
        let compile_info_blob = compile_info.serialize().map_err(|e| {
            CompileError::Codegen(format!("cannot serialize CompileModuleInfo: {e}"))
        })?;
        emit_metadata_and_link(
            &pool,
            target,
            &compile_info_blob,
            CompiledObjects {
                emit_trap_offsets: false,
                object_files,
                import_trampoline_object_files: Vec::new(),
                trampoline_object_files: trampolines_objects,
                dynamic_trampoline_object_files: dynamic_trampolines_objects,
            },
            self.config
                .callbacks
                .as_ref()
                .map(|callbacks| callbacks.debug_dir().clone()),
            module_hash,
        )
    }

    /// Add suggested optimizations to this compiler.
    fn with_opts(
        &mut self,
        _suggested_compiler_opts: &wasmer_types::target::UserCompilerOptimizations,
    ) -> Result<(), CompileError> {
        Ok(())
    }
}
