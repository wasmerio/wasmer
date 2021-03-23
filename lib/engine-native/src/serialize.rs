use serde::{Deserialize, Serialize};
use wasmer_compiler::{CompileModuleInfo, SectionIndex, Symbol, SymbolRegistry, CompileError};
use wasmer_types::entity::{EntityRef, PrimaryMap};
use wasmer_types::{FunctionIndex, LocalFunctionIndex, OwnedDataInitializer, SignatureIndex};
use wasmer_engine::DeserializeError;
use rkyv::{
    ser::{Serializer as RkyvSerializer, serializers::WriteSerializer}, archived_value,
    de::{deserializers::AllocDeserializer, adapters::SharedDeserializerAdapter},
    Deserialize as RkyvDeserialize,
    Serialize as RkyvSerialize,
    ser::adapters::SharedSerializerAdapter,
    Archive
};
use std::error::Error;

fn to_compile_error(err: impl Error) -> CompileError {
    CompileError::Codegen(format!("{}", err))
}

/// Serializable struct that represents the compiled metadata.
#[derive(Serialize, Deserialize, Debug, RkyvSerialize, RkyvDeserialize, Archive, PartialEq, Eq)]
pub struct ModuleMetadata {
    pub compile_info: CompileModuleInfo,
    pub prefix: String,
    pub data_initializers: Box<[OwnedDataInitializer]>,
    // The function body lengths (used to find function by address)
    pub function_body_lengths: PrimaryMap<LocalFunctionIndex, u64>,
}

pub struct ModuleMetadataSymbolRegistry<'a> {
    pub prefix: &'a String,
}

impl ModuleMetadata {
    pub fn split<'a>(
        &'a mut self,
    ) -> (&'a mut CompileModuleInfo, ModuleMetadataSymbolRegistry<'a>) {
        let compile_info = &mut self.compile_info;
        let symbol_registry = ModuleMetadataSymbolRegistry {
            prefix: &self.prefix,
        };
        (compile_info, symbol_registry)
    }

    pub fn get_symbol_registry<'a>(&'a self) -> ModuleMetadataSymbolRegistry<'a> {
        ModuleMetadataSymbolRegistry {
            prefix: &self.prefix,
        }
    }

    pub fn serialize(&mut self) -> Result<Vec<u8>, CompileError> {
        let mut serializer = SharedSerializerAdapter::new(WriteSerializer::new(vec![0u8; 9]));
        let pos = serializer.serialize_value(self)
            .map_err(to_compile_error)? as u64;
        let mut serialized_data = serializer.into_inner().into_inner();
        if cfg!(target_endian = "big") {
            serialized_data[0] = b'b';
        } else if cfg!(target_endian = "little") {
            serialized_data[0] = b'l';
        }
        serialized_data[1..9].copy_from_slice(&pos.to_le_bytes());
        Ok(serialized_data)
    }

    pub unsafe fn deserialize(metadata_slice: &[u8]) -> Result<Self, DeserializeError> {
        let archived = Self::archive_from_slice(metadata_slice)?;
        Self::deserialize_from_archive(archived)
    }

    unsafe fn archive_from_slice<'a>(metadata_slice: &'a [u8]) -> Result<&'a ArchivedModuleMetadata, DeserializeError> {
        let mut pos: [u8; 8] = Default::default();
        let endian = metadata_slice[0];
        if (cfg!(target_endian = "big") && endian == b'l') || (cfg!(target_endian = "little") && endian == b'b') {
            return Err(DeserializeError::Incompatible("incompatible endian".into()));
        }
        pos.copy_from_slice(&metadata_slice[1..9]);
        let pos: u64 = u64::from_le_bytes(pos);
        Ok(archived_value::<ModuleMetadata>(&metadata_slice[9..], pos as usize))
    }

    pub fn deserialize_from_archive(archived: &ArchivedModuleMetadata) -> Result<Self, DeserializeError> {
        let mut deserializer = SharedDeserializerAdapter::new(AllocDeserializer);
        RkyvDeserialize::deserialize(archived, &mut deserializer)
            .map_err(|e| DeserializeError::CorruptedBinary(format!("{:?}", e)))
    }
}

impl<'a> SymbolRegistry for ModuleMetadataSymbolRegistry<'a> {
    fn symbol_to_name(&self, symbol: Symbol) -> String {
        match symbol {
            Symbol::LocalFunction(index) => {
                format!("wasmer_function_{}_{}", self.prefix, index.index())
            }
            Symbol::Section(index) => format!("wasmer_section_{}_{}", self.prefix, index.index()),
            Symbol::FunctionCallTrampoline(index) => format!(
                "wasmer_trampoline_function_call_{}_{}",
                self.prefix,
                index.index()
            ),
            Symbol::DynamicFunctionTrampoline(index) => format!(
                "wasmer_trampoline_dynamic_function_{}_{}",
                self.prefix,
                index.index()
            ),
        }
    }

    fn name_to_symbol(&self, name: &str) -> Option<Symbol> {
        if let Some(index) = name.strip_prefix(&format!("wasmer_function_{}_", self.prefix)) {
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
