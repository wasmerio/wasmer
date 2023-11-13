//! A filesystem that is created on demand

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tempfile::TempDir;

use crate::{
    host_fs, BoxFuture, FileSystem, Metadata, OpenOptions, ReadDir,
    Result,
};

#[derive(Debug, Clone)]
pub struct TempDirHostFileSystem {
    fs: host_fs::FileSystem,
    temp_dir: TempDir,
}

impl TempDirHostFileSystem {
    pub fn new() -> Result<Self> {
        Self {
            fs: host_fs::FileSystem::new(),
            temp_dir: TempDir::new()?,
        }
    }

    pub fn new_open_options_ext(&self) -> &mem_fs::FileSystem {
        self.fs.new_open_options_ext()
    }
}

impl FileSystem for TempDirHostFileSystem {
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
