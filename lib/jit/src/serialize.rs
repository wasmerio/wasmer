use crate::data::OwnedDataInitializer;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use wasmer_compiler::{CompiledFunctionFrameInfo, FunctionBody, JumpTableOffsets, Relocation};
use wasmer_runtime::Module;

use wasm_common::entity::PrimaryMap;
use wasm_common::{LocalFuncIndex, MemoryIndex, TableIndex};
use wasmer_runtime::{MemoryPlan, TablePlan};

/// The compilation related data for a serialized modules
#[derive(Serialize, Deserialize)]
pub struct SerializableCompilation {
    pub function_bodies: PrimaryMap<LocalFuncIndex, FunctionBody>,
    pub function_relocations: PrimaryMap<LocalFuncIndex, Vec<Relocation>>,
    pub function_jt_offsets: PrimaryMap<LocalFuncIndex, JumpTableOffsets>,
    pub function_frame_info: PrimaryMap<LocalFuncIndex, CompiledFunctionFrameInfo>,
}

/// Serializable struct that is able to serialize from and to
/// a `CompiledModule`.
#[derive(Serialize, Deserialize)]
pub struct SerializableModule {
    pub compilation: SerializableCompilation,
    pub module: Arc<Module>,
    pub data_initializers: Box<[OwnedDataInitializer]>,
    // Plans for that module
    pub memory_plans: PrimaryMap<MemoryIndex, MemoryPlan>,
    pub table_plans: PrimaryMap<TableIndex, TablePlan>,
}
