//! Cache pre-compiled [`wasmer::Module`]s.
//!
//! The core of this module is the [`ModuleCache`] trait, which is designed to
//! be implemented by different cache storage strategies, such as in-memory
//! caches ([`SharedCache`] and [`ThreadLocalCache`]), file-based caches
//! ([`FileSystemCache`]), or distributed caches. Implementing custom caching
//! strategies allows you to optimize for your specific use case.
//!
//! ## Assumptions and Requirements
//!
//! The `module_cache` module makes several assumptions:
//!
//! - Cache keys are unique, typically derived from the original `*.wasm` or
//!   `*.wat` file, and using the same key to load or save will always result in
//!   the "same" module.
//! - The [`ModuleCache::load()`] method will be called more often than the
//!   [`ModuleCache::save()`] method, allowing for cache implementations to
//!   optimize their strategy accordingly.
//!
//! Cache implementations are encouraged to take
//! [`wasmer::Engine::deterministic_id()`] into account when saving and loading
//! cached modules to ensure correct module retrieval.
//!
//! Cache implementations should choose a suitable eviction policy and implement
//! invalidation transparently as part of [`ModuleCache::load()`] or
//! [`ModuleCache::save()`].
//!
//! ## Combinators
//!
//! The `module_cache` module provides combinators for extending and combining
//! caching strategies. For example, you could use the [`FallbackCache`] to
//! chain a fast in-memory cache with a slower file-based cache as a fallback.

mod fallback;
#[cfg(feature = "sys-thread")]
mod filesystem;
mod shared;
mod thread_local;
mod types;

pub use self::{
    fallback::FallbackCache,
    shared::SharedCache,
    thread_local::ThreadLocalCache,
    types::{CacheError, ModuleCache, ModuleHash},
};

#[cfg(feature = "sys-thread")]
pub use self::filesystem::FileSystemCache;

/// Get a [`ModuleCache`] which should be good enough for most in-memory use
/// cases.
///
/// # Platform-specific Notes
///
/// This will use the [`ThreadLocalCache`] when running in the browser.  Each
/// thread lives in a separate worker, so sharing compiled modules in the
/// browser requires using a custom [`ModuleCache`] built on top of
/// [`postMessage()`][pm] and [`SharedArrayBuffer`][sab].
///
/// [pm]: https://developer.mozilla.org/en-US/docs/Web/API/Worker/postMessage
/// [sab]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/SharedArrayBuffer
pub fn in_memory() -> impl ModuleCache + Send + Sync {
    cfg_if::cfg_if! {
        if #[cfg(feature = "js")] {
            ThreadLocalCache::default()
        } else {
            SharedCache::default()
        }
    }
}
