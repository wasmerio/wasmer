use crate::{
    module::{Module, ModuleInfo},
    sys::Memory,
};
use memmap::Mmap;
use serde_bench::{deserialize, serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Seek, SeekFrom, Write},
    mem,
    path::Path,
    slice,
};

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

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WasmHash([u8; 32]);

impl WasmHash {
    pub fn generate(wasm: &[u8]) -> Self {
        let mut array = [0u8; 32];
        array.copy_from_slice(Sha256::digest(wasm).as_slice());
        WasmHash(array)
    }

    pub fn encode(self) -> String {
        hex::encode(self.0)
    }

    pub(crate) fn into_array(self) -> [u8; 32] {
        self.0
    }
}

const CURRENT_CACHE_VERSION: u64 = 0;

/// The header of a cache file.
#[repr(C, packed)]
struct SerializedCacheHeader {
    magic: [u8; 8], // [W, A, S, M, E, R, \0, \0]
    version: u64,
    data_len: u64,
    wasm_hash: [u8; 32], // Sha256 of the wasm in binary format.
}

impl SerializedCacheHeader {
    pub fn read_from_slice(buffer: &[u8]) -> Result<(&Self, &[u8]), Error> {
        if buffer.len() >= mem::size_of::<SerializedCacheHeader>() {
            if &buffer[..8] == "WASMER\0\0".as_bytes() {
                let (header_slice, body_slice) =
                    buffer.split_at(mem::size_of::<SerializedCacheHeader>());
                let header = unsafe { &*(header_slice.as_ptr() as *const SerializedCacheHeader) };

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
        let ptr = self as *const SerializedCacheHeader as *const u8;
        unsafe { slice::from_raw_parts(ptr, mem::size_of::<SerializedCacheHeader>()) }
    }
}

#[derive(Serialize, Deserialize)]
struct SerializedCacheInner {
    info: Box<ModuleInfo>,
    #[serde(with = "serde_bytes")]
    backend_metadata: Box<[u8]>,
    compiled_code: Memory,
}

pub struct SerializedCache {
    inner: SerializedCacheInner,
}

impl SerializedCache {
    pub(crate) fn from_parts(
        info: Box<ModuleInfo>,
        backend_metadata: Box<[u8]>,
        compiled_code: Memory,
    ) -> Self {
        Self {
            inner: SerializedCacheInner {
                info,
                backend_metadata,
                compiled_code,
            },
        }
    }

    pub fn open<P>(path: P) -> Result<Self, Error>
    where
        P: AsRef<Path>,
    {
        let file = File::open(path).map_err(|e| Error::IoError(e))?;

        let mmap = unsafe { Mmap::map(&file).map_err(|e| Error::IoError(e))? };

        let (_header, body_slice) = SerializedCacheHeader::read_from_slice(&mmap[..])?;

        let inner =
            deserialize(body_slice).map_err(|e| Error::DeserializeError(format!("{:#?}", e)))?;

        Ok(SerializedCache { inner })
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

    pub fn store<P>(&self, path: P) -> Result<(), Error>
    where
        P: AsRef<Path>,
    {
        let mut file = File::create(path).map_err(|e| Error::IoError(e))?;

        let mut buffer = Vec::new();

        serialize(&mut buffer, &self.inner).map_err(|e| Error::SerializeError(e.to_string()))?;

        let data_len = buffer.len() as u64;

        file.seek(SeekFrom::Start(
            mem::size_of::<SerializedCacheHeader>() as u64
        ))
        .map_err(|e| Error::IoError(e))?;

        file.write(buffer.as_slice())
            .map_err(|e| Error::IoError(e))?;

        file.seek(SeekFrom::Start(0))
            .map_err(|e| Error::Unknown(e.to_string()))?;

        let wasm_hash = self.inner.info.wasm_hash.into_array();

        let cache_header = SerializedCacheHeader {
            magic: [
                'W' as u8, 'A' as u8, 'S' as u8, 'M' as u8, 'E' as u8, 'R' as u8, 0, 0,
            ],
            version: CURRENT_CACHE_VERSION,
            data_len,
            wasm_hash,
        };

        file.write(cache_header.as_slice())
            .map_err(|e| Error::IoError(e))?;

        Ok(())
    }
}

pub trait Cache {
    type LoadError;
    type StoreError;

    unsafe fn load(&self, key: WasmHash) -> Result<Module, Self::LoadError>;
    fn store(&mut self, module: Module) -> Result<WasmHash, Self::StoreError>;
}
