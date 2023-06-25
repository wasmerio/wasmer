use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;
use wasmer::{Engine, Module};

use crate::runtime::module_cache::{CacheError, ModuleCache, ModuleHash};

/// A cache that saves modules to a folder on the host filesystem using
/// [`Module::serialize()`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSystemCache {
    cache_dir: PathBuf,
}

impl FileSystemCache {
    pub fn new(cache_dir: impl Into<PathBuf>) -> Self {
        FileSystemCache {
            cache_dir: cache_dir.into(),
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
    #[tracing::instrument(level = "debug", skip_all, fields(%key))]
    async fn load(&self, key: ModuleHash, engine: &Engine) -> Result<Module, CacheError> {
        let path = self.path(key, engine.deterministic_id());

        // FIXME: This will all block the thread at the moment. Ideally,
        // deserializing and uncompressing would happen on a thread pool in the
        // background.
        // https://github.com/wasmerio/wasmer/issues/3851

        let uncompressed = read_compressed(&path)?;

        let res = unsafe { Module::deserialize(&engine, uncompressed) };
        match res {
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

                Err(CacheError::Deserialize(e))
            }
        }
    }

    #[tracing::instrument(level = "debug", skip_all, fields(%key))]
    async fn save(
        &self,
        key: ModuleHash,
        engine: &Engine,
        module: &Module,
    ) -> Result<(), CacheError> {
        let path = self.path(key, engine.deterministic_id());

        // FIXME: This will all block the thread at the moment. Ideally,
        // serializing and compressing would happen on a thread pool in the
        // background.
        // https://github.com/wasmerio/wasmer/issues/3851

        let parent = path
            .parent()
            .expect("Unreachable - always created by joining onto cache_dir");

        if let Err(e) = std::fs::create_dir_all(parent) {
            tracing::warn!(
                dir=%parent.display(),
                error=&e as &dyn std::error::Error,
                "Unable to create the cache directory",
            );
        }

        // Note: We save to a temporary file and persist() it at the end so
        // concurrent readers won't see a partially written module.
        let mut f = NamedTempFile::new_in(parent).map_err(CacheError::other)?;
        let serialized = module.serialize()?;

        if let Err(e) = save_compressed(&mut f, &serialized) {
            return Err(CacheError::FileWrite { path, error: e });
        }

        f.persist(&path).map_err(CacheError::other)?;

        Ok(())
    }
}

fn save_compressed(writer: impl std::io::Write, data: &[u8]) -> Result<(), std::io::Error> {
    let mut encoder = weezl::encode::Encoder::new(weezl::BitOrder::Msb, 8);
    encoder
        .into_stream(writer)
        .encode_all(std::io::Cursor::new(data))
        .status?;

    Ok(())
}

fn read_compressed(path: &Path) -> Result<Vec<u8>, CacheError> {
    let compressed = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(CacheError::NotFound);
        }
        Err(error) => {
            return Err(CacheError::FileRead {
                path: path.to_path_buf(),
                error,
            });
        }
    };

    let mut uncompressed = Vec::new();
    let mut decoder = weezl::decode::Decoder::new(weezl::BitOrder::Msb, 8);
    decoder
        .into_vec(&mut uncompressed)
        .decode_all(&compressed)
        .status
        .map_err(CacheError::other)?;

    Ok(uncompressed)
}

#[cfg(test)]
mod tests {
    use std::fs::File;

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
        let cache = FileSystemCache::new(temp.path());
        let key = ModuleHash::from_bytes([0; 32]);
        let expected_path = cache.path(key, engine.deterministic_id());

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
        let cache = FileSystemCache::new(&cache_dir);
        let key = ModuleHash::from_bytes([0; 32]);

        cache.save(key, &engine, &module).await.unwrap();

        assert!(cache_dir.is_dir());
    }

    #[tokio::test]
    async fn missing_file() {
        let temp = TempDir::new().unwrap();
        let engine = Engine::default();
        let key = ModuleHash::from_bytes([0; 32]);
        let cache = FileSystemCache::new(temp.path());

        let err = cache.load(key, &engine).await.unwrap_err();

        assert!(matches!(err, CacheError::NotFound));
    }

    #[tokio::test]
    async fn load_from_disk() {
        let temp = TempDir::new().unwrap();
        let engine = Engine::default();
        let module = Module::new(&engine, ADD_WAT).unwrap();
        let key = ModuleHash::from_bytes([0; 32]);
        let cache = FileSystemCache::new(temp.path());
        let expected_path = cache.path(key, engine.deterministic_id());
        std::fs::create_dir_all(expected_path.parent().unwrap()).unwrap();
        let serialized = module.serialize().unwrap();
        save_compressed(File::create(&expected_path).unwrap(), &serialized).unwrap();

        let module = cache.load(key, &engine).await.unwrap();

        let exports: Vec<_> = module
            .exports()
            .map(|export| export.name().to_string())
            .collect();
        assert_eq!(exports, ["add"]);
    }
}
