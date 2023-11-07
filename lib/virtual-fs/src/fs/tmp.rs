//! Wraps the memory file system implementation - this has been
//! enhanced to support mounting file systems, shared static files,
//! readonly files, etc...

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    limiter::DynFsMemoryLimiter, mem_fs, BoxFuture, FileSystem, Metadata, OpenOptions, ReadDir,
    Result,
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

    pub fn union(&self, other: &Arc<dyn FileSystem + Send + Sync>) {
        self.fs.union(other)
    }

    /// See [`mem_fs::FileSystem::mount_directory_entries`].
    pub fn mount_directory_entries(
        &self,
        target_path: &Path,
        other: &Arc<dyn crate::FileSystem + Send + Sync>,
        source_path: &Path,
    ) -> Result<()> {
        self.fs
            .mount_directory_entries(target_path, other, source_path)
    }

    pub fn mount(
        &self,
        src_path: PathBuf,
        other: &Arc<dyn FileSystem + Send + Sync>,
        dst_path: PathBuf,
    ) -> Result<()> {
        self.fs.mount(src_path, other, dst_path)
    }

    /// Canonicalize a path without validating that it actually exists.
    pub fn canonicalize_unchecked(&self, path: &Path) -> Result<PathBuf> {
        self.fs.canonicalize_unchecked(path)
    }
}

impl FileSystem for TmpFileSystem {
    fn read_dir(&self, path: &Path) -> Result<ReadDir> {
        self.fs.read_dir(path)
    }

    fn create_dir(&self, path: &Path) -> Result<()> {
        self.fs.create_dir(path)
    }

    fn remove_dir(&self, path: &Path) -> Result<()> {
        self.fs.remove_dir(path)
    }

    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { self.fs.rename(from, to).await })
    }

    fn metadata(&self, path: &Path) -> Result<Metadata> {
        self.fs.metadata(path)
    }

    fn symlink_metadata(&self, path: &Path) -> Result<Metadata> {
        self.fs.symlink_metadata(path)
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        self.fs.remove_file(path)
    }

    fn new_open_options(&self) -> OpenOptions {
        self.fs.new_open_options()
    }
}
