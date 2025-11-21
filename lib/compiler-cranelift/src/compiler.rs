//! Support for compiling with Cranelift.

#[cfg(feature = "unwind")]
use crate::dwarf::WriterRelocate;

#[cfg(feature = "unwind")]
use crate::translator::CraneliftUnwindInfo;
use crate::{
    address_map::get_function_address_map,
    config::Cranelift,
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
use gimli::write::{Address, EhFrame, FrameTable, Writer};

#[cfg(feature = "rayon")]
use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};
use std::sync::Arc;

#[cfg(feature = "unwind")]
use wasmer_compiler::types::{section::SectionIndex, unwind::CompiledFunctionUnwindInfo};
use wasmer_compiler::{
    Compiler, FunctionBinaryReader, FunctionBodyData, MiddlewareBinaryReader, ModuleMiddleware,
    ModuleMiddlewareChain, ModuleTranslationState,
    types::{
        function::{
            Compilation, CompiledFunction, CompiledFunctionFrameInfo, FunctionBody, UnwindInfo,
        },
        module::CompileModuleInfo,
        relocation::{Relocation, RelocationTarget},
    },
};
#[cfg(feature = "unwind")]
use wasmer_types::entity::EntityRef;
use wasmer_types::entity::PrimaryMap;
#[cfg(feature = "unwind")]
use wasmer_types::target::CallingConvention;
use wasmer_types::target::Target;
use wasmer_types::{
    CompileError, FunctionIndex, LocalFunctionIndex, ModuleInfo, SignatureIndex, TrapCode,
    TrapInformation,
};

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
        module_translation_state: &ModuleTranslationState,
        function_body_inputs: PrimaryMap<LocalFunctionIndex, FunctionBodyData<'_>>,
    ) -> Result<Compilation, CompileError> {
        let isa = self
            .config()
            .isa(target)
            .map_err(|error| CompileError::Codegen(error.to_string()))?;
        let frontend_config = isa.frontend_config();
        let memory_styles = &compile_info.memory_styles;
        let table_styles = &compile_info.table_styles;
        let module = &compile_info.module;
        let signatures = module
            .signatures
            .iter()
            .map(|(_sig_index, func_type)| signature_to_cranelift_ir(func_type, frontend_config))
            .collect::<PrimaryMap<SignatureIndex, ir::Signature>>();

        // Generate the frametable
        #[cfg(feature = "unwind")]
        let dwarf_frametable = if function_body_inputs.is_empty() {
            // If we have no function body inputs, we don't need to
            // construct the `FrameTable`. Constructing it, with empty
            // FDEs will cause some issues in Linux.
            None
        } else {
            match target.triple().default_calling_convention() {
                Ok(CallingConvention::SystemV) => {
                    match isa.create_systemv_cie() {
                        Some(cie) => {
                            let mut dwarf_frametable = FrameTable::default();
                            let cie_id = dwarf_frametable.add_cie(cie);
                            Some((dwarf_frametable, cie_id))
                        }
                        // Even though we are in a SystemV system, Cranelift doesn't support it
                        None => None,
                    }
                }
                _ => None,
            }
        };

        let compile_function =
            |func_translator: &mut FuncTranslator,
             (i, input): (&LocalFunctionIndex, &FunctionBodyData)| {
                let func_index = module.func_index(*i);
                let mut context = Context::new();
                let mut func_env = FuncEnvironment::new(
                    isa.frontend_config(),
                    module,
                    &signatures,
                    memory_styles,
                    table_styles,
                );
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
                        &code_buf,
                    );
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

                let (unwind_info, fde) = match compiled_function_unwind_info(&*isa, &context)? {
                    #[cfg(feature = "unwind")]
                    CraneliftUnwindInfo::Fde(fde) => {
                        if dwarf_frametable.is_some() {
                            let fde = fde.to_fde(Address::Symbol {
                                // The symbol is the kind of relocation.
                                // "0" is used for functions
                                symbol: WriterRelocate::FUNCTION_SYMBOL,
                                // We use the addend as a way to specify the
                                // function index
                                addend: i.index() as _,
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

                Ok((
                    CompiledFunction {
                        body: FunctionBody {
                            body: code_buf,
                            unwind_info,
                        },
                        relocations: func_relocs,
                        frame_info: CompiledFunctionFrameInfo { address_map, traps },
                    },
                    fde,
                ))
            };

        #[cfg_attr(not(feature = "unwind"), allow(unused_mut))]
        let mut custom_sections = PrimaryMap::new();

        #[cfg(not(feature = "rayon"))]
        let mut func_translator = FuncTranslator::new();
        #[cfg(not(feature = "rayon"))]
        #[cfg_attr(not(feature = "unwind"), allow(unused_variables))]
        let (functions, fdes): (Vec<CompiledFunction>, Vec<_>) = function_body_inputs
            .iter()
            .collect::<Vec<(LocalFunctionIndex, &FunctionBodyData<'_>)>>()
            .into_iter()
            .map(|(i, input)| compile_function(&mut func_translator, (&i, input)))
            .collect::<Result<Vec<_>, CompileError>>()?
            .into_iter()
            .unzip();
        #[cfg(feature = "rayon")]
        #[cfg_attr(not(feature = "unwind"), allow(unused_variables))]
        let (functions, fdes): (Vec<CompiledFunction>, Vec<_>) = function_body_inputs
            .iter()
            .collect::<Vec<(LocalFunctionIndex, &FunctionBodyData<'_>)>>()
            .par_iter()
            .map_init(FuncTranslator::new, |func_translator, (i, input)| {
                compile_function(func_translator, (i, input))
            })
            .collect::<Result<Vec<_>, CompileError>>()?
            .into_iter()
            .unzip();

        #[cfg_attr(not(feature = "unwind"), allow(unused_mut))]
        let mut unwind_info = UnwindInfo::default();

        #[cfg(feature = "unwind")]
        if let Some((mut dwarf_frametable, cie_id)) = dwarf_frametable {
            for fde in fdes.into_iter().flatten() {
                dwarf_frametable.add_fde(cie_id, fde);
            }
            let mut eh_frame = EhFrame(WriterRelocate::new(target.triple().endianness().ok()));
            dwarf_frametable.write_eh_frame(&mut eh_frame).unwrap();
            eh_frame.write(&[0, 0, 0, 0]).unwrap(); // Write a 0 length at the end of the table.

            let eh_frame_section = eh_frame.0.into_section();
            custom_sections.push(eh_frame_section);
            unwind_info.eh_frame = Some(SectionIndex::new(custom_sections.len() - 1));
        };

        // function call trampolines (only for local functions, by signature)
        #[cfg(not(feature = "rayon"))]
        let mut cx = FunctionBuilderContext::new();
        #[cfg(not(feature = "rayon"))]
        let function_call_trampolines = module
            .signatures
            .values()
            .collect::<Vec<_>>()
            .into_iter()
            .map(|sig| make_trampoline_function_call(&self.config().callbacks, &*isa, &mut cx, sig))
            .collect::<Result<Vec<FunctionBody>, CompileError>>()?
            .into_iter()
            .collect();
        #[cfg(feature = "rayon")]
        let function_call_trampolines = module
            .signatures
            .values()
            .collect::<Vec<_>>()
            .par_iter()
            .map_init(FunctionBuilderContext::new, |cx, sig| {
                make_trampoline_function_call(&self.config().callbacks, &*isa, cx, sig)
            })
            .collect::<Result<Vec<FunctionBody>, CompileError>>()?
            .into_iter()
            .collect();

        use wasmer_types::VMOffsets;
        let offsets = VMOffsets::new_for_trampolines(frontend_config.pointer_bytes());
        // dynamic function trampolines (only for imported functions)
        #[cfg(not(feature = "rayon"))]
        let mut cx = FunctionBuilderContext::new();
        #[cfg(not(feature = "rayon"))]
        let dynamic_function_trampolines = module
            .imported_function_types()
            .collect::<Vec<_>>()
            .into_iter()
            .map(|func_type| {
                make_trampoline_dynamic_function(
                    &self.config().callbacks,
                    &*isa,
                    &offsets,
                    &mut cx,
                    &func_type,
                )
            })
            .collect::<Result<Vec<_>, CompileError>>()?
            .into_iter()
            .collect();
        #[cfg(feature = "rayon")]
        let dynamic_function_trampolines = module
            .imported_function_types()
            .collect::<Vec<_>>()
            .par_iter()
            .map_init(FunctionBuilderContext::new, |cx, func_type| {
                make_trampoline_dynamic_function(
                    &self.config().callbacks,
                    &*isa,
                    &offsets,
                    cx,
                    func_type,
                )
            })
            .collect::<Result<Vec<_>, CompileError>>()?
            .into_iter()
            .collect();

        let got = wasmer_compiler::types::function::GOT::empty();

        Ok(Compilation {
            functions: functions.into_iter().collect(),
            custom_sections,
            function_call_trampolines,
            dynamic_function_trampolines,
            unwind_info,
            got,
        })
    }
}

impl Compiler for CraneliftCompiler {
    fn name(&self) -> &str {
        "cranelift"
    }

    fn get_perfmap_enabled(&self) -> bool {
        self.config.enable_perfmap
    }

    fn deterministic_id(&self) -> String {
        String::from("cranelift")
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
        module_translation_state: &ModuleTranslationState,
        function_body_inputs: PrimaryMap<LocalFunctionIndex, FunctionBodyData<'_>>,
    ) -> Result<Compilation, CompileError> {
        #[cfg(feature = "rayon")]
        {
            let num_threads = self.config.num_threads.get();
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(num_threads)
                .build()
                .unwrap();

            pool.install(|| {
                self.compile_module_internal(
                    target,
                    compile_info,
                    module_translation_state,
                    function_body_inputs,
                )
            })
        }

        #[cfg(not(feature = "rayon"))]
        {
            self.compile_module_internal(
                target,
                compile_info,
                module_translation_state,
                function_body_inputs,
            )
        }
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
