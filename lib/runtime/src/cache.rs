use crate::Module;
use memmap::Mmap;
use std::{
    fs::{create_dir_all, File},
    io::{self, Write},
    path::PathBuf,
};

use wasmer_runtime_core::cache::Error as CacheError;
pub use wasmer_runtime_core::cache::{Artifact, Cache, WasmHash, WASMER_VERSION_HASH};

/// Representation of a directory that contains compiled wasm artifacts.
///
/// The `FileSystemCache` type implements the [`Cache`] trait, which allows it to be used
/// generically when some sort of cache is required.
///
/// [`Cache`]: trait.Cache.html
///
/// # Usage:
///
/// ```rust
/// use wasmer_runtime::cache::{Cache, FileSystemCache, WasmHash};
///
/// # use wasmer_runtime::{Module, error::CacheError};
/// fn store_module(module: Module) -> Result<Module, CacheError> {
///     // Create a new file system cache.
///     // This is unsafe because we can't ensure that the artifact wasn't
///     // corrupted or tampered with.
///     let mut fs_cache = unsafe { FileSystemCache::new("some/directory/goes/here")? };
///     // Compute a key for a given WebAssembly binary
///     let key = WasmHash::generate(&[]);
///     // Store a module into the cache given a key
///     fs_cache.store(key, module.clone())?;
///     Ok(module)
/// }
/// ```
pub struct FileSystemCache {
    path: PathBuf,
}

impl FileSystemCache {
    /// Construct a new `FileSystemCache` around the specified directory.
    /// The contents of the cache are stored in sub-versioned directories.
    ///
    /// # Note:
    /// This method is unsafe because there's no way to ensure the artifacts
    /// stored in this cache haven't been corrupted or tampered with.
    pub unsafe fn new<P: Into<PathBuf>>(path: P) -> io::Result<Self> {
        let path: PathBuf = {
            let mut path = path.into();
            path.push(WASMER_VERSION_HASH);
            path
        };

        if path.exists() {
            let metadata = path.metadata()?;
            if metadata.is_dir() {
                if !metadata.permissions().readonly() {
                    Ok(Self { path })
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
            create_dir_all(&path)?;
            Ok(Self { path })
        }
    }
}

impl Cache for FileSystemCache {
    type LoadError = CacheError;
    type StoreError = CacheError;

    fn load(&self, key: WasmHash) -> Result<Module, CacheError> {
        let filename = key.encode();
        let mut new_path_buf = self.path.clone();
        new_path_buf.push(filename);
        let file = File::open(new_path_buf)?;
        let mmap = unsafe { Mmap::map(&file)? };

        let serialized_cache = Artifact::deserialize(&mmap[..])?;
        unsafe {
            wasmer_runtime_core::load_cache_with(serialized_cache, &super::default_compiler())
        }
    }

    fn store(&mut self, key: WasmHash, module: Module) -> Result<(), CacheError> {
        let filename = key.encode();
        let mut new_path_buf = self.path.clone();
        new_path_buf.push(filename);

        let serialized_cache = module.cache()?;
        let buffer = serialized_cache.serialize()?;

        let mut file = File::create(new_path_buf)?;
        file.write_all(&buffer)?;

        Ok(())
    }
}
