use serde::{Deserialize, Serialize};
use wasmer_compiler::{CompileModuleInfo, SectionIndex, Symbol, SymbolRegistry};
use wasmer_types::entity::{EntityRef, PrimaryMap};
use wasmer_types::{FunctionIndex, LocalFunctionIndex, OwnedDataInitializer, SignatureIndex};

/// Serializable struct that represents the compiled metadata.
#[derive(Serialize, Deserialize, Debug)]
pub struct ModuleMetadata {
    pub compile_info: CompileModuleInfo,
    pub prefix: String,
    pub data_initializers: Box<[OwnedDataInitializer]>,
    // The function body lengths (used to find function by address)
    pub function_body_lengths: PrimaryMap<LocalFunctionIndex, u64>,
}

impl ModuleMetadata {
    pub fn split(&mut self) -> (&mut CompileModuleInfo, &dyn SymbolRegistry) {
        let compile_info = &self.compile_info;
        let symbol_registry = self as &dyn SymbolRegistry;
        #[allow(mutable_transmutes)]
        let compile_info = unsafe { std::mem::transmute::<&_, &mut _>(compile_info) };
        (compile_info, symbol_registry)
    }
}

impl SymbolRegistry for ModuleMetadata {
    fn symbol_to_name(&self, symbol: Symbol) -> String {
        match symbol {
            Symbol::LocalFunction(index) => {
                format!("wasmer_function_{}_{}", self.prefix, index.index())
            }
            Symbol::Section(index) => format!("wasmer_section_{}_{}", self.prefix, index.index()),
            Symbol::FunctionCallTrampoline(index) => format!(
                "wasmer_trampoline_function_call_{}_{}",
                self.prefix,
                index.index()
            ),
            Symbol::DynamicFunctionTrampoline(index) => format!(
                "wasmer_trampoline_dynamic_function_{}_{}",
                self.prefix,
                index.index()
            ),
        }
    }

    fn name_to_symbol(&self, name: &str) -> Option<Symbol> {
        if let Some(index) = name.strip_prefix(&format!("wasmer_function_{}_", self.prefix)) {
            index
                .parse::<u32>()
                .ok()
                .map(|index| Symbol::LocalFunction(LocalFunctionIndex::from_u32(index)))
        } else if let Some(index) = name.strip_prefix(&format!("wasmer_section_{}_", self.prefix)) {
            index
                .parse::<u32>()
                .ok()
                .map(|index| Symbol::Section(SectionIndex::from_u32(index)))
        } else if let Some(index) =
            name.strip_prefix(&format!("wasmer_trampoline_function_call_{}_", self.prefix))
        {
            index
                .parse::<u32>()
                .ok()
                .map(|index| Symbol::FunctionCallTrampoline(SignatureIndex::from_u32(index)))
        } else if let Some(index) = name.strip_prefix(&format!(
            "wasmer_trampoline_dynamic_function_{}_",
            self.prefix
        )) {
            index
                .parse::<u32>()
                .ok()
                .map(|index| Symbol::DynamicFunctionTrampoline(FunctionIndex::from_u32(index)))
        } else {
            None
        }
    }
}
