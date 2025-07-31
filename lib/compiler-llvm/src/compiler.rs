use crate::config::LLVM;
use crate::trampoline::FuncTrampoline;
use crate::translator::FuncTranslator;
use crate::CompiledKind;
use inkwell::context::Context;
use inkwell::memory_buffer::MemoryBuffer;
use inkwell::module::{Linkage, Module};
use inkwell::targets::FileType;
use inkwell::DLLStorageClass;
use rayon::iter::ParallelBridge;
use rayon::prelude::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};
use rayon::ThreadPoolBuilder;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use wasmer_compiler::types::function::{Compilation, UnwindInfo};
use wasmer_compiler::types::module::CompileModuleInfo;
use wasmer_compiler::types::relocation::RelocationKind;
use wasmer_compiler::{
    types::{
        relocation::RelocationTarget,
        section::{CustomSection, CustomSectionProtection, SectionBody, SectionIndex},
        symbols::{Symbol, SymbolRegistry},
    },
    Compiler, FunctionBodyData, ModuleMiddleware, ModuleTranslationState,
};
use wasmer_types::entity::{EntityRef, PrimaryMap};
use wasmer_types::target::Target;
use wasmer_types::{CompileError, FunctionIndex, LocalFunctionIndex, ModuleInfo, SignatureIndex};
use wasmer_vm::LibCall;

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

struct ShortNames {}

impl SymbolRegistry for ShortNames {
    fn symbol_to_name(&self, symbol: Symbol) -> String {
        match symbol {
            Symbol::Metadata => "M".to_string(),
            Symbol::LocalFunction(index) => format!("f{}", index.index()),
            Symbol::Section(index) => format!("s{}", index.index()),
            Symbol::FunctionCallTrampoline(index) => format!("t{}", index.index()),
            Symbol::DynamicFunctionTrampoline(index) => format!("d{}", index.index()),
        }
    }

    fn name_to_symbol(&self, name: &str) -> Option<Symbol> {
        if name.len() < 2 {
            return None;
        }
        let (ty, idx) = name.split_at(1);
        if ty.starts_with('M') {
            return Some(Symbol::Metadata);
        }

        let idx = idx.parse::<u32>().ok()?;
        match ty.chars().next().unwrap() {
            'f' => Some(Symbol::LocalFunction(LocalFunctionIndex::from_u32(idx))),
            's' => Some(Symbol::Section(SectionIndex::from_u32(idx))),
            't' => Some(Symbol::FunctionCallTrampoline(SignatureIndex::from_u32(
                idx,
            ))),
            'd' => Some(Symbol::DynamicFunctionTrampoline(FunctionIndex::from_u32(
                idx,
            ))),
            _ => None,
        }
    }
}

struct ModuleBasedSymbolRegistry {
    wasm_module: Arc<ModuleInfo>,
    local_func_names: HashMap<String, LocalFunctionIndex>,
    short_names: ShortNames,
}

impl ModuleBasedSymbolRegistry {
    fn new(wasm_module: Arc<ModuleInfo>) -> Self {
        let local_func_names = HashMap::from_iter(
            wasm_module
                .function_names
                .iter()
                .map(|(f, v)| (wasm_module.local_func_index(*f), v))
                .filter(|(f, _)| f.is_some())
                .map(|(f, v)| (v.clone(), f.unwrap())),
        );
        Self {
            wasm_module,
            local_func_names,
            short_names: ShortNames {},
        }
    }
}

impl SymbolRegistry for ModuleBasedSymbolRegistry {
    fn symbol_to_name(&self, symbol: Symbol) -> String {
        match symbol {
            Symbol::LocalFunction(index) => self
                .wasm_module
                .function_names
                .get(&self.wasm_module.func_index(index))
                .cloned()
                .unwrap_or(self.short_names.symbol_to_name(symbol)),
            _ => self.short_names.symbol_to_name(symbol),
        }
    }

    fn name_to_symbol(&self, name: &str) -> Option<Symbol> {
        if let Some(idx) = self.local_func_names.get(name) {
            Some(Symbol::LocalFunction(*idx))
        } else {
            self.short_names.name_to_symbol(name)
        }
    }
}

impl LLVMCompiler {
    #[allow(clippy::too_many_arguments)]
    fn compile_native_object(
        &self,
        target: &Target,
        compile_info: &CompileModuleInfo,
        module_translation: &ModuleTranslationState,
        function_body_inputs: &PrimaryMap<LocalFunctionIndex, FunctionBodyData<'_>>,
        symbol_registry: &dyn SymbolRegistry,
        wasmer_metadata: &[u8],
        binary_format: target_lexicon::BinaryFormat,
    ) -> Result<Vec<u8>, CompileError> {
        let target_machine = self.config().target_machine(target);
        let ctx = Context::create();

        // TODO: https:/github.com/rayon-rs/rayon/issues/822

        let merged_bitcode = function_body_inputs.into_iter().par_bridge().map_init(
            || {
                let target_machine = self.config().target_machine(target);
                FuncTranslator::new(target_machine, binary_format).unwrap()
            },
            |func_translator, (i, input)| {
                let module = func_translator.translate_to_module(
                    &compile_info.module,
                    module_translation,
                    &i,
                    input,
                    self.config(),
                    &compile_info.memory_styles,
                    &compile_info.table_styles,
                    symbol_registry,
                )?;

                Ok(module.write_bitcode_to_memory().as_slice().to_vec())
            },
        );

        let trampolines_bitcode = compile_info.module.signatures.iter().par_bridge().map_init(
            || {
                let target_machine = self.config().target_machine(target);
                FuncTrampoline::new(target_machine, binary_format).unwrap()
            },
            |func_trampoline, (i, sig)| {
                let name = symbol_registry.symbol_to_name(Symbol::FunctionCallTrampoline(i));
                let module = func_trampoline.trampoline_to_module(
                    sig,
                    self.config(),
                    &name,
                    compile_info,
                )?;
                Ok(module.write_bitcode_to_memory().as_slice().to_vec())
            },
        );

        let dynamic_trampolines_bitcode =
            compile_info.module.functions.iter().par_bridge().map_init(
                || {
                    let target_machine = self.config().target_machine(target);
                    (
                        FuncTrampoline::new(target_machine, binary_format).unwrap(),
                        &compile_info.module.signatures,
                    )
                },
                |(func_trampoline, signatures), (i, sig)| {
                    let sig = &signatures[*sig];
                    let name = symbol_registry.symbol_to_name(Symbol::DynamicFunctionTrampoline(i));
                    let module =
                        func_trampoline.dynamic_trampoline_to_module(sig, self.config(), &name)?;
                    Ok(module.write_bitcode_to_memory().as_slice().to_vec())
                },
            );

        let merged_bitcode = merged_bitcode
            .chain(trampolines_bitcode)
            .chain(dynamic_trampolines_bitcode)
            .collect::<Result<Vec<_>, CompileError>>()?
            .into_par_iter()
            .reduce_with(|bc1, bc2| {
                let ctx = Context::create();
                let membuf = MemoryBuffer::create_from_memory_range(&bc1, "");
                let m1 = Module::parse_bitcode_from_buffer(&membuf, &ctx).unwrap();
                let membuf = MemoryBuffer::create_from_memory_range(&bc2, "");
                let m2 = Module::parse_bitcode_from_buffer(&membuf, &ctx).unwrap();
                m1.link_in_module(m2).unwrap();
                m1.write_bitcode_to_memory().as_slice().to_vec()
            });
        let merged_module = if let Some(bc) = merged_bitcode {
            let membuf = MemoryBuffer::create_from_memory_range(&bc, "");
            Module::parse_bitcode_from_buffer(&membuf, &ctx).unwrap()
        } else {
            ctx.create_module("")
        };

        let i8_ty = ctx.i8_type();
        let metadata_init = i8_ty.const_array(
            wasmer_metadata
                .iter()
                .map(|v| i8_ty.const_int(*v as u64, false))
                .collect::<Vec<_>>()
                .as_slice(),
        );
        let metadata_gv = merged_module.add_global(
            metadata_init.get_type(),
            None,
            &symbol_registry.symbol_to_name(wasmer_compiler::types::symbols::Symbol::Metadata),
        );
        metadata_gv.set_initializer(&metadata_init);
        metadata_gv.set_linkage(Linkage::DLLExport);
        metadata_gv.set_dll_storage_class(DLLStorageClass::Export);
        metadata_gv.set_alignment(16);

        if self.config().enable_verifier {
            merged_module.verify().unwrap();
        }

        let memory_buffer = target_machine
            .write_to_memory_buffer(&merged_module, FileType::Object)
            .unwrap();
        if let Some(ref callbacks) = self.config.callbacks {
            callbacks.obj_memory_buffer(&CompiledKind::Module, &memory_buffer);
        }

        tracing::trace!("Finished compling the module!");
        Ok(memory_buffer.as_slice().to_vec())
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
        let mut ret = format!(
            "llvm-{}",
            match self.config.opt_level {
                inkwell::OptimizationLevel::None => "opt0",
                inkwell::OptimizationLevel::Less => "optl",
                inkwell::OptimizationLevel::Default => "optd",
                inkwell::OptimizationLevel::Aggressive => "opta",
            }
        );

        if self.config.enable_g0m0_opt {
            ret.push_str("-g0m0");
        }

        ret
    }

    /// Get the middlewares for this compiler
    fn get_middlewares(&self) -> &[Arc<dyn ModuleMiddleware>] {
        &self.config.middlewares
    }

    fn experimental_native_compile_module(
        &self,
        target: &Target,
        compile_info: &CompileModuleInfo,
        module_translation: &ModuleTranslationState,
        // The list of function bodies
        function_body_inputs: &PrimaryMap<LocalFunctionIndex, FunctionBodyData<'_>>,
        symbol_registry: &dyn SymbolRegistry,
        // The metadata to inject into the wasmer_metadata section of the object file.
        wasmer_metadata: &[u8],
    ) -> Option<Result<Vec<u8>, CompileError>> {
        Some(self.compile_native_object(
            target,
            compile_info,
            module_translation,
            function_body_inputs,
            symbol_registry,
            wasmer_metadata,
            self.config.target_binary_format(target),
        ))
    }

    /// Compile the module using LLVM, producing a compilation result with
    /// associated relocations.
    fn compile_module(
        &self,
        target: &Target,
        compile_info: &CompileModuleInfo,
        module_translation: &ModuleTranslationState,
        function_body_inputs: PrimaryMap<LocalFunctionIndex, FunctionBodyData<'_>>,
    ) -> Result<Compilation, CompileError> {
        //let data = Arc::new(Mutex::new(0));

        let memory_styles = &compile_info.memory_styles;
        let table_styles = &compile_info.table_styles;
        let binary_format = self.config.target_binary_format(target);

        let module = &compile_info.module;

        // TODO: merge constants in sections.

        let mut module_custom_sections = PrimaryMap::new();

        let mut eh_frame_section_bytes = vec![];
        let mut eh_frame_section_relocations = vec![];

        let mut compact_unwind_section_bytes = vec![];
        let mut compact_unwind_section_relocations = vec![];

        let mut got_targets: HashSet<wasmer_compiler::types::relocation::RelocationTarget> = if matches!(
            target.triple().binary_format,
            target_lexicon::BinaryFormat::Macho
        ) {
            HashSet::from_iter(vec![RelocationTarget::LibCall(LibCall::EHPersonality)])
        } else {
            HashSet::default()
        };

        let symbol_registry = ModuleBasedSymbolRegistry::new(module.clone());

        let functions = if self.config.num_threads.get() > 1 {
            let pool = ThreadPoolBuilder::new()
                .num_threads(self.config.num_threads.get())
                .build()
                .map_err(|e| CompileError::Resource(e.to_string()))?;
            pool.install(|| {
                function_body_inputs
                    .iter()
                    .collect::<Vec<(LocalFunctionIndex, &FunctionBodyData<'_>)>>()
                    .par_iter()
                    .map_init(
                        || {
                            let target_machine = self.config().target_machine(target);
                            FuncTranslator::new(target_machine, binary_format).unwrap()
                        },
                        |func_translator, (i, input)| {
                            // TODO: remove (to serialize)
                            //let _data = data.lock().unwrap();

                            func_translator.translate(
                                module,
                                module_translation,
                                i,
                                input,
                                self.config(),
                                memory_styles,
                                table_styles,
                                &symbol_registry,
                            )
                        },
                    )
                    .collect::<Result<Vec<_>, CompileError>>()
            })?
        } else {
            let target_machine = self.config().target_machine(target);
            let func_translator = FuncTranslator::new(target_machine, binary_format).unwrap();

            function_body_inputs
                .iter()
                .collect::<Vec<(LocalFunctionIndex, &FunctionBodyData<'_>)>>()
                .into_iter()
                .map(|(i, input)| {
                    // TODO: remove (to serialize)
                    //let _data = data.lock().unwrap();

                    func_translator.translate(
                        module,
                        module_translation,
                        &i,
                        input,
                        self.config(),
                        memory_styles,
                        table_styles,
                        &symbol_registry,
                    )
                })
                .collect::<Result<Vec<_>, CompileError>>()?
        };

        let functions = functions
            .into_iter()
            .map(|mut compiled_function| {
                let first_section = module_custom_sections.len() as u32;
                for (section_index, custom_section) in compiled_function.custom_sections.iter() {
                    // TODO: remove this call to clone()
                    let mut custom_section = custom_section.clone();
                    for reloc in &mut custom_section.relocations {
                        if let RelocationTarget::CustomSection(index) = reloc.reloc_target {
                            reloc.reloc_target = RelocationTarget::CustomSection(
                                SectionIndex::from_u32(first_section + index.as_u32()),
                            )
                        }

                        if reloc.kind.needs_got() {
                            got_targets.insert(reloc.reloc_target);
                        }
                    }

                    if compiled_function
                        .eh_frame_section_indices
                        .contains(&section_index)
                    {
                        let offset = eh_frame_section_bytes.len() as u32;
                        for reloc in &mut custom_section.relocations {
                            reloc.offset += offset;
                        }
                        eh_frame_section_bytes.extend_from_slice(custom_section.bytes.as_slice());
                        eh_frame_section_relocations.extend(custom_section.relocations);
                        // TODO: we do this to keep the count right, remove it.
                        module_custom_sections.push(CustomSection {
                            protection: CustomSectionProtection::Read,
                            alignment: None,
                            bytes: SectionBody::new_with_vec(vec![]),
                            relocations: vec![],
                        });
                    } else if compiled_function
                        .compact_unwind_section_indices
                        .contains(&section_index)
                    {
                        let offset = compact_unwind_section_bytes.len() as u32;
                        for reloc in &mut custom_section.relocations {
                            reloc.offset += offset;
                        }
                        compact_unwind_section_bytes
                            .extend_from_slice(custom_section.bytes.as_slice());
                        compact_unwind_section_relocations.extend(custom_section.relocations);
                        // TODO: we do this to keep the count right, remove it.
                        module_custom_sections.push(CustomSection {
                            protection: CustomSectionProtection::Read,
                            alignment: None,
                            bytes: SectionBody::new_with_vec(vec![]),
                            relocations: vec![],
                        });
                    } else {
                        module_custom_sections.push(custom_section);
                    }
                }
                for reloc in &mut compiled_function.compiled_function.relocations {
                    if let RelocationTarget::CustomSection(index) = reloc.reloc_target {
                        reloc.reloc_target = RelocationTarget::CustomSection(
                            SectionIndex::from_u32(first_section + index.as_u32()),
                        )
                    }

                    if reloc.kind.needs_got() {
                        got_targets.insert(reloc.reloc_target);
                    }
                }
                compiled_function.compiled_function
            })
            .collect::<PrimaryMap<LocalFunctionIndex, _>>();

        let mut unwind_info = UnwindInfo::default();

        if !eh_frame_section_bytes.is_empty() {
            let eh_frame_idx = SectionIndex::from_u32(module_custom_sections.len() as u32);
            // Terminate dwarf info with a zero-length CIE.
            // This is ok, even though more info will be added later
            // in lib/object/src/module.rs emit_compilation, but that won't be called on all code paths.
            eh_frame_section_bytes.extend_from_slice(&[0, 0, 0, 0]);
            module_custom_sections.push(CustomSection {
                protection: CustomSectionProtection::Read,
                alignment: None,
                bytes: SectionBody::new_with_vec(eh_frame_section_bytes),
                relocations: eh_frame_section_relocations,
            });
            unwind_info.eh_frame = Some(eh_frame_idx);
        }

        if !compact_unwind_section_bytes.is_empty() {
            let cu_index = SectionIndex::from_u32(module_custom_sections.len() as u32);
            module_custom_sections.push(CustomSection {
                protection: CustomSectionProtection::Read,
                alignment: None,
                bytes: SectionBody::new_with_vec(compact_unwind_section_bytes),
                relocations: compact_unwind_section_relocations,
            });
            unwind_info.compact_unwind = Some(cu_index);
        }

        let function_call_trampolines = if self.config.num_threads.get() > 1 {
            let pool = ThreadPoolBuilder::new()
                .num_threads(self.config.num_threads.get())
                .build()
                .map_err(|e| CompileError::Resource(e.to_string()))?;
            pool.install(|| {
                module
                    .signatures
                    .values()
                    .collect::<Vec<_>>()
                    .par_iter()
                    .map_init(
                        || {
                            let target_machine = self.config().target_machine(target);
                            FuncTrampoline::new(target_machine, binary_format).unwrap()
                        },
                        |func_trampoline, sig| {
                            func_trampoline.trampoline(sig, self.config(), "", compile_info)
                        },
                    )
                    .collect::<Vec<_>>()
                    .into_iter()
                    .collect::<Result<PrimaryMap<_, _>, CompileError>>()
            })?
        } else {
            let target_machine = self.config().target_machine(target);
            let func_trampoline = FuncTrampoline::new(target_machine, binary_format).unwrap();
            module
                .signatures
                .values()
                .collect::<Vec<_>>()
                .into_iter()
                .map(|sig| func_trampoline.trampoline(sig, self.config(), "", compile_info))
                .collect::<Vec<_>>()
                .into_iter()
                .collect::<Result<PrimaryMap<_, _>, CompileError>>()?
        };

        let dynamic_function_trampolines = if self.config.num_threads.get() > 1 {
            let pool = ThreadPoolBuilder::new()
                .num_threads(self.config.num_threads.get())
                .build()
                .map_err(|e| CompileError::Resource(e.to_string()))?;
            pool.install(|| {
                module
                    .imported_function_types()
                    .collect::<Vec<_>>()
                    .par_iter()
                    .map_init(
                        || {
                            let target_machine = self.config().target_machine(target);
                            FuncTrampoline::new(target_machine, binary_format).unwrap()
                        },
                        |func_trampoline, func_type| {
                            func_trampoline.dynamic_trampoline(func_type, self.config(), "")
                        },
                    )
                    .collect::<Vec<_>>()
                    .into_iter()
                    .collect::<Result<PrimaryMap<_, _>, CompileError>>()
            })?
        } else {
            let target_machine = self.config().target_machine(target);
            let func_trampoline = FuncTrampoline::new(target_machine, binary_format).unwrap();
            module
                .imported_function_types()
                .collect::<Vec<_>>()
                .into_iter()
                .map(|func_type| func_trampoline.dynamic_trampoline(&func_type, self.config(), ""))
                .collect::<Vec<_>>()
                .into_iter()
                .collect::<Result<PrimaryMap<_, _>, CompileError>>()?
        };

        let mut got = wasmer_compiler::types::function::GOT::empty();

        if !got_targets.is_empty() {
            let pointer_width = target
                .triple()
                .pointer_width()
                .map_err(|_| CompileError::Codegen("Could not get pointer width".to_string()))?;

            let got_entry_size = match pointer_width {
                target_lexicon::PointerWidth::U64 => 8,
                target_lexicon::PointerWidth::U32 => 4,
                target_lexicon::PointerWidth::U16 => todo!(),
            };

            let got_entry_reloc_kind = match pointer_width {
                target_lexicon::PointerWidth::U64 => RelocationKind::Abs8,
                target_lexicon::PointerWidth::U32 => RelocationKind::Abs4,
                target_lexicon::PointerWidth::U16 => todo!(),
            };

            let got_data: Vec<u8> = vec![0; got_targets.len() * got_entry_size];
            let mut got_relocs = vec![];

            for (i, target) in got_targets.into_iter().enumerate() {
                got_relocs.push(wasmer_compiler::types::relocation::Relocation {
                    kind: got_entry_reloc_kind,
                    reloc_target: target,
                    offset: (i * got_entry_size) as u32,
                    addend: 0,
                });
            }

            let got_idx = SectionIndex::from_u32(module_custom_sections.len() as u32);
            module_custom_sections.push(CustomSection {
                protection: CustomSectionProtection::Read,
                alignment: None,
                bytes: SectionBody::new_with_vec(got_data),
                relocations: got_relocs,
            });
            got.index = Some(got_idx);
        };

        tracing::trace!("Finished compling the module!");
        Ok(Compilation {
            functions,
            custom_sections: module_custom_sections,
            function_call_trampolines,
            dynamic_function_trampolines,
            unwind_info,
            got,
        })
    }

    fn with_opts(
        &mut self,
        suggested_compiler_opts: &wasmer_types::target::UserCompilerOptimizations,
    ) -> Result<(), CompileError> {
        if suggested_compiler_opts.pass_params.is_some_and(|v| v) {
            self.config.enable_g0m0_opt = true;
        }
        Ok(())
    }
}
