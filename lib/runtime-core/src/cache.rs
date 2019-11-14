//! The cache module provides the common data structures used by compiler backends to allow
//! serializing compiled wasm code to a binary format.  The binary format can be persisted,
//! and loaded to allow skipping compilation and fast startup.

use crate::{
    backend::Backend,
    module::{Module, ModuleInfo},
    sys::Memory,
};
use blake2b_simd::blake2bp;
use std::{fmt, io, mem, slice};

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
    UnsupportedBackend(Backend),
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
// WasmHash is made up of two 32 byte arrays instead of a 64 byte array
// because derive only works on fixed sized arrays size 32 or below
// TODO: fix this when this gets fixed by improved const generics
pub struct WasmHash([u8; 32], [u8; 32]);

impl WasmHash {
    /// Hash a wasm module.
    ///
    /// # Note:
    /// This does no verification that the supplied data
    /// is, in fact, a wasm module.
    pub fn generate(wasm: &[u8]) -> Self {
        let mut first_part = [0u8; 32];
        let mut second_part = [0u8; 32];

        let mut state = blake2bp::State::new();
        state.update(wasm);

        let hasher = state.finalize();
        let generic_array = hasher.as_bytes();

        first_part.copy_from_slice(&generic_array[0..32]);
        second_part.copy_from_slice(&generic_array[32..64]);
        WasmHash(first_part, second_part)
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
        if bytes.len() != 64 {
            return Err(Error::DeserializeError(
                "Prehashed keys must deserialze into exactly 64 bytes".to_string(),
            ));
        }
        use std::convert::TryInto;
        Ok(WasmHash(
            bytes[0..32].try_into().map_err(|e| {
                Error::DeserializeError(format!("Could not get first 32 bytes: {}", e))
            })?,
            bytes[32..64].try_into().map_err(|e| {
                Error::DeserializeError(format!("Could not get last 32 bytes: {}", e))
            })?,
        ))
    }

    pub(crate) fn into_array(self) -> [u8; 64] {
        let mut total = [0u8; 64];
        total[0..32].copy_from_slice(&self.0);
        total[32..64].copy_from_slice(&self.1);
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
                    Err(Error::InvalidatedCache)
                }
            } else {
                Err(Error::InvalidFile(InvalidFileType::InvalidMagic))
            }
        } else {
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

#[derive(Serialize, Deserialize)]
struct ArtifactInner {
    info: Box<ModuleInfo>,
    #[serde(with = "serde_bytes")]
    backend_metadata: Box<[u8]>,
    compiled_code: Memory,
}

/// Artifact are produced by caching, are serialized/deserialized to binaries, and contain
/// module info, backend metadata, and compiled code.
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

    /// Deserializes an `Artifact` from the given byte slice.
    pub fn deserialize(bytes: &[u8]) -> Result<Self, Error> {
        let (_, body_slice) = ArtifactHeader::read_from_slice(bytes)?;

        let inner = serde_bench::deserialize(body_slice)
            .map_err(|e| Error::DeserializeError(format!("{:#?}", e)))?;

        Ok(Artifact { inner })
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

/// A generic cache for storing and loading compiled wasm modules.
///
/// The `wasmer-runtime` supplies a naive `FileSystemCache` api.
pub trait Cache {
    /// Error type to return when load error occurs
    type LoadError: fmt::Debug;
    /// Error type to return when store error occurs
    type StoreError: fmt::Debug;

    /// loads a module using the default `Backend`
    fn load(&self, key: WasmHash) -> Result<Module, Self::LoadError>;
    /// loads a cached module using a specific `Backend`
    fn load_with_backend(&self, key: WasmHash, backend: Backend)
        -> Result<Module, Self::LoadError>;
    /// Store a module into the cache with the given key
    fn store(&mut self, key: WasmHash, module: Module) -> Result<(), Self::StoreError>;
}

/// A unique ID generated from the version of Wasmer for use with cache versioning
pub const WASMER_VERSION_HASH: &'static str =
    include_str!(concat!(env!("OUT_DIR"), "/wasmer_version_hash.txt"));
