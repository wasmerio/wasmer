//! Support for compiling with Cranelift.

use crate::address_map::get_function_address_map;
use crate::config::Cranelift;
#[cfg(feature = "unwind")]
use crate::dwarf::WriterRelocate;
use crate::func_environ::{get_function_name, FuncEnvironment};
use crate::sink::{RelocSink, TrapSink};
use crate::trampoline::{
    make_trampoline_dynamic_function, make_trampoline_function_call, FunctionBuilderContext,
};
use crate::translator::{
    compiled_function_unwind_info, signature_to_cranelift_ir, transform_jump_table,
    CraneliftUnwindInfo, FuncTranslator,
};
use cranelift_codegen::ir;
use cranelift_codegen::print_errors::pretty_error;
use cranelift_codegen::{binemit, Context};
#[cfg(feature = "unwind")]
use gimli::write::{Address, EhFrame, FrameTable};
use loupe::MemoryUsage;
use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};
use std::sync::Arc;
use target_lexicon::{Architecture, OperatingSystem};
use wasmer_compiler::CompileError;
use wasmer_compiler::{CallingConvention, ModuleTranslationState, Target};
use wasmer_compiler::{
    Compilation, CompileModuleInfo, CompiledFunction, CompiledFunctionFrameInfo,
    CompiledFunctionUnwindInfo, Compiler, Dwarf, FunctionBinaryReader, FunctionBody,
    FunctionBodyData, MiddlewareBinaryReader, ModuleMiddleware, ModuleMiddlewareChain,
    SectionIndex,
};
use wasmer_compiler::{
    CustomSection, CustomSectionProtection, Relocation, RelocationKind, RelocationTarget,
    SectionBody,
};
use wasmer_types::entity::{EntityRef, PrimaryMap};
use wasmer_types::{FunctionIndex, LocalFunctionIndex, SignatureIndex};
use wasmer_vm::libcalls::LibCall;

/// A compiler that compiles a WebAssembly module with Cranelift, translating the Wasm to Cranelift IR,
/// optimizing it and then translating to assembly.
#[derive(MemoryUsage)]
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
}

impl Compiler for CraneliftCompiler {
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
        let isa = self.config().isa(target);
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

        let mut custom_sections = PrimaryMap::new();

        let probestack_trampoline_relocation_target = if target.triple().operating_system
            == OperatingSystem::Linux
            && target.triple().architecture == Architecture::X86_64
        {
            let probestack_trampoline = CustomSection {
                protection: CustomSectionProtection::ReadExecute,
                // We create a jump to an absolute 64bits address
                // with an indrect jump immediatly followed but the absolute address
                // JMP [IP+0]   FF 25 00 00 00 00
                // 64bits ADDR  00 00 00 00 00 00 00 00 preset to 0 until the relocation takes place
                bytes: SectionBody::new_with_vec(vec![
                    0xff, 0x25, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                    0x00,
                ]),
                relocations: vec![Relocation {
                    kind: RelocationKind::Abs8,
                    reloc_target: RelocationTarget::LibCall(LibCall::Probestack),
                    // 6 is the size of the jmp instruction. The relocated address must follow
                    offset: 6,
                    addend: 0,
                }],
            };
            custom_sections.push(probestack_trampoline);

            Some(SectionIndex::new(custom_sections.len() - 1))
        } else {
            None
        };

        let (functions, fdes): (Vec<CompiledFunction>, Vec<_>) = function_body_inputs
            .iter()
            .collect::<Vec<(LocalFunctionIndex, &FunctionBodyData<'_>)>>()
            .par_iter()
            .map_init(FuncTranslator::new, |func_translator, (i, input)| {
                let func_index = module.func_index(*i);
                let mut context = Context::new();
                let mut func_env = FuncEnvironment::new(
                    isa.frontend_config(),
                    module,
                    &signatures,
                    &memory_styles,
                    &table_styles,
                );
                context.func.name = get_function_name(func_index);
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

                let mut code_buf: Vec<u8> = Vec::new();
                let mut reloc_sink =
                    RelocSink::new(&module, func_index, probestack_trampoline_relocation_target);
                let mut trap_sink = TrapSink::new();
                let mut stackmap_sink = binemit::NullStackMapSink {};
                context
                    .compile_and_emit(
                        &*isa,
                        &mut code_buf,
                        &mut reloc_sink,
                        &mut trap_sink,
                        &mut stackmap_sink,
                    )
                    .map_err(|error| {
                        CompileError::Codegen(pretty_error(&context.func, Some(&*isa), error))
                    })?;

                let (unwind_info, fde) = match compiled_function_unwind_info(&*isa, &context)? {
                    #[cfg(feature = "unwind")]
                    CraneliftUnwindInfo::FDE(fde) => {
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
                let address_map = get_function_address_map(&context, range, code_buf.len(), &*isa);

                // We transform the Cranelift JumpTable's into compiler JumpTables
                let func_jt_offsets = transform_jump_table(context.func.jt_offsets);

                Ok((
                    CompiledFunction {
                        body: FunctionBody {
                            body: code_buf,
                            unwind_info,
                        },
                        jt_offsets: func_jt_offsets,
                        relocations: reloc_sink.func_relocs,
                        frame_info: CompiledFunctionFrameInfo {
                            address_map,
                            traps: trap_sink.traps,
                        },
                    },
                    fde,
                ))
            })
            .collect::<Result<Vec<_>, CompileError>>()?
            .into_iter()
            .unzip();

        #[cfg(feature = "unwind")]
        let dwarf = if let Some((mut dwarf_frametable, cie_id)) = dwarf_frametable {
            for fde in fdes {
                if let Some(fde) = fde {
                    dwarf_frametable.add_fde(cie_id, fde);
                }
            }
            let mut eh_frame = EhFrame(WriterRelocate::new(target.triple().endianness().ok()));
            dwarf_frametable.write_eh_frame(&mut eh_frame).unwrap();

            let eh_frame_section = eh_frame.0.into_section();
            custom_sections.push(eh_frame_section);
            Some(Dwarf::new(SectionIndex::new(custom_sections.len() - 1)))
        } else {
            None
        };
        #[cfg(not(feature = "unwind"))]
        let dwarf = None;

        // function call trampolines (only for local functions, by signature)
        let function_call_trampolines = module
            .signatures
            .values()
            .collect::<Vec<_>>()
            .par_iter()
            .map_init(FunctionBuilderContext::new, |mut cx, sig| {
                make_trampoline_function_call(&*isa, &mut cx, sig)
            })
            .collect::<Result<Vec<FunctionBody>, CompileError>>()?
            .into_iter()
            .collect::<PrimaryMap<SignatureIndex, FunctionBody>>();

        use wasmer_vm::VMOffsets;
        let offsets = VMOffsets::new_for_trampolines(frontend_config.pointer_bytes());
        // dynamic function trampolines (only for imported functions)
        let dynamic_function_trampolines = module
            .imported_function_types()
            .collect::<Vec<_>>()
            .par_iter()
            .map_init(FunctionBuilderContext::new, |mut cx, func_type| {
                make_trampoline_dynamic_function(&*isa, &offsets, &mut cx, &func_type)
            })
            .collect::<Result<Vec<_>, CompileError>>()?
            .into_iter()
            .collect::<PrimaryMap<FunctionIndex, FunctionBody>>();

        Ok(Compilation::new(
            functions.into_iter().collect(),
            custom_sections,
            function_call_trampolines,
            dynamic_function_trampolines,
            dwarf,
        ))
    }
}
