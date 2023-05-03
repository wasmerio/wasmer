mod and_then;
mod on_disk;
mod shared;
mod thread_local;
mod types;

pub use self::{
    and_then::AndThen,
    on_disk::OnDiskCache,
    shared::SharedCache,
    thread_local::ThreadLocalCache,
    types::{CacheError, CompiledModuleCache},
};

/// Get a [`CompiledModuleCache`] which should be good enough for most use
/// cases.
///
/// The returned object will use a mix of in-memory and on-disk caching
/// strategies.
pub fn default_cache(cache_dir: &std::path::Path) -> impl CompiledModuleCache {
    ThreadLocalCache::default()
        .and_then(SharedCache::default())
        .and_then(OnDiskCache::new(cache_dir))
}
