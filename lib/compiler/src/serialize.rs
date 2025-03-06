/*
 * ! Remove me once rkyv generates doc-comments for fields or generates an #[allow(missing_docs)]
 * on their own.
 */
#![allow(missing_docs)]

use crate::types::{
    function::{CompiledFunctionFrameInfo, FunctionBody, UnwindInfo, GOT},
    module::CompileModuleInfo,
    relocation::Relocation,
    section::{CustomSection, SectionIndex},
};
use enumset::EnumSet;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use wasmer_types::{
    entity::PrimaryMap, target::CpuFeature, DeserializeError, Features, FunctionIndex,
    LocalFunctionIndex, MemoryIndex, MemoryStyle, ModuleInfo, OwnedDataInitializer, SerializeError,
    SignatureIndex, TableIndex, TableStyle,
};

pub use wasmer_types::MetadataHeader;

/// The compilation related data for a serialized modules
#[derive(Archive, Default, RkyvDeserialize, RkyvSerialize)]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[allow(missing_docs)]
#[rkyv(derive(Debug))]
pub struct SerializableCompilation {
    pub function_bodies: PrimaryMap<LocalFunctionIndex, FunctionBody>,
    pub function_relocations: PrimaryMap<LocalFunctionIndex, Vec<Relocation>>,
    pub function_frame_info: PrimaryMap<LocalFunctionIndex, CompiledFunctionFrameInfo>,
    pub function_call_trampolines: PrimaryMap<SignatureIndex, FunctionBody>,
    pub dynamic_function_trampolines: PrimaryMap<FunctionIndex, FunctionBody>,
    pub custom_sections: PrimaryMap<SectionIndex, CustomSection>,
    pub custom_section_relocations: PrimaryMap<SectionIndex, Vec<Relocation>>,
    // The section indices corresponding to the Dwarf debug info
    pub unwind_info: UnwindInfo,
    pub got: GOT,
    // Custom section containing libcall trampolines.
    pub libcall_trampolines: SectionIndex,
    // Length of each libcall trampoline.
    pub libcall_trampoline_len: u32,
}

impl SerializableCompilation {
    /// Serialize a Compilation into bytes
    /// The bytes will have the following format:
    /// RKYV serialization (any length) + POS (8 bytes)
    pub fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        rkyv::to_bytes::<rkyv::rancor::Error>(self)
            .map(|v| v.into_vec())
            .map_err(|e| SerializeError::Generic(e.to_string()))
    }
}

/// Serializable struct that is able to serialize from and to a `ArtifactInfo`.
#[derive(Archive, RkyvDeserialize, RkyvSerialize)]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[allow(missing_docs)]
#[rkyv(derive(Debug))]
pub struct SerializableModule {
    /// The main serializable compilation object
    pub compilation: SerializableCompilation,
    /// Compilation informations
    pub compile_info: CompileModuleInfo,
    /// Datas initializers
    pub data_initializers: Box<[OwnedDataInitializer]>,
    /// CPU Feature flags for this compilation
    pub cpu_features: u64,
}

impl SerializableModule {
    /// Serialize a Module into bytes
    /// The bytes will have the following format:
    /// RKYV serialization (any length) + POS (8 bytes)
    pub fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        rkyv::to_bytes::<rkyv::rancor::Error>(self)
            .map(|v| v.into_vec())
            .map_err(|e| SerializeError::Generic(e.to_string()))
    }

    /// Deserialize a Module from a slice.
    /// The slice must have the following format:
    /// RKYV serialization (any length) + POS (8 bytes)
    ///
    /// # Safety
    ///
    /// This method is unsafe since it deserializes data directly
    /// from memory.
    /// Right now we are not doing any extra work for validation, but
    /// `rkyv` has an option to do bytecheck on the serialized data before
    /// serializing (via `rkyv::check_archived_value`).
    pub unsafe fn deserialize_unchecked(metadata_slice: &[u8]) -> Result<Self, DeserializeError> {
        let archived = Self::archive_from_slice(metadata_slice)?;
        Self::deserialize_from_archive(archived)
    }

    /// Deserialize a Module from a slice.
    /// The slice must have the following format:
    /// RKYV serialization (any length) + POS (8 bytes)
    ///
    /// Unlike [`Self::deserialize`], this function will validate the data.
    ///
    /// # Safety
    /// Unsafe because it loads executable code into memory.
    /// The loaded bytes must be trusted.
    pub unsafe fn deserialize(metadata_slice: &[u8]) -> Result<Self, DeserializeError> {
        let archived = Self::archive_from_slice_checked(metadata_slice)?;
        Self::deserialize_from_archive(archived)
    }

    /// # Safety
    ///
    /// This method is unsafe.
    /// Please check `SerializableModule::deserialize` for more details.
    pub unsafe fn archive_from_slice(
        metadata_slice: &[u8],
    ) -> Result<&ArchivedSerializableModule, DeserializeError> {
        Ok(rkyv::access_unchecked(metadata_slice))
    }

    /// Deserialize an archived module.
    ///
    /// In contrast to [`Self::deserialize`], this method performs validation
    /// and is not unsafe.
    pub fn archive_from_slice_checked(
        metadata_slice: &[u8],
    ) -> Result<&ArchivedSerializableModule, DeserializeError> {
        rkyv::access::<_, rkyv::rancor::Error>(metadata_slice)
            .map_err(|e| DeserializeError::CorruptedBinary(e.to_string()))
    }

    /// Deserialize a compilation module from an archive
    pub fn deserialize_from_archive(
        archived: &ArchivedSerializableModule,
    ) -> Result<Self, DeserializeError> {
        rkyv::deserialize::<_, rkyv::rancor::Error>(archived)
            .map_err(|e| DeserializeError::CorruptedBinary(e.to_string()))
    }

    /// Create a `ModuleInfo` for instantiation
    pub fn create_module_info(&self) -> ModuleInfo {
        self.compile_info.module.as_ref().clone()
    }

    /// Returns the `ModuleInfo` for instantiation
    pub fn module_info(&self) -> &ModuleInfo {
        &self.compile_info.module
    }

    /// Returns the features for this Artifact
    pub fn features(&self) -> &Features {
        &self.compile_info.features
    }

    /// Returns the CPU features for this Artifact
    pub fn cpu_features(&self) -> EnumSet<CpuFeature> {
        EnumSet::from_u64(self.cpu_features)
    }

    /// Returns data initializers to pass to `VMInstance::initialize`
    pub fn data_initializers(&self) -> &[OwnedDataInitializer] {
        &self.data_initializers
    }

    /// Returns the memory styles associated with this `Artifact`.
    pub fn memory_styles(&self) -> &PrimaryMap<MemoryIndex, MemoryStyle> {
        &self.compile_info.memory_styles
    }

    /// Returns the table plans associated with this `Artifact`.
    pub fn table_styles(&self) -> &PrimaryMap<TableIndex, TableStyle> {
        &self.compile_info.table_styles
    }
}
