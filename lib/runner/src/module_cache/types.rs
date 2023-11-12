use std::{
    fmt::{self, Debug, Display, Formatter},
    ops::Deref,
    path::PathBuf,
};

use sha2::{Digest, Sha256};
use wasmer::{Engine, Module};

use crate::module_cache::FallbackCache;

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
    /// use wasmer_runner::module_cache::{
    ///     ModuleCache, ThreadLocalCache, FileSystemCache, SharedCache,
    /// };
    ///
    /// let cache = SharedCache::default()
    ///     .with_fallback(FileSystemCache::new("~/.local/cache"));
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

/// The SHA-256 hash of a WebAssembly module.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModuleHash([u8; 32]);

impl ModuleHash {
    /// Create a new [`ModuleHash`] from the raw SHA-256 hash.
    pub fn from_bytes(key: [u8; 32]) -> Self {
        ModuleHash(key)
    }

    /// Parse a sha256 hash from a hex-encoded string.
    pub fn parse_hex(hex_str: &str) -> Result<Self, hex::FromHexError> {
        let mut hash = [0_u8; 32];
        hex::decode_to_slice(hex_str, &mut hash)?;
        Ok(Self(hash))
    }

    /// Generate a new [`ModuleCache`] based on the SHA-256 hash of some bytes.
    pub fn sha256(wasm: impl AsRef<[u8]>) -> Self {
        let wasm = wasm.as_ref();

        let mut hasher = Sha256::default();
        hasher.update(wasm);
        ModuleHash::from_bytes(hasher.finalize().into())
    }

    /// Get the raw SHA-256 hash.
    pub fn as_bytes(self) -> [u8; 32] {
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
        let key = ModuleHash::from_bytes([
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b,
            0x1c, 0x1d, 0x1e, 0x1f,
        ]);

        let repr = key.to_string();

        assert_eq!(
            repr,
            "000102030405060708090A0B0C0D0E0F101112131415161718191A1B1C1D1E1F"
        );
    }

    #[test]
    fn module_hash_is_just_sha_256() {
        let wasm = b"\0asm...";
        let raw = [
            0x5a, 0x39, 0xfe, 0xef, 0x52, 0xe5, 0x3b, 0x8f, 0xfe, 0xdf, 0xd7, 0x05, 0x15, 0x56,
            0xec, 0x10, 0x5e, 0xd8, 0x69, 0x82, 0xf1, 0x22, 0xa0, 0x5d, 0x27, 0x28, 0xd9, 0x67,
            0x78, 0xe4, 0xeb, 0x96,
        ];

        let hash = ModuleHash::sha256(wasm);

        assert_eq!(hash.as_bytes(), raw);
    }
}
