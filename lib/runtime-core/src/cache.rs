//! The cache module provides the common data structures used by compiler backends to allow
//! serializing compiled wasm code to a binary format.  The binary format can be persisted,
//! and loaded to allow skipping compilation and fast startup.

use crate::{module::ModuleInfo, sys::Memory, sys::ArchivableMemory};
use rkyv::{Archive, Serialize as RkyvSerialize, Deserialize as RkyvDeserialize};
use std::{io, mem, slice};

/// Indicates the invalid type of invalid cache file
#[derive(Debug)]
pub enum InvalidFileType {
    /// Given cache header slice does not match the expected size of an `ArtifactHeader`
    InvalidSize,
    /// Given cache header slice does not contain the expected magic bytes
    InvalidMagic,
}

/// Kinds of caching errors
#[derive(Debug)]
pub enum Error {
    /// An IO error while reading/writing a cache binary.
    IoError(io::Error),
    /// An error deserializing bytes into a cache data structure.
    DeserializeError(String),
    /// An error serializing bytes from a cache data structure.
    SerializeError(String),
    /// An undefined caching error with a message.
    Unknown(String),
    /// An invalid cache binary given.
    InvalidFile(InvalidFileType),
    /// The cached binary has been invalidated.
    InvalidatedCache,
    /// The current backend does not support caching.
    UnsupportedBackend(String),
}

impl From<io::Error> for Error {
    fn from(io_err: io::Error) -> Self {
        Error::IoError(io_err)
    }
}

/// The hash of a wasm module.
///
/// Used as a key when loading and storing modules in a [`Cache`].
///
/// [`Cache`]: trait.Cache.html
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
// WasmHash is made up of a 32 byte array
pub struct WasmHash([u8; 32]);

impl WasmHash {
    /// Hash a wasm module.
    ///
    /// # Note:
    /// This does no verification that the supplied data
    /// is, in fact, a wasm module.
    pub fn generate(wasm: &[u8]) -> Self {
        let hash = blake3::hash(wasm);
        WasmHash(hash.into())
    }

    /// Create the hexadecimal representation of the
    /// stored hash.
    pub fn encode(self) -> String {
        hex::encode(&self.into_array() as &[u8])
    }

    /// Create hash from hexadecimal representation
    pub fn decode(hex_str: &str) -> Result<Self, Error> {
        let bytes = hex::decode(hex_str).map_err(|e| {
            Error::DeserializeError(format!(
                "Could not decode prehashed key as hexadecimal: {}",
                e
            ))
        })?;
        if bytes.len() != 32 {
            return Err(Error::DeserializeError(
                "Prehashed keys must deserialze into exactly 32 bytes".to_string(),
            ));
        }
        use std::convert::TryInto;
        Ok(WasmHash(bytes[0..32].try_into().map_err(|e| {
            Error::DeserializeError(format!("Could not get first 32 bytes: {}", e))
        })?))
    }

    pub(crate) fn into_array(self) -> [u8; 32] {
        let mut total = [0u8; 32];
        total[0..32].copy_from_slice(&self.0);
        total
    }
}

const CURRENT_CACHE_VERSION: u64 = 0;
static WASMER_CACHE_MAGIC: [u8; 8] = *b"WASMER\0\0";

/// The header of a cache file.
#[repr(C, packed)]
struct ArtifactHeader {
    magic: [u8; 8], // [W, A, S, M, E, R, \0, \0]
    version: u64,
    data_len: u64,
}

impl ArtifactHeader {
    pub fn read_from_slice(buffer: &[u8]) -> Result<(&Self, &[u8]), Error> {
        if buffer.len() >= mem::size_of::<ArtifactHeader>() {
            if &buffer[..8] == &WASMER_CACHE_MAGIC {
                let (header_slice, body_slice) = buffer.split_at(mem::size_of::<ArtifactHeader>());
                let header = unsafe { &*(header_slice.as_ptr() as *const ArtifactHeader) };

                if header.version == CURRENT_CACHE_VERSION {
                    Ok((header, body_slice))
                } else {
                    println!("invalidated cache");
                    Err(Error::InvalidatedCache)
                }
            } else {
                println!("invalid magic");
                Err(Error::InvalidFile(InvalidFileType::InvalidMagic))
            }
        } else {
            println!("invalid size");
            Err(Error::InvalidFile(InvalidFileType::InvalidSize))
        }
    }

    pub fn read_from_slice_mut(buffer: &mut [u8]) -> Result<(&mut Self, &mut [u8]), Error> {
        if buffer.len() >= mem::size_of::<ArtifactHeader>() {
            if &buffer[..8] == &WASMER_CACHE_MAGIC {
                let (header_slice, body_slice) =
                    buffer.split_at_mut(mem::size_of::<ArtifactHeader>());
                let header = unsafe { &mut *(header_slice.as_ptr() as *mut ArtifactHeader) };

                if header.version == CURRENT_CACHE_VERSION {
                    Ok((header, body_slice))
                } else {
                    Err(Error::InvalidatedCache)
                }
            } else {
                Err(Error::InvalidFile(InvalidFileType::InvalidMagic))
            }
        } else {
            Err(Error::InvalidFile(InvalidFileType::InvalidSize))
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        let ptr = self as *const ArtifactHeader as *const u8;
        unsafe { slice::from_raw_parts(ptr, mem::size_of::<ArtifactHeader>()) }
    }
}


/// Inner information of an Artifact.
#[derive(Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct ArtifactInner {
    info: Box<ModuleInfo>,
    #[serde(with = "serde_bytes")]
    backend_metadata: Box<[u8]>,

    #[with(ArchivableMemory)]
    compiled_code: Memory,
}

/// Artifact are produced by caching, are serialized/deserialized to binaries, and contain
/// module info, backend metadata, and compiled code.
#[derive(Archive, RkyvSerialize, RkyvDeserialize)]
pub struct Artifact {
    inner: ArtifactInner,
}

impl Artifact {
    pub(crate) fn from_parts(
        info: Box<ModuleInfo>,
        backend_metadata: Box<[u8]>,
        compiled_code: Memory,
    ) -> Self {
        Self {
            inner: ArtifactInner {
                info,
                backend_metadata,
                compiled_code,
            },
        }
    }

    /// A reference to the `Artifact`'s stored `ModuleInfo`
    pub fn info(&self) -> &ModuleInfo {
        &self.inner.info
    }

    #[doc(hidden)]
    pub fn consume(self) -> (ModuleInfo, Box<[u8]>, Memory) {
        (
            *self.inner.info,
            self.inner.backend_metadata,
            self.inner.compiled_code,
        )
    }

    /// Deserializes an `Artifact` from the given byte slice.
    pub fn deserialize(bytes: &[u8]) -> Result<Self, Error> {
        let (_, body_slice) = ArtifactHeader::read_from_slice(bytes)?;
        
        let inner = serde_bench::deserialize(body_slice)
            .map_err(|e| Error::DeserializeError(format!("{:#?}", e)))?;

        Ok(Artifact { inner })
    }

    /// Serializes the `Artifact` into a vector of bytes
    pub fn serialize(&self) -> Result<Vec<u8>, Error> {
        let cache_header = ArtifactHeader {
            magic: WASMER_CACHE_MAGIC,
            version: CURRENT_CACHE_VERSION,
            data_len: 0,
        };

        let mut buffer = cache_header.as_slice().to_vec();

        serde_bench::serialize(&mut buffer, &self.inner)
            .map_err(|e| Error::SerializeError(e.to_string()))?;

        let data_len = (buffer.len() - mem::size_of::<ArtifactHeader>()) as u64;
        let (header, _) = ArtifactHeader::read_from_slice_mut(&mut buffer)?;
        header.data_len = data_len;

        Ok(buffer)
    }
}

/// A unique ID generated from the version of Wasmer for use with cache versioning
pub const WASMER_VERSION_HASH: &'static str =
    include_str!(concat!(env!("OUT_DIR"), "/wasmer_version_hash.txt"));

#[cfg(test)]
mod tests {
    use super::Artifact;
    use super::ArtifactInner;
    use super::Memory;
    use super::ModuleInfo;
    use std::collections::HashMap;
    use crate::structures::Map;
    use crate::module::StringTable;
    use rkyv::ser::serializers::AllocSerializer;
    use rkyv::ser::Serializer as RkyvSerializer;
    use rkyv::Deserialize;
    use rkyv::Archived;

    #[test]
    fn test_rkyv_artifact() {
        let bytes = make_test_bytes();
        let memory = make_test_memory(&bytes);

        let module_info = make_empty_module_info();
        let artifact = Artifact::from_parts(
            Box::new(module_info), 
            b"test_backend".to_vec().into_boxed_slice(),
            memory,
        );

        let mut serializer = AllocSerializer::<4096>::default();
        serializer.serialize_value(&artifact).unwrap();
        let serialized = serializer.into_serializer().into_inner();
        assert!(serialized.len() > 0);
        print!("{:?}", serialized);

        let archived: &Archived<Artifact>
            = unsafe { rkyv::archived_root::<Artifact>(&serialized[..]) };

        let deserialized_artifact = Deserialize::<Artifact, _>::deserialize(archived, &mut rkyv::Infallible).unwrap();
        unsafe { assert_eq!(deserialized_artifact.inner.compiled_code.as_slice(), artifact.inner.compiled_code.as_slice()) };
        assert_eq!(deserialized_artifact.inner.compiled_code.protection(), artifact.inner.compiled_code.protection());
    }

    #[test]
    fn test_rkyv_artifact_inner() {
        let bytes = make_test_bytes();
        let memory = make_test_memory(&bytes);

        let module_info = make_empty_module_info();
        let artifact_inner = ArtifactInner {
            info: Box::new(module_info),
            backend_metadata: b"test_backend".to_vec().into_boxed_slice(),
            compiled_code: memory,
        };

        let mut serializer = AllocSerializer::<4096>::default();
        serializer.serialize_value(&artifact_inner).unwrap();
        let serialized = serializer.into_serializer().into_inner();
        assert!(serialized.len() > 0);
        print!("{:?}", serialized);

        let archived: &Archived<ArtifactInner> 
            = unsafe { rkyv::archived_root::<ArtifactInner>(&serialized[..]) };

        let deserialized_artifact_inner = Deserialize::<ArtifactInner, _>::deserialize(archived, &mut rkyv::Infallible).unwrap();
        unsafe { assert_eq!(deserialized_artifact_inner.compiled_code.as_slice(), artifact_inner.compiled_code.as_slice()) };
        assert_eq!(deserialized_artifact_inner.compiled_code.protection(), artifact_inner.compiled_code.protection());
    }

    fn make_empty_module_info() -> ModuleInfo {
        ModuleInfo {
            memories: Map::new(),
            globals: Map::new(),
            tables: Map::new(),
            imported_functions: Map::new(),
            imported_memories: Map::new(),
            imported_tables: Map::new(),
            imported_globals: Map::new(),
            exports: Default::default(),
            data_initializers: Vec::new(),
            elem_initializers: Vec::new(),
            start_func: None,
            func_assoc: Map::new(),
            signatures: Map::new(),
            backend: "test".to_string(),
            namespace_table: StringTable::new(),
            name_table: StringTable::new(),
            em_symbol_map: None,
            custom_sections: HashMap::new(),
            generate_debug_info: false,
            #[cfg(feature = "generate-debug-information")]
            debug_info_manager: crate::jit_debug::JitCodeDebugInfoManager::new(),
        }
    }

    fn make_test_memory(bytes: &Vec<u8>) -> Memory {
        let mut memory = Memory::with_size_protect(1000, crate::sys::Protect::ReadWrite)
            .expect("Could not create memory");
        unsafe {
            memory.as_slice_mut().copy_from_slice(&bytes[..]);
        }
        memory
    }

    fn make_test_bytes() -> Vec<u8> {
        let page_size = page_size::get();
        let mut bytes = b"abcdefghijkl".to_vec();
        let padding_zeros = [0 as u8; 1].repeat(page_size - bytes.len());
        bytes.extend(padding_zeros);
        bytes
    }
}
