use std::hash::Hash;
use std::{
    fmt::{self, Debug, Display, Formatter},
    ops::Deref,
    path::PathBuf,
};

use rand::RngCore;
use wasmer::{Engine, Module};

use crate::runtime::module_cache::FallbackCache;

/// A cache for compiled WebAssembly modules.
///
/// ## Deterministic ID
///
/// Implementations are encouraged to take the [`Engine::deterministic_id()`]
/// into account when saving and loading cached a [`Module`].
///
/// ## Assumptions
///
/// Implementations can assume that cache keys are unique and that using the
/// same key to load or save will always result in the "same" module.
///
/// Implementations can also assume that [`ModuleCache::load()`] will
/// be called more often than [`ModuleCache::save()`] and optimise
/// their caching strategy accordingly.
///
#[async_trait::async_trait]
pub trait ModuleCache: Debug {
    /// Load a module based on its hash.
    async fn load(&self, key: ModuleHash, engine: &Engine) -> Result<Module, CacheError>;

    /// Save a module so it can be retrieved with [`ModuleCache::load()`] at a
    /// later time.
    ///
    /// # Panics
    ///
    /// Implementations are free to assume the [`Module`] being passed in was
    /// compiled using the provided [`Engine`], and may panic if this isn't the
    /// case.
    async fn save(
        &self,
        key: ModuleHash,
        engine: &Engine,
        module: &Module,
    ) -> Result<(), CacheError>;

    /// Chain a second [`ModuleCache`] that will be used as a fallback if
    /// lookups on the primary cache fail.
    ///
    /// The general assumption is that each subsequent cache in the chain will
    /// be significantly slower than the previous one.
    ///
    /// ```rust
    /// use wasmer_wasix::runtime::module_cache::{
    ///     ModuleCache, ThreadLocalCache, FileSystemCache, SharedCache,
    /// };
    /// use wasmer_wasix::runtime::task_manager::tokio::{RuntimeOrHandle, TokioTaskManager};
    ///
    /// let runtime = tokio::runtime::Runtime::new().unwrap();
    /// let rt_handle = RuntimeOrHandle::from(runtime);
    /// let task_manager = std::sync::Arc::new(TokioTaskManager::new(rt_handle));
    ///
    /// let cache = SharedCache::default()
    ///     .with_fallback(FileSystemCache::new("~/.local/cache", task_manager));
    /// ```
    fn with_fallback<C>(self, other: C) -> FallbackCache<Self, C>
    where
        Self: Sized,
        C: ModuleCache,
    {
        FallbackCache::new(self, other)
    }
}

#[async_trait::async_trait]
impl<D, C> ModuleCache for D
where
    D: Deref<Target = C> + Debug + Send + Sync,
    C: ModuleCache + Send + Sync + ?Sized,
{
    async fn load(&self, key: ModuleHash, engine: &Engine) -> Result<Module, CacheError> {
        (**self).load(key, engine).await
    }

    async fn save(
        &self,
        key: ModuleHash,
        engine: &Engine,
        module: &Module,
    ) -> Result<(), CacheError> {
        (**self).save(key, engine, module).await
    }
}

/// Possible errors that may occur during [`ModuleCache`] operations.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("Unable to serialize the module")]
    Serialize(#[from] wasmer::SerializeError),
    #[error("Unable to deserialize the module")]
    Deserialize(#[from] wasmer::DeserializeError),
    #[error("Unable to read from \"{}\"", path.display())]
    FileRead {
        path: PathBuf,
        #[source]
        error: std::io::Error,
    },
    #[error("Unable to write to \"{}\"", path.display())]
    FileWrite {
        path: PathBuf,
        #[source]
        error: std::io::Error,
    },
    /// The item was not found.
    #[error("Not found")]
    NotFound,
    /// A catch-all variant for any other errors that may occur.
    #[error(transparent)]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

impl CacheError {
    pub fn other(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        CacheError::Other(Box::new(error))
    }
}

/// The XXHash hash of a WebAssembly module.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModuleHash([u8; 8]);

impl ModuleHash {
    /// Create a new [`ModuleHash`] from the raw XXHash hash.
    pub fn from_bytes(key: [u8; 8]) -> Self {
        ModuleHash(key)
    }

    // Creates a random hash for the module
    pub fn random() -> Self {
        let mut rand = rand::thread_rng();
        let mut key = [0u8; 8];
        rand.fill_bytes(&mut key);
        Self(key)
    }

    /// Parse a XXHash hash from a hex-encoded string.
    pub fn parse_hex(hex_str: &str) -> Result<Self, hex::FromHexError> {
        let mut hash = [0_u8; 8];
        hex::decode_to_slice(hex_str, &mut hash)?;
        Ok(Self(hash))
    }

    /// Generate a new [`ModuleCache`] based on the XXHash hash of some bytes.
    pub fn hash(wasm: impl AsRef<[u8]>) -> Self {
        let wasm = wasm.as_ref();

        let hash = xxhash_rust::xxh64::xxh64(wasm, 0);

        Self(hash.to_ne_bytes())
    }

    /// Get the raw XXHash hash.
    pub fn as_bytes(self) -> [u8; 8] {
        self.0
    }
}

impl Display for ModuleHash {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02X}")?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_object_safe() {
        let _: Option<Box<dyn ModuleCache>> = None;
    }

    #[test]
    fn key_is_displayed_as_hex() {
        let key = ModuleHash::from_bytes([0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07]);

        let repr = key.to_string();

        assert_eq!(repr, "0001020304050607");
    }

    #[test]
    fn module_hash_is_just_sha_256() {
        let wasm = b"\0asm...";
        let raw = [0x0c, 0xc7, 0x88, 0x60, 0xd4, 0x14, 0x71, 0x4c];

        let hash = ModuleHash::hash(wasm);

        assert_eq!(hash.as_bytes(), raw);
    }
}
