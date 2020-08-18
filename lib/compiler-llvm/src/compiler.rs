use crate::config::LLVM;
use crate::trampoline::FuncTrampoline;
use crate::translator::FuncTranslator;
use crate::CompiledFunctionKind;
use inkwell::context::Context;
use inkwell::memory_buffer::MemoryBuffer;
use inkwell::module::Module;
use inkwell::targets::FileType;
use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};
use wasmer_compiler::{
    Compilation, CompileError, CompileModuleInfo, Compiler, CustomSection, CustomSectionProtection,
    Dwarf, FunctionBodyData, ModuleTranslationState, RelocationTarget, SectionBody, SectionIndex,
    Symbol, SymbolRegistry, Target,
};
use wasmer_types::entity::{EntityRef, PrimaryMap};
use wasmer_types::{FunctionIndex, LocalFunctionIndex, SignatureIndex};

//use std::sync::{Arc, Mutex};

/// A compiler that compiles a WebAssembly module with LLVM, translating the Wasm to LLVM IR,
/// optimizing it and then translating to assembly.
pub struct LLVMCompiler {
    config: LLVM,
}

impl LLVMCompiler {
    /// Creates a new LLVM compiler
    pub fn new(config: &LLVM) -> LLVMCompiler {
        LLVMCompiler {
            config: config.clone(),
        }
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
        let idx = match idx.parse::<u32>() {
            Ok(v) => v,
            Err(_) => return None,
        };
        match ty.chars().nth(0).unwrap() {
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

impl LLVMCompiler {
    fn compile_native_object<'data, 'module>(
        &self,
        target: &Target,
        compile_info: &'module CompileModuleInfo,
        module_translation: &ModuleTranslationState,
        function_body_inputs: &PrimaryMap<LocalFunctionIndex, FunctionBodyData<'data>>,
        symbol_registry: &dyn SymbolRegistry,
        wasmer_metadata: &[u8],
    ) -> Result<Vec<u8>, CompileError> {
        let target_machine = self.config().target_machine(target);
        let ctx = Context::create();
        let merged_module = ctx.create_module("");

        // TODO: make these steps run in parallel instead of in three phases
        // with a serial step in between them.

        function_body_inputs
            .into_iter()
            .collect::<Vec<_>>()
            .par_iter()
            .map_init(
                || {
                    let target_machine = self.config().target_machine(target);
                    FuncTranslator::new(target_machine)
                },
                |func_translator, (i, input)| {
                    let module = func_translator.translate_to_module(
                        &compile_info.module,
                        module_translation,
                        i,
                        input,
                        self.config(),
                        &compile_info.memory_styles,
                        &compile_info.table_styles,
                        symbol_registry,
                    )?;
                    Ok(module.write_bitcode_to_memory().as_slice().to_vec())
                },
            )
            .collect::<Result<Vec<_>, CompileError>>()?
            .into_iter()
            .for_each(|bc| {
                let membuf = MemoryBuffer::create_from_memory_range(&bc, "");
                let m = Module::parse_bitcode_from_buffer(&membuf, &ctx).unwrap();
                merged_module.link_in_module(m).unwrap();
            });

        compile_info
            .module
            .signatures
            .into_iter()
            .collect::<Vec<_>>()
            .par_iter()
            .map_init(
                || {
                    let target_machine = self.config().target_machine(target);
                    FuncTrampoline::new(target_machine)
                },
                |func_trampoline, (i, sig)| {
                    let name = symbol_registry.symbol_to_name(Symbol::FunctionCallTrampoline(*i));
                    let module = func_trampoline.trampoline_to_module(sig, self.config(), &name)?;
                    Ok(module.write_bitcode_to_memory().as_slice().to_vec())
                },
            )
            .collect::<Result<Vec<_>, CompileError>>()?
            .into_iter()
            .for_each(|bc| {
                let membuf = MemoryBuffer::create_from_memory_range(&bc, "");
                let m = Module::parse_bitcode_from_buffer(&membuf, &ctx).unwrap();
                merged_module.link_in_module(m).unwrap();
            });

        compile_info
            .module
            .functions
            .into_iter()
            .collect::<Vec<_>>()
            .par_iter()
            .map_init(
                || {
                    let target_machine = self.config().target_machine(target);
                    (
                        FuncTrampoline::new(target_machine),
                        &compile_info.module.signatures,
                    )
                },
                |(func_trampoline, signatures), (i, sig)| {
                    let sig = &signatures[**sig];
                    let name =
                        symbol_registry.symbol_to_name(Symbol::DynamicFunctionTrampoline(*i));
                    let module =
                        func_trampoline.dynamic_trampoline_to_module(sig, self.config(), &name)?;
                    Ok(module.write_bitcode_to_memory().as_slice().to_vec())
                },
            )
            .collect::<Result<Vec<_>, CompileError>>()?
            .into_iter()
            .for_each(|bc| {
                let membuf = MemoryBuffer::create_from_memory_range(&bc, "");
                let m = Module::parse_bitcode_from_buffer(&membuf, &ctx).unwrap();
                merged_module.link_in_module(m).unwrap();
            });

        let i8_ty = ctx.i8_type();
        let metadata_init = i8_ty.const_array(
            wasmer_metadata
                .iter()
                .map(|v| i8_ty.const_int(*v as u64, false))
                .collect::<Vec<_>>()
                .as_slice(),
        );
        let metadata_gv =
            merged_module.add_global(metadata_init.get_type(), None, "WASMER_METADATA");
        metadata_gv.set_initializer(&metadata_init);

        if self.config().enable_verifier {
            merged_module.verify().unwrap();
        }

        let memory_buffer = target_machine
            .write_to_memory_buffer(&merged_module, FileType::Object)
            .unwrap();
        if let Some(ref callbacks) = self.config.callbacks {
            callbacks.obj_memory_buffer(
                &CompiledFunctionKind::Local(function_body_inputs.iter().next().unwrap().0),
                &memory_buffer,
            );
        }

        Ok(memory_buffer.as_slice().to_vec())
    }
}

impl Compiler for LLVMCompiler {
    fn experimental_native_compile_module<'data, 'module>(
        &self,
        target: &Target,
        module: &'module CompileModuleInfo,
        module_translation: &ModuleTranslationState,
        // The list of function bodies
        function_body_inputs: &PrimaryMap<LocalFunctionIndex, FunctionBodyData<'data>>,
        symbol_registry: &dyn SymbolRegistry,
        // The metadata to inject into the wasmer_metadata section of the object file.
        wasmer_metadata: &[u8],
    ) -> Option<Result<Vec<u8>, CompileError>> {
        Some(self.compile_native_object(
            target,
            module,
            module_translation,
            function_body_inputs,
            symbol_registry,
            wasmer_metadata,
        ))
    }

    /// Compile the module using LLVM, producing a compilation result with
    /// associated relocations.
    fn compile_module<'data, 'module>(
        &self,
        target: &Target,
        compile_info: &'module CompileModuleInfo,
        module_translation: &ModuleTranslationState,
        function_body_inputs: PrimaryMap<LocalFunctionIndex, FunctionBodyData<'data>>,
    ) -> Result<Compilation, CompileError> {
        //let data = Arc::new(Mutex::new(0));
        let memory_styles = &compile_info.memory_styles;
        let table_styles = &compile_info.table_styles;
        let module = &compile_info.module;

        // TODO: merge constants in sections.

        let mut module_custom_sections = PrimaryMap::new();
        let mut frame_section_bytes = vec![];
        let mut frame_section_relocations = vec![];
        let functions = function_body_inputs
            .into_iter()
            .collect::<Vec<(LocalFunctionIndex, &FunctionBodyData<'_>)>>()
            .par_iter()
            .map_init(
                || {
                    let target_machine = self.config().target_machine(target);
                    FuncTranslator::new(target_machine)
                },
                |func_translator, (i, input)| {
                    // TODO: remove (to serialize)
                    //let _data = data.lock().unwrap();
                    func_translator.translate(
                        &module,
                        module_translation,
                        i,
                        input,
                        self.config(),
                        memory_styles,
                        &table_styles,
                        &mut ShortNames {},
                    )
                },
            )
            .collect::<Result<Vec<_>, CompileError>>()?
            .into_iter()
            .map(|mut compiled_function| {
                let first_section = module_custom_sections.len() as u32;
                for (section_index, custom_section) in compiled_function.custom_sections.iter() {
                    // TODO: remove this call to clone()
                    let mut custom_section = custom_section.clone();
                    for mut reloc in &mut custom_section.relocations {
                        if let RelocationTarget::CustomSection(index) = reloc.reloc_target {
                            reloc.reloc_target = RelocationTarget::CustomSection(
                                SectionIndex::from_u32(first_section + index.as_u32()),
                            )
                        }
                    }
                    if compiled_function
                        .eh_frame_section_indices
                        .contains(&section_index)
                    {
                        let offset = frame_section_bytes.len() as u32;
                        for mut reloc in &mut custom_section.relocations {
                            reloc.offset += offset;
                        }
                        frame_section_bytes.extend_from_slice(custom_section.bytes.as_slice());
                        frame_section_relocations.extend(custom_section.relocations);
                        // TODO: we do this to keep the count right, remove it.
                        module_custom_sections.push(CustomSection {
                            protection: CustomSectionProtection::Read,
                            bytes: SectionBody::new_with_vec(vec![]),
                            relocations: vec![],
                        });
                    } else {
                        module_custom_sections.push(custom_section);
                    }
                }
                for mut reloc in &mut compiled_function.compiled_function.relocations {
                    if let RelocationTarget::CustomSection(index) = reloc.reloc_target {
                        reloc.reloc_target = RelocationTarget::CustomSection(
                            SectionIndex::from_u32(first_section + index.as_u32()),
                        )
                    }
                }
                compiled_function.compiled_function
            })
            .collect::<PrimaryMap<LocalFunctionIndex, _>>();

        let dwarf = if !frame_section_bytes.is_empty() {
            let dwarf = Some(Dwarf::new(SectionIndex::from_u32(
                module_custom_sections.len() as u32,
            )));
            // Terminating zero-length CIE.
            frame_section_bytes.extend(vec![
                0x00, 0x00, 0x00, 0x00, // Length
                0x00, 0x00, 0x00, 0x00, // CIE ID
                0x10, // Version (must be 1)
                0x00, // Augmentation data
                0x00, // Code alignment factor
                0x00, // Data alignment factor
                0x00, // Return address register
                0x00, 0x00, 0x00, // Padding to a multiple of 4 bytes
            ]);
            module_custom_sections.push(CustomSection {
                protection: CustomSectionProtection::Read,
                bytes: SectionBody::new_with_vec(frame_section_bytes),
                relocations: frame_section_relocations,
            });
            dwarf
        } else {
            None
        };

        let function_call_trampolines = module
            .signatures
            .values()
            .collect::<Vec<_>>()
            .par_iter()
            .map_init(
                || {
                    let target_machine = self.config().target_machine(target);
                    FuncTrampoline::new(target_machine)
                },
                |func_trampoline, sig| func_trampoline.trampoline(sig, self.config(), ""),
            )
            .collect::<Vec<_>>()
            .into_iter()
            .collect::<Result<PrimaryMap<_, _>, CompileError>>()?;

        let dynamic_function_trampolines = module
            .imported_function_types()
            .collect::<Vec<_>>()
            .par_iter()
            .map_init(
                || {
                    let target_machine = self.config().target_machine(target);
                    FuncTrampoline::new(target_machine)
                },
                |func_trampoline, func_type| {
                    func_trampoline.dynamic_trampoline(&func_type, self.config(), "")
                },
            )
            .collect::<Result<Vec<_>, CompileError>>()?
            .into_iter()
            .collect::<PrimaryMap<_, _>>();

        Ok(Compilation::new(
            functions,
            module_custom_sections,
            function_call_trampolines,
            dynamic_function_trampolines,
            dwarf,
        ))
    }
}
