//! Wraps the memory file system implementation - this has been
//! enhanced to support mounting file systems, shared static files,
//! readonly files, etc...

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    BoxFuture, FileSystem, Metadata, OpenOptions, ReadDir, Result, limiter::DynFsMemoryLimiter,
    mem_fs,
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

    pub fn create_symlink(&self, source: &Path, target: &Path) -> Result<()> {
        self.fs.create_symlink(source, target)
    }
}

impl FileSystem for TmpFileSystem {
    fn readlink(&self, path: &Path) -> Result<PathBuf> {
        self.fs.readlink(path)
    }

    fn read_dir(&self, path: &Path) -> Result<ReadDir> {
        self.fs.read_dir(path)
    }

    fn create_dir(&self, path: &Path) -> Result<()> {
        self.fs.create_dir(path)
    }

    fn rmdir(&self, path: &Path) -> Result<()> {
        self.fs.rmdir(path)
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

    fn unlink(&self, path: &Path) -> Result<()> {
        self.fs.unlink(path)
    }

    fn new_open_options(&self) -> OpenOptions<'_> {
        self.fs.new_open_options()
    }

    fn mount(
        &self,
        name: String,
        path: &Path,
        fs: Box<dyn FileSystem + Send + Sync>,
    ) -> Result<()> {
        FileSystem::mount(&self.fs, name, path, fs)
    }
}
