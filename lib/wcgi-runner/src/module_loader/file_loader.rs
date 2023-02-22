use std::path::PathBuf;

use bytes::Bytes;

use crate::{
    module_loader::{LoadedModule, ModuleLoader, ModuleLoaderContext, WasmLoader, WebcLoader},
    Error,
};

/// A [`ModuleLoader`] that will read a file to disk and try to interpret it as
/// a known module type.
///
/// This will re-read the file on every [`ModuleLoader::load()`] call. You may
/// want to add a [`ModuleLoader::cached()`] in front of it to only reload when
/// the file has been changed.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FileLoader {
    path: PathBuf,
}

impl FileLoader {
    pub(crate) fn new(path: impl Into<PathBuf>) -> Self {
        FileLoader { path: path.into() }
    }
}

#[async_trait::async_trait]
impl ModuleLoader for FileLoader {
    async fn load(&self, ctx: ModuleLoaderContext<'_>) -> Result<LoadedModule, Error> {
        let bytes: Bytes = tokio::fs::read(&self.path)
            .await
            .map_err(|error| Error::File {
                error,
                path: self.path.clone(),
            })?
            .into();

        if webc::detect(bytes.as_ref()).is_ok() {
            let loader = WebcLoader::new(bytes)?;
            loader.load(ctx).await
        } else if wasmer::is_wasm(&bytes) {
            let loader = WasmLoader::new(self.path.display().to_string(), bytes);
            loader.load(ctx).await
        } else {
            Err(Error::UnknownFormat)
        }
    }
}
