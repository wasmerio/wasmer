use crate::Module;
use std::{fs::create_dir_all, io, path::PathBuf};

pub use wasmer_runtime_core::cache::{Cache, WasmHash};
use wasmer_runtime_core::cache::{Error as CacheError, SerializedCache};

// ///
// /// # Drawbacks:
// ///
// /// Due to internal shortcomings, you cannot convert
// /// a module into a `Cache`. This means that compiling
// /// into a `Cache` and then converting into a module
// /// has more overhead than directly compiling
// /// into a [`Module`].
// ///
// /// [`Module`]: struct.Module.html
// pub struct Cache(pub(crate) CoreCache);

// impl Cache {
//     /// Load a `Cache` from the file specified by `path`.
//     ///
//     /// # Usage:
//     ///
//     /// ```
//     /// use wasmer_runtime::Cache;
//     /// # use wasmer_runtime::error::CacheError;
//     ///
//     /// # fn load_cache() -> Result<(), CacheError> {
//     /// let cache = Cache::load("some_file.cache")?;
//     /// # Ok(())
//     /// # }
//     /// ```
//     pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
//         CoreCache::open(path).map(|core_cache| Cache(core_cache))
//     }

//     /// Convert a `Cache` into a [`Module`].
//     ///
//     /// [`Module`]: struct.Module.html
//     ///
//     /// # Usage:
//     ///
//     /// ```
//     /// use wasmer_runtime::Cache;
//     ///
//     /// # use wasmer_runtime::error::CacheError;
//     /// # fn cache2module(cache: Cache) -> Result<(), CacheError> {
//     /// let module = unsafe { cache.into_module()? };
//     /// # Ok(())
//     /// # }
//     /// ```
//     ///
//     /// # Notes:
//     ///
//     /// This method is unsafe because the runtime cannot confirm
//     /// that this cache was not tampered with or corrupted.
//     pub unsafe fn into_module(self) -> Result<Module, Error> {
//         let default_compiler = super::default_compiler();

//         wasmer_runtime_core::load_cache_with(self.0, default_compiler)
//     }

//     /// Compare the Sha256 hash of the wasm this cache was build
//     /// from with some other WebAssembly.
//     ///
//     /// The main use-case for this is invalidating old caches.
//     pub fn compare_wasm(&self, wasm: &[u8]) -> bool {
//         let param_wasm_hash = hash_data(wasm);
//         self.0.wasm_hash() as &[u8] == &param_wasm_hash as &[u8]
//     }

//     /// Store this cache in a file.
//     ///
//     /// # Notes:
//     ///
//     /// If a file exists at the specified path, it will be overwritten.
//     ///
//     /// # Usage:
//     ///
//     /// ```
//     /// use wasmer_runtime::Cache;
//     ///
//     /// # use wasmer_runtime::error::CacheError;
//     /// # fn store_cache(cache: Cache) -> Result<(), CacheError> {
//     /// cache.store("some_file.cache")?;
//     /// # Ok(())
//     /// # }
//     /// ```
//     pub fn store<P: AsRef<Path>>(&self, path: P) -> Result<(), Error> {
//         self.0.store(path)
//     }
// }

pub struct FSCache {
    path: PathBuf,
}

impl FSCache {
    pub fn open<P: Into<PathBuf>>(path: P) -> io::Result<FSCache> {
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

impl Cache for FSCache {
    type LoadError = CacheError;
    type StoreError = CacheError;

    unsafe fn load(&self, key: WasmHash) -> Result<Module, CacheError> {
        let filename = key.encode();
        let mut new_path_buf = self.path.clone();
        new_path_buf.push(filename);

        let serialized_cache = SerializedCache::open(new_path_buf)?;
        wasmer_runtime_core::load_cache_with(serialized_cache, super::default_compiler())
    }

    fn store(&mut self, module: Module) -> Result<WasmHash, CacheError> {
        let key = module.info().wasm_hash;
        let filename = key.encode();
        let mut new_path_buf = self.path.clone();
        new_path_buf.push(filename);

        let serialized_cache = module.cache()?;
        serialized_cache.store(new_path_buf)?;

        Ok(key)
    }
}
