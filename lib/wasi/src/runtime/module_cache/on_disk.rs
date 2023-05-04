use std::path::{Path, PathBuf};

use wasmer::{Engine, Module};

use crate::runtime::module_cache::{CacheError, CompiledModuleCache};

/// A cache that saves modules to a folder on disk using
/// [`Module::serialize()`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnDiskCache {
    cache_dir: PathBuf,
}

impl OnDiskCache {
    pub fn new(cache_dir: impl Into<PathBuf>) -> Self {
        OnDiskCache {
            cache_dir: cache_dir.into(),
        }
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    fn path(&self, key: &str) -> PathBuf {
        let illegal_path_characters = ['/', '\\', ':', '.'];
        let sanitized_key = key.replace(illegal_path_characters, "_");

        self.cache_dir.join(sanitized_key).with_extension("bin")
    }
}

#[async_trait::async_trait]
impl CompiledModuleCache for OnDiskCache {
    async fn load(&self, key: &str, engine: &Engine) -> Result<Module, CacheError> {
        let path = self.path(key);

        // FIXME: Use spawn_blocking() to avoid blocking the thread
        Module::deserialize_from_file_checked(&engine, &path)
            .map_err(|e| deserialize_error(e, path))
    }

    async fn save(&self, key: &str, module: &Module) -> Result<(), CacheError> {
        let path = self.path(key);

        // FIXME: Use spawn_blocking() to avoid blocking the thread

        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                tracing::warn!(
                    dir=%parent.display(),
                    error=&e as &dyn std::error::Error,
                    "Unable to create the cache dir",
                );
            }
        }

        // PERF: We can reduce disk usage by using the weezl crate to
        // LZW-encode the serialized module.
        module
            .serialize_to_file(&path)
            .map_err(|e| CacheError::Other(Box::new(SerializeError { path, inner: e })))
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Unable to save to \"{}\"", path.display())]
struct SerializeError {
    path: PathBuf,
    #[source]
    inner: wasmer::SerializeError,
}

#[derive(Debug, thiserror::Error)]
#[error("Unable to deserialize from \"{}\"", path.display())]
struct DeserializeError {
    path: PathBuf,
    #[source]
    inner: wasmer::DeserializeError,
}

fn deserialize_error(e: wasmer::DeserializeError, path: PathBuf) -> CacheError {
    match e {
        wasmer::DeserializeError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => {
            CacheError::NotFound
        }
        other => CacheError::Other(Box::new(DeserializeError { path, inner: other })),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    const ADD_WAT: &[u8] = br#"(
        module
            (func
                (export "add")
                (param $x i64)
                (param $y i64)
                (result i64)
                (i64.add (local.get $x) (local.get $y)))
        )"#;

    #[tokio::test]
    async fn save_to_disk() {
        let temp = TempDir::new().unwrap();
        let engine = Engine::default();
        let module = Module::new(&engine, ADD_WAT).unwrap();
        let cache = OnDiskCache::new(temp.path());
        let key = "wat";

        cache.save(key, &module).await.unwrap();

        assert!(temp.path().join(key).with_extension("bin").exists());
    }

    #[tokio::test]
    async fn create_cache_dir_automatically() {
        let temp = TempDir::new().unwrap();
        let engine = Engine::default();
        let module = Module::new(&engine, ADD_WAT).unwrap();
        let cache_dir = temp.path().join("this").join("doesn't").join("exist");
        assert!(!cache_dir.exists());
        let cache = OnDiskCache::new(&cache_dir);
        let key = "wat";

        cache.save(key, &module).await.unwrap();

        assert!(cache_dir.is_dir());
    }

    #[tokio::test]
    async fn missing_file() {
        let temp = TempDir::new().unwrap();
        let engine = Engine::default();
        let key = "wat";
        let cache = OnDiskCache::new(temp.path());

        let err = cache.load(key, &engine).await.unwrap_err();

        assert!(matches!(err, CacheError::NotFound));
    }

    #[tokio::test]
    async fn load_from_disk() {
        let temp = TempDir::new().unwrap();
        let engine = Engine::default();
        let module = Module::new(&engine, ADD_WAT).unwrap();
        let key = "wat";
        module
            .serialize_to_file(temp.path().join(key).with_extension("bin"))
            .unwrap();
        let cache = OnDiskCache::new(temp.path());

        let module = cache.load(key, &engine).await.unwrap();

        let exports: Vec<_> = module
            .exports()
            .map(|export| export.name().to_string())
            .collect();
        assert_eq!(exports, ["add"]);
    }
}
