use std::sync::Arc;

use wasmer_types::UserAbort;

#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum ModuleLoadProgress {
    LocalCacheHit,
    RemoteArtifact(ModuleLoadProgressRemote),
    DownloadingModule(DownloadProgress),
    CompilingModule(wasmer_types::CompilationProgress),
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum ModuleLoadProgressRemote {
    RemoteCacheCheck,
    RemoteCacheHit,
    LoadingArtifact(DownloadProgress),
    /// This exists as a variant since local compilation can be used instead if artifact fails.
    ArtifactLoadFailed(ProgressError),
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct DownloadProgress {
    pub total_bytes: u64,
    pub downloaded_bytes: Option<u64>,
}

impl DownloadProgress {
    pub fn new(total_bytes: u64, downloaded_bytes: Option<u64>) -> Self {
        Self {
            total_bytes,
            downloaded_bytes,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ProgressError {
    message: String,
}

impl ProgressError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ProgressError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

#[derive(Clone)]
pub struct ModuleLoadProgressReporter {
    callback: Arc<dyn Fn(ModuleLoadProgress) -> Result<(), UserAbort> + Send + Sync>,
}

impl ModuleLoadProgressReporter {
    pub fn new<F>(callback: F) -> Self
    where
        F: Fn(ModuleLoadProgress) -> Result<(), UserAbort> + Send + Sync + 'static,
    {
        Self {
            callback: Arc::new(callback),
        }
    }

    pub fn notify(&self, progress: ModuleLoadProgress) -> Result<(), UserAbort> {
        (self.callback)(progress)
    }
}

impl std::fmt::Debug for ModuleLoadProgressReporter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModuleLoadProgressReporter").finish()
    }
}
