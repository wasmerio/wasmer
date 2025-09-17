use std::{
    convert::{TryFrom, TryInto},
    io::Cursor,
    path::{Path, PathBuf},
    pin::Pin,
    result::Result,
    task::Poll,
};

use futures::future::BoxFuture;
use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite};
use webc::{
    compat::SharedBytes, Container, Metadata as WebcMetadata, PathSegmentError, PathSegments,
    ToPathSegments, Volume,
};

use crate::{
    DirEntry, EmptyFileSystem, FileOpener, FileSystem, FileType, FsError, Metadata,
    OpenOptionsConfig, OverlayFileSystem, ReadDir, VirtualFile,
};

#[derive(Debug, Clone)]
pub struct WebcVolumeFileSystem {
    volume: Volume,
}

impl WebcVolumeFileSystem {
    pub fn new(volume: Volume) -> Self {
        WebcVolumeFileSystem { volume }
    }

    pub fn volume(&self) -> &Volume {
        &self.volume
    }

    /// Get a filesystem where all [`Volume`]s in a [`Container`] are mounted to
    /// the root directory.
    pub fn mount_all(
        container: &Container,
    ) -> OverlayFileSystem<EmptyFileSystem, Vec<WebcVolumeFileSystem>> {
        let mut filesystems = Vec::new();

        for volume in container.volumes().into_values() {
            filesystems.push(WebcVolumeFileSystem::new(volume));
        }

        OverlayFileSystem::new(EmptyFileSystem::default(), filesystems)
    }
}

impl FileSystem for WebcVolumeFileSystem {
    fn readlink(&self, _path: &Path) -> crate::Result<PathBuf> {
        Err(FsError::InvalidInput)
    }

    fn read_dir(&self, path: &Path) -> Result<crate::ReadDir, FsError> {
        let meta = self.metadata(path)?;

        if !meta.is_dir() {
            return Err(FsError::BaseNotDirectory);
        }

        let path = normalize(path).map_err(|_| FsError::InvalidInput)?;

        let mut entries = Vec::new();

        for (name, _, meta) in self
            .volume()
            .read_dir(&path)
            .ok_or(FsError::EntryNotFound)?
        {
            let path = PathBuf::from(path.join(name).to_string());
            entries.push(DirEntry {
                path,
                metadata: Ok(compat_meta(meta)),
            });
        }

        Ok(ReadDir::new(entries))
    }

    fn create_dir(&self, path: &Path) -> Result<(), FsError> {
        // the directory shouldn't exist yet
        if self.metadata(path).is_ok() {
            return Err(FsError::AlreadyExists);
        }

        // it's parent should exist
        let parent = path.parent().unwrap_or_else(|| Path::new("/"));

        match self.metadata(parent) {
            Ok(parent_meta) if parent_meta.is_dir() => {
                // The operation would normally be doable... but we're a readonly
                // filesystem
                Err(FsError::PermissionDenied)
            }
            Ok(_) | Err(FsError::EntryNotFound) => Err(FsError::BaseNotDirectory),
            Err(other) => Err(other),
        }
    }

    fn remove_dir(&self, path: &Path) -> Result<(), FsError> {
        // The original directory should exist
        let meta = self.metadata(path)?;

        // and it should be a directory
        if !meta.is_dir() {
            return Err(FsError::BaseNotDirectory);
        }

        // but we are a readonly filesystem, so you can't modify anything
        Err(FsError::PermissionDenied)
    }

    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<(), FsError>> {
        Box::pin(async {
            // The original file should exist
            let _ = self.metadata(from)?;

            // we also want to make sure the destination's folder exists, too
            let dest_parent = to.parent().unwrap_or_else(|| Path::new("/"));
            let parent_meta = self.metadata(dest_parent)?;
            if !parent_meta.is_dir() {
                return Err(FsError::BaseNotDirectory);
            }

            // but we are a readonly filesystem, so you can't modify anything
            Err(FsError::PermissionDenied)
        })
    }

    fn metadata(&self, path: &Path) -> Result<Metadata, FsError> {
        let path = normalize(path).map_err(|_| FsError::InvalidInput)?;

        self.volume()
            .metadata(path)
            .map(compat_meta)
            .ok_or(FsError::EntryNotFound)
    }

    fn symlink_metadata(&self, path: &Path) -> crate::Result<Metadata> {
        self.metadata(path)
    }

    fn remove_file(&self, path: &Path) -> Result<(), FsError> {
        let meta = self.metadata(path)?;

        if !meta.is_file() {
            return Err(FsError::NotAFile);
        }

        Err(FsError::PermissionDenied)
    }

    fn new_open_options(&self) -> crate::OpenOptions {
        crate::OpenOptions::new(self)
    }

    fn mount(
        &self,
        _name: String,
        _path: &Path,
        _fs: Box<dyn FileSystem + Send + Sync>,
    ) -> Result<(), FsError> {
        Err(FsError::Unsupported)
    }
}

impl FileOpener for WebcVolumeFileSystem {
    fn open(
        &self,
        path: &Path,
        conf: &OpenOptionsConfig,
    ) -> crate::Result<Box<dyn crate::VirtualFile + Send + Sync + 'static>> {
        if let Some(parent) = path.parent() {
            let parent_meta = self.metadata(parent)?;
            if !parent_meta.is_dir() {
                return Err(FsError::BaseNotDirectory);
            }
        }

        let timestamps = match self.volume().metadata(path) {
            Some(m) if m.is_file() => m.timestamps(),
            Some(_) => return Err(FsError::NotAFile),
            None if conf.create() || conf.create_new() => {
                // The file would normally be created, but we are a readonly fs.
                return Err(FsError::PermissionDenied);
            }
            None => return Err(FsError::EntryNotFound),
        };

        match self.volume().read_file(path) {
            Some((bytes, _)) => Ok(Box::new(File {
                timestamps,
                content: Cursor::new(bytes),
            })),
            None => {
                // The metadata() call should guarantee this, so something
                // probably went wrong internally
                Err(FsError::UnknownError)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct File {
    timestamps: Option<webc::Timestamps>,
    content: Cursor<SharedBytes>,
}

impl VirtualFile for File {
    fn last_accessed(&self) -> u64 {
        0
    }

    fn last_modified(&self) -> u64 {
        self.timestamps
            .map(|t| t.modified())
            .unwrap_or_else(|| get_modified(None))
    }

    fn created_time(&self) -> u64 {
        0
    }

    fn size(&self) -> u64 {
        self.content.get_ref().len().try_into().unwrap()
    }

    fn set_len(&mut self, _new_size: u64) -> crate::Result<()> {
        Err(FsError::PermissionDenied)
    }

    fn unlink(&mut self) -> crate::Result<()> {
        Err(FsError::PermissionDenied)
    }

    fn poll_read_ready(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<usize>> {
        let bytes_remaining =
            self.content.get_ref().len() - usize::try_from(self.content.position()).unwrap();
        Poll::Ready(Ok(bytes_remaining))
    }

    fn poll_write_ready(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<usize>> {
        Poll::Ready(Err(std::io::ErrorKind::PermissionDenied.into()))
    }

    fn as_owned_buffer(&self) -> Option<SharedBytes> {
        Some(self.content.get_ref().clone())
    }
}

impl AsyncRead for File {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        AsyncRead::poll_read(Pin::new(&mut self.content), cx, buf)
    }
}

impl AsyncSeek for File {
    fn start_seek(mut self: Pin<&mut Self>, position: std::io::SeekFrom) -> std::io::Result<()> {
        AsyncSeek::start_seek(Pin::new(&mut self.content), position)
    }

    fn poll_complete(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<u64>> {
        AsyncSeek::poll_complete(Pin::new(&mut self.content), cx)
    }
}

impl AsyncWrite for File {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        _buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        Poll::Ready(Err(std::io::ErrorKind::PermissionDenied.into()))
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Poll::Ready(Err(std::io::ErrorKind::PermissionDenied.into()))
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Poll::Ready(Err(std::io::ErrorKind::PermissionDenied.into()))
    }
}

// HACK: WebC v2 doesn't have timestamps, and WebC v3 files sometimes
// have directories with a zero timestamp as well. Since some programs
// interpret a zero timestamp as the absence of a value, we return
// 1 second past epoch instead.
fn get_modified(timestamps: Option<webc::Timestamps>) -> u64 {
    let modified = timestamps.map(|t| t.modified()).unwrap_or_default();
    // 1 billion nanoseconds = 1 second
    modified.max(1_000_000_000)
}

fn compat_meta(meta: WebcMetadata) -> Metadata {
    match meta {
        WebcMetadata::Dir { timestamps } => Metadata {
            ft: FileType {
                dir: true,
                ..Default::default()
            },
            modified: get_modified(timestamps),
            ..Default::default()
        },
        WebcMetadata::File {
            length, timestamps, ..
        } => Metadata {
            ft: FileType {
                file: true,
                ..Default::default()
            },
            len: length.try_into().unwrap(),
            modified: get_modified(timestamps),
            ..Default::default()
        },
    }
}

/// Normalize a [`Path`] into a [`PathSegments`], dealing with things like `..`
/// and skipping `.`'s.
fn normalize(path: &Path) -> Result<PathSegments, PathSegmentError> {
    // normalization is handled by the ToPathSegments impl for Path
    let result = path.to_path_segments();

    if let Err(e) = &result {
        tracing::debug!(
            error = e as &dyn std::error::Error,
            path=%path.display(),
            "Unable to normalize a path",
        );
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DirEntry;
    use std::convert::TryFrom;
    use tokio::io::AsyncReadExt;
    use wasmer_package::utils::from_bytes;

    const PYTHON_WEBC: &[u8] = include_bytes!("../../c-api/examples/assets/python-0.1.0.wasmer");

    #[test]
    fn normalize_paths() {
        let inputs: Vec<(&str, &[&str])> = vec![
            ("/", &[]),
            ("/path/to/", &["path", "to"]),
            ("/path/to/file.txt", &["path", "to", "file.txt"]),
            ("/folder/..", &[]),
            ("/.hidden", &[".hidden"]),
            ("/folder/../../../../../../../file.txt", &["file.txt"]),
            #[cfg(windows)]
            (r"C:\path\to\file.txt", &["path", "to", "file.txt"]),
        ];

        for (path, expected) in inputs {
            let normalized = normalize(path.as_ref()).unwrap();
            assert_eq!(normalized, expected.to_path_segments().unwrap());
        }
    }

    #[test]
    #[cfg_attr(not(windows), ignore = "Only works with PathBuf's Windows logic")]
    fn normalize_windows_paths() {
        let inputs: Vec<(&str, &[&str])> = vec![
            (r"C:\path\to\file.txt", &["path", "to", "file.txt"]),
            (r"C:/path/to/file.txt", &["path", "to", "file.txt"]),
            (r"\\system07\C$\", &[]),
            (r"c:\temp\test-file.txt", &["temp", "test-file.txt"]),
            (
                r"\\127.0.0.1\c$\temp\test-file.txt",
                &["temp", "test-file.txt"],
            ),
            (r"\\.\c:\temp\test-file.txt", &["temp", "test-file.txt"]),
            (r"\\?\c:\temp\test-file.txt", &["temp", "test-file.txt"]),
            (
                r"\\127.0.0.1\c$\temp\test-file.txt",
                &["temp", "test-file.txt"],
            ),
            (
                r"\\.\Volume{b75e2c83-0000-0000-0000-602f00000000}\temp\test-file.txt",
                &["temp", "test-file.txt"],
            ),
        ];

        for (path, expected) in inputs {
            let normalized = normalize(path.as_ref()).unwrap();
            assert_eq!(normalized, expected.to_path_segments().unwrap(), "{}", path);
        }
    }

    #[test]
    fn invalid_paths() {
        let paths = [".", "..", "./file.txt", ""];

        for path in paths {
            assert!(normalize(path.as_ref()).is_err(), "{}", path);
        }
    }

    #[test]
    fn mount_all_volumes_in_python() {
        let container = from_bytes(PYTHON_WEBC).unwrap();

        let fs = WebcVolumeFileSystem::mount_all(&container);

        // We should now have access to the python directory
        let lib_meta = fs.metadata("/lib/python3.6/".as_ref()).unwrap();
        assert!(lib_meta.is_dir());
    }

    #[test]
    fn read_dir() {
        let container = from_bytes(PYTHON_WEBC).unwrap();
        let volumes = container.volumes();
        let volume = volumes["atom"].clone();

        let fs = WebcVolumeFileSystem::new(volume);

        let entries: Vec<_> = fs
            .read_dir("/lib".as_ref())
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        let modified = get_modified(None);
        let expected = vec![
            DirEntry {
                path: "/lib/.DS_Store".into(),
                metadata: Ok(Metadata {
                    ft: FileType {
                        file: true,
                        ..Default::default()
                    },
                    accessed: 0,
                    created: 0,
                    modified,
                    len: 6148,
                }),
            },
            DirEntry {
                path: "/lib/Parser".into(),
                metadata: Ok(Metadata {
                    ft: FileType {
                        dir: true,
                        ..Default::default()
                    },
                    accessed: 0,
                    created: 0,
                    modified,
                    len: 0,
                }),
            },
            DirEntry {
                path: "/lib/python.wasm".into(),
                metadata: Ok(crate::Metadata {
                    ft: crate::FileType {
                        file: true,
                        ..Default::default()
                    },
                    accessed: 0,
                    created: 0,
                    modified,
                    len: 4694941,
                }),
            },
            DirEntry {
                path: "/lib/python3.6".into(),
                metadata: Ok(crate::Metadata {
                    ft: crate::FileType {
                        dir: true,
                        ..Default::default()
                    },
                    accessed: 0,
                    created: 0,
                    modified,
                    len: 0,
                }),
            },
        ];
        assert_eq!(entries, expected);
    }

    #[test]
    fn metadata() {
        let container = from_bytes(PYTHON_WEBC).unwrap();
        let volumes = container.volumes();
        let volume = volumes["atom"].clone();

        let fs = WebcVolumeFileSystem::new(volume);

        let modified = get_modified(None);
        let python_wasm = crate::Metadata {
            ft: crate::FileType {
                file: true,
                ..Default::default()
            },
            accessed: 0,
            created: 0,
            modified,
            len: 4694941,
        };
        assert_eq!(
            fs.metadata("/lib/python.wasm".as_ref()).unwrap(),
            python_wasm,
        );
        assert_eq!(
            fs.metadata("/../../../../lib/python.wasm".as_ref())
                .unwrap(),
            python_wasm,
        );
        assert_eq!(
            fs.metadata("/lib/python3.6/../python3.6/../python.wasm".as_ref())
                .unwrap(),
            python_wasm,
        );
        assert_eq!(
            fs.metadata("/lib/python3.6".as_ref()).unwrap(),
            crate::Metadata {
                ft: crate::FileType {
                    dir: true,
                    ..Default::default()
                },
                accessed: 0,
                created: 0,
                modified,
                len: 0,
            },
        );
        assert_eq!(
            fs.metadata("/this/does/not/exist".as_ref()).unwrap_err(),
            FsError::EntryNotFound
        );
    }

    #[tokio::test]
    async fn file_opener() {
        let container = from_bytes(PYTHON_WEBC).unwrap();
        let volumes = container.volumes();
        let volume = volumes["atom"].clone();

        let fs = WebcVolumeFileSystem::new(volume);

        assert_eq!(
            fs.new_open_options()
                .create(true)
                .write(true)
                .open("/file.txt")
                .unwrap_err(),
            FsError::PermissionDenied,
        );
        assert_eq!(
            fs.new_open_options().read(true).open("/lib").unwrap_err(),
            FsError::NotAFile,
        );
        assert_eq!(
            fs.new_open_options()
                .read(true)
                .open("/this/does/not/exist.txt")
                .unwrap_err(),
            FsError::EntryNotFound,
        );

        // We should be able to actually read the file
        let mut f = fs
            .new_open_options()
            .read(true)
            .open("/lib/python.wasm")
            .unwrap();
        let mut buffer = Vec::new();
        f.read_to_end(&mut buffer).await.unwrap();
        assert!(buffer.starts_with(b"\0asm"));
        assert_eq!(
            fs.metadata("/lib/python.wasm".as_ref()).unwrap().len(),
            u64::try_from(buffer.len()).unwrap(),
        );
    }

    #[test]
    fn remove_dir_is_not_allowed() {
        let container = from_bytes(PYTHON_WEBC).unwrap();
        let volumes = container.volumes();
        let volume = volumes["atom"].clone();

        let fs = WebcVolumeFileSystem::new(volume);

        assert_eq!(
            fs.remove_dir("/lib".as_ref()).unwrap_err(),
            FsError::PermissionDenied,
        );
        assert_eq!(
            fs.remove_dir("/this/does/not/exist".as_ref()).unwrap_err(),
            FsError::EntryNotFound,
        );
        assert_eq!(
            fs.remove_dir("/lib/python.wasm".as_ref()).unwrap_err(),
            FsError::BaseNotDirectory,
        );
    }

    #[test]
    fn remove_file_is_not_allowed() {
        let container = from_bytes(PYTHON_WEBC).unwrap();
        let volumes = container.volumes();
        let volume = volumes["atom"].clone();

        let fs = WebcVolumeFileSystem::new(volume);

        assert_eq!(
            fs.remove_file("/lib".as_ref()).unwrap_err(),
            FsError::NotAFile,
        );
        assert_eq!(
            fs.remove_file("/this/does/not/exist".as_ref()).unwrap_err(),
            FsError::EntryNotFound,
        );
        assert_eq!(
            fs.remove_file("/lib/python.wasm".as_ref()).unwrap_err(),
            FsError::PermissionDenied,
        );
    }

    #[test]
    fn create_dir_is_not_allowed() {
        let container = from_bytes(PYTHON_WEBC).unwrap();
        let volumes = container.volumes();
        let volume = volumes["atom"].clone();

        let fs = WebcVolumeFileSystem::new(volume);

        assert_eq!(
            fs.create_dir("/lib".as_ref()).unwrap_err(),
            FsError::AlreadyExists,
        );
        assert_eq!(
            fs.create_dir("/this/does/not/exist".as_ref()).unwrap_err(),
            FsError::BaseNotDirectory,
        );
        assert_eq!(
            fs.create_dir("/lib/nested/".as_ref()).unwrap_err(),
            FsError::PermissionDenied,
        );
    }

    #[tokio::test]
    async fn rename_is_not_allowed() {
        let container = from_bytes(PYTHON_WEBC).unwrap();
        let volumes = container.volumes();
        let volume = volumes["atom"].clone();

        let fs = WebcVolumeFileSystem::new(volume);

        assert_eq!(
            fs.rename("/lib".as_ref(), "/other".as_ref())
                .await
                .unwrap_err(),
            FsError::PermissionDenied,
        );
        assert_eq!(
            fs.rename("/this/does/not/exist".as_ref(), "/another".as_ref())
                .await
                .unwrap_err(),
            FsError::EntryNotFound,
        );
        assert_eq!(
            fs.rename("/lib/python.wasm".as_ref(), "/lib/another.wasm".as_ref())
                .await
                .unwrap_err(),
            FsError::PermissionDenied,
        );
    }
}
