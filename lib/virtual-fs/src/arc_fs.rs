//! Wraps a clonable Arc of a file system - in practice this is useful so you
//! can pass clonable file systems with a `Box<dyn FileSystem>` to other
//! interfaces

use std::{path::Path, sync::Arc};

use crate::*;

#[derive(Debug)]
pub struct ArcFileSystem {
    fs: Arc<dyn FileSystem + Send + Sync + 'static>,
}

impl ArcFileSystem {
    pub fn new(inner: Arc<dyn FileSystem + Send + Sync + 'static>) -> Self {
        Self { fs: inner }
    }
}

impl FileSystem for ArcFileSystem {
    fn readlink(&self, path: &Path) -> Result<PathBuf> {
        self.fs.readlink(path)
    }

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

    fn mount(
        &self,
        name: String,
        path: &Path,
        fs: Box<dyn crate::FileSystem + Send + Sync>,
    ) -> Result<()> {
        self.fs.mount(name, path, fs)
    }
}
