use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::NamedTempFile;
use tokio::io::AsyncWriteExt;
use wasmer::{Engine, Module};

use crate::runtime::module_cache::{CacheError, ModuleCache, ModuleHash};
use crate::runtime::task_manager::tokio::TokioTaskManager;
use crate::runtime::task_manager::VirtualTaskManagerExt;

/// A cache that saves modules to a folder on the host filesystem using
/// [`Module::serialize()`].
#[derive(Debug, Clone)]
pub struct FileSystemCache {
    cache_dir: PathBuf,
    task_manager: Arc<TokioTaskManager>,
}

impl FileSystemCache {
    pub fn new(cache_dir: impl Into<PathBuf>, task_manager: Arc<TokioTaskManager>) -> Self {
        FileSystemCache {
            cache_dir: cache_dir.into(),
            task_manager,
        }
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    fn path(&self, key: ModuleHash, deterministic_id: &str) -> PathBuf {
        let artifact_version = wasmer_types::MetadataHeader::CURRENT_VERSION;
        self.cache_dir
            .join(format!("{deterministic_id}-v{artifact_version}"))
            .join(key.to_string())
            .with_extension("bin")
    }
}

#[async_trait::async_trait]
impl ModuleCache for FileSystemCache {
    #[tracing::instrument(level = "debug", skip_all, fields(% key))]
    async fn load(&self, key: ModuleHash, engine: &Engine) -> Result<Module, CacheError> {
        let path = self.path(key, &engine.deterministic_id());

        self.task_manager
            .runtime_handle()
            .spawn({
                let task_manager = self.task_manager.clone();
                let engine = engine.clone();

                async move {
                    let bytes = read_file(&path).await?;

                    task_manager
                    .spawn_await({

                        move || match deserialize(&bytes, &engine) {
                            Ok(m) => {
                                tracing::debug!("Cache hit!");
                                Ok(m)
                            }
                            Err(e) => {
                                tracing::debug!(
                                    %key,
                                    path=%path.display(),
                                    error=&e as &dyn std::error::Error,
                                    "Deleting the cache file because the artifact couldn't be deserialized",
                                );

                                if let Err(e) = std::fs::remove_file(&path) {
                                    tracing::warn!(
                                        %key,
                                        path=%path.display(),
                                        error=&e as &dyn std::error::Error,
                                        "Unable to remove the corrupted cache file",
                                    );
                                }

                                Err(e)
                            }
                        }
                    })
                    .await
                    .unwrap()
                }
            })
            .await
            .unwrap()
    }

    #[tracing::instrument(level = "debug", skip_all, fields(% key))]
    async fn save(
        &self,
        key: ModuleHash,
        engine: &Engine,
        module: &Module,
    ) -> Result<(), CacheError> {
        let path = self.path(key, &engine.deterministic_id());

        self.task_manager
            .runtime_handle()
            .spawn({
                let task_manager = self.task_manager.clone();
                let module = module.clone();

                async move {
                    let parent = path
                        .parent()
                        .expect("Unreachable - always created by joining onto cache_dir");

                    if let Err(e) = tokio::fs::create_dir_all(parent).await {
                        tracing::warn!(
                            dir=%parent.display(),
                            error=&e as &dyn std::error::Error,
                            "Unable to create the cache directory",
                        );
                    }

                    // Note: We save to a temporary file and persist() it at the end so
                    // concurrent readers won't see a partially written module.
                    let (file, temp) = NamedTempFile::new_in(parent)
                        .map_err(CacheError::other)?
                        .into_parts();

                    let mut file = tokio::fs::File::from_std(file);

                    let serialized = task_manager
                        .spawn_await(move || module.serialize())
                        .await
                        .unwrap()?;

                    let mut writer = tokio::io::BufWriter::new(&mut file);
                    if let Err(error) = writer.write_all(&serialized).await {
                        return Err(CacheError::FileWrite { path, error });
                    }
                    if let Err(error) = writer.flush().await {
                        return Err(CacheError::FileWrite { path, error });
                    }

                    temp.persist(&path).map_err(CacheError::other)?;
                    tracing::debug!(path=%path.display(), "Saved to disk");

                    Ok(())
                }
            })
            .await
            .unwrap()
    }
}

async fn read_file(path: &Path) -> Result<Vec<u8>, CacheError> {
    match tokio::fs::read(path).await {
        Ok(bytes) => Ok(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(CacheError::NotFound),
        Err(error) => Err(CacheError::FileRead {
            path: path.to_path_buf(),
            error,
        }),
    }
}

fn deserialize(bytes: &[u8], engine: &Engine) -> Result<Module, CacheError> {
    // We used to compress our compiled modules using LZW encoding in the past.
    // This was removed because it has a negative impact on startup times for
    // "wasmer run", so all new compiled modules should be saved directly to
    // disk.
    //
    // For perspective, compiling php.wasm with cranelift took about 4.75
    // seconds on a M1 Mac.
    //
    // Without LZW compression:
    // - ModuleCache::save(): 408ms, 142MB binary
    // - ModuleCache::load(): 155ms
    // With LZW compression:
    // - ModuleCache::save(): 2.4s, 72MB binary
    // - ModuleCache::load(): 822ms

    match unsafe { Module::deserialize(engine, bytes) } {
        // The happy case
        Ok(m) => Ok(m),
        Err(wasmer::DeserializeError::Incompatible(_)) => {
            let bytes = weezl::decode::Decoder::new(weezl::BitOrder::Msb, 8)
                .decode(bytes)
                .map_err(CacheError::other)?;

            let m = unsafe { Module::deserialize(engine, bytes)? };

            Ok(m)
        }
        Err(e) => Err(CacheError::Deserialize(e)),
    }
}

#[cfg(test)]
mod tests {
    use crate::runtime::task_manager::tokio::TokioTaskManager;
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

    fn create_tokio_task_manager() -> Arc<TokioTaskManager> {
        Arc::new(TokioTaskManager::new(tokio::runtime::Handle::current()))
    }

    #[tokio::test]
    async fn save_to_disk() {
        let temp = TempDir::new().unwrap();
        let engine = Engine::default();
        let module = Module::new(&engine, ADD_WAT).unwrap();
        let cache = FileSystemCache::new(temp.path(), create_tokio_task_manager());
        let key = ModuleHash::xxhash_from_bytes([0; 8]);
        let expected_path = cache.path(key, &engine.deterministic_id());

        cache.save(key, &engine, &module).await.unwrap();

        assert!(expected_path.exists());
    }

    #[tokio::test]
    async fn create_cache_dir_automatically() {
        let temp = TempDir::new().unwrap();
        let engine = Engine::default();
        let module = Module::new(&engine, ADD_WAT).unwrap();
        let cache_dir = temp.path().join("this").join("doesn't").join("exist");
        assert!(!cache_dir.exists());
        let cache = FileSystemCache::new(&cache_dir, create_tokio_task_manager());
        let key = ModuleHash::xxhash_from_bytes([0; 8]);

        cache.save(key, &engine, &module).await.unwrap();

        assert!(cache_dir.is_dir());
    }

    #[tokio::test]
    async fn missing_file() {
        let temp = TempDir::new().unwrap();
        let engine = Engine::default();
        let key = ModuleHash::xxhash_from_bytes([0; 8]);
        let cache = FileSystemCache::new(temp.path(), create_tokio_task_manager());

        let err = cache.load(key, &engine).await.unwrap_err();

        assert!(matches!(err, CacheError::NotFound));
    }

    #[tokio::test]
    async fn load_from_disk() {
        let temp = TempDir::new().unwrap();
        let engine = Engine::default();
        let module = Module::new(&engine, ADD_WAT).unwrap();
        let key = ModuleHash::xxhash_from_bytes([0; 8]);
        let cache = FileSystemCache::new(temp.path(), create_tokio_task_manager());
        let expected_path = cache.path(key, &engine.deterministic_id());
        std::fs::create_dir_all(expected_path.parent().unwrap()).unwrap();
        let serialized = module.serialize().unwrap();
        std::fs::write(&expected_path, &serialized).unwrap();

        let module = cache.load(key, &engine).await.unwrap();

        let exports: Vec<_> = module
            .exports()
            .map(|export| export.name().to_string())
            .collect();
        assert_eq!(exports, ["add"]);
    }

    /// For backwards compatibility, make sure we can still work with LZW
    /// compressed modules.
    #[tokio::test]
    async fn can_still_load_lzw_compressed_binaries() {
        let temp = TempDir::new().unwrap();
        let engine = Engine::default();
        let module = Module::new(&engine, ADD_WAT).unwrap();
        let key = ModuleHash::xxhash_from_bytes([0; 8]);
        let cache = FileSystemCache::new(temp.path(), create_tokio_task_manager());
        let expected_path = cache.path(key, &engine.deterministic_id());
        std::fs::create_dir_all(expected_path.parent().unwrap()).unwrap();
        let serialized = module.serialize().unwrap();
        let mut encoder = weezl::encode::Encoder::new(weezl::BitOrder::Msb, 8);
        std::fs::write(&expected_path, encoder.encode(&serialized).unwrap()).unwrap();

        let module = cache.load(key, &engine).await.unwrap();

        let exports: Vec<_> = module
            .exports()
            .map(|export| export.name().to_string())
            .collect();
        assert_eq!(exports, ["add"]);
    }
}
