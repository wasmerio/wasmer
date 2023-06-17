//! Wraps a boxed file system with an implemented trait VirtualSystem - this is
//! needed so that a `Box<dyn VirtualFileSystem>` can be wrapped in an Arc and
//! shared - some of the interfaces pass around a `Box<dyn VirtualFileSystem>`

use std::path::Path;

use crate::*;

#[derive(Debug)]
pub struct PassthruFileSystem {
    fs: Box<dyn FileSystem + Send + Sync + 'static>,
}

impl PassthruFileSystem {
    pub fn new(inner: Box<dyn FileSystem + Send + Sync + 'static>) -> Self {
        Self { fs: inner }
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

#[cfg(test)]
mod test_builder {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use crate::{FileSystem, PassthruFileSystem};

    #[tokio::test]
    async fn test_passthru_fs_2() {
        let mem_fs = crate::mem_fs::FileSystem::default();

        mem_fs
            .new_open_options()
            .read(true)
            .write(true)
            .create(true)
            .open("/foo.txt")
            .unwrap()
            .write_all(b"hello")
            .await
            .unwrap();

        let mut buf = Vec::new();
        mem_fs
            .new_open_options()
            .read(true)
            .open("/foo.txt")
            .unwrap()
            .read_to_end(&mut buf)
            .await
            .unwrap();
        assert_eq!(buf, b"hello");

        let passthru_fs = PassthruFileSystem::new(Box::new(mem_fs.clone()));
        let mut buf = Vec::new();
        passthru_fs
            .new_open_options()
            .read(true)
            .open("/foo.txt")
            .unwrap()
            .read_to_end(&mut buf)
            .await
            .unwrap();
        assert_eq!(buf, b"hello");
    }
}
