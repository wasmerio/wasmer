use crate::config::LLVM;
use crate::object_file::get_frame_info;
use crate::trampoline::FuncTrampoline;
use crate::translator::FuncTranslator;
use inkwell::targets::FileType;
use loupe::MemoryUsage;
use rayon::iter::ParallelBridge;
use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};
use std::sync::Arc;
use wasmer_compiler::{
    Compilation, CompileError, CompileModuleInfo, CompiledFunctionFrameInfo, Compiler,
    CustomSection, CustomSectionProtection, Dwarf, ExperimentalNativeCompilation, FunctionBodyData,
    ModuleMiddleware, ModuleTranslationState, RelocationTarget, SectionBody, SectionIndex, Symbol,
    SymbolRegistry, Target,
};
use wasmer_types::entity::{EntityRef, PrimaryMap};
use wasmer_types::{FunctionIndex, LocalFunctionIndex, SignatureIndex};

//use std::sync::Mutex;

/// A compiler that compiles a WebAssembly module with LLVM, translating the Wasm to LLVM IR,
/// optimizing it and then translating to assembly.
#[derive(MemoryUsage)]
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

impl LLVMCompiler {
    fn compile_native_object<'data, 'module>(
        &self,
        target: &Target,
        compile_info: &'module CompileModuleInfo,
        module_translation: &ModuleTranslationState,
        function_body_inputs: &PrimaryMap<LocalFunctionIndex, FunctionBodyData<'data>>,
        symbol_registry: &dyn SymbolRegistry,
    ) -> Result<ExperimentalNativeCompilation, CompileError> {
        // TODO: https:/github.com/rayon-rs/rayon/issues/822

        let function_bodies_and_frame_infos = function_body_inputs
            .into_iter()
            .par_bridge()
            .map_init(
                || {
                    let target_machine = self.config().target_machine(target);
                    FuncTranslator::new(target_machine)
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
                    let memory_buffer = func_translator
                        .target_machine
                        .write_to_memory_buffer(&module, FileType::Object)
                        .unwrap();
                    let memory_buffer = memory_buffer.as_slice().to_vec();
                    let frame_info = get_frame_info(&memory_buffer)?;
                    Ok((memory_buffer, frame_info))
                },
            )
            .collect::<Result<Vec<_>, CompileError>>()?;

        let (function_bodies, frame_infos): (Vec<Vec<u8>>, Vec<CompiledFunctionFrameInfo>) =
            function_bodies_and_frame_infos.into_iter().unzip();
        let function_bodies = function_bodies.into_iter().par_bridge().map(|b| Ok(b));

        let frame_infos = frame_infos
            .into_iter()
            .collect::<PrimaryMap<LocalFunctionIndex, CompiledFunctionFrameInfo>>();
        let trampolines = compile_info.module.signatures.iter().par_bridge().map_init(
            || {
                let target_machine = self.config().target_machine(target);
                FuncTrampoline::new(target_machine)
            },
            |func_trampoline, (i, sig)| {
                let symbol = Symbol::FunctionCallTrampoline(i);
                let name = symbol_registry.symbol_to_name(symbol);
                let module = func_trampoline.trampoline_to_module(sig, self.config(), &name)?;
                let memory_buffer = func_trampoline
                    .target_machine
                    .write_to_memory_buffer(&module, FileType::Object)
                    .unwrap();
                Ok(memory_buffer.as_slice().to_vec())
            },
        );

        let dynamic_trampolines = compile_info.module.functions.iter().par_bridge().map_init(
            || {
                let target_machine = self.config().target_machine(target);
                (
                    FuncTrampoline::new(target_machine),
                    &compile_info.module.signatures,
                )
            },
            |(func_trampoline, signatures), (i, sig)| {
                let sig = &signatures[*sig];
                let symbol = Symbol::DynamicFunctionTrampoline(i);
                let name = symbol_registry.symbol_to_name(symbol);
                let module =
                    func_trampoline.dynamic_trampoline_to_module(sig, self.config(), &name)?;
                let memory_buffer = func_trampoline
                    .target_machine
                    .write_to_memory_buffer(&module, FileType::Object)
                    .unwrap();
                Ok(memory_buffer.as_slice().to_vec())
            },
        );

        let object_files = function_bodies
            .chain(trampolines)
            .chain(dynamic_trampolines)
            .collect::<Result<Vec<_>, CompileError>>()?;

        Ok(ExperimentalNativeCompilation {
            object_files,
            frame_infos,
        })
    }
}

impl Compiler for LLVMCompiler {
    /// Get the middlewares for this compiler
    fn get_middlewares(&self) -> &[Arc<dyn ModuleMiddleware>] {
        &self.config.middlewares
    }

    fn experimental_native_compile_module<'data, 'module>(
        &self,
        target: &Target,
        compile_info: &'module CompileModuleInfo,
        module_translation: &ModuleTranslationState,
        // The list of function bodies
        function_body_inputs: &PrimaryMap<LocalFunctionIndex, FunctionBodyData<'data>>,
        symbol_registry: &dyn SymbolRegistry,
    ) -> Option<Result<ExperimentalNativeCompilation, CompileError>> {
        Some(self.compile_native_object(
            target,
            compile_info,
            module_translation,
            function_body_inputs,
            symbol_registry,
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
            .iter()
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
                        module,
                        module_translation,
                        i,
                        input,
                        self.config(),
                        memory_styles,
                        &table_styles,
                        &ShortNames {},
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
