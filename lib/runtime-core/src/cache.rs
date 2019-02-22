use crate::{
    module::{Module, ModuleInfo},
    sys::Memory,
};
use sha2::{Digest, Sha256};
use std::{io, mem, slice, fmt};

#[derive(Debug)]
pub enum InvalidFileType {
    InvalidSize,
    InvalidMagic,
}

#[derive(Debug)]
pub enum Error {
    IoError(io::Error),
    DeserializeError(String),
    SerializeError(String),
    Unknown(String),
    InvalidFile(InvalidFileType),
    InvalidatedCache,
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
pub struct WasmHash([u8; 32]);

impl WasmHash {
    /// Hash a wasm module.
    ///
    /// # Note:
    /// This does no verification that the supplied data
    /// is, in fact, a wasm module.
    pub fn generate(wasm: &[u8]) -> Self {
        let mut array = [0u8; 32];
        array.copy_from_slice(Sha256::digest(wasm).as_slice());
        WasmHash(array)
    }

    /// Create the hexadecimal representation of the
    /// stored hash.
    pub fn encode(self) -> String {
        hex::encode(self.0)
    }

    pub(crate) fn into_array(self) -> [u8; 32] {
        self.0
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
    wasm_hash: [u8; 32], // Sha256 of the wasm in binary format.
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

    pub fn deserialize(bytes: &[u8]) -> Result<Self, Error> {
        let (_, body_slice) = ArtifactHeader::read_from_slice(bytes)?;

        let inner = serde_bench::deserialize(body_slice)
            .map_err(|e| Error::DeserializeError(format!("{:#?}", e)))?;

        Ok(Artifact { inner })
    }

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

    pub fn serialize(&self) -> Result<Vec<u8>, Error> {
        let cache_header = ArtifactHeader {
            magic: WASMER_CACHE_MAGIC,
            version: CURRENT_CACHE_VERSION,
            data_len: 0,
            wasm_hash: self.inner.info.wasm_hash.into_array(),
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
    type LoadError: fmt::Debug;
    type StoreError: fmt::Debug;

    fn load(&self, key: WasmHash) -> Result<Module, Self::LoadError>;
    fn store(&mut self, module: Module) -> Result<WasmHash, Self::StoreError>;
}
