use crate::Module;
use std::path::Path;
use wasmer_runtime_core::cache::{hash_data, Cache as CoreCache};

pub use wasmer_runtime_core::cache::Error;

/// On-disk storage of compiled WebAssembly.
///
/// A `Cache` can be used to quickly reload already
/// compiled WebAssembly from a previous execution
/// during which the wasm was explicitly compiled
/// as a `Cache`.
///
/// # Usage:
///
/// ```
/// use wasmer_runtime::{compile_cache, Cache};
///
/// # use wasmer_runtime::error::{CompileResult, CacheError};
/// # fn make_cache(wasm: &[u8]) -> CompileResult<()> {
/// // Make a cache.
/// let cache = compile_cache(wasm)?;
///
/// # Ok(())
/// # }
/// # fn usage_cache(cache: Cache) -> Result<(), CacheError> {
/// // Store the cache in a file.
/// cache.store("some_cache_file")?;
///
/// // Load the cache.
/// let cache = Cache::load("some_cache_file")?;
/// let module = unsafe { cache.into_module()? };
/// # Ok(())
/// # }
/// ```
///
/// # Performance Characteristics:
///
/// Loading caches from files has been optimized for latency.
/// There is still more work to do that will reduce
/// loading time, especially for very large modules,
/// but it will require signifigant internal work.
///
/// # Drawbacks:
///
/// Due to internal shortcomings, you cannot convert
/// a module into a `Cache`. This means that compiling
/// into a `Cache` and then converting into a module
/// has more overhead than directly compiling
/// into a [`Module`].
///
/// [`Module`]: struct.Module.html
pub struct Cache(pub(crate) CoreCache);

impl Cache {
    /// Load a `Cache` from the file specified by `path`.
    ///
    /// # Usage:
    ///
    /// ```
    /// use wasmer_runtime::Cache;
    /// # use wasmer_runtime::error::CacheError;
    ///
    /// # fn load_cache() -> Result<(), CacheError> {
    /// let cache = Cache::load("some_file.cache")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        CoreCache::open(path).map(|core_cache| Cache(core_cache))
    }

    /// Convert a `Cache` into a [`Module`].
    ///
    /// [`Module`]: struct.Module.html
    ///
    /// # Usage:
    ///
    /// ```
    /// use wasmer_runtime::Cache;
    ///
    /// # use wasmer_runtime::error::CacheError;
    /// # fn cache2module(cache: Cache) -> Result<(), CacheError> {
    /// let module = unsafe { cache.into_module()? };
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Notes:
    ///
    /// This method is unsafe because the runtime cannot confirm
    /// that this cache was not tampered with or corrupted.
    pub unsafe fn into_module(self) -> Result<Module, Error> {
        let default_compiler = super::default_compiler();

        wasmer_runtime_core::load_cache_with(self.0, default_compiler)
    }

    /// Compare the Sha256 hash of the wasm this cache was build
    /// from with some other WebAssembly.
    ///
    /// The main use-case for this is invalidating old caches.
    pub fn compare_wasm(&self, wasm: &[u8]) -> bool {
        let param_wasm_hash = hash_data(wasm);
        self.0.wasm_hash() as &[u8] == &param_wasm_hash as &[u8]
    }

    /// Store this cache in a file.
    ///
    /// # Notes:
    ///
    /// If a file exists at the specified path, it will be overwritten.
    ///
    /// # Usage:
    ///
    /// ```
    /// use wasmer_runtime::Cache;
    ///
    /// # use wasmer_runtime::error::CacheError;
    /// # fn store_cache(cache: Cache) -> Result<(), CacheError> {
    /// cache.store("some_file.cache")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn store<P: AsRef<Path>>(&self, path: P) -> Result<(), Error> {
        self.0.store(path)
    }
}
