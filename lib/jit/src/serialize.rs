use crate::data::OwnedDataInitializer;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use wasmer_compiler::{CompiledFunctionFrameInfo, FunctionBody, JumpTableOffsets, Relocation};
use wasmer_runtime::Module;

use wasm_common::entity::PrimaryMap;
use wasm_common::{LocalFuncIndex, MemoryIndex, TableIndex};
use wasmer_runtime::{MemoryPlan, TablePlan};

#[derive(Serialize, Deserialize)]
pub struct SerializedCompilation {
    pub function_bodies: PrimaryMap<LocalFuncIndex, FunctionBody>,
    pub function_relocations: PrimaryMap<LocalFuncIndex, Vec<Relocation>>,
    pub function_jt_offsets: PrimaryMap<LocalFuncIndex, JumpTableOffsets>,
    pub function_frame_info: PrimaryMap<LocalFuncIndex, CompiledFunctionFrameInfo>,
}

/// Structure to cache the content ot the compilation
#[derive(Serialize, Deserialize)]
pub struct SerializedModule {
    pub compilation: SerializedCompilation,
    pub module: Arc<Module>,
    pub data_initializers: Box<[OwnedDataInitializer]>,
    // Plans for that module
    pub memory_plans: PrimaryMap<MemoryIndex, MemoryPlan>,
    pub table_plans: PrimaryMap<TableIndex, TablePlan>,
}
