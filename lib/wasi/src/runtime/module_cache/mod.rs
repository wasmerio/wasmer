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
    types::{CacheError, ModuleCache},
};

pub(crate) use self::disabled::Disabled;

/// Get a [`ModuleCache`] which should be good enough for most in-memory use
/// cases.
///
/// # Platform-specific Notes
///
/// This will use the [`ThreadLocalCache`] when running in the browser because
/// threads are run in separate workers. If you wish to share compiled modules
/// between threads, you will need to use a custom [`ModuleCache`]
/// implementation.
pub fn in_memory() -> impl ModuleCache + Send + Sync {
    cfg_if::cfg_if! {
        if #[cfg(feature = "js")] {
            ThreadLocalCache::default()
        } else {
            SharedCache::default()
        }
    }
}
