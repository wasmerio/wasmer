use loupe::MemoryUsage;
use serde::{Deserialize, Serialize};
use wasmer_compiler::{
    CompileModuleInfo, CompiledFunctionFrameInfo, CustomSection, Dwarf, FunctionBody,
    JumpTableOffsets, Relocation, SectionIndex,
};
use wasmer_types::entity::PrimaryMap;
use wasmer_types::{FunctionIndex, LocalFunctionIndex, OwnedDataInitializer, SignatureIndex};

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
#[derive(Serialize, Deserialize, MemoryUsage)]
pub struct SerializableCompilation {
    pub function_bodies: PrimaryMap<LocalFunctionIndex, FunctionBody>,
    pub function_relocations: PrimaryMap<LocalFunctionIndex, Vec<Relocation>>,
    pub function_jt_offsets: PrimaryMap<LocalFunctionIndex, JumpTableOffsets>,
    pub function_frame_info: PrimaryMap<LocalFunctionIndex, CompiledFunctionFrameInfo>,
    pub function_call_trampolines: PrimaryMap<SignatureIndex, FunctionBody>,
    pub dynamic_function_trampolines: PrimaryMap<FunctionIndex, FunctionBody>,
    pub custom_sections: PrimaryMap<SectionIndex, CustomSection>,
    pub custom_section_relocations: PrimaryMap<SectionIndex, Vec<Relocation>>,
    // The section indices corresponding to the Dwarf debug info
    pub debug: Option<Dwarf>,
}

/// Serializable struct that is able to serialize from and to
/// a `JITArtifactInfo`.
#[derive(Serialize, Deserialize, MemoryUsage)]
pub struct SerializableModule {
    pub compilation: SerializableCompilation,
    pub compile_info: CompileModuleInfo,
    pub data_initializers: Box<[OwnedDataInitializer]>,
}
