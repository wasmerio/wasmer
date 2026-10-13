//! Support for compiling with Cranelift.

#[cfg(feature = "unwind")]
use crate::dwarf::WriterRelocate;

#[cfg(feature = "unwind")]
use crate::eh::{FunctionLsdaData, build_function_lsda, compact_unwind_encoding_aarch64};

#[cfg(feature = "unwind")]
use crate::translator::CraneliftUnwindInfo;
use crate::{
    address_map::get_function_address_map,
    config::{Cranelift, CraneliftOptLevel},
    func_environ::{FuncEnvironment, get_function_name},
    trampoline::{
        FunctionBuilderContext, make_trampoline_dynamic_function, make_trampoline_function_call,
    },
    translator::{
        FuncTranslator, compiled_function_unwind_info, irlibcall_to_libcall,
        irreloc_to_relocationkind, signature_to_cranelift_ir,
    },
};
use cranelift_codegen::{
    Context, FinalizedMachReloc, FinalizedRelocTarget, MachTrap,
    ir::{self, ExternalName, UserFuncName},
};

#[cfg(feature = "unwind")]
use cranelift_codegen::gimli::{
    constants::DW_EH_PE_absptr,
    write::{Address, FrameDescriptionEntry, FrameTable},
};

use itertools::Itertools;
use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};
use std::sync::Arc;
use wasmer_compiler::WASM_TRAMPOLINE_ESTIMATED_BODY_SIZE;
use wasmer_compiler::elf::CompileOutput;
use wasmer_compiler::types::function::Compilation;

use wasmer_compiler::progress::ProgressContext;
#[cfg(feature = "unwind")]
use wasmer_compiler::types::unwind::CompiledFunctionUnwindInfo;
use wasmer_compiler::{
    Compiler, FunctionBinaryReader, FunctionBodyData, MiddlewareBinaryReader, ModuleMiddleware,
    ModuleMiddlewareChain, ModuleTranslationState, WasmSourceMap,
    types::{
        function::{CompiledFunction, CompiledFunctionFrameInfo, FunctionBody},
        module::CompileModuleInfo,
        relocation::{Relocation, RelocationTarget},
    },
};
use wasmer_compiler::{build_function_buckets, translate_function_buckets};
#[cfg(feature = "unwind")]
use wasmer_types::entity::EntityRef;
use wasmer_types::entity::PrimaryMap;
#[cfg(feature = "unwind")]
use wasmer_types::target::CallingConvention;
use wasmer_types::target::Target;
use wasmer_types::{
    CompilationProgressCallback, CompileError, FunctionIndex, LocalFunctionIndex, ModuleInfo,
    SignatureIndex, TrapCode, TrapInformation,
};

pub struct CraneliftCompiledFunction {
    function: CompiledFunction,
    #[cfg(feature = "unwind")]
    fde: Option<FrameDescriptionEntry>,
    #[cfg(feature = "unwind")]
    function_lsda: Option<FunctionLsdaData>,
    #[cfg(feature = "unwind")]
    compact_unwind_encoding: Option<u32>,
}

impl wasmer_compiler::CompiledFunction for CraneliftCompiledFunction {}

/// A compiler that compiles a WebAssembly module with Cranelift, translating the Wasm to Cranelift IR,
/// optimizing it and then translating to assembly.
#[derive(Debug)]
pub struct CraneliftCompiler {
    config: Cranelift,
}

impl CraneliftCompiler {
    /// Creates a new Cranelift compiler
    pub fn new(config: Cranelift) -> Self {
        Self { config }
    }

    /// Gets the WebAssembly features for this Compiler
    pub fn config(&self) -> &Cranelift {
        &self.config
    }

    // Helper function to create an easy scope boundary for the thread pool used
    // in [`Self::compile_module`].
    fn compile_module_internal(
        &self,
        target: &Target,
        compile_info: &CompileModuleInfo,
        compile_info_blob: &[u8],
        module_translation_state: &ModuleTranslationState,
        function_body_inputs: PrimaryMap<LocalFunctionIndex, FunctionBodyData<'_>>,
        progress_callback: Option<&CompilationProgressCallback>,
    ) -> Result<Compilation, CompileError> {
        wasmer_compiler::validate_module_fixed_table_size(
            &compile_info.module,
            self.config.max_table_elements,
        )?;
        let function_max_stack_usage = function_body_inputs.iter().map(|_| None).collect();
        let isa = self
            .config()
            .isa(target)
            .map_err(|error| CompileError::Codegen(error.to_string()))?;
        let frontend_config = isa.frontend_config();
        #[cfg(feature = "unwind")]
        let pointer_bytes = frontend_config.pointer_bytes();
        #[cfg(feature = "unwind")]
        let emit_macho_compact_unwind = matches!(
            target.triple(),
            target_lexicon::Triple {
                binary_format: target_lexicon::BinaryFormat::Macho,
                operating_system: target_lexicon::OperatingSystem::Darwin(_),
                architecture: target_lexicon::Architecture::Aarch64(_),
                ..
            }
        );
        let memory_styles = &compile_info.memory_styles;
        let table_styles = &compile_info.table_styles;
        let module = &compile_info.module;
        let source_map = Arc::new(
            WasmSourceMap::new(module, module_translation_state, &function_body_inputs)
                .map_err(CompileError::Codegen)?,
        );

        let signatures = module
            .signatures
            .iter()
            .map(|(_sig_index, func_type)| {
                signature_to_cranelift_ir(func_type, frontend_config, target.triple().architecture)
            })
            .collect::<PrimaryMap<SignatureIndex, ir::Signature>>();
        let signature_hashes = &module.signature_hashes;

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
            .map(|cb| ProgressContext::new(cb, total_steps, "cranelift::functions"));

        // Generate the frametable
        #[cfg(feature = "unwind")]
        let dwarf_frametable = if function_body_inputs.is_empty() {
            // If we have no function body inputs, we don't need to
            // construct the `FrameTable`. Constructing it, with empty
            // FDEs will cause some issues in Linux.
            None
        } else {
            match target.triple().default_calling_convention() {
                Ok(CallingConvention::SystemV) => match isa.create_systemv_cie() {
                    Some(mut cie) => {
                        cie.personality = Some((
                            DW_EH_PE_absptr,
                            Address::Symbol {
                                symbol: WriterRelocate::PERSONALITY_SYMBOL,
                                addend: 0,
                            },
                        ));
                        cie.lsda_encoding = Some(DW_EH_PE_absptr);
                        let mut dwarf_frametable = FrameTable::default();
                        let cie_id = dwarf_frametable.add_cie(cie);
                        Some((dwarf_frametable, cie_id))
                    }
                    // Even though we are in a SystemV system, Cranelift doesn't support it
                    None => None,
                },
                _ => None,
            }
        };

        // The `compile_function` closure is used for both the sequential and
        // parallel compilation paths to avoid code duplication.
        let compile_function = |func_translator: &mut FuncTranslator,
                                i: &LocalFunctionIndex,
                                input: &FunctionBodyData|
         -> Result<CompileOutput, CompileError> {
            let func_index = module.func_index(*i);
            let mut context = Context::new();
            let mut func_env = FuncEnvironment::new(
                isa.frontend_config(),
                target.triple().architecture,
                module,
                &signatures,
                signature_hashes,
                memory_styles,
                table_styles,
            )?;
            context.func.name = match get_function_name(&mut context.func, func_index) {
                ExternalName::User(nameref) => {
                    if context.func.params.user_named_funcs().is_valid(nameref) {
                        let name = &context.func.params.user_named_funcs()[nameref];
                        UserFuncName::User(name.clone())
                    } else {
                        UserFuncName::default()
                    }
                }
                ExternalName::TestCase(testcase) => UserFuncName::Testcase(testcase),
                _ => UserFuncName::default(),
            };
            context.func.signature = signatures[module.functions[func_index]].clone();
            // if generate_debug_info {
            //     context.func.collect_debug_info();
            // }

            let mut reader =
                MiddlewareBinaryReader::new_with_offset(input.data, input.module_offset);
            reader.set_middleware_chain(
                self.config
                    .middlewares
                    .generate_function_middleware_chain(*i),
            );

            func_translator.translate(
                module_translation_state,
                &mut reader,
                &mut context.func,
                &mut func_env,
                *i,
            )?;

            if let Some(callbacks) = self.config.callbacks.as_ref() {
                use wasmer_compiler::misc::CompiledKind;

                callbacks.preopt_ir(
                    &CompiledKind::Local(*i, compile_info.module.get_function_name(func_index)),
                    &compile_info.module.hash_string(),
                    context.func.display().to_string().as_bytes(),
                );
            }

            let mut code_buf: Vec<u8> = Vec::new();
            let mut ctrl_plane = Default::default();
            let func_name_map = context.func.params.user_named_funcs().clone();
            let result = context
                .compile(&*isa, &mut ctrl_plane)
                .map_err(|error| CompileError::Codegen(format!("{error:#?}")))?;
            code_buf.extend_from_slice(result.code_buffer());

            if let Some(callbacks) = self.config.callbacks.as_ref() {
                use wasmer_compiler::misc::CompiledKind;

                callbacks.obj_memory_buffer(
                    &CompiledKind::Local(*i, compile_info.module.get_function_name(func_index)),
                    &compile_info.module.hash_string(),
                    &code_buf,
                );
                callbacks.asm_memory_buffer(
                    &CompiledKind::Local(*i, compile_info.module.get_function_name(func_index)),
                    &compile_info.module.hash_string(),
                    target.triple().architecture,
                    &code_buf,
                )?;
            }

            let func_relocs = result
                .buffer
                .relocs()
                .iter()
                .map(|r| mach_reloc_to_reloc(module, &func_name_map, r))
                .collect::<Vec<_>>();

            let traps = result
                .buffer
                .traps()
                .iter()
                .map(mach_trap_to_trap)
                .collect::<Vec<_>>();

            #[cfg(feature = "unwind")]
            let emit_lsda = dwarf_frametable.is_some() || emit_macho_compact_unwind;

            #[cfg(feature = "unwind")]
            let compact_unwind_encoding = if emit_macho_compact_unwind {
                Some(
                    compact_unwind_encoding_aarch64(&result.buffer.unwind_info).map_err(|error| {
                        CompileError::Codegen(format!(
                            "failed to encode aarch64 Mach-O compact unwind for function {}: {error}",
                            i.index()
                        ))
                    })?,
                )
            } else {
                None
            };

            #[cfg(feature = "unwind")]
            let function_lsda = if emit_lsda {
                build_function_lsda(
                    result.buffer.call_sites(),
                    result.buffer.data().len(),
                    pointer_bytes,
                    true,
                )
            } else {
                None
            };

            #[allow(unused)]
            let (unwind_info, fde) = match compiled_function_unwind_info(&*isa, &context)? {
                #[cfg(feature = "unwind")]
                CraneliftUnwindInfo::Fde(fde) => {
                    if dwarf_frametable.is_some() {
                        // For the ELF artifact format each function's
                        // `.eh_frame` relocates against its own text symbol,
                        // so the FDE's initial location must not be shifted.
                        let fde = fde.to_fde(Address::Symbol {
                            // The symbol is the kind of relocation.
                            // "0" is used for functions
                            symbol: WriterRelocate::FUNCTION_SYMBOL,
                            addend: 0,
                        });
                        // The unwind information is inserted into the dwarf section
                        (Some(CompiledFunctionUnwindInfo::Dwarf), Some(fde))
                    } else {
                        (None, None)
                    }
                }
                #[cfg(feature = "unwind")]
                other => (other.maybe_into_to_windows_unwind(), None),

                // This is a bit hacky, but necessary since gimli is not
                // available when the "unwind" feature is disabled.
                #[cfg(not(feature = "unwind"))]
                other => (other.maybe_into_to_windows_unwind(), None::<()>),
            };

            let range = reader.range();
            let address_map = get_function_address_map(&context, range, code_buf.len());

            let compiled = CraneliftCompiledFunction {
                function: CompiledFunction {
                    body: FunctionBody {
                        body: code_buf,
                        unwind_info,
                    },
                    relocations: func_relocs,
                    frame_info: CompiledFunctionFrameInfo { address_map, traps },
                    maximum_stack_usage: None,
                },
                #[cfg(feature = "unwind")]
                fde,
                #[cfg(feature = "unwind")]
                function_lsda,
                #[cfg(feature = "unwind")]
                compact_unwind_encoding,
            };

            let object = crate::elf::emit_local_function(
                #[cfg(feature = "unwind")]
                &*isa,
                target,
                *i,
                &compile_info.module.get_function_name(func_index),
                compile_info.module.name.as_deref(),
                &compiled.function,
                &source_map,
                #[cfg(feature = "unwind")]
                compiled.fde,
                #[cfg(feature = "unwind")]
                compiled.function_lsda,
                #[cfg(feature = "unwind")]
                compiled.compact_unwind_encoding,
            )?;
            Ok(CompileOutput {
                object,
                maximum_stack_usage: None,
            })
        };

        let num_threads = self.config.num_threads.get();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .build()
            .unwrap();
        let object_files = {
            use wasmer_compiler::WASM_LARGE_FUNCTION_THRESHOLD;

            let buckets =
                build_function_buckets(&function_body_inputs, WASM_LARGE_FUNCTION_THRESHOLD / 3);
            let largest_bucket = buckets.first().map(|b| b.size).unwrap_or_default();
            tracing::debug!(buckets = buckets.len(), largest_bucket, "buckets built");

            translate_function_buckets(
                &pool,
                || FuncTranslator::new(self.config.allow_experimental_unaligned_memory_accesses),
                |func_translator, i, input| compile_function(func_translator, i, input),
                progress.clone(),
                &buckets,
            )?
        };

        let module_hash = module.hash_string();

        // function call trampolines (only for local functions, by signature)
        let trampoline_objects = module
            .signatures
            .iter()
            .collect_vec()
            .par_iter()
            .map_init(FunctionBuilderContext::new, |cx, (sig_index, sig)| {
                let kind = wasmer_compiler::misc::CompiledKind::FunctionCallTrampoline(
                    *sig_index,
                    (*sig).clone(),
                );
                let trampoline = make_trampoline_function_call(
                    &self.config().callbacks,
                    &*isa,
                    target.triple().architecture,
                    cx,
                    &kind,
                    sig,
                    &module_hash,
                )?;
                if let Some(progress) = progress.as_ref() {
                    progress.notify_steps(WASM_TRAMPOLINE_ESTIMATED_BODY_SIZE)?;
                }
                Ok(CompileOutput {
                    object: wasmer_compiler::elf::emit_function_body(target, &kind, &trampoline)?,
                    maximum_stack_usage: None,
                })
            })
            .collect::<Result<Vec<_>, CompileError>>()?;

        use wasmer_types::VMOffsets;
        let offsets = VMOffsets::new_for_trampolines(frontend_config.pointer_bytes());
        // dynamic function trampolines (only for imported functions)
        let dynamic_trampoline_objects = module
            .imported_function_types()
            .enumerate()
            .collect_vec()
            .par_iter()
            .map_init(FunctionBuilderContext::new, |cx, (index, func_type)| {
                let kind = wasmer_compiler::misc::CompiledKind::DynamicFunctionTrampoline(
                    FunctionIndex::from_u32(*index as u32),
                    func_type.clone(),
                );
                let trampoline = make_trampoline_dynamic_function(
                    &self.config().callbacks,
                    &*isa,
                    target.triple().architecture,
                    &offsets,
                    cx,
                    &kind,
                    func_type,
                    &module_hash,
                )?;
                if let Some(progress) = progress.as_ref() {
                    progress.notify_steps(WASM_TRAMPOLINE_ESTIMATED_BODY_SIZE)?;
                }
                Ok(CompileOutput {
                    object: wasmer_compiler::elf::emit_function_body(target, &kind, &trampoline)?,
                    maximum_stack_usage: None,
                })
            })
            .collect::<Result<Vec<_>, CompileError>>()?;

        wasmer_compiler::elf::link_module(
            &pool,
            target,
            compile_info_blob,
            object_files,
            Vec::new(),
            trampoline_objects,
            dynamic_trampoline_objects,
            self.config
                .callbacks
                .as_ref()
                .map(|callbacks| callbacks.debug_dir().clone()),
            module.hash().map(|hash| hash.to_string()),
            function_max_stack_usage,
        )
    }
}

impl Compiler for CraneliftCompiler {
    fn name(&self) -> &str {
        "cranelift"
    }

    fn get_perfmap_enabled(&self) -> bool {
        self.config.enable_perfmap
    }

    fn get_debugger(&self) -> Option<wasmer_compiler::Debugger> {
        self.config.debugger
    }

    fn deterministic_id(&self) -> String {
        use wasmer_compiler::DeterministicIdComponent as Component;

        let mut components = vec![Component::Cranelift];
        components.push(match self.config.opt_level {
            CraneliftOptLevel::None => Component::OptNone,
            CraneliftOptLevel::Speed => Component::OptSpeed,
            CraneliftOptLevel::SpeedAndSize => Component::OptSpeedAndSize,
        });
        if self.config.enable_nan_canonicalization {
            components.push(Component::NanCanonicalization);
        }
        if self.config.enable_pic {
            components.push(Component::Pic);
        }
        if self.config.allow_experimental_unaligned_memory_accesses {
            components.push(Component::ExperimentalUnalignedMemoryAccesses);
        }

        components
            .into_iter()
            .map(|component| component.to_string())
            .collect_vec()
            .join("-")
    }

    fn artifact_format(&self) -> String {
        wasmer_compiler::ArtifactFormat::Native.to_string()
    }

    /// Get the middlewares for this compiler
    fn get_middlewares(&self) -> &[Arc<dyn ModuleMiddleware>] {
        &self.config.middlewares
    }

    /// Compile the module using Cranelift, producing a compilation result with
    /// associated relocations.
    fn compile_module(
        &self,
        target: &Target,
        compile_info: &CompileModuleInfo,
        compile_info_blob: &[u8],
        module_translation_state: &ModuleTranslationState,
        function_body_inputs: PrimaryMap<LocalFunctionIndex, FunctionBodyData<'_>>,
        progress_callback: Option<&CompilationProgressCallback>,
    ) -> Result<Compilation, CompileError> {
        self.compile_module_internal(
            target,
            compile_info,
            compile_info_blob,
            module_translation_state,
            function_body_inputs,
            progress_callback,
        )
    }
}

fn mach_reloc_to_reloc(
    module: &ModuleInfo,
    func_index_map: &cranelift_entity::PrimaryMap<ir::UserExternalNameRef, ir::UserExternalName>,
    reloc: &FinalizedMachReloc,
) -> Relocation {
    let FinalizedMachReloc {
        offset,
        kind,
        addend,
        target,
    } = &reloc;
    let name = match target {
        FinalizedRelocTarget::ExternalName(external_name) => external_name,
        FinalizedRelocTarget::Func(_) => {
            unimplemented!("relocations to offset in the same function are not yet supported")
        }
    };
    let reloc_target: RelocationTarget = if let ExternalName::User(extname_ref) = name {
        let func_index = func_index_map[*extname_ref].index;
        //debug_assert_eq!(namespace, 0);
        RelocationTarget::LocalFunc(
            module
                .local_func_index(FunctionIndex::from_u32(func_index))
                .expect("The provided function should be local"),
        )
    } else if let ExternalName::LibCall(libcall) = name {
        RelocationTarget::LibCall(irlibcall_to_libcall(*libcall))
    } else {
        panic!("unrecognized external target")
    };
    Relocation {
        kind: irreloc_to_relocationkind(*kind),
        reloc_target,
        offset: *offset,
        addend: *addend,
    }
}

fn mach_trap_to_trap(trap: &MachTrap) -> TrapInformation {
    let &MachTrap { offset, code } = trap;
    TrapInformation {
        code_offset: offset,
        trap_code: translate_ir_trapcode(code),
    }
}

/// Translates the Cranelift IR TrapCode into generic Trap Code
fn translate_ir_trapcode(trap: ir::TrapCode) -> TrapCode {
    if trap == ir::TrapCode::STACK_OVERFLOW {
        TrapCode::StackOverflow
    } else if trap == ir::TrapCode::HEAP_OUT_OF_BOUNDS {
        TrapCode::HeapAccessOutOfBounds
    } else if trap == crate::TRAP_HEAP_MISALIGNED {
        TrapCode::UnalignedAtomic
    } else if trap == crate::TRAP_TABLE_OUT_OF_BOUNDS {
        TrapCode::TableAccessOutOfBounds
    } else if trap == crate::TRAP_INDIRECT_CALL_TO_NULL {
        TrapCode::IndirectCallToNull
    } else if trap == crate::TRAP_BAD_SIGNATURE {
        TrapCode::BadSignature
    } else if trap == ir::TrapCode::INTEGER_OVERFLOW {
        TrapCode::IntegerOverflow
    } else if trap == ir::TrapCode::INTEGER_DIVISION_BY_ZERO {
        TrapCode::IntegerDivisionByZero
    } else if trap == ir::TrapCode::BAD_CONVERSION_TO_INTEGER {
        TrapCode::BadConversionToInteger
    } else if trap == crate::TRAP_UNREACHABLE {
        TrapCode::UnreachableCodeReached
    } else if trap == crate::TRAP_INTERRUPT {
        unimplemented!("Interrupts not supported")
    } else if trap == crate::TRAP_NULL_REFERENCE || trap == crate::TRAP_NULL_I31_REF {
        unimplemented!("Null reference not supported")
    } else {
        unimplemented!("Trap code {trap:?} not supported")
    }
}
