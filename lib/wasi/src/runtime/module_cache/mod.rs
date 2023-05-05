mod and_then;
mod disabled;
mod on_disk;
mod shared;
mod thread_local;
mod types;

pub use self::{
    and_then::AndThen,
    on_disk::OnDiskCache,
    shared::SharedCache,
    thread_local::ThreadLocalCache,
    types::{CacheError, Key, ModuleCache},
};

pub(crate) use self::disabled::Disabled;

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
