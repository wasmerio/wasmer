//! Support for compiling with Cranelift.
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
        signature_to_cranelift_ir,
    },
};
use cranelift_codegen::{
    Context, FinalizedMachReloc, FinalizedRelocTarget, MachTrap,
    binemit::Reloc,
    ir::{self, ExternalName, UserFuncName},
};
#[cfg(feature = "unwind")]
use wasmer_compiler::dwarf::{DwarfState, init_dwarf_unit};

#[cfg(feature = "unwind")]
use gimli::{
    constants::{DW_EH_PE_indirect, DW_EH_PE_pcrel, DW_EH_PE_sdata4},
    write::{Address, EhFrame, FrameTable},
};
#[cfg(feature = "unwind")]
use wasmer_compiler::dwarf::EhTarget;

use object::{
    RelocationEncoding, RelocationFlags, RelocationKind as ObjectRelocationKind, SectionKind, elf,
    macho,
    write::{
        Object, Relocation as ObjectRelocation, StandardSection, StandardSegment,
        Symbol as ObjectSymbol, SymbolSection,
    },
    {SymbolFlags, SymbolKind, SymbolScope},
};
use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::{NamedTempFile, tempdir};
use wasmer_compiler::object::get_object_for_target;
use wasmer_compiler::progress::ProgressContext;
use wasmer_compiler::serialize::SerializableModule;
use wasmer_compiler::types::address_map::FunctionAddressMap;
use wasmer_compiler::{
    CompiledObjects, Compiler, FunctionBinaryReader, FunctionBodyData, MiddlewareBinaryReader,
    ModuleMiddleware, ModuleMiddlewareChain, ModuleTranslationState, WASM_LARGE_FUNCTION_THRESHOLD,
    WASM_TRAMPOLINE_ESTIMATED_BODY_SIZE, WASMER_TRAPS_SECTION_NAME, build_function_buckets,
    emit_metadata_and_link,
    misc::{CompiledFunctionExt, CompiledKind},
    translate_function_buckets,
    types::{function::FunctionBody, module::CompileModuleInfo},
};
use wasmer_types::entity::{EntityRef, PrimaryMap};
use wasmer_types::target::{BinaryFormat, CallingConvention, Target, Triple};
use wasmer_types::{
    Addend, CodeOffset, CompilationProgressCallback, CompileError, FunctionIndex, LibCall,
    LocalFunctionIndex, ModuleInfo, SignatureIndex, TrapCode, TrapInformation, VMOffsets,
};

#[derive(Debug)]
pub enum RelocationTarget {
    /// A relocation to a function defined locally in the wasm (not an imported one).
    LocalFunc(LocalFunctionIndex),
    /// A compiler-generated libcall.
    LibCall(LibCall),
}

/// A relocation emitted by Cranelift for a compiled function body. The kind is
/// kept as Cranelift's own [`Reloc`] so it can be mapped straight onto
/// object-file relocation flags without an intermediate Wasmer enum.
struct CraneliftRelocation {
    kind: Reloc,
    reloc_target: RelocationTarget,
    offset: CodeOffset,
    addend: Addend,
}

/// The result of compiling a single Wasm function: everything required to
/// serialize it into its own relocatable object file.
struct CraneliftCompiledFunction {
    body: Vec<u8>,
    relocations: Vec<CraneliftRelocation>,
    traps: Vec<TrapInformation>,
    address_map: FunctionAddressMap,
    #[cfg(feature = "unwind")]
    fde: Option<gimli::write::FrameDescriptionEntry>,
    #[cfg(feature = "unwind")]
    lsda: Option<crate::eh::FunctionLsdaData>,
}

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
        mut serializable: SerializableModule,
        module_translation_state: &ModuleTranslationState,
        function_body_inputs: PrimaryMap<LocalFunctionIndex, FunctionBodyData<'_>>,
        progress_callback: Option<&CompilationProgressCallback>,
    ) -> Result<(NamedTempFile, SerializableModule), CompileError> {
        let triple = target.triple();
        let isa = self
            .config()
            .isa(target)
            .map_err(|error| CompileError::Codegen(error.to_string()))?;
        let frontend_config = isa.frontend_config();
        let pointer_bytes = frontend_config.pointer_bytes();

        let emit_eh_frame = matches!(
            triple.default_calling_convention(),
            Ok(CallingConvention::SystemV | CallingConvention::AppleAarch64)
        );

        let build_directory = tempdir().map_err(|err| {
            CompileError::Codegen(format!("cannot create temporary build folder: {err}"))
        })?;
        let build_dir = build_directory.path();

        let memory_styles = &compile_info.memory_styles;
        let table_styles = &compile_info.table_styles;
        let module = &compile_info.module;
        let module_name = module.name.as_deref();
        let module_hash = module.hash_string();

        let signatures = module
            .signatures
            .iter()
            .map(|(_sig_index, func_type)| signature_to_cranelift_ir(func_type, frontend_config))
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

        let isa_ref: &dyn cranelift_codegen::isa::TargetIsa = &*isa;

        // Compiles a single function and emits its relocatable object file.
        let compile_function = |func_translator: &mut FuncTranslator,
                                i: &LocalFunctionIndex,
                                input: &FunctionBodyData|
         -> Result<PathBuf, CompileError> {
            let func_index = module.func_index(*i);
            let mut context = Context::new();
            let mut func_env = FuncEnvironment::new(
                isa_ref.frontend_config(),
                module,
                &signatures,
                signature_hashes,
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
                    &CompiledKind::Local(*i, module.get_function_name(func_index)),
                    &module_hash,
                    context.func.display().to_string().as_bytes(),
                );
            }

            let mut code_buf: Vec<u8> = Vec::new();
            let mut ctrl_plane = Default::default();
            let func_name_map = context.func.params.user_named_funcs().clone();
            let result = context
                .compile(isa_ref, &mut ctrl_plane)
                .map_err(|error| CompileError::Codegen(format!("{error:#?}")))?;
            code_buf.extend_from_slice(result.code_buffer());

            if let Some(callbacks) = self.config.callbacks.as_ref() {
                use wasmer_compiler::misc::CompiledKind;

                callbacks.obj_memory_buffer(
                    &CompiledKind::Local(*i, module.get_function_name(func_index)),
                    &module_hash,
                    &code_buf,
                );
                callbacks.asm_memory_buffer(
                    &CompiledKind::Local(*i, module.get_function_name(func_index)),
                    &module_hash,
                    triple.architecture,
                    &code_buf,
                )?;
            }

            let relocations = result
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

            // Build the LSDA (`.gcc_except_table` payload) for exception
            // handling. This consumes the last borrow of `result`.
            #[cfg(feature = "unwind")]
            let lsda = if emit_eh_frame {
                crate::eh::build_function_lsda(
                    result.buffer.call_sites(),
                    code_buf.len(),
                    pointer_bytes,
                )
            } else {
                None
            };

            #[cfg(feature = "unwind")]
            let fde = if emit_eh_frame {
                match compiled_function_unwind_info(isa_ref, &context)? {
                    CraneliftUnwindInfo::Fde(fde) => {
                        use wasmer_compiler::dwarf::WriterRelocate;

                        Some(fde.to_fde(Address::Symbol {
                            // References this function's own text symbol.
                            symbol: WriterRelocate::FUNCTION_SYMBOL,
                            addend: 0,
                        }))
                    }
                    _ => None,
                }
            } else {
                None
            };

            let range = reader.range();
            let address_map = get_function_address_map(&context, range, code_buf.len());

            let function_name = module
                .function_names
                .get(&func_index)
                .cloned()
                .unwrap_or_else(|| "<unnamed>".to_string());

            let compiled = CraneliftCompiledFunction {
                body: code_buf,
                relocations,
                traps,
                address_map,
                #[cfg(feature = "unwind")]
                fde,
                #[cfg(feature = "unwind")]
                lsda,
            };

            let object_path =
                build_dir.join(CompiledKind::Local(*i, function_name.clone()).object_filename());
            emit_function_object(
                isa_ref,
                triple,
                *i,
                &function_name,
                module_name,
                &compiled,
                object_path,
            )
        };

        let object_files = {
            let buckets =
                build_function_buckets(&function_body_inputs, WASM_LARGE_FUNCTION_THRESHOLD / 3);
            let largest_bucket = buckets.first().map(|b| b.size).unwrap_or_default();
            tracing::debug!(buckets = buckets.len(), largest_bucket, "buckets built");
            let num_threads = self.config.num_threads.get();
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(num_threads)
                .build()
                .map_err(|e| {
                    CompileError::Codegen(format!("failed to build rayon thread pool: {e}"))
                })?;

            translate_function_buckets(
                &pool,
                || FuncTranslator::new(self.config.allow_experimental_unaligned_memory_accesses),
                |func_translator, i, input| compile_function(func_translator, i, input),
                progress.clone(),
                &buckets,
            )?
        };

        let save_object =
            |obj: Object<'static>, filename: String| -> Result<PathBuf, CompileError> {
                let object_path = build_dir.join(&filename);
                let mut object_file = OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&object_path)
                    .map_err(|e| {
                        CompileError::Codegen(format!("failed to create Wasmer object file: {e}"))
                    })?;
                obj.write_stream(&mut object_file).map_err(|e| {
                    CompileError::Codegen(format!("failed to write Wasmer object file: {e}"))
                })?;
                Ok(object_path)
            };

        // Function call trampolines (only for local functions, by signature).
        let trampoline_object_files = module
            .signatures
            .iter()
            .collect::<Vec<_>>()
            .par_iter()
            .map_init(FunctionBuilderContext::new, |cx, (index, func_type)| {
                let kind = CompiledKind::FunctionCallTrampoline(*index, (*func_type).clone());
                let trampoline = make_trampoline_function_call(
                    &self.config().callbacks,
                    isa_ref,
                    triple.architecture,
                    cx,
                    &kind,
                    func_type,
                    &module_hash,
                )?;
                if let Some(progress) = progress.as_ref() {
                    progress.notify_steps(WASM_TRAMPOLINE_ESTIMATED_BODY_SIZE)?;
                }
                let obj = emit_trampoline_object(triple, &kind.linkage_name(), &trampoline)?;
                save_object(obj, kind.object_filename())
            })
            .collect::<Result<Vec<_>, CompileError>>()?;

        // Dynamic function trampolines (only for imported functions).
        let offsets = VMOffsets::new_for_trampolines(pointer_bytes);
        let dynamic_trampoline_object_files = module
            .imported_function_types()
            .enumerate()
            .collect::<Vec<_>>()
            .par_iter()
            .map_init(FunctionBuilderContext::new, |cx, (index, func_type)| {
                let kind = CompiledKind::DynamicFunctionTrampoline(
                    FunctionIndex::new(*index),
                    func_type.clone(),
                );
                let trampoline = make_trampoline_dynamic_function(
                    &self.config().callbacks,
                    isa_ref,
                    triple.architecture,
                    &offsets,
                    cx,
                    &kind,
                    func_type,
                    &module_hash,
                )?;
                if let Some(progress) = progress.as_ref() {
                    progress.notify_steps(WASM_TRAMPOLINE_ESTIMATED_BODY_SIZE)?;
                }
                let obj = emit_trampoline_object(triple, &kind.linkage_name(), &trampoline)?;
                save_object(obj, kind.object_filename())
            })
            .collect::<Result<Vec<_>, CompileError>>()?;

        let module_file = tempfile::Builder::new()
            .prefix("wasmer-image")
            .suffix(".so")
            .tempfile()
            .map_err(|err| CompileError::Codegen(format!("cannot create temporary file: {err}")))?;

        serializable.compile_info.function_max_stack_usage = function_body_inputs
            .keys()
            .map(|_| None)
            .collect::<PrimaryMap<LocalFunctionIndex, Option<usize>>>();
        let compile_info_blob = serializable
            .serialize()
            .map_err(|e| CompileError::Codegen(format!("cannot serialize SerializeModule: {e}")))?;

        let module_file = emit_metadata_and_link(
            target,
            compile_info_blob,
            build_dir,
            module_file,
            &CompiledObjects {
                object_files: &object_files,
                import_trampoline_object_files: &[],
                trampoline_object_files: &trampoline_object_files,
                dynamic_trampoline_object_files: &dynamic_trampoline_object_files,
            },
            self.config
                .callbacks
                .as_ref()
                .map(|callbacks| callbacks.debug_dir.clone()),
            module_hash,
        )?;

        Ok((module_file, serializable))
    }
}

/// Serialize a single compiled function into its own relocatable object file.
#[allow(unused_variables)]
fn emit_function_object(
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    triple: &Triple,
    local_func_index: LocalFunctionIndex,
    function_name: &str,
    module_name: Option<&str>,
    compiled: &CraneliftCompiledFunction,
    object_path: PathBuf,
) -> Result<PathBuf, CompileError> {
    let mut obj = get_object_for_target(triple)
        .map_err(|e| CompileError::Codegen(format!("cannot create object: {e}")))?;

    let kind = CompiledKind::Local(local_func_index, function_name.to_string());
    // Emit the function body into the text section.
    let function_symbol = obj.add_symbol(ObjectSymbol {
        name: kind.linkage_name().into(),
        value: 0,
        size: compiled.body.len() as u64,
        kind: SymbolKind::Text,
        scope: SymbolScope::Linkage,
        weak: false,
        section: SymbolSection::Undefined,
        flags: SymbolFlags::None,
    });
    let text_section = obj.section_id(StandardSection::Text);
    let body_offset = obj.add_symbol_data(function_symbol, text_section, &compiled.body, 16);

    // Apply the function's relocations, lazily declaring referenced symbols.
    let mut referenced_symbols: HashMap<String, object::write::SymbolId> = HashMap::new();
    for r in &compiled.relocations {
        let flags = relocation_to_flags(obj.format(), triple, r.kind)?;
        let (symbol, addend) = match &r.reloc_target {
            RelocationTarget::LocalFunc(index) => {
                let name = CompiledKind::Local(*index, String::new()).linkage_name();
                let symbol = *referenced_symbols.entry(name.clone()).or_insert_with(|| {
                    obj.add_symbol(ObjectSymbol {
                        name: name.into_bytes(),
                        value: 0,
                        size: 0,
                        kind: SymbolKind::Text,
                        scope: SymbolScope::Linkage,
                        weak: false,
                        section: SymbolSection::Undefined,
                        flags: SymbolFlags::None,
                    })
                });
                (symbol, r.addend)
            }
            RelocationTarget::LibCall(libcall) => {
                let mut name = libcall.to_function_name().to_string();
                if matches!(triple.binary_format, BinaryFormat::Macho) {
                    name = format!("_{name}");
                }
                let symbol = *referenced_symbols.entry(name.clone()).or_insert_with(|| {
                    obj.add_symbol(ObjectSymbol {
                        name: name.into_bytes(),
                        value: 0,
                        size: 0,
                        kind: SymbolKind::Unknown,
                        scope: SymbolScope::Dynamic,
                        weak: false,
                        section: SymbolSection::Undefined,
                        flags: SymbolFlags::None,
                    })
                });
                (symbol, r.addend)
            }
        };

        obj.add_relocation(
            text_section,
            ObjectRelocation {
                offset: body_offset + r.offset as u64,
                symbol,
                addend,
                flags,
            },
        )
        .map_err(|e| CompileError::Codegen(format!("failed to add function relocation: {e}")))?;
    }

    // Populate DWARF line info from the address map.
    #[cfg(feature = "unwind")]
    if let Ok(mut dwarf_state) = init_dwarf_unit(function_name, module_name, "Wasmer (Cranelift)") {
        emit_dwarf_lines(
            &mut dwarf_state,
            &mut obj,
            function_symbol,
            &compiled.address_map,
            compiled.body.len() as u64,
        )?;
    }

    // Emit the per-function trap table.
    let mut trap_data = Vec::with_capacity(compiled.traps.len() * 8 + size_of::<u32>());
    trap_data.extend_from_slice(&(compiled.traps.len() as u32).to_le_bytes());
    for trap in &compiled.traps {
        trap_data.extend_from_slice(&trap.code_offset.to_le_bytes());
        trap_data.extend_from_slice(&(trap.trap_code as u32).to_le_bytes());
    }
    let traps_section = obj.add_section(
        obj.segment_name(StandardSegment::Data).to_vec(),
        WASMER_TRAPS_SECTION_NAME.to_vec(),
        SectionKind::Other,
    );
    let trap_symbol = obj.add_symbol(ObjectSymbol {
        name: kind.traps_name().into(),
        value: 0,
        size: trap_data.len() as u64,
        kind: SymbolKind::Data,
        scope: SymbolScope::Linkage,
        weak: true,
        section: SymbolSection::Section(traps_section),
        flags: SymbolFlags::None,
    });
    obj.add_symbol_data(trap_symbol, traps_section, &trap_data, 4);

    // Emit the per-function `.eh_frame` unwind table (and, for functions that
    // catch exceptions, the matching `.gcc_except_table` LSDA).
    #[cfg(feature = "unwind")]
    if let Some(fde) = &compiled.fde
        && let Some(mut cie) = isa.create_systemv_cie()
    {
        let pointer_bytes = isa.frontend_config().pointer_bytes();

        // Emit the LSDA into `.gcc_except_table`, plus a per-object tag section
        // holding the exception tag constants referenced by its type table.
        let lsda_section_symbol = if let Some(lsda) = &compiled.lsda {
            let tag_section_symbol = emit_eh_tag_section(&mut obj, lsda);

            let gcc_section = obj.add_section(
                obj.segment_name(StandardSegment::Data).to_vec(),
                b".gcc_except_table".to_vec(),
                SectionKind::ReadOnlyData,
            );
            let lsda_offset =
                obj.append_section_data(gcc_section, &lsda.bytes, u64::from(pointer_bytes));
            // The type-table slots use `DW_EH_PE_pcrel | sdata4` encoding, so
            // their relocations are PC-relative 32-bit (`R_X86_64_PC32`). This
            // keeps `.gcc_except_table` position-independent and read-only.
            let pcrel32 = RelocationFlags::Generic {
                kind: ObjectRelocationKind::Relative,
                encoding: RelocationEncoding::Generic,
                size: 32,
            };
            for reloc in &lsda.relocations {
                let (tag_symbol, tag_offset) = tag_section_symbol
                    .as_ref()
                    .and_then(|(symbol, offsets)| {
                        offsets.get(&reloc.tag).map(|offset| (*symbol, *offset))
                    })
                    .ok_or_else(|| {
                        CompileError::Codegen(format!(
                            "missing exception tag {} for LSDA relocation",
                            reloc.tag
                        ))
                    })?;
                obj.add_relocation(
                    gcc_section,
                    ObjectRelocation {
                        offset: lsda_offset + reloc.offset as u64,
                        flags: pcrel32,
                        symbol: tag_symbol,
                        addend: tag_offset as i64,
                    },
                )
                .map_err(|e| {
                    CompileError::Codegen(format!("failed to add LSDA relocation: {e}"))
                })?;
            }
            Some(obj.section_symbol(gcc_section))
        } else {
            None
        };

        cie.fde_address_encoding = DW_EH_PE_pcrel | DW_EH_PE_sdata4;
        let mut fde = fde.clone();
        if lsda_section_symbol.is_some() {
            // The personality routine is an undefined symbol resolved at load
            // time. Reference it GOT-indirect (PC-relative) so the linker emits
            // a GOT slot with a dynamic relocation the runtime loader applies; a
            // plain data relocation against an undefined symbol would be
            // dropped. The LSDA lives in the same image and is referenced
            // directly, PC-relative.
            cie.personality = Some((
                DW_EH_PE_indirect | DW_EH_PE_pcrel | DW_EH_PE_sdata4,
                Address::Symbol {
                    symbol: wasmer_compiler::dwarf::WriterRelocate::PERSONALITY_SYMBOL,
                    addend: 0,
                },
            ));
            cie.lsda_encoding = Some(DW_EH_PE_pcrel | DW_EH_PE_sdata4);
            fde.lsda = Some(Address::Symbol {
                symbol: wasmer_compiler::dwarf::WriterRelocate::LSDA_SYMBOL,
                addend: 0,
            });
        }

        let mut frametable = FrameTable::default();
        let cie_id = frametable.add_cie(cie);
        frametable.add_fde(cie_id, fde);

        let mut eh_frame = EhFrame(wasmer_compiler::dwarf::WriterRelocate::new());
        frametable
            .write_eh_frame(&mut eh_frame)
            .map_err(|e| CompileError::Codegen(format!("failed to write eh_frame: {e}")))?;

        let section_id = obj.add_section(
            obj.segment_name(StandardSegment::Debug).to_vec(),
            b".eh_frame".to_vec(),
            SectionKind::Other,
        );
        let eh_relocs = eh_frame.0.relocs.clone();
        let data_offset = obj.append_section_data(section_id, &eh_frame.0.into_bytes(), 4);

        // The personality symbol is added lazily, the first time a relocation
        // references it.
        let mut personality_symbol = None;
        for reloc in &eh_relocs {
            let symbol = match reloc.target {
                EhTarget::Function => function_symbol,
                EhTarget::Personality => *personality_symbol.get_or_insert_with(|| {
                    let mut name = LibCall::EHPersonality.to_function_name().to_string();
                    if matches!(triple.binary_format, BinaryFormat::Macho) {
                        name = format!("_{name}");
                    }
                    obj.add_symbol(ObjectSymbol {
                        name: name.into_bytes(),
                        value: 0,
                        size: 0,
                        kind: SymbolKind::Unknown,
                        scope: SymbolScope::Dynamic,
                        weak: false,
                        section: SymbolSection::Undefined,
                        flags: SymbolFlags::None,
                    })
                }),
                EhTarget::Lsda => lsda_section_symbol.ok_or_else(|| {
                    CompileError::Codegen(
                        "eh_frame references an LSDA but none was emitted".to_string(),
                    )
                })?,
            };
            obj.add_relocation(
                section_id,
                ObjectRelocation {
                    offset: data_offset + reloc.offset,
                    flags: RelocationFlags::Generic {
                        kind: reloc.kind,
                        encoding: RelocationEncoding::Generic,
                        size: u8::checked_mul(reloc.size, 8).unwrap_or(64),
                    },
                    symbol,
                    addend: reloc.addend,
                },
            )
            .map_err(|e| {
                CompileError::Codegen(format!("failed to add eh_frame relocation: {e}"))
            })?;
        }
    }

    let mut object_file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&object_path)
        .map_err(|e| {
            CompileError::Codegen(format!(
                "failed to create Wasmer object {}: {e}",
                object_path.display()
            ))
        })?;
    obj.write_stream(&mut object_file).map_err(|e| {
        CompileError::Codegen(format!(
            "failed to write Wasmer object {}: {e}",
            object_path.display(),
        ))
    })?;

    Ok(object_path)
}

/// Emit a per-object section holding the exception tag constants referenced by
/// a function's LSDA type table, returning a section symbol and a tag->offset
/// map. Returns `None` when the LSDA references no tags.
#[cfg(feature = "unwind")]
fn emit_eh_tag_section(
    obj: &mut Object<'static>,
    lsda: &crate::eh::FunctionLsdaData,
) -> Option<(object::write::SymbolId, HashMap<u32, u32>)> {
    let mut tags: Vec<u32> = lsda.relocations.iter().map(|r| r.tag).collect();
    tags.sort_unstable();
    tags.dedup();
    if tags.is_empty() {
        return None;
    }

    let mut bytes = Vec::with_capacity(tags.len() * size_of::<u32>());
    let mut offsets = HashMap::new();
    for tag in tags {
        offsets.insert(tag, bytes.len() as u32);
        bytes.extend_from_slice(&tag.to_ne_bytes());
    }

    let section = obj.add_section(
        obj.segment_name(StandardSegment::Data).to_vec(),
        b".wasmer.eh_tags".to_vec(),
        SectionKind::ReadOnlyData,
    );
    obj.append_section_data(section, &bytes, 4);
    Some((obj.section_symbol(section), offsets))
}

#[cfg(feature = "unwind")]
fn emit_dwarf_lines(
    dwarf_state: &mut DwarfState,
    obj: &mut Object<'static>,
    function_symbol: object::write::SymbolId,
    address_map: &FunctionAddressMap,
    body_len: u64,
) -> Result<(), CompileError> {
    for inst in &address_map.instructions {
        dwarf_state.add_row(inst.code_offset as u64, inst.srcloc);
    }
    dwarf_state.write_sections(obj, function_symbol, body_len, None)
}

/// Serialize a trampoline's body into its own relocatable object file.
fn emit_trampoline_object(
    triple: &Triple,
    symbol_name: &str,
    trampoline: &FunctionBody,
) -> Result<Object<'static>, CompileError> {
    let mut obj = get_object_for_target(triple)
        .map_err(|e| CompileError::Codegen(format!("cannot create object: {e}")))?;
    let symbol = obj.add_symbol(ObjectSymbol {
        name: symbol_name.into(),
        value: 0,
        size: trampoline.body.len() as u64,
        kind: SymbolKind::Text,
        scope: SymbolScope::Linkage,
        weak: false,
        section: SymbolSection::Undefined,
        flags: SymbolFlags::None,
    });
    let text_section = obj.section_id(StandardSection::Text);
    obj.add_symbol_data(symbol, text_section, &trampoline.body, 16);
    Ok(obj)
}

/// Map a Cranelift relocation kind onto the corresponding object-file relocation flags.
fn relocation_to_flags(
    format: object::BinaryFormat,
    triple: &Triple,
    kind: Reloc,
) -> Result<RelocationFlags, CompileError> {
    use ObjectRelocationKind as K;
    Ok(match kind {
        Reloc::Abs4 => RelocationFlags::Generic {
            kind: K::Absolute,
            encoding: RelocationEncoding::Generic,
            size: 32,
        },
        Reloc::Abs8 => RelocationFlags::Generic {
            kind: K::Absolute,
            encoding: RelocationEncoding::Generic,
            size: 64,
        },
        Reloc::X86PCRel4 => RelocationFlags::Generic {
            kind: K::Relative,
            encoding: RelocationEncoding::Generic,
            size: 32,
        },
        Reloc::X86CallPCRel4 => RelocationFlags::Generic {
            kind: K::Relative,
            encoding: RelocationEncoding::X86Branch,
            size: 32,
        },
        Reloc::X86CallPLTRel4 => RelocationFlags::Generic {
            kind: K::PltRelative,
            encoding: RelocationEncoding::X86Branch,
            size: 32,
        },
        Reloc::X86GOTPCRel4 => RelocationFlags::Generic {
            kind: K::GotRelative,
            encoding: RelocationEncoding::Generic,
            size: 32,
        },
        Reloc::Arm64Call => match format {
            object::BinaryFormat::Elf => RelocationFlags::Elf {
                r_type: elf::R_AARCH64_CALL26,
            },
            object::BinaryFormat::MachO => RelocationFlags::MachO {
                r_type: macho::ARM64_RELOC_BRANCH26,
                r_pcrel: true,
                r_length: 32,
            },
            fmt => {
                return Err(CompileError::Codegen(format!(
                    "unsupported binary format {fmt:?}"
                )));
            }
        },
        Reloc::ElfX86_64TlsGd => RelocationFlags::Elf {
            r_type: elf::R_X86_64_TLSGD,
        },
        // For RISC-V relocations, please refer to:
        // https://github.com/riscv-non-isa/riscv-elf-psabi-doc/blob/2484f950a551c653f1823f1bd11926bf5a57fae3/riscv-elf.adoc#relocations
        Reloc::RiscvPCRelHi20 => RelocationFlags::Elf {
            r_type: elf::R_RISCV_PCREL_HI20,
        },
        Reloc::RiscvPCRelLo12I => RelocationFlags::Elf {
            r_type: elf::R_RISCV_PCREL_LO12_I,
        },
        Reloc::RiscvCallPlt => RelocationFlags::Elf {
            r_type: elf::R_RISCV_CALL_PLT,
        },
        other => {
            return Err(CompileError::Codegen(format!(
                "{} (relocation: {other:?}) is not supported",
                triple.architecture
            )));
        }
    })
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
        serializable: SerializableModule,
        module_translation_state: &ModuleTranslationState,
        function_body_inputs: PrimaryMap<LocalFunctionIndex, FunctionBodyData<'_>>,
        progress_callback: Option<&CompilationProgressCallback>,
    ) -> Result<(NamedTempFile, SerializableModule), CompileError> {
        self.compile_module_internal(
            target,
            compile_info,
            serializable,
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
) -> CraneliftRelocation {
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
    CraneliftRelocation {
        kind: *kind,
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
