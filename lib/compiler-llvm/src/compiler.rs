use crate::config::LLVMConfig;
use crate::trampoline::FuncTrampoline;
use crate::translator::FuncTranslator;
use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};
use wasm_common::entity::{EntityRef, PrimaryMap, SecondaryMap};
use wasm_common::Features;
use wasm_common::{FunctionIndex, FunctionType, LocalFunctionIndex, MemoryIndex, TableIndex};
use wasmer_compiler::{
    Compilation, CompileError, Compiler, CompilerConfig, FunctionBody, FunctionBodyData,
    ModuleTranslationState, RelocationTarget, SectionIndex, Target,
};
use wasmer_runtime::{MemoryPlan, ModuleInfo, TablePlan};

//use std::sync::{Arc, Mutex};

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
        module: &'module ModuleInfo,
        _module_translation: &ModuleTranslationState,
        function_body_inputs: PrimaryMap<LocalFunctionIndex, FunctionBodyData<'data>>,
        memory_plans: PrimaryMap<MemoryIndex, MemoryPlan>,
        table_plans: PrimaryMap<TableIndex, TablePlan>,
    ) -> Result<Compilation, CompileError> {
        //let data = Arc::new(Mutex::new(0));
        let mut func_names = SecondaryMap::new();

        // TODO: merge constants in sections.

        for (func_index, _) in &module.functions {
            func_names[func_index] = module
                .func_names
                .get(&func_index)
                .cloned()
                .unwrap_or_else(|| format!("fn{}", func_index.index()));
        }
        let mut module_custom_sections = PrimaryMap::new();
        let functions = function_body_inputs
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
            .map(|(mut compiled_function, function_custom_sections)| {
                let first_section = module_custom_sections.len() as u32;
                for (_, custom_section) in function_custom_sections.iter() {
                    // TODO: remove this call to clone()
                    let mut custom_section = custom_section.clone();
                    for mut reloc in &mut custom_section.relocations {
                        if let RelocationTarget::CustomSection(index) = reloc.reloc_target {
                            reloc.reloc_target = RelocationTarget::CustomSection(
                                SectionIndex::from_u32(first_section + index.as_u32()),
                            )
                        }
                    }
                    module_custom_sections.push(custom_section);
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

        Ok(Compilation::new(functions, module_custom_sections))
    }

    fn compile_function_call_trampolines(
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

    fn compile_dynamic_function_trampolines(
        &self,
        module: &ModuleInfo,
    ) -> Result<PrimaryMap<FunctionIndex, FunctionBody>, CompileError> {
        Ok(module
            .functions
            .values()
            .take(module.num_imported_funcs)
            .collect::<Vec<_>>()
            .par_iter()
            .map_init(FuncTrampoline::new, |func_trampoline, sig_index| {
                func_trampoline.dynamic_trampoline(&module.signatures[**sig_index], self.config())
            })
            .collect::<Result<Vec<_>, CompileError>>()?
            .into_iter()
            .collect::<PrimaryMap<FunctionIndex, FunctionBody>>())
    }
}
