use std::sync::Arc;

use wasmer_types::UserAbort;

/// Progress during module loading.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum ModuleLoadProgress {
    /// Module was found in local cache.
    LocalCacheHit,
    /// Module is being downloaded.
    DownloadingModule(DownloadProgress),
    /// Module artifact was found in local cache.
    ArtifactCacheHit,
    /// Module artifact is being loaded from remote source.
    RemoteArtifact(RemoteArtifactProgress),
    /// Module is being compiled.
    CompilingModule(wasmer_types::CompilationProgress),
}

/// Progress during remote module artifact resolution.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum RemoteArtifactProgress {
    /// Checking remote cache for artifact.
    RemoteCacheCheck,
    /// Artifact found in remote cache.
    RemoteCacheHit,
    /// Artifact is being downloaded.
    LoadingArtifact(DownloadProgress),
    /// Artifact loading failed - module will be compiled locally instead.
    ArtifactLoadFailed(ProgressError),
}

/// Generic download progress.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct DownloadProgress {
    pub total_bytes: u64,
    pub downloaded_bytes: Option<u64>,
}

impl DownloadProgress {
    /// Creates a new [`DownloadProgress`].
    pub fn new(total_bytes: u64, downloaded_bytes: Option<u64>) -> Self {
        Self {
            total_bytes,
            downloaded_bytes,
        }
    }
}

/// Error that can occur during module loading.
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

/// Reports progress during module loading.
///
/// See [`ModuleLoadProgressReporter::new`] for details.
#[derive(Clone)]
pub struct ModuleLoadProgressReporter {
    callback: Arc<dyn Fn(ModuleLoadProgress) -> Result<(), UserAbort> + Send + Sync>,
}

impl ModuleLoadProgressReporter {
    /// Construct a new `ModuleLoadProgressReporter`.
    ///
    /// The callback function has the signature:
    /// `Fn(ModuleLoadProgress) -> Result<(), UserAbort> + Send + Sync + 'static'.
    ///
    /// # Aborting module loading
    ///
    /// The callback has to return a `Result<(), UserAbort>`.
    /// If an error is returned, the module loading process will be aborted.
    pub fn new<F>(callback: F) -> Self
    where
        F: Fn(ModuleLoadProgress) -> Result<(), UserAbort> + Send + Sync + 'static,
    {
        Self {
            callback: Arc::new(callback),
        }
    }

    /// Notify the reporter about new progress information.
    pub fn notify(&self, progress: ModuleLoadProgress) -> Result<(), UserAbort> {
        (self.callback)(progress)
    }
}

impl std::fmt::Debug for ModuleLoadProgressReporter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModuleLoadProgressReporter").finish()
    }
}
