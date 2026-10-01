#![cfg_attr(not(feature = "filesystem"), allow(unused))]
use crate::cache::Cache;
use crate::hash::Hash;
use std::fs::create_dir_all;
use std::io::{self, Write};
use std::path::PathBuf;
use wasmer::{AsEngineRef, DeserializeError, Module, SerializeError};

/// Representation of a directory that contains compiled wasm artifacts.
///
/// The `FileSystemCache` type implements the [`Cache`] trait, which allows it to be used
/// generically when some sort of cache is required.
///
/// # Usage
///
/// ```
/// use wasmer::{DeserializeError, SerializeError};
/// use wasmer_cache::{Cache, FileSystemCache, Hash};
///
/// # use wasmer::{Module};
/// fn store_module(module: &Module, bytes: &[u8]) -> Result<(), SerializeError> {
///     // Create a new file system cache.
///     let mut fs_cache = FileSystemCache::new("some/directory/goes/here")?;
///
///     // Compute a key for a given WebAssembly binary
///     let key = Hash::generate(bytes);
///
///     // Store a module into the cache given a key
///     fs_cache.store(key, module)?;
///
///     Ok(())
/// }
/// ```
#[derive(Debug, Clone)]
pub struct FileSystemCache {
    path: PathBuf,
    ext: Option<String>,
}

#[cfg(feature = "filesystem")]
impl FileSystemCache {
    /// Construct a new `FileSystemCache` around the specified directory.
    pub fn new<P: Into<PathBuf>>(path: P) -> io::Result<Self> {
        let path: PathBuf = path.into();
        if path.exists() {
            let metadata = path.metadata()?;
            if metadata.is_dir() {
                if !metadata.permissions().readonly() {
                    Ok(Self { path, ext: None })
                } else {
                    // This directory is readonly.
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        format!("the supplied path is readonly: {}", path.display()),
                    ))
                }
            } else {
                // This path points to a file.
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!(
                        "the supplied path already points to a file: {}",
                        path.display()
                    ),
                ))
            }
        } else {
            // Create the directory and any parent directories if they don't yet exist.
            let res = create_dir_all(&path);
            if res.is_err() {
                Err(io::Error::other(format!(
                    "failed to create cache directory: {}",
                    path.display()
                )))
            } else {
                Ok(Self { path, ext: None })
            }
        }
    }

    /// Set the extension for this cached file.
    ///
    /// This is needed for loading native files from Windows, as otherwise
    /// loading the library will fail (it requires a `.dll` extension)
    pub fn set_cache_extension(&mut self, ext: Option<impl ToString>) {
        self.ext = ext.map(|ext| ext.to_string());
    }
}

#[cfg(feature = "filesystem")]
impl Cache for FileSystemCache {
    type DeserializeError = DeserializeError;
    type SerializeError = SerializeError;

    unsafe fn load(
        &self,
        engine: &impl AsEngineRef,
        key: Hash,
    ) -> Result<Module, Self::DeserializeError> {
        let filename = if let Some(ref ext) = self.ext {
            format!("{key}.{ext}")
        } else {
            key.to_string()
        };
        let path = self.path.join(filename);
        let ret = unsafe { Module::deserialize_from_file(engine, path.clone()) };
        if ret.is_err() {
            // If an error occurs while deserializing then we can not trust it anymore
            // so delete the cache file
            let _ = std::fs::remove_file(path);
        }
        ret
    }

    fn store(&mut self, key: Hash, module: &Module) -> Result<(), Self::SerializeError> {
        let filename = if let Some(ref ext) = self.ext {
            format!("{key}.{ext}")
        } else {
            key.to_string()
        };
        let path = self.path.join(filename);
        let buffer = module.serialize()?;
        // Deserialization maps cache files. Truncating an existing entry while
        // another thread reads that mapping can deliver SIGBUS to the host.
        // Write a complete sibling inode and atomically replace the path so
        // readers with an open handle continue to see the old complete entry.
        let mut file = tempfile::NamedTempFile::new_in(&self.path)?;
        file.write_all(&buffer)?;
        file.persist(path).map_err(|error| error.error)?;

        Ok(())
    }
}

#[cfg(all(test, feature = "filesystem"))]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn test_fs_cache() {
        let dir = tempfile::tempdir().unwrap();

        let mut cache = FileSystemCache::new(dir.path()).unwrap();

        let engine = wasmer::Engine::default();

        let bytes = b"\0asm\x01\0\0\0";

        let module = Module::from_binary(&engine, bytes).unwrap();
        let key = Hash::generate(bytes);

        cache.store(key, &module).unwrap();
        let _restored = unsafe { cache.load(&engine, key).unwrap() };
    }

    #[test]
    fn replacing_cache_entry_does_not_truncate_open_reader() {
        let dir = tempfile::tempdir().unwrap();
        let mut cache = FileSystemCache::new(dir.path()).unwrap();
        let engine = wasmer::Engine::default();
        let bytes = b"\0asm\x01\0\0\0";
        let module = Module::from_binary(&engine, bytes).unwrap();
        let other_bytes = b"\0asm\x01\0\0\0\x01\x04\x01\x60\0\0\x03\x02\x01\0\x07\x05\x01\x01f\0\0\x0a\x04\x01\x02\0\x0b";
        let other_module = Module::from_binary(&engine, other_bytes).unwrap();
        let key = Hash::generate(bytes);
        cache.store(key, &module).unwrap();

        let path = dir.path().join(key.to_string());
        let original_contents = std::fs::read(&path).unwrap();
        let mut reader = std::fs::File::open(&path).unwrap();
        // A cache key is merely a caller-provided value: replacement must
        // preserve readers of the old inode even when the new artifact differs.
        cache.store(key, &other_module).unwrap();

        let mut old_contents = Vec::new();
        reader.read_to_end(&mut old_contents).unwrap();
        assert_eq!(old_contents, original_contents);
        assert_ne!(std::fs::read(&path).unwrap(), original_contents);
        let restored = unsafe { cache.load(&engine, key).unwrap() };
        assert_eq!(restored.exports().count(), 1);
    }
}
