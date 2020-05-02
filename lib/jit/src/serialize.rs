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

/// This is the unserialized verison of `CompiledFunctionFrameInfo`.
#[derive(Clone, Serialize, Deserialize)]
pub struct UnprocessedFunctionFrameInfo {
    #[serde(with = "serde_bytes")]
    bytes: Vec<u8>,
}

impl UnprocessedFunctionFrameInfo {
    /// Converts the `UnprocessedFunctionFrameInfo` to a `CompiledFunctionFrameInfo`
    pub fn deserialize(&self) -> CompiledFunctionFrameInfo {
        bincode::deserialize(&self.bytes).expect("Can't deserialize the info")
    }

    /// Converts the `CompiledFunctionFrameInfo` to a `UnprocessedFunctionFrameInfo`
    pub fn serialize(processed: CompiledFunctionFrameInfo) -> Self {
        Self {
            bytes: bincode::serialize(&processed).expect("Can't serialize the info"),
        }
    }
}

/// We hold the frame info in two states, mainly because we want to
/// process it lazily to speed up execution.
///
/// When a Trap occurs, we process the frame info lazily for each
/// function in the frame. That way we minimize as much as we can
/// the upfront effort.
///
/// The data can also be processed upfront. This will happen in the case
/// of compiling at the same time that emiting the JIT.
/// In that case, we don't need to deserialize/process anything
/// as the data is already in memory.
#[derive(Clone)]
pub enum SerializableFunctionFrameInfo {
    /// The unprocessed frame info (binary)
    Unprocessed(UnprocessedFunctionFrameInfo),
    /// The processed frame info (memory struct)
    Processed(CompiledFunctionFrameInfo),
}

impl SerializableFunctionFrameInfo {
    /// Returns true if the extra function info is not yet
    /// processed
    pub fn is_unprocessed(&self) -> bool {
        match self {
            Self::Unprocessed(_) => true,
            _ => false,
        }
    }
}
