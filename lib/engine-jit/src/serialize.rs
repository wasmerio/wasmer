use serde::{Deserialize, Serialize};
use std::sync::Arc;
use wasm_common::entity::PrimaryMap;
use wasm_common::{
    Features, FunctionIndex, LocalFunctionIndex, MemoryIndex, OwnedDataInitializer, SignatureIndex,
    TableIndex,
};
use wasmer_compiler::{
    CompileModuleInfo, CustomSection, Dwarf, FunctionBody, JumpTableOffsets, Relocation,
    SectionIndex,
};
use wasmer_engine::SerializableFunctionFrameInfo;
use wasmer_runtime::ModuleInfo;
use wasmer_runtime::{MemoryPlan, TablePlan};

// /// The serializable function data
// #[derive(Serialize, Deserialize)]
// pub struct SerializableFunction {
//     #[serde(with = "serde_bytes")]
//     pub body: &[u8],
//     /// The unwind info for Windows
//     #[serde(with = "serde_bytes")]
//     pub windows_unwind_info: &[u8],
// }

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
    pub function_call_trampolines: PrimaryMap<SignatureIndex, FunctionBody>,
    pub dynamic_function_trampolines: PrimaryMap<FunctionIndex, FunctionBody>,
    pub custom_sections: PrimaryMap<SectionIndex, CustomSection>,
    pub custom_section_relocations: PrimaryMap<SectionIndex, Vec<Relocation>>,
    // The section indices corresponding to the Dwarf debug info
    pub debug: Option<Dwarf>,
}

/// Serializable struct that is able to serialize from and to
/// a `JITArtifactInfo`.
#[derive(Serialize, Deserialize)]
pub struct SerializableModule {
    pub compilation: SerializableCompilation,
    pub compile_info: CompileModuleInfo,
    pub data_initializers: Box<[OwnedDataInitializer]>,
}
