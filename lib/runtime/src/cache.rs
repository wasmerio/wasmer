//! The cache module provides the common data structures used by compiler backends to allow
//! serializing compiled wasm code to a binary format.  The binary format can be persisted,
//! and loaded to allow skipping compilation and fast startup.

use crate::Module;
use memmap::Mmap;
use std::{
    fs::{create_dir_all, File},
    io::{self, Write},
    path::PathBuf,
};

use wasmer_runtime_core::cache::Error as CacheError;
pub use wasmer_runtime_core::{
    backend::Backend,
    cache::{Artifact, Cache, WasmHash},
};

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
        let path: PathBuf = path.into();
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
        self.load_with_backend(key, Backend::default())
    }

    fn load_with_backend(&self, key: WasmHash, backend: Backend) -> Result<Module, CacheError> {
        let filename = key.encode();
        let mut new_path_buf = self.path.clone();
        new_path_buf.push(backend.to_string());
        new_path_buf.push(filename);
        let file = File::open(new_path_buf)?;
        let mmap = unsafe { Mmap::map(&file)? };

        let serialized_cache = Artifact::deserialize(&mmap[..])?;
        unsafe {
            wasmer_runtime_core::load_cache_with(
                serialized_cache,
                crate::compiler_for_backend(backend)
                    .ok_or_else(|| CacheError::UnsupportedBackend(backend))?
                    .as_ref(),
            )
        }
    }

    fn store(&mut self, key: WasmHash, module: Module) -> Result<(), CacheError> {
        let filename = key.encode();
        let backend_str = module.info().backend.to_string();
        let mut new_path_buf = self.path.clone();
        new_path_buf.push(backend_str);

        let serialized_cache = module.cache()?;
        let buffer = serialized_cache.serialize()?;

        std::fs::create_dir_all(&new_path_buf)?;
        new_path_buf.push(filename);
        let mut file = File::create(new_path_buf)?;
        file.write_all(&buffer)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use std::env;

    #[test]
    fn test_file_system_cache_run() {
        use crate::{compile, imports, Func};
        use wabt::wat2wasm;

        static WAT: &'static str = r#"
            (module
              (type $t0 (func (param i32) (result i32)))
              (func $add_one (export "add_one") (type $t0) (param $p0 i32) (result i32)
                get_local $p0
                i32.const 1
                i32.add))
        "#;

        let wasm = wat2wasm(WAT).unwrap();

        let module = compile(&wasm).unwrap();

        let cache_dir = env::temp_dir();
        println!("test temp_dir {:?}", cache_dir);

        let mut fs_cache = unsafe {
            FileSystemCache::new(cache_dir)
                .map_err(|e| format!("Cache error: {:?}", e))
                .unwrap()
        };
        // store module
        let key = WasmHash::generate(&wasm);
        fs_cache.store(key, module.clone()).unwrap();

        // load module
        let cached_module = fs_cache.load(key).unwrap();

        let import_object = imports! {};
        let instance = cached_module.instantiate(&import_object).unwrap();
        let add_one: Func<i32, i32> = instance.func("add_one").unwrap();

        let value = add_one.call(42).unwrap();

        // verify it works
        assert_eq!(value, 43);
    }
}
