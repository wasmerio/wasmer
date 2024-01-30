use std::path::{Component, Path, PathBuf};

use futures::future::BoxFuture;

use crate::{
    DirEntry, FileOpener, FileSystem, FsError, Metadata, OpenOptions, OpenOptionsConfig, ReadDir,
    VirtualFile,
};

/// A [`FileSystem`] implementation that is scoped to a specific directory on
/// the host.
#[derive(Debug, Clone)]
pub struct ScopedDirectoryFileSystem {
    root: PathBuf,
    inner: crate::host_fs::FileSystem,
}

impl ScopedDirectoryFileSystem {
    pub fn new(root: impl Into<PathBuf>, inner: crate::host_fs::FileSystem) -> Self {
        ScopedDirectoryFileSystem {
            root: root.into(),
            inner,
        }
    }

    /// Create a new [`ScopedDirectoryFileSystem`] using the current
    /// [`tokio::runtime::Handle`].
    ///
    /// # Panics
    ///
    /// This will panic if called outside of a `tokio` context.
    pub fn new_with_default_runtime(root: impl Into<PathBuf>) -> Self {
        let handle = tokio::runtime::Handle::current();
        let fs = crate::host_fs::FileSystem::new(handle);
        ScopedDirectoryFileSystem::new(root, fs)
    }

    fn prepare_path(&self, path: &Path) -> PathBuf {
        let path = normalize_path(path);
        let path = path.strip_prefix("/").unwrap_or(&path);

        let path = if !path.starts_with(&self.root) {
            self.root.join(path)
        } else {
            path.to_owned()
        };

        debug_assert!(path.starts_with(&self.root));
        path
    }
}

impl FileSystem for ScopedDirectoryFileSystem {
    fn read_dir(&self, path: &Path) -> Result<ReadDir, FsError> {
        let path = self.prepare_path(path);

        let mut entries = Vec::new();

        for entry in self.inner.read_dir(&path)? {
            let entry = entry?;
            let path = entry
                .path
                .strip_prefix(&self.root)
                .map_err(|_| FsError::InvalidData)?;
            entries.push(DirEntry {
                path: Path::new("/").join(path),
                ..entry
            });
        }

        Ok(ReadDir::new(entries))
    }

    fn create_dir(&self, path: &Path) -> Result<(), FsError> {
        let path = self.prepare_path(path);
        self.inner.create_dir(&path)
    }

    fn remove_dir(&self, path: &Path) -> Result<(), FsError> {
        let path = self.prepare_path(path);
        self.inner.remove_dir(&path)
    }

    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<(), FsError>> {
        Box::pin(async move {
            let from = self.prepare_path(from);
            let to = self.prepare_path(to);
            self.inner.rename(&from, &to).await
        })
    }

    fn metadata(&self, path: &Path) -> Result<Metadata, FsError> {
        let path = self.prepare_path(path);
        self.inner.metadata(&path)
    }

    fn remove_file(&self, path: &Path) -> Result<(), FsError> {
        let path = self.prepare_path(path);
        self.inner.remove_file(&path)
    }

    fn new_open_options(&self) -> OpenOptions {
        OpenOptions::new(self)
    }
}

impl FileOpener for ScopedDirectoryFileSystem {
    fn open(
        &self,
        path: &Path,
        conf: &OpenOptionsConfig,
    ) -> Result<Box<dyn VirtualFile + Send + Sync + 'static>, FsError> {
        let path = self.prepare_path(path);
        self.inner
            .new_open_options()
            .options(conf.clone())
            .open(&path)
    }
}

// Copied from cargo
// https://github.com/rust-lang/cargo/blob/fede83ccf973457de319ba6fa0e36ead454d2e20/src/cargo/util/paths.rs#L61
fn normalize_path(path: &Path) -> PathBuf {
    let mut components = path.components().peekable();

    if matches!(components.peek(), Some(Component::Prefix(..))) {
        // This bit diverges from the original cargo implementation, but we want
        // to ignore the drive letter or UNC prefix on Windows. This shouldn't
        // make a difference in practice because WASI is meant to give us
        // Unix-style paths, not Windows-style ones.
        let _ = components.next();
    }

    let mut ret = PathBuf::new();

    for component in components {
        match component {
            Component::Prefix(..) => unreachable!(),
            Component::RootDir => {}
            Component::CurDir => {}
            Component::ParentDir => {
                ret.pop();
            }
            Component::Normal(c) => {
                ret.push(c);
            }
        }
    }
    ret
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    use tokio::io::AsyncReadExt;

    use super::*;

    #[tokio::test]
    async fn open_files() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("file.txt"), "Hello, World!").unwrap();
        let fs = ScopedDirectoryFileSystem::new_with_default_runtime(temp.path());

        let mut f = fs.new_open_options().read(true).open("/file.txt").unwrap();
        let mut contents = String::new();
        f.read_to_string(&mut contents).await.unwrap();

        assert_eq!(contents, "Hello, World!");
    }

    #[tokio::test]
    async fn cant_access_outside_the_scoped_directory() {
        let scoped_directory = TempDir::new().unwrap();
        std::fs::write(scoped_directory.path().join("file.txt"), "").unwrap();
        std::fs::create_dir_all(scoped_directory.path().join("nested").join("dir")).unwrap();
        let fs = ScopedDirectoryFileSystem::new_with_default_runtime(scoped_directory.path());

        // Using ".." shouldn't let you escape the scoped directory
        let mut directory_entries: Vec<_> = fs
            .read_dir("/../../../".as_ref())
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        directory_entries.sort();
        assert_eq!(
            directory_entries,
            vec![PathBuf::from("/file.txt"), PathBuf::from("/nested")],
        );

        // Using a directory's absolute path also shouldn't work
        let other_dir = TempDir::new().unwrap();
        assert_eq!(
            fs.read_dir(other_dir.path()).unwrap_err(),
            FsError::EntryNotFound
        );
    }
}
