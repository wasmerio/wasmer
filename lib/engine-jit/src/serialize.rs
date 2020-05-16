use serde::{Deserialize, Serialize};
use std::sync::Arc;
use wasm_common::entity::PrimaryMap;
use wasm_common::{
    Features, FunctionIndex, LocalFunctionIndex, MemoryIndex, OwnedDataInitializer, SignatureIndex,
    TableIndex,
};
use wasmer_compiler::{FunctionBody, JumpTableOffsets, Relocation, SectionBody, SectionIndex};
use wasmer_engine::SerializableFunctionFrameInfo;
use wasmer_runtime::Module;
use wasmer_runtime::{MemoryPlan, TablePlan};

/// The compilation related data for a serialized modules
#[derive(Serialize, Deserialize)]
pub struct SerializableCompilation {
    pub function_bodies: PrimaryMap<LocalFunctionIndex, FunctionBody>,
    pub function_relocations: PrimaryMap<LocalFunctionIndex, Vec<Relocation>>,
    pub function_jt_offsets: PrimaryMap<LocalFunctionIndex, JumpTableOffsets>,
    // This is `SerializableFunctionFrameInfo` instead of `CompiledFunctionFrameInfo`,
    // to allow lazy frame_info deserialization, we convert it to it's lazy binary
    // format upon serialization.
    pub function_frame_info: PrimaryMap<LocalFunctionIndex, SerializableFunctionFrameInfo>,
    pub trampolines: PrimaryMap<SignatureIndex, FunctionBody>,
    pub reverse_trampolines: PrimaryMap<FunctionIndex, FunctionBody>,
    pub custom_sections: PrimaryMap<SectionIndex, SectionBody>,
    pub custom_section_relocations: PrimaryMap<SectionIndex, Vec<Relocation>>,
}

/// Serializable struct that is able to serialize from and to
/// a `CompiledModule`.
#[derive(Serialize, Deserialize)]
pub struct SerializableModule {
    pub compilation: SerializableCompilation,
    pub features: Features,
    pub module: Arc<Module>,
    pub data_initializers: Box<[OwnedDataInitializer]>,
    // Plans for that module
    pub memory_plans: PrimaryMap<MemoryIndex, MemoryPlan>,
    pub table_plans: PrimaryMap<TableIndex, TablePlan>,
}
