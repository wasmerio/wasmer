use crate::{module::ModuleInfo, sys::Memory};
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

const CURRENT_CACHE_VERSION: u64 = 0;

/// The header of a cache file.
#[repr(C, packed)]
struct CacheHeader {
    magic: [u8; 8], // [W, A, S, M, E, R, \0, \0]
    version: u64,
    data_len: u64,
    wasm_hash: [u8; 32], // Sha256 of the wasm in binary format.
}

impl CacheHeader {
    pub fn read_from_slice(buffer: &[u8]) -> Result<(&CacheHeader, &[u8]), Error> {
        if buffer.len() >= mem::size_of::<CacheHeader>() {
            if &buffer[..8] == "WASMER\0\0".as_bytes() {
                let (header_slice, body_slice) = buffer.split_at(mem::size_of::<CacheHeader>());
                let header = unsafe { &*(header_slice.as_ptr() as *const CacheHeader) };

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
        let ptr = self as *const CacheHeader as *const u8;
        unsafe { slice::from_raw_parts(ptr, mem::size_of::<CacheHeader>()) }
    }
}

#[derive(Serialize, Deserialize)]
struct CacheInner {
    info: Box<ModuleInfo>,
    #[serde(with = "serde_bytes")]
    backend_metadata: Vec<u8>,
    compiled_code: Memory,
}

pub struct Cache {
    inner: CacheInner,
    wasm_hash: Box<[u8; 32]>,
}

impl Cache {
    pub(crate) fn new(
        wasm: &[u8],
        info: Box<ModuleInfo>,
        backend_metadata: Vec<u8>,
        compiled_code: Memory,
    ) -> Self {
        let wasm_hash = hash_data(wasm);

        Self {
            inner: CacheInner {
                info,
                backend_metadata,
                compiled_code,
            },
            wasm_hash: Box::new(wasm_hash),
        }
    }

    pub fn open<P>(path: P) -> Result<Cache, Error>
    where
        P: AsRef<Path>,
    {
        let file = File::open(path).map_err(|e| Error::IoError(e))?;

        let mmap = unsafe { Mmap::map(&file).map_err(|e| Error::IoError(e))? };

        let (header, body_slice) = CacheHeader::read_from_slice(&mmap[..])?;

        let inner =
            deserialize(body_slice).map_err(|e| Error::DeserializeError(format!("{:#?}", e)))?;

        Ok(Cache {
            inner,
            wasm_hash: Box::new(header.wasm_hash),
        })
    }

    pub fn info(&self) -> &ModuleInfo {
        &self.inner.info
    }

    pub fn wasm_hash(&self) -> &[u8; 32] {
        &self.wasm_hash
    }

    #[doc(hidden)]
    pub fn consume(self) -> (ModuleInfo, Vec<u8>, Memory) {
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

        file.seek(SeekFrom::Start(mem::size_of::<CacheHeader>() as u64))
            .map_err(|e| Error::IoError(e))?;

        file.write(buffer.as_slice())
            .map_err(|e| Error::IoError(e))?;

        file.seek(SeekFrom::Start(0))
            .map_err(|e| Error::Unknown(e.to_string()))?;

        let wasm_hash = {
            let mut array = [0u8; 32];
            array.copy_from_slice(&*self.wasm_hash);
            array
        };

        let cache_header = CacheHeader {
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

pub fn hash_data(data: &[u8]) -> [u8; 32] {
    let mut array = [0u8; 32];
    array.copy_from_slice(Sha256::digest(data).as_slice());
    array
}
