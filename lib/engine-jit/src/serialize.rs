use serde::{Deserialize, Serialize};
use borsh::{BorshSerialize, BorshDeserialize};
use wasmer_compiler::{
    CompileModuleInfo, CustomSection, Dwarf, FunctionBody, JumpTableOffsets, Relocation,
    SectionIndex,
};
use wasmer_engine::SerializableFunctionFrameInfo;
use wasmer_types::entity::PrimaryMap;
use wasmer_types::{FunctionIndex, LocalFunctionIndex, OwnedDataInitializer, SignatureIndex};
use std::iter::FromIterator;

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
#[derive(Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct SerializableModule {
    pub compilation: SerializableCompilation,
    pub compile_info: CompileModuleInfo,
    pub data_initializers: Box<[OwnedDataInitializer]>,
}

impl BorshSerialize for SerializableCompilation {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        BorshSerialize::serialize(&self.function_bodies.into_iter().collect::<Vec<_>>(), writer)?;
        BorshSerialize::serialize(&self.function_relocations.into_iter().collect::<Vec<_>>(), writer)?;
        // JumpTableOffsets is a SecondaryMap, non trivial to impl borsh
        let v = bincode::serialize(&self.function_jt_offsets)?;
        BorshSerialize::serialize(&v, writer)?;
        BorshSerialize::serialize(&self.function_frame_info.into_iter().collect::<Vec<_>>(), writer)?;
        BorshSerialize::serialize(&self.function_call_trampolines.into_iter().collect::<Vec<_>>(), writer)?;
        BorshSerialize::serialize(&self.dynamic_function_trampolines.into_iter().collect::<Vec<_>>(), writer)?;
        BorshSerialize::serialize(&self.custom_sections.into_iter().collect::<Vec<_>>(), writer)?;
        BorshSerialize::serialize(&self.custom_section_relocations.into_iter().collect::<Vec<_>>(), writer)?;
        BorshSerialize::serialize(&self.debug, writer)?;
    }
}

impl BorshDeserialize for SerializableCompilation {
    fn deserialize(buf: &mut &[u8]) -> std::io::Result<Self> {
        let function_bodies: Vec<FunctionBody> = BorshDeserialize::deserialize(buf)?;
        let function_bodies = PrimaryMap::from_iter(function_bodies.into_iter());
        let function_relocations: Vec<Vec<Relocation>> = BorshDeserialize::deserialize(buf)?;
        let function_relocations = PrimaryMap::from_iter(function_relocations.into_iter());
        let v: Vec<u8> = BorshDeserialize::deserialize(buf)?;
        let function_jt_offsets = bincode::deserialize(&v)?;
        let function_frame_info: Vec<SerializableFunctionFrameInfo> = BorshDeserialize::deserialize(buf)?;
        let function_frame_info = PrimaryMap::from_iter(function_frame_info.into_iter());
        let function_call_trampolines: Vec<FunctionBody> = BorshDeserialize::deserialize(buf)?;
        let function_call_trampolines = PrimaryMap::from_iter(function_call_trampolines.into_iter());
        let dynamic_function_trampolines: Vec<FunctionBody> = BorshDeserialize::deserialize(buf)?;
        let dynamic_function_trampolines = PrimaryMap::from_iter(dynamic_function_trampolines.into_iter());
        let custom_sections: Vec<CustomSection> = BorshDeserialize::deserialize(buf)?;
        let custom_sections = PrimaryMap::from_iter(custom_sections.into_iter());
        let custom_section_relocations: Vec<Vec<Relocation>> = BorshDeserialize::deserialize(buf)?;
        let custom_section_relocations = PrimaryMap::from_iter(custom_section_relocations.into_iter());
        let debug = BorshDeserialize::deserialize(buf)?;
        Self {
            function_bodies,
            function_relocations,
            function_jt_offsets,
            function_frame_info,
            function_call_trampolines,
            dynamic_function_trampolines,
            custom_sections,
            custom_section_relocations,
            debug
        }
    }
}