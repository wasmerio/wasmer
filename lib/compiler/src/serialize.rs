//! Serialization of compilation metadata embedded in native artifacts.

use crate::types::module::{ArchivedCompileModuleInfo, CompileModuleInfo};
use wasmer_types::{DeserializeError, SerializeError};

pub use wasmer_types::MetadataHeader;

impl CompileModuleInfo {
    /// Serialize compilation metadata into bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        rkyv::to_bytes::<rkyv::rancor::Error>(self)
            .map(|v| v.into_vec())
            .map_err(|e| SerializeError::Generic(e.to_string()))
    }

    /// Deserialize compilation metadata without validating the archive.
    ///
    /// # Safety
    /// The bytes must contain a valid archive of `CompileModuleInfo`.
    pub unsafe fn deserialize_unchecked(metadata_slice: &[u8]) -> Result<Self, DeserializeError> {
        unsafe {
            let archived = Self::archive_from_slice(metadata_slice)?;
            Self::deserialize_from_archive(archived)
        }
    }

    /// Deserialize compilation metadata, validating the archive first.
    pub fn deserialize(metadata_slice: &[u8]) -> Result<Self, DeserializeError> {
        let archived = Self::archive_from_slice_checked(metadata_slice)?;
        Self::deserialize_from_archive(archived)
    }

    /// Access archived compilation metadata without validation.
    ///
    /// # Safety
    /// The bytes must contain a valid archive of `CompileModuleInfo`.
    pub unsafe fn archive_from_slice(
        metadata_slice: &[u8],
    ) -> Result<&ArchivedCompileModuleInfo, DeserializeError> {
        unsafe { Ok(rkyv::access_unchecked(metadata_slice)) }
    }

    /// Access archived compilation metadata after validating it.
    pub fn archive_from_slice_checked(
        metadata_slice: &[u8],
    ) -> Result<&ArchivedCompileModuleInfo, DeserializeError> {
        rkyv::access::<_, rkyv::rancor::Error>(metadata_slice)
            .map_err(|e| DeserializeError::CorruptedBinary(e.to_string()))
    }

    /// Deserialize compilation metadata from an archive.
    pub fn deserialize_from_archive(
        archived: &ArchivedCompileModuleInfo,
    ) -> Result<Self, DeserializeError> {
        rkyv::deserialize::<_, rkyv::rancor::Error>(archived)
            .map_err(|e| DeserializeError::CorruptedBinary(e.to_string()))
    }
}
