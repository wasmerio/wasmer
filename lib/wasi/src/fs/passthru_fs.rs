use std::path::Path;
#[allow(unused_imports, dead_code)]
use tracing::{debug, error, info, trace, warn};

use wasmer_vfs::*;

#[derive(Debug)]
pub struct PassthruFileSystem {
    fs: Box<dyn FileSystem + Send + Sync + 'static>,
}

impl PassthruFileSystem {
    pub fn new(inner: Box<dyn FileSystem + Send + Sync + 'static>) -> Self {
        Self {
            fs: inner,
        }
    }
}

impl FileSystem for PassthruFileSystem {
    fn read_dir(&self, path: &Path) -> Result<ReadDir> {
        self.fs.read_dir(path)
    }

    fn create_dir(&self, path: &Path) -> Result<()> {
        self.fs.create_dir(path)
    }

    fn remove_dir(&self, path: &Path) -> Result<()> {
        self.fs.remove_dir(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        self.fs.rename(from, to)
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
