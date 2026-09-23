use std::sync::Arc;

/// Progress acquiring one WEBC image. Byte counts refer to decoded HTTP bodies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageDownloadProgress {
    pub phase: PackageDownloadPhase,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub cached: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageDownloadPhase {
    Downloading,
    Loading,
    Ready,
}

/// Observers must return promptly. They are never called while a cache is locked.
pub type PackageDownloadObserver = Arc<dyn Fn(PackageDownloadProgress) + Send + Sync>;
