//! Common [`FileSystem`] operations.
#![allow(dead_code)] // Most of these helpers are used during testing

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
};

use futures::future::BoxFuture;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{DirEntry, FileSystem, FsError};

/// Does this item exists?
pub async fn exists<F>(fs: &F, path: impl AsRef<Path>) -> bool
where
    F: FileSystem + ?Sized,
{
    fs.metadata(path.as_ref()).await.is_ok()
}

/// Does this path refer to a directory?
pub async fn is_dir<F>(fs: &F, path: impl AsRef<Path>) -> bool
where
    F: FileSystem + ?Sized,
{
    match fs.metadata(path.as_ref()).await {
        Ok(meta) => meta.is_dir(),
        Err(_) => false,
    }
}

/// Does this path refer to a file?
pub async fn is_file<F>(fs: &F, path: impl AsRef<Path>) -> bool
where
    F: FileSystem + ?Sized,
{
    match fs.metadata(path.as_ref()).await {
        Ok(meta) => meta.is_file(),
        Err(_) => false,
    }
}

/// Make sure a directory (and all its parents) exist.
///
/// This is analogous to [`std::fs::create_dir_all()`].
pub async fn create_dir_all<F>(fs: &F, path: impl AsRef<Path>) -> Result<(), FsError>
where
    F: FileSystem + ?Sized,
{
    let path = path.as_ref();
    let mut stack = Vec::new();
    let mut current = Some(path);
    while let Some(path) = current {
        stack.push(path);
        current = path.parent();
    }

    for path in stack.into_iter().rev() {
        if let Ok(metadata) = fs.metadata(path).await {
            if metadata.is_dir() {
                continue;
            }
            if metadata.is_file() {
                return Err(FsError::BaseNotDirectory);
            }
        }

        fs.create_dir(path).await?;
    }

    Ok(())
}

static WHITEOUT_PREFIX: &str = ".wh.";

/// Creates a white out file which hides it from secondary file systems
pub async fn create_white_out<F>(fs: &F, path: impl AsRef<Path>) -> Result<(), FsError>
where
    F: FileSystem + ?Sized,
{
    if let Some(filename) = path.as_ref().file_name() {
        let mut path = path.as_ref().to_owned();
        path.set_file_name(format!("{}{}", WHITEOUT_PREFIX, filename.to_string_lossy()));

        if let Some(parent) = path.parent() {
            create_dir_all(fs, parent).await.ok();
        }

        fs.new_open_options()
            .create_new(true)
            .truncate(true)
            .write(true)
            .open(path)
            .await?;
        Ok(())
    } else {
        Err(FsError::EntryNotFound)
    }
}

/// Removes a white out file from the primary
pub async fn remove_white_out<F>(fs: &F, path: impl AsRef<Path>)
where
    F: FileSystem + ?Sized,
{
    if let Some(filename) = path.as_ref().file_name() {
        let mut path = path.as_ref().to_owned();
        path.set_file_name(format!("{}{}", WHITEOUT_PREFIX, filename.to_string_lossy()));
        fs.remove_file(&path).await.ok();
    }
}

/// Returns true if the path has been hidden by a whiteout file
pub async fn has_white_out<F>(fs: &F, path: impl AsRef<Path>) -> bool
where
    F: FileSystem + ?Sized,
{
    if let Some(filename) = path.as_ref().file_name() {
        let mut path = path.as_ref().to_owned();
        path.set_file_name(format!("{}{}", WHITEOUT_PREFIX, filename.to_string_lossy()));
        fs.metadata(&path).await.is_ok()
    } else {
        false
    }
}

/// Returns true if the path is a whiteout file
pub fn is_white_out(path: impl AsRef<Path>) -> Option<PathBuf> {
    if let Some(filename) = path.as_ref().file_name()
        && let Some(filename) = filename.to_string_lossy().strip_prefix(WHITEOUT_PREFIX)
    {
        let mut path = path.as_ref().to_owned();
        path.set_file_name(filename);
        return Some(path);
    }
    None
}

/// Copies the reference of a file from one file system to another
pub fn copy_reference<'a>(
    source: &'a (impl FileSystem + ?Sized),
    destination: &'a (impl FileSystem + ?Sized),
    path: &'a Path,
) -> BoxFuture<'a, Result<(), std::io::Error>> {
    Box::pin(async { copy_reference_ext(source, destination, path, path).await })
}

/// Copies the reference of a file from one file system to another
pub fn copy_reference_ext<'a>(
    source: &'a (impl FileSystem + ?Sized),
    destination: &'a (impl FileSystem + ?Sized),
    from: &Path,
    to: &Path,
) -> BoxFuture<'a, Result<(), std::io::Error>> {
    let from = from.to_owned();
    let to = to.to_owned();
    Box::pin(async move {
        let src = source.new_open_options().read(true).open(from).await?;
        let mut dst = destination
            .new_open_options()
            .create(true)
            .write(true)
            .truncate(true)
            .open(to)
            .await?;

        dst.copy_reference(src).await?;
        Ok(())
    })
}

/// Move a file across filesystem boundaries by copying it into the destination
/// and then removing the original.
pub fn move_across_filesystems<'a>(
    source: &'a (impl FileSystem + ?Sized),
    destination: &'a (impl FileSystem + ?Sized),
    from: &'a Path,
    to: &'a Path,
) -> BoxFuture<'a, crate::Result<()>> {
    let from = from.to_owned();
    let to = to.to_owned();
    Box::pin(async move {
        let metadata = source.metadata(&from).await?;
        if metadata.is_dir() {
            return Err(FsError::InvalidInput);
        }

        copy_reference_ext(source, destination, &from, &to)
            .await
            .map_err(FsError::from)?;

        if let Err(error) = source.remove_file(&from).await {
            tracing::warn!(
                ?from,
                ?to,
                ?error,
                "Failed to remove file after cross-filesystem rename"
            );
        }

        Ok(())
    })
}

/// Asynchronously write some bytes to a file.
///
/// This is analogous to [`std::fs::write()`].
pub async fn write<F>(
    fs: &F,
    path: impl AsRef<Path> + Send,
    data: impl AsRef<[u8]> + Send,
) -> Result<(), FsError>
where
    F: FileSystem + ?Sized,
{
    let path = path.as_ref();
    let data = data.as_ref();

    let mut f = fs
        .new_open_options()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .await?;

    f.write_all(data).await?;
    f.flush().await?;

    Ok(())
}

/// Asynchronously read a file's contents into memory.
///
/// This is analogous to [`std::fs::read()`].
pub async fn read<F>(fs: &F, path: impl AsRef<Path> + Send) -> Result<Vec<u8>, FsError>
where
    F: FileSystem + ?Sized,
{
    let mut f = fs.new_open_options().read(true).open(path.as_ref()).await?;
    let mut buffer = Vec::new();
    f.read_to_end(&mut buffer).await?;

    Ok(buffer)
}

/// Asynchronously read a file's contents into memory as a string.
///
/// This is analogous to [`std::fs::read_to_string()`].
pub async fn read_to_string<F>(fs: &F, path: impl AsRef<Path> + Send) -> Result<String, FsError>
where
    F: FileSystem + ?Sized,
{
    let mut f = fs.new_open_options().read(true).open(path.as_ref()).await?;
    let mut buffer = String::new();
    f.read_to_string(&mut buffer).await?;

    Ok(buffer)
}

/// Update a file's modification and access times, creating the file if it
/// doesn't already exist.
pub async fn touch<F>(fs: &F, path: impl AsRef<Path> + Send) -> Result<(), FsError>
where
    F: FileSystem + ?Sized,
{
    let _ = fs
        .new_open_options()
        .create(true)
        .write(true)
        .open(path)
        .await?;

    Ok(())
}

/// Recursively iterate over all paths inside a directory, ignoring any
/// errors that may occur along the way.
pub async fn walk<F>(fs: &F, path: impl AsRef<Path>) -> Vec<DirEntry>
where
    F: FileSystem + ?Sized,
{
    let path = path.as_ref();
    let mut dirs_to_visit: VecDeque<_> = fs
        .read_dir(path)
        .await
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|result| result.ok())
        .collect();
    let mut entries = Vec::new();

    while let Some(next) = dirs_to_visit.pop_back() {
        if let Ok(children) = fs.read_dir(&next.path).await {
            dirs_to_visit.extend(children.flatten());
        }

        entries.push(next);
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem_fs::FileSystem as MemFS;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn write() {
        let fs = MemFS::default();

        super::write(&fs, "/file.txt", b"Hello, World!")
            .await
            .unwrap();

        let mut contents = String::new();
        fs.new_open_options()
            .read(true)
            .open("/file.txt")
            .await
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
            .await
            .unwrap()
            .write_all(b"Hello, World!")
            .await
            .unwrap();

        let contents = super::read_to_string(&fs, "/file.txt").await.unwrap();
        assert_eq!(contents, "Hello, World!");

        let contents = super::read(&fs, "/file.txt").await.unwrap();
        assert_eq!(contents, b"Hello, World!");
    }

    #[tokio::test]
    async fn create_dir_all() {
        let fs = MemFS::default();
        super::write(&fs, "/file.txt", b"").await.unwrap();

        assert!(!super::exists(&fs, "/really/nested/directory").await);
        super::create_dir_all(&fs, "/really/nested/directory").await.unwrap();
        assert!(super::exists(&fs, "/really/nested/directory").await);

        // It's okay to create the same directory multiple times
        super::create_dir_all(&fs, "/really/nested/directory").await.unwrap();

        // You can't create a directory on top of a file
        assert_eq!(
            super::create_dir_all(&fs, "/file.txt").await.unwrap_err(),
            FsError::BaseNotDirectory
        );
        assert_eq!(
            super::create_dir_all(&fs, "/file.txt/invalid/path").await.unwrap_err(),
            FsError::BaseNotDirectory
        );
    }

    #[tokio::test]
    async fn touch() {
        let fs = MemFS::default();

        super::touch(&fs, "/file.txt").await.unwrap();

        assert_eq!(super::read(&fs, "/file.txt").await.unwrap(), b"");
    }
}
