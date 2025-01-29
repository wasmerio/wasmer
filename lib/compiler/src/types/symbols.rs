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
    entity::{EntityRef, PrimaryMap},
    DeserializeError, FunctionIndex, LocalFunctionIndex, OwnedDataInitializer, SerializeError,
    SignatureIndex,
};

use super::{module::CompileModuleInfo, section::SectionIndex};

/// The kinds of wasmer_types objects that might be found in a native object file.
#[derive(
    RkyvSerialize, RkyvDeserialize, Archive, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug,
)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[rkyv(derive(Debug), compare(PartialEq, PartialOrd))]
pub enum Symbol {
    /// A metadata section, indexed by a unique prefix
    /// (usually the wasm file SHA256 hash)
    Metadata,

    /// A function defined in the wasm.
    LocalFunction(LocalFunctionIndex),

    /// A wasm section.
    Section(SectionIndex),

    /// The function call trampoline for a given signature.
    FunctionCallTrampoline(SignatureIndex),

    /// The dynamic function trampoline for a given function.
    DynamicFunctionTrampoline(FunctionIndex),
}

/// This trait facilitates symbol name lookups in a native object file.
pub trait SymbolRegistry: Send + Sync {
    /// Given a `Symbol` it returns the name for that symbol in the object file
    fn symbol_to_name(&self, symbol: Symbol) -> String;

    /// Given a name it returns the `Symbol` for that name in the object file
    ///
    /// This function is the inverse of [`SymbolRegistry::symbol_to_name`]
    fn name_to_symbol(&self, name: &str) -> Option<Symbol>;
}

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
    /// CPU features used (See [`CpuFeature`](crate::CpuFeature))
    pub cpu_features: u64,
}

/// A simple metadata registry
pub struct ModuleMetadataSymbolRegistry {
    /// Symbol prefix stirng
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
        let archived = Self::archive_from_slice(metadata_slice)?;
        Self::deserialize_from_archive(archived)
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
        Ok(rkyv::access_unchecked(metadata_slice))
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

impl SymbolRegistry for ModuleMetadataSymbolRegistry {
    fn symbol_to_name(&self, symbol: Symbol) -> String {
        match symbol {
            Symbol::Metadata => {
                format!("WASMER_METADATA_{}", self.prefix.to_uppercase())
            }
            Symbol::LocalFunction(index) => {
                format!("wasmer_function_{}_{}", self.prefix, index.index())
            }
            Symbol::Section(index) => format!("wasmer_section_{}_{}", self.prefix, index.index()),
            Symbol::FunctionCallTrampoline(index) => {
                format!(
                    "wasmer_trampoline_function_call_{}_{}",
                    self.prefix,
                    index.index()
                )
            }
            Symbol::DynamicFunctionTrampoline(index) => {
                format!(
                    "wasmer_trampoline_dynamic_function_{}_{}",
                    self.prefix,
                    index.index()
                )
            }
        }
    }

    fn name_to_symbol(&self, name: &str) -> Option<Symbol> {
        if name == self.symbol_to_name(Symbol::Metadata) {
            Some(Symbol::Metadata)
        } else if let Some(index) = name.strip_prefix(&format!("wasmer_function_{}_", self.prefix))
        {
            index
                .parse::<u32>()
                .ok()
                .map(|index| Symbol::LocalFunction(LocalFunctionIndex::from_u32(index)))
        } else if let Some(index) = name.strip_prefix(&format!("wasmer_section_{}_", self.prefix)) {
            index
                .parse::<u32>()
                .ok()
                .map(|index| Symbol::Section(SectionIndex::from_u32(index)))
        } else if let Some(index) =
            name.strip_prefix(&format!("wasmer_trampoline_function_call_{}_", self.prefix))
        {
            index
                .parse::<u32>()
                .ok()
                .map(|index| Symbol::FunctionCallTrampoline(SignatureIndex::from_u32(index)))
        } else if let Some(index) = name.strip_prefix(&format!(
            "wasmer_trampoline_dynamic_function_{}_",
            self.prefix
        )) {
            index
                .parse::<u32>()
                .ok()
                .map(|index| Symbol::DynamicFunctionTrampoline(FunctionIndex::from_u32(index)))
        } else {
            None
        }
    }
}
