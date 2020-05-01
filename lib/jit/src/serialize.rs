use crate::instantiate::OwnedDataInitializer;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use wasmer_compiler::Compilation;
use wasmer_runtime::Module;

use wasm_common::entity::PrimaryMap;
use wasm_common::{MemoryIndex, TableIndex};
use wasmer_runtime::{MemoryPlan, TablePlan};

// #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
// pub struct CompiledFunction {
//     /// The function body.
//     #[serde(with = "serde_bytes")]
//     pub body: Vec<u8>,

//     /// The relocations (in the body)
//     pub relocations: Vec<Relocation>,

//     /// The jump tables offsets (in the body).
//     pub jt_offsets: JumpTableOffsets,

//     /// The unwind information.
//     pub unwind_info: CompiledFunctionUnwindInfo,

//     /// The frame information.
//     pub frame_info: CompiledFunctionFrameInfo,
// }

/// Structure to cache the content ot the compilation
#[derive(Serialize, Deserialize)]
pub struct CacheRawCompiledModule {
    pub compilation: Arc<Compilation>,
    pub module: Arc<Module>,
    pub data_initializers: Arc<Box<[OwnedDataInitializer]>>,
    // Plans for that module
    pub memory_plans: PrimaryMap<MemoryIndex, MemoryPlan>,
    pub table_plans: PrimaryMap<TableIndex, TablePlan>,
}
