use crate::data::OwnedDataInitializer;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use wasmer_compiler::{
    Compilation, CompiledFunctionFrameInfo, CompiledFunctionUnwindInfo, JumpTableOffsets,
    Relocation,
};
use wasmer_runtime::Module;

use wasm_common::entity::PrimaryMap;
use wasm_common::{LocalFuncIndex, MemoryIndex, TableIndex};
use wasmer_runtime::{MemoryPlan, TablePlan};

#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub struct FunctionBody {
    /// The function body.
    #[serde(with = "serde_bytes")]
    pub body: Vec<u8>,
}

// /// The compiled functions map (index in the Wasm -> function)
// pub type Functions = PrimaryMap<LocalFuncIndex, CompiledFunction>;

pub struct SerializedCompilation {
    function_bodies: PrimaryMap<LocalFuncIndex, FunctionBody>,
    function_relocations: PrimaryMap<LocalFuncIndex, Vec<Relocation>>,
    function_jt_offsets: PrimaryMap<LocalFuncIndex, JumpTableOffsets>,
    function_unwind_info: PrimaryMap<LocalFuncIndex, CompiledFunctionUnwindInfo>,
    function_frame_info: PrimaryMap<LocalFuncIndex, CompiledFunctionFrameInfo>,
}

/// Structure to cache the content ot the compilation
#[derive(Serialize, Deserialize)]
pub struct SerializedModule {
    pub compilation: Compilation,
    pub module: Arc<Module>,
    pub data_initializers: Box<[OwnedDataInitializer]>,
    // Plans for that module
    pub memory_plans: PrimaryMap<MemoryIndex, MemoryPlan>,
    pub table_plans: PrimaryMap<TableIndex, TablePlan>,
}
