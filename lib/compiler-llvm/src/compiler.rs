use crate::config::LLVM;
use crate::trampoline::FuncTrampoline;
use crate::translator::FuncTranslator;
use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};
use wasm_common::entity::{EntityRef, PrimaryMap, SecondaryMap};
use wasm_common::LocalFunctionIndex;
use wasmer_compiler::{
    Compilation, CompileError, CompileModuleInfo, Compiler, FunctionBodyData,
    ModuleTranslationState, RelocationTarget, SectionIndex, Target, CustomSection,
    SectionBody, CustomSectionProtection, Dwarf,
};
use wasmer_runtime::{MemoryPlan, ModuleInfo, TablePlan};

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
        let mut func_names = SecondaryMap::new();
        let memory_plans = &compile_info.memory_plans;
        let table_plans = &compile_info.table_plans;
        let module = &compile_info.module;

        // TODO: merge constants in sections.

        for (func_index, _) in &module.functions {
            func_names[func_index] = module
                .func_names
                .get(&func_index)
                .cloned()
                .unwrap_or_else(|| format!("fn{}", func_index.index()));
        }
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
                    //let mut data = data.lock().unwrap();
                    func_translator.translate(
                        &module,
                        module_translation,
                        i,
                        input,
                        self.config(),
                        &memory_plans,
                        &table_plans,
                        &func_names,
                    )
                },
            )
            .collect::<Result<Vec<_>, CompileError>>()?
            .into_iter()
            .map(|(mut compiled_function, function_custom_sections, eh_frame_section_indices)| {
                let first_section = module_custom_sections.len() as u32;
                for (section_index, custom_section) in function_custom_sections.iter() {
                    // TODO: remove this call to clone()
                    let mut custom_section = custom_section.clone();
                    for mut reloc in &mut custom_section.relocations {
                        if let RelocationTarget::CustomSection(index) = reloc.reloc_target {
                            reloc.reloc_target = RelocationTarget::CustomSection(
                                SectionIndex::from_u32(first_section + index.as_u32()),
                            )
                        }
                    }
                    if eh_frame_section_indices.contains(&section_index) {
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
                            relocations: vec![]
                        });
                    } else {
                        module_custom_sections.push(custom_section);
                    }
                }
                for mut reloc in &mut compiled_function.relocations {
                    if let RelocationTarget::CustomSection(index) = reloc.reloc_target {
                        reloc.reloc_target = RelocationTarget::CustomSection(
                            SectionIndex::from_u32(first_section + index.as_u32()),
                        )
                    }
                }
                compiled_function
            })
            .collect::<PrimaryMap<LocalFunctionIndex, _>>();

        let dwarf = if !frame_section_bytes.is_empty() {
            let dwarf = Some(Dwarf::new(SectionIndex::from_u32(module_custom_sections.len() as u32)));
            // Terminator CIE.
            frame_section_bytes.extend(vec!
                [0x00, 0x00, 0x00, 0x00,  // Length
                 0x00, 0x00, 0x00, 0x00,  // CIE ID
                 0x10,  // Version (must be 1)
                 0x00,  // Augmentation data
                 0x00, 0x00,  
                 0xa4, 0x2e, 0x00, 0x00,
                 0x30, 0x73, 0xff, 0xff,
                 0x00, 0x00, 0x00, 0x00,
                 0x00, 0x00, 0x00, 0x00,
                 0x00, 0x00, 0x00, 0x00]);
            module_custom_sections.push(CustomSection {
                protection: CustomSectionProtection::Read,
                bytes: SectionBody::new_with_vec(frame_section_bytes),
                relocations: frame_section_relocations
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
