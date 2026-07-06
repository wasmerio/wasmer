//! Wraps a cloneable Arc of a file system - in practice this is useful so you
//! can pass cloneable file systems with a `Box<dyn FileSystem>` to other
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

    pub fn inner(&self) -> &Arc<dyn FileSystem + Send + Sync + 'static> {
        &self.fs
    }
}

#[async_trait::async_trait]
impl FileSystem for ArcFileSystem {
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
        self.fs.create_symlink(source, target).await
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
