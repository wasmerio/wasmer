use loupe::MemoryUsage;
use rkyv::{
    archived_value,
    de::{adapters::SharedDeserializerAdapter, deserializers::AllocDeserializer},
    ser::adapters::SharedSerializerAdapter,
    ser::{serializers::WriteSerializer, Serializer as RkyvSerializer},
    Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize,
};
use serde::{Deserialize, Serialize};
use wasmer_compiler::{
    CompileModuleInfo, CustomSection, Dwarf, FunctionBody, JumpTableOffsets, Relocation,
    SectionIndex,
};
use wasmer_engine::{DeserializeError, SerializableFunctionFrameInfo, SerializeError};
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
#[derive(Serialize, Deserialize, MemoryUsage, RkyvSerialize, RkyvDeserialize, Archive)]
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
#[derive(Serialize, Deserialize, MemoryUsage, RkyvSerialize, RkyvDeserialize, Archive)]
pub struct SerializableModule {
    pub compilation: SerializableCompilation,
    pub compile_info: CompileModuleInfo,
    pub data_initializers: Box<[OwnedDataInitializer]>,
}

impl SerializableModule {
    pub fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        let mut serializer = SharedSerializerAdapter::new(WriteSerializer::new(vec![]));
        let pos = serializer
            .serialize_value(self)
            .map_err(|e| SerializeError::Generic(format!("{:?}", e)))? as u64;
        let mut serialized_data = serializer.into_inner().into_inner();
        serialized_data.extend_from_slice(&pos.to_le_bytes());
        if cfg!(target_endian = "big") {
            serialized_data.extend_from_slice(&[b'b']);
        } else if cfg!(target_endian = "little") {
            serialized_data.extend_from_slice(&[b'l']);
        }
        Ok(serialized_data)
    }

    pub unsafe fn deserialize(serializable_module_slice: &[u8]) -> Result<Self, DeserializeError> {
        let archived = Self::archive_from_slice(serializable_module_slice)?;
        Self::deserialize_from_archive(archived)
    }

    unsafe fn archive_from_slice<'a>(
        serializable_module_slice: &'a [u8],
    ) -> Result<&'a ArchivedSerializableModule, DeserializeError> {
        let mut pos: [u8; 8] = Default::default();
        let endian = serializable_module_slice[serializable_module_slice.len() - 1];
        if (cfg!(target_endian = "big") && endian == b'l')
            || (cfg!(target_endian = "little") && endian == b'b')
        {
            return Err(DeserializeError::Incompatible("incompatible endian".into()));
        }
        pos.copy_from_slice(
            &serializable_module_slice
                [serializable_module_slice.len() - 9..serializable_module_slice.len() - 1],
        );
        let pos: u64 = u64::from_le_bytes(pos);
        Ok(archived_value::<SerializableModule>(
            &serializable_module_slice[..serializable_module_slice.len() - 9],
            pos as usize,
        ))
    }

    pub fn deserialize_from_archive(
        archived: &ArchivedSerializableModule,
    ) -> Result<Self, DeserializeError> {
        let mut deserializer = SharedDeserializerAdapter::new(AllocDeserializer);
        RkyvDeserialize::deserialize(archived, &mut deserializer)
            .map_err(|e| DeserializeError::CorruptedBinary(format!("{:?}", e)))
    }
}
