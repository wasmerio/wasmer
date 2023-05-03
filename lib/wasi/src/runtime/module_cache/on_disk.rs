use std::path::{Path, PathBuf};

use wasmer::{Engine, Module};

use crate::runtime::{
    module_cache::{CacheError, CompiledModuleCache},
    VirtualTaskManager,
};

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
    async fn load(
        &self,
        key: &str,
        engine: &Engine,
        task_manager: &dyn VirtualTaskManager,
    ) -> Result<Module, CacheError> {
        let path = self.path(key);

        // Note: engine is reference-counted so it's cheap to clone
        let engine = engine.clone();

        let result = task_manager
            .runtime()
            .spawn_blocking(move || {
                Module::deserialize_from_file_checked(&engine, &path)
                    .map_err(|e| deserialize_error(e, path))
            })
            .await;

        propagate_panic(result)
    }

    async fn save(
        &self,
        key: &str,
        module: &Module,
        task_manager: &dyn VirtualTaskManager,
    ) -> Result<(), CacheError> {
        let path = self.path(key);

        // Note: module is reference-counted so it's cheap to clone
        let module = module.clone();

        let result = task_manager
            .runtime()
            .spawn_blocking(move || {
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
            })
            .await;

        propagate_panic(result)
    }
}

/// Resume any panics that may have occurred inside a spawned task.
fn propagate_panic<Ret>(
    result: Result<Result<Ret, CacheError>, tokio::task::JoinError>,
) -> Result<Ret, CacheError> {
    match result {
        Ok(ret) => ret,
        Err(e) => match e.try_into_panic() {
            Ok(payload) => std::panic::resume_unwind(payload),
            Err(e) => Err(CacheError::Other(e.into())),
        },
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
    use crate::runtime::task_manager::tokio::TokioTaskManager;

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
        let task_manager = TokioTaskManager::new(tokio::runtime::Handle::current());
        let temp = TempDir::new().unwrap();
        let engine = Engine::default();
        let module = Module::new(&engine, ADD_WAT).unwrap();
        let cache = OnDiskCache::new(temp.path());
        let key = "wat";

        cache.save(key, &module, &task_manager).await.unwrap();

        assert!(temp.path().join(key).with_extension("bin").exists());
    }

    #[tokio::test]
    async fn create_cache_dir_automatically() {
        let task_manager = TokioTaskManager::new(tokio::runtime::Handle::current());
        let temp = TempDir::new().unwrap();
        let engine = Engine::default();
        let module = Module::new(&engine, ADD_WAT).unwrap();
        let cache_dir = temp.path().join("this").join("doesn't").join("exist");
        assert!(!cache_dir.exists());
        let cache = OnDiskCache::new(&cache_dir);
        let key = "wat";

        cache.save(key, &module, &task_manager).await.unwrap();

        assert!(cache_dir.is_dir());
    }

    #[tokio::test]
    async fn missing_file() {
        let task_manager = TokioTaskManager::new(tokio::runtime::Handle::current());
        let temp = TempDir::new().unwrap();
        let engine = Engine::default();
        let key = "wat";
        let cache = OnDiskCache::new(temp.path());

        let err = cache.load(key, &engine, &task_manager).await.unwrap_err();

        assert!(matches!(err, CacheError::NotFound));
    }

    #[tokio::test]
    async fn load_from_disk() {
        let task_manager = TokioTaskManager::new(tokio::runtime::Handle::current());
        let temp = TempDir::new().unwrap();
        let engine = Engine::default();
        let module = Module::new(&engine, ADD_WAT).unwrap();
        let key = "wat";
        module
            .serialize_to_file(temp.path().join(key).with_extension("bin"))
            .unwrap();
        let cache = OnDiskCache::new(temp.path());

        let module = cache.load(key, &engine, &task_manager).await.unwrap();

        let exports: Vec<_> = module
            .exports()
            .map(|export| export.name().to_string())
            .collect();
        assert_eq!(exports, ["add"]);
    }
}
