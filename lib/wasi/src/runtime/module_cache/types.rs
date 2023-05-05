use std::{
    fmt::{self, Debug, Display, Formatter},
    ops::Deref,
};

use sha2::{Digest, Sha256};
use wasmer::{Engine, Module};

use crate::runtime::module_cache::AndThen;

/// A cache for compiled WebAssembly modules.
///
/// ## Assumptions
///
/// Implementations can assume that cache keys are unique and that using the
/// same key to load or save will always result in the "same" module.
///
/// Implementations can also assume that [`CompiledModuleCache::load()`] will
/// be called more often than [`CompiledModuleCache::save()`] and optimise
/// their caching strategy accordingly.
#[async_trait::async_trait]
pub trait ModuleCache: Debug {
    async fn load(&self, key: Key, engine: &Engine) -> Result<Module, CacheError>;

    async fn save(&self, key: Key, module: &Module) -> Result<(), CacheError>;

    /// Chain a second cache onto this one.
    ///
    /// The general assumption is that each subsequent cache in the chain will
    /// be significantly slower than the previous one.
    ///
    /// ```rust
    /// use wasmer_wasix::runtime::module_cache::{
    ///     CompiledModuleCache, ThreadLocalCache, OnDiskCache, SharedCache,
    /// };
    ///
    /// let cache = ThreadLocalCache::default()
    ///     .and_then(SharedCache::default())
    ///     .and_then(OnDiskCache::new("~/.local/cache"));
    /// ```
    fn and_then<C>(self, other: C) -> AndThen<Self, C>
    where
        Self: Sized,
        C: ModuleCache,
    {
        AndThen::new(self, other)
    }
}

#[async_trait::async_trait]
impl<D, C> ModuleCache for D
where
    D: Deref<Target = C> + Debug + Send + Sync,
    C: ModuleCache + Send + Sync + ?Sized,
{
    async fn load(&self, key: Key, engine: &Engine) -> Result<Module, CacheError> {
        (**self).load(key, engine).await
    }

    async fn save(&self, key: Key, module: &Module) -> Result<(), CacheError> {
        (**self).save(key, module).await
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    /// The item was not found.
    #[error("Not found")]
    NotFound,
    #[error(transparent)]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

/// A 256-bit key used for caching.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct Key([u8; 32]);

impl Key {
    pub fn new(key: [u8; 32]) -> Self {
        Key(key)
    }

    /// Generate a new [`Key`] based on the SHA-256 hash of some bytes.
    pub fn sha256(data: impl AsRef<[u8]>) -> Self {
        let mut hasher = Sha256::default();
        hasher.update(data);
        Key::new(hasher.finalize().into())
    }

    /// Generate a new [`Key`] which combines this key with the hash of some
    /// extra data.
    ///
    /// If combining a large amount of data, you probably want to hash with
    /// [`Sha256`] directly.
    pub fn combined_with(self, other_data: impl AsRef<[u8]>) -> Self {
        let mut hasher = Sha256::default();
        hasher.update(self.0);
        hasher.update(other_data);
        Key::new(hasher.finalize().into())
    }

    /// Get the raw bytes for.
    pub fn as_bytes(self) -> [u8; 32] {
        self.0
    }
}

impl Display for Key {
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
        let key = Key::new([
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
}
