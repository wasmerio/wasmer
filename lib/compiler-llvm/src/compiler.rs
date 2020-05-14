//! Support for compiling with LLVM.
// Allow unused imports while developing.
#![allow(unused_imports, dead_code)]

use crate::config::LLVMConfig;
use crate::trampoline::FuncTrampoline;
use crate::translator::FuncTranslator;
use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};
use wasm_common::entity::{EntityRef, PrimaryMap, SecondaryMap};
use wasm_common::Features;
use wasm_common::{FunctionIndex, FunctionType, LocalFunctionIndex, MemoryIndex, TableIndex};
use wasmer_compiler::{
    Compilation, CompileError, CompiledFunction, Compiler, CompilerConfig, CustomSection,
    CustomSectionProtection, FunctionBody, FunctionBodyData, ModuleTranslationState, Relocation,
    RelocationTarget, SectionBody, SectionIndex, Target, TrapInformation,
};
use wasmer_runtime::{MemoryPlan, Module, TablePlan, TrapCode};

use inkwell::targets::{InitializationConfig, Target as InkwellTarget};

use std::collections::HashMap;
use std::sync::{Arc, Mutex}; // TODO: remove

/// A compiler that compiles a WebAssembly module with LLVM, translating the Wasm to LLVM IR,
/// optimizing it and then translating to assembly.
pub struct LLVMCompiler {
    config: LLVMConfig,
}

impl LLVMCompiler {
    /// Creates a new LLVM compiler
    pub fn new(config: &LLVMConfig) -> LLVMCompiler {
        LLVMCompiler {
            config: config.clone(),
        }
    }

    /// Gets the WebAssembly features for this Compiler
    fn config(&self) -> &LLVMConfig {
        &self.config
    }
}

impl Compiler for LLVMCompiler {
    /// Gets the WebAssembly features for this Compiler
    fn features(&self) -> &Features {
        self.config.features()
    }

    /// Gets the target associated to this Compiler.
    fn target(&self) -> &Target {
        self.config.target()
    }

    /// Compile the module using LLVM, producing a compilation result with
    /// associated relocations.
    fn compile_module<'data, 'module>(
        &self,
        module: &'module Module,
        _module_translation: &ModuleTranslationState,
        function_body_inputs: PrimaryMap<LocalFunctionIndex, FunctionBodyData<'data>>,
        memory_plans: PrimaryMap<MemoryIndex, MemoryPlan>,
        table_plans: PrimaryMap<TableIndex, TablePlan>,
    ) -> Result<Compilation, CompileError> {
        //let data = Arc::new(Mutex::new(0));
        let mut func_names = SecondaryMap::new();

        // We're going to "link" the sections by simply appending all compatible
        // sections, then building the new relocations.
        // TODO: merge constants.
        let mut used_readonly_section = false;
        let mut readonly_section = CustomSection {
            protection: CustomSectionProtection::Read,
            bytes: SectionBody::default(),
            relocations: vec![],
        };

        for (func_index, _) in &module.functions {
            func_names[func_index] = module
                .func_names
                .get(&func_index)
                .cloned()
                .unwrap_or_else(|| format!("fn{}", func_index.index()));
        }
        let mut functions = function_body_inputs
            .into_iter()
            .collect::<Vec<(LocalFunctionIndex, &FunctionBodyData<'_>)>>()
            .par_iter()
            .map_init(FuncTranslator::new, |func_translator, (i, input)| {
                // TODO: remove (to serialize)
                //let mut data = data.lock().unwrap();
                func_translator.translate(
                    module,
                    i,
                    input,
                    self.config(),
                    &memory_plans,
                    &table_plans,
                    &func_names,
                )
            })
            .collect::<Result<Vec<_>, CompileError>>()?
            .into_iter()
            .map(|(mut function, local_relocations, custom_sections)| {
                /// We collect the sections data
                for (local_idx, custom_section) in custom_sections.iter().enumerate() {
                    let local_idx = local_idx as u32;
                    // TODO: these section numbers are potentially wrong, if there's
                    // no Read and only a ReadExecute then ReadExecute is 0.
                    let (ref mut section, section_num) = match &custom_section.protection {
                        CustomSectionProtection::Read => {
                            (&mut readonly_section, SectionIndex::from_u32(0))
                        }
                    };
                    let offset = section.bytes.len() as i64;
                    section.bytes.append(&custom_section.bytes);
                    // TODO: we're needlessly rescanning the whole list.
                    for local_relocation in &local_relocations {
                        if local_relocation.local_section_index == local_idx {
                            used_readonly_section = true;
                            function.relocations.push(Relocation {
                                kind: local_relocation.kind,
                                reloc_target: RelocationTarget::CustomSection(section_num),
                                offset: local_relocation.offset,
                                addend: local_relocation.addend + offset,
                            });
                        }
                    }
                }
                Ok(function)
            })
            .collect::<Result<Vec<_>, CompileError>>()?
            .into_iter()
            .collect::<PrimaryMap<LocalFunctionIndex, _>>();

        let mut custom_sections = PrimaryMap::new();
        if used_readonly_section {
            custom_sections.push(readonly_section);
        }
        Ok(Compilation::new(functions, custom_sections))
    }

    fn compile_wasm_trampolines(
        &self,
        signatures: &[FunctionType],
    ) -> Result<Vec<FunctionBody>, CompileError> {
        signatures
            .par_iter()
            .map_init(FuncTrampoline::new, |func_trampoline, sig| {
                func_trampoline.trampoline(sig, self.config())
            })
            .collect::<Result<Vec<_>, CompileError>>()
    }
}
