use crate::module::ModuleInfo;
// use bincode::{deserialize, deserialize_from, serialize_into};
use memmap::Mmap;
use serde_bench::{deserialize, serialize};
use std::{
    fs::File,
    io::{self, Write},
    path::Path,
};

#[derive(Debug)]
pub enum Error {
    IoError(io::Error),
    DeserializeError(String),
    SerializeError(String),
    Unknown(String),
}

#[cfg_attr(feature = "cache", derive(Serialize, Deserialize))]
pub struct Cache {
    pub info: Box<ModuleInfo>,
    #[serde(with = "serde_bytes")]
    pub backend_data: Vec<u8>,
}

impl Cache {
    pub fn open<P>(path: P) -> Result<Cache, Error>
    where
        P: AsRef<Path>,
    {
        let file = File::open(path).map_err(|e| Error::IoError(e))?;

        let mmap = unsafe { Mmap::map(&file).map_err(|e| Error::IoError(e))? };

        let cache =
            deserialize(&mmap[..]).map_err(|e| Error::DeserializeError(format!("{:#?}", e)))?;

        Ok(cache)
    }

    pub fn info(&self) -> &ModuleInfo {
        &self.info
    }

    pub fn consume(self) -> (ModuleInfo, Vec<u8>) {
        (*self.info, self.backend_data)
    }

    pub fn write_to_disk<P>(self, path: P) -> Result<(), Error>
    where
        P: AsRef<Path>,
    {
        let mut file = File::create(path).map_err(|e| Error::IoError(e))?;

        let mut buffer = Vec::new();

        serialize(&mut buffer, &self).map_err(|e| Error::SerializeError(e.to_string()))?;

        file.write(buffer.as_slice())
            .map_err(|e| Error::IoError(e))?;

        Ok(())
    }
}
