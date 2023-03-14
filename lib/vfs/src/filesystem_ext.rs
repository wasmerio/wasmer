use std::{collections::VecDeque, path::Path};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{DirEntry, FileSystem, FsError};

/// Helper methods for working with [`FileSystem`]s.
#[async_trait::async_trait]
pub trait FileSystemExt {
    /// Does this item exists?
    fn exists(&self, path: impl AsRef<Path>) -> bool;

    /// Does this path refer to a directory?
    fn is_dir(&self, path: impl AsRef<Path>) -> bool;

    /// Does this path refer to a file?
    fn is_file(&self, path: impl AsRef<Path>) -> bool;

    /// Make sure a directory (and all its parents) exist.
    ///
    /// This is analogous to [`std::fs::create_dir_all()`].
    fn create_dir_all(&self, path: impl AsRef<Path>) -> Result<(), FsError>;

    /// Asynchronously write some bytes to a file.
    ///
    /// This is analogous to [`std::fs::write()`].
    async fn write(
        &self,
        path: impl AsRef<Path> + Send,
        data: impl AsRef<[u8]> + Send,
    ) -> Result<(), FsError>;

    /// Asynchronously read a file's contents into memory.
    ///
    /// This is analogous to [`std::fs::read()`].
    async fn read(&self, path: impl AsRef<Path> + Send) -> Result<Vec<u8>, FsError>;

    /// Asynchronously read a file's contents into memory as a string.
    ///
    /// This is analogous to [`std::fs::read_to_string()`].
    async fn read_to_string(&self, path: impl AsRef<Path> + Send) -> Result<String, FsError>;

    /// Update a file's modification and access times, creating the file if it
    /// doesn't already exist.
    fn touch(&self, path: impl AsRef<Path> + Send) -> Result<(), FsError>;

    /// Recursively iterate over all paths inside a directory, ignoring any
    /// errors that may occur along the way.
    fn walk(&self, path: impl AsRef<Path>) -> Box<dyn Iterator<Item = DirEntry> + '_>;
}

#[async_trait::async_trait]
impl<F: FileSystem + ?Sized> FileSystemExt for F {
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
            .truncate(true)
            .write(true)
            .open(path)?;

        f.write_all(data).await?;
        f.flush().await?;

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
            .write(true)
            .open(path)?;

        Ok(())
    }

    fn walk(&self, path: impl AsRef<Path>) -> Box<dyn Iterator<Item = DirEntry> + '_> {
        let path = path.as_ref();
        let mut dirs_to_visit: VecDeque<_> = self
            .read_dir(path)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|result| result.ok())
            .collect();

        Box::new(std::iter::from_fn(move || {
            let next = dirs_to_visit.pop_back()?;

            if let Ok(children) = self.read_dir(&next.path) {
                dirs_to_visit.extend(children.flatten());
            }

            Some(next)
        }))
    }
}

fn create_dir_all<F>(fs: &F, path: &Path) -> Result<(), FsError>
where
    F: FileSystem + ?Sized,
{
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
    use super::*;
    use crate::mem_fs::FileSystem as MemFS;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn write() {
        let fs = MemFS::default();

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
        let fs = MemFS::default();
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
        let fs = MemFS::default();
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

    #[tokio::test]
    async fn touch() {
        let fs = MemFS::default();

        fs.touch("/file.txt").unwrap();

        assert_eq!(fs.read("/file.txt").await.unwrap(), b"");
    }
}
