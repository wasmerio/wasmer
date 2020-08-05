use crate::config::LLVM;
use crate::trampoline::FuncTrampoline;
use crate::translator::FuncTranslator;
use inkwell::context::Context;
use inkwell::memory_buffer::MemoryBuffer;
use inkwell::module::Module;
use inkwell::targets::FileType;
use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};
use wasm_common::entity::{EntityRef, PrimaryMap};
use wasm_common::{FunctionIndex, LocalFunctionIndex, SignatureIndex};
use wasmer_compiler::{
    Compilation, CompileError, CompileModuleInfo, Compiler, CustomSection, CustomSectionProtection,
    Dwarf, FunctionBodyData, ModuleTranslationState, RelocationTarget, SectionBody, SectionIndex,
    Target,
};
use wasmer_object::CompilationNamer;

use std::collections::HashMap;

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

impl InvertibleCompilationNamer for ShortNames {
    /// Gets the function name given a local function index
    fn get_function_name(&mut self, index: &LocalFunctionIndex) -> String {
        format!("f{}", index.index())
    }

    /// Gets the section name given a section index
    fn get_section_name(&mut self, index: &SectionIndex) -> String {
        format!("s{}", index.index())
    }

    /// Gets the function call trampoline name given a signature index
    fn get_function_call_trampoline_name(&mut self, index: &SignatureIndex) -> String {
        format!("t{}", index.index())
    }

    /// Gets the dynamic function trampoline name given a function index
    fn get_dynamic_function_trampoline_name(&mut self, index: &FunctionIndex) -> String {
        format!("d{}", index.index())
    }

    fn get_symbol_from_name(&self, name: &str) -> Option<Symbol> {
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

/// Symbols we may have produced names for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Symbol {
    LocalFunction(LocalFunctionIndex),
    Section(SectionIndex),
    FunctionCallTrampoline(SignatureIndex),
    DynamicFunctionTrampoline(FunctionIndex),
}

pub trait InvertibleCompilationNamer {
    /// Gets the function name given a local function index.
    fn get_function_name(&mut self, index: &LocalFunctionIndex) -> String;

    /// Gets the section name given a section index.
    fn get_section_name(&mut self, index: &SectionIndex) -> String;

    /// Gets the function call trampoline name given a signature index.
    fn get_function_call_trampoline_name(&mut self, index: &SignatureIndex) -> String;

    /// Gets the dynamic function trampoline name given a function index.
    fn get_dynamic_function_trampoline_name(&mut self, index: &FunctionIndex) -> String;

    /// Gets the type of symbol from a given name.
    fn get_symbol_from_name(&self, name: &str) -> Option<Symbol>;
}

pub struct CachingInvertibleCompilationNamer<'a> {
    cache: HashMap<String, Symbol>,
    namer: &'a dyn CompilationNamer,
}

impl<'a> InvertibleCompilationNamer for CachingInvertibleCompilationNamer<'a> {
    fn get_function_name(&mut self, index: &LocalFunctionIndex) -> String {
        let value = self.namer.get_function_name(index);
        self.cache
            .insert(value.clone(), Symbol::LocalFunction(*index));
        value
    }

    fn get_section_name(&mut self, index: &SectionIndex) -> String {
        let value = self.namer.get_section_name(index);
        self.cache.insert(value.clone(), Symbol::Section(*index));
        value
    }

    /// Gets the function call trampoline name given a signature index
    fn get_function_call_trampoline_name(&mut self, index: &SignatureIndex) -> String {
        let value = self.namer.get_function_call_trampoline_name(index);
        self.cache
            .insert(value.clone(), Symbol::FunctionCallTrampoline(*index));
        value
    }

    /// Gets the dynamic function trampoline name given a function index
    fn get_dynamic_function_trampoline_name(&mut self, index: &FunctionIndex) -> String {
        let value = self.namer.get_dynamic_function_trampoline_name(index);
        self.cache
            .insert(value.clone(), Symbol::DynamicFunctionTrampoline(*index));
        value
    }

    fn get_symbol_from_name(&self, name: &str) -> Option<Symbol> {
        self.cache.get(name).cloned()
    }
}

impl LLVMCompiler {
    fn _compile_native_object<'data, 'module>(
        &self,
        target: &Target,
        compile_info: &'module CompileModuleInfo,
        module_translation: &ModuleTranslationState,
        function_body_inputs: PrimaryMap<LocalFunctionIndex, FunctionBodyData<'data>>,
        namer: &dyn CompilationNamer,
    ) -> Result<Vec<u8>, CompileError> {
        let target_machine = self.config().target_machine(target);
        let ctx = Context::create();
        let merged_module = ctx.create_module("");
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
                    let mut namer = CachingInvertibleCompilationNamer {
                        cache: HashMap::new(),
                        namer,
                    };
                    let module = func_translator.translate_to_module(
                        &compile_info.module,
                        module_translation,
                        i,
                        input,
                        self.config(),
                        &compile_info.memory_styles,
                        &compile_info.table_styles,
                        &mut namer,
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

        let memory_buffer = target_machine
            .write_to_memory_buffer(&merged_module, FileType::Object)
            .unwrap();

        Ok(memory_buffer.as_slice().to_vec())
    }
}

impl Compiler for LLVMCompiler {
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
                |func_trampoline, sig| func_trampoline.trampoline(sig, self.config()),
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
                    func_trampoline.dynamic_trampoline(&func_type, self.config())
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
