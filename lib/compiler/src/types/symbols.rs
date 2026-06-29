/*
 * ! Remove me once rkyv generates doc-comments for fields or generates an #[allow(missing_docs)]
 * on their own.
 */
#![allow(missing_docs)]

//! This module define the required structures for compilation symbols.
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
use wasmer_types::{
    DeserializeError, LocalFunctionIndex, OwnedDataInitializer, SerializeError, entity::PrimaryMap,
};

use super::module::CompileModuleInfo;

/// Serializable struct that represents the compiled metadata.
#[derive(Debug, RkyvSerialize, RkyvDeserialize, Archive)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[rkyv(derive(Debug))]
pub struct ModuleMetadata {
    /// Compile info
    pub compile_info: CompileModuleInfo,
    /// Prefix for function etc symbols
    pub prefix: String,
    /// Data initializers
    pub data_initializers: Box<[OwnedDataInitializer]>,
    /// The function body lengths (used to find function by address)
    pub function_body_lengths: PrimaryMap<LocalFunctionIndex, u64>,
    /// CPU features used (see [`wasmer_types::CpuFeature`])
    pub cpu_features: u64,
}

/// A simple metadata registry
pub struct ModuleMetadataSymbolRegistry {
    /// Symbol prefix string
    pub prefix: String,
}

impl ModuleMetadata {
    /// Get mutable ref to compile info and a copy of the registry
    pub fn split(&mut self) -> (&mut CompileModuleInfo, ModuleMetadataSymbolRegistry) {
        let compile_info = &mut self.compile_info;
        let symbol_registry = ModuleMetadataSymbolRegistry {
            prefix: self.prefix.clone(),
        };
        (compile_info, symbol_registry)
    }

    /// Returns symbol registry.
    pub fn get_symbol_registry(&self) -> ModuleMetadataSymbolRegistry {
        ModuleMetadataSymbolRegistry {
            prefix: self.prefix.clone(),
        }
    }
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
        unsafe {
            let archived = Self::archive_from_slice(metadata_slice)?;
            Self::deserialize_from_archive(archived)
        }
    }

    /// Deserialize a Module from a slice.
    /// The slice must have the following format:
    /// RKYV serialization (any length) + POS (8 bytes)
    pub fn deserialize(metadata_slice: &[u8]) -> Result<Self, DeserializeError> {
        let archived = Self::archive_from_slice_checked(metadata_slice)?;
        Self::deserialize_from_archive(archived)
    }

    /// # Safety
    ///
    /// This method is unsafe.
    /// Please check `ModuleMetadata::deserialize` for more details.
    unsafe fn archive_from_slice(
        metadata_slice: &[u8],
    ) -> Result<&ArchivedModuleMetadata, DeserializeError> {
        unsafe { Ok(rkyv::access_unchecked(metadata_slice)) }
    }

    /// # Safety
    ///
    /// This method is unsafe.
    /// Please check `ModuleMetadata::deserialize` for more details.
    fn archive_from_slice_checked(
        metadata_slice: &[u8],
    ) -> Result<&ArchivedModuleMetadata, DeserializeError> {
        rkyv::access::<_, rkyv::rancor::Error>(metadata_slice)
            .map_err(|e| DeserializeError::CorruptedBinary(e.to_string()))
    }

    /// Deserialize a compilation module from an archive
    pub fn deserialize_from_archive(
        archived: &ArchivedModuleMetadata,
    ) -> Result<Self, DeserializeError> {
        rkyv::deserialize::<_, rkyv::rancor::Error>(archived)
            .map_err(|e| DeserializeError::CorruptedBinary(format!("{e:?}")))
    }
}
