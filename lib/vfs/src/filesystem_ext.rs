use std::path::Path;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{FileSystem, FsError};

/// Helper methods for working with [`FileSystem`]s.
#[async_trait::async_trait]
pub trait FileSystemExt {
    fn exists(&self, path: impl AsRef<Path>) -> bool;
    fn is_dir(&self, path: impl AsRef<Path>) -> bool;
    fn is_file(&self, path: impl AsRef<Path>) -> bool;
    fn create_dir_all(&self, path: impl AsRef<Path>) -> Result<(), FsError>;
    async fn write(
        &self,
        path: impl AsRef<Path> + Send,
        data: impl AsRef<[u8]> + Send,
    ) -> Result<(), FsError>;
    async fn read(&self, path: impl AsRef<Path> + Send) -> Result<Vec<u8>, FsError>;
    async fn read_to_string(&self, path: impl AsRef<Path> + Send) -> Result<String, FsError>;
    fn touch(&self, path: impl AsRef<Path> + Send) -> Result<(), FsError>;
}

#[async_trait::async_trait]
impl<F: FileSystem> FileSystemExt for F {
    fn exists(&self, path: impl AsRef<Path>) -> bool {
        self.metadata(path.as_ref()).is_ok()
    }

    fn is_dir(&self, path: impl AsRef<Path>) -> bool {
        match self.metadata(path.as_ref()) {
            Ok(meta) => meta.is_dir(),
            Err(_) => false,
        }
    }

    fn is_file(&self, path: impl AsRef<Path>) -> bool {
        match self.metadata(path.as_ref()) {
            Ok(meta) => meta.is_file(),
            Err(_) => false,
        }
    }

    fn create_dir_all(&self, path: impl AsRef<Path>) -> Result<(), FsError> {
        create_dir_all(self, path.as_ref())
    }

    async fn write(
        &self,
        path: impl AsRef<Path> + Send,
        data: impl AsRef<[u8]> + Send,
    ) -> Result<(), FsError> {
        let path = path.as_ref();
        let data = data.as_ref();

        let mut f = self
            .new_open_options()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)?;

        f.write_all(data).await?;

        Ok(())
    }

    async fn read(&self, path: impl AsRef<Path> + Send) -> Result<Vec<u8>, FsError> {
        let mut f = self.new_open_options().read(true).open(path.as_ref())?;
        let mut buffer = Vec::new();
        f.read_to_end(&mut buffer).await?;

        Ok(buffer)
    }

    async fn read_to_string(&self, path: impl AsRef<Path> + Send) -> Result<String, FsError> {
        let mut f = self.new_open_options().read(true).open(path.as_ref())?;
        let mut buffer = String::new();
        f.read_to_string(&mut buffer).await?;

        Ok(buffer)
    }

    fn touch(&self, path: impl AsRef<Path> + Send) -> Result<(), FsError> {
        let _ = self
            .new_open_options()
            .create(true)
            .append(true)
            .write(true)
            .open(path)?;

        Ok(())
    }
}

fn create_dir_all(fs: &impl FileSystem, path: &Path) -> Result<(), FsError> {
    if let Some(parent) = path.parent() {
        create_dir_all(fs, parent)?;
    }

    if let Ok(metadata) = fs.metadata(path) {
        if metadata.is_dir() {
            return Ok(());
        }
        if metadata.is_file() {
            return Err(FsError::BaseNotDirectory);
        }
    }

    fs.create_dir(path)
}

#[cfg(test)]
mod tests {
    use tokio::io::AsyncReadExt;

    use super::*;

    #[tokio::test]
    async fn write() {
        let fs = crate::mem_fs::FileSystem::default();

        fs.write("/file.txt", b"Hello, World!").await.unwrap();

        let mut contents = String::new();
        fs.new_open_options()
            .read(true)
            .open("/file.txt")
            .unwrap()
            .read_to_string(&mut contents)
            .await
            .unwrap();
        assert_eq!(contents, "Hello, World!");
    }

    #[tokio::test]
    async fn read() {
        let fs = crate::mem_fs::FileSystem::default();
        fs.new_open_options()
            .create(true)
            .write(true)
            .open("/file.txt")
            .unwrap()
            .write_all(b"Hello, World!")
            .await
            .unwrap();

        let contents = fs.read_to_string("/file.txt").await.unwrap();
        assert_eq!(contents, "Hello, World!");

        let contents = fs.read("/file.txt").await.unwrap();
        assert_eq!(contents, b"Hello, World!");
    }

    #[tokio::test]
    async fn create_dir_all() {
        let fs = crate::mem_fs::FileSystem::default();
        fs.write("/file.txt", b"").await.unwrap();

        assert!(!fs.exists("/really/nested/directory"));
        fs.create_dir_all("/really/nested/directory").unwrap();
        assert!(fs.exists("/really/nested/directory"));

        // It's okay to create the same directory multiple times
        fs.create_dir_all("/really/nested/directory").unwrap();

        // You can't create a directory on top of a file
        assert_eq!(
            fs.create_dir_all("/file.txt").unwrap_err(),
            FsError::BaseNotDirectory
        );
        assert_eq!(
            fs.create_dir_all("/file.txt/invalid/path").unwrap_err(),
            FsError::BaseNotDirectory
        );
    }
}
