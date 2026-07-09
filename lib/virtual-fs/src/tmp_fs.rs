//! Wraps the memory file system implementation - this has been
//! enhanced to support shared static files, readonly files, etc...

use std::path::{Path, PathBuf};

use crate::{
    FileSystem, Metadata, OpenOptions, OpenOptionsConfig, ReadDir, Result, VirtualFile,
    limiter::DynFsMemoryLimiter, mem_fs,
};

#[derive(Debug, Default, Clone)]
pub struct TmpFileSystem {
    fs: mem_fs::FileSystem,
}

impl TmpFileSystem {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_memory_limiter(&self, limiter: DynFsMemoryLimiter) {
        self.fs.set_memory_limiter(limiter);
    }

    pub fn new_open_options_ext(&self) -> &mem_fs::FileSystem {
        self.fs.new_open_options_ext()
    }

    pub fn union(&self, other: &std::sync::Arc<dyn FileSystem + Send + Sync>) {
        self.fs.union(other)
    }

    /// Canonicalize a path without validating that it actually exists.
    pub fn canonicalize_unchecked(&self, path: &Path) -> Result<PathBuf> {
        self.fs.canonicalize_unchecked(path)
    }

    pub fn create_symlink(&self, source: &Path, target: &Path) -> Result<()> {
        self.fs.create_symlink(source, target)
    }
}

#[async_trait::async_trait]
impl FileSystem for TmpFileSystem {
    async fn readlink(&self, path: &Path) -> Result<PathBuf> {
        self.fs.readlink(path).await
    }

    async fn read_dir(&self, path: &Path) -> Result<ReadDir> {
        self.fs.read_dir(path).await
    }

    async fn create_dir(&self, path: &Path) -> Result<()> {
        self.fs.create_dir(path).await
    }

    async fn create_symlink(&self, source: &Path, target: &Path) -> Result<()> {
        self.fs.create_symlink(source, target)
    }

    async fn hard_link(&self, source: &Path, target: &Path) -> Result<()> {
        self.fs.hard_link(source, target).await
    }

    async fn remove_dir(&self, path: &Path) -> Result<()> {
        self.fs.remove_dir(path).await
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        self.fs.rename(from, to).await
    }

    async fn metadata(&self, path: &Path) -> Result<Metadata> {
        self.fs.metadata(path).await
    }

    async fn symlink_metadata(&self, path: &Path) -> Result<Metadata> {
        self.fs.symlink_metadata(path).await
    }

    async fn remove_file(&self, path: &Path) -> Result<()> {
        self.fs.remove_file(path).await
    }

    async fn open(
        &self,
        path: &Path,
        conf: &OpenOptionsConfig,
    ) -> Result<Box<dyn VirtualFile + Send + Sync + 'static>> {
        self.fs.open(path, conf).await
    }

    fn new_open_options(&self) -> OpenOptions<'_> {
        self.fs.new_open_options()
    }
}
