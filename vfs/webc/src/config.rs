
use std::path::PathBuf;

use webc::Volume;

/// Configuration for mounting a WebC volume as a VFS filesystem.
#[derive(Debug, Clone)]
pub struct WebcFsConfig {
    /// The WebC volume to expose.
    pub volume: Volume,
    /// Optional root path inside the volume that should act as the mount root.
    pub root: Option<PathBuf>,
}

impl WebcFsConfig {
    pub fn new(volume: Volume) -> Self {
        Self { volume, root: None }
    }

    pub fn with_root(mut self, root: PathBuf) -> Self {
        self.root = Some(root);
        self
    }
}

/// An exported mapping helper used by callers to prepare mounts.
#[derive(Debug, Clone)]
pub struct WebcVolumeMapping {
    pub mount_path: vfs_core::path_types::VfsPathBuf,
    pub volume: Volume,
    pub root: Option<PathBuf>,
}
