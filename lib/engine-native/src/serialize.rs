use serde::{Deserialize, Serialize};
use std::sync::Arc;
use wasm_common::entity::{EntityRef, PrimaryMap};
use wasm_common::{
    Features, FunctionIndex, LocalFunctionIndex, MemoryIndex, OwnedDataInitializer, SignatureIndex,
    TableIndex,
};
use wasmer_compiler::SectionIndex;
use wasmer_runtime::ModuleInfo;
use wasmer_runtime::{MemoryPlan, TablePlan};

/// Serializable struct that represents the compiled metadata.
#[derive(Serialize, Deserialize, Debug)]
pub struct ModuleMetadata {
    pub features: Features,
    pub module: Arc<ModuleInfo>,
    pub prefix: String,
    pub data_initializers: Box<[OwnedDataInitializer]>,
    // The function body lengths (used for reverse-locate traps in the function)
    pub function_body_lengths: PrimaryMap<LocalFunctionIndex, u64>,
    // Plans for that module
    pub memory_plans: PrimaryMap<MemoryIndex, MemoryPlan>,
    pub table_plans: PrimaryMap<TableIndex, TablePlan>,
}

impl ModuleMetadata {
    /// Gets the function name given a local function index
    pub fn get_function_name(&self, index: LocalFunctionIndex) -> String {
        format!("wasmer_function_{}_{}", self.prefix, index.index())
    }

    /// Gets the section name given a section index
    pub fn get_section_name(&self, index: SectionIndex) -> String {
        format!("wasmer_section_{}_{}", self.prefix, index.index())
    }

    /// Gets the function call trampoline name given a signature index
    pub fn get_function_call_trampoline_name(&self, index: SignatureIndex) -> String {
        format!(
            "wasmer_trampoline_function_call_{}_{}",
            self.prefix,
            index.index()
        )
    }

    /// Gets the dynamic function trampoline name given a function index
    pub fn get_dynamic_function_trampoline_name(&self, index: FunctionIndex) -> String {
        format!(
            "wasmer_trampoline_dynamic_function_{}_{}",
            self.prefix,
            index.index()
        )
    }
}
