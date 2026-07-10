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
    Container, Metadata as WebcMetadata, PathSegment, PathSegmentError, PathSegments,
    ToPathSegments, Volume, compat::SharedBytes,
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

    /// Resolve `path` to its final target, following symlinks in the trailing
    /// component *and* any intermediate directory components.
    ///
    /// Relative targets resolve against the link's parent, absolute targets
    /// against the volume root. Returns the resolved path together with the
    /// target's metadata (`None` if it doesn't exist) so the caller needn't look
    /// it up again.
    fn resolve_symlinks(&self, path: &Path) -> Result<(PathBuf, Option<WebcMetadata>), FsError> {
        // Maximum number of symlinks to follow before giving up, matching Linux's
        // MAXSYMLINKS. Guards against symlink loops.
        const MAX_SYMLINK_DEPTH: usize = 40;

        let mut current = normalize(path).map_err(|_| FsError::InvalidInput)?;

        for _ in 0..=MAX_SYMLINK_DEPTH {
            match self.volume().metadata(&current) {
                // Fully resolved: the whole path exists and isn't a symlink.
                Some(meta) if !meta.is_symlink() => {
                    return Ok((PathBuf::from(current.to_string()), Some(meta)));
                }
                // The trailing component is a symlink; follow it.
                Some(_) => {
                    let segments: Vec<PathSegment> = current.iter().cloned().collect();
                    current = self.expand_symlink_at(&segments, segments.len() - 1)?;
                }
                // The path doesn't resolve directly. An intermediate component may
                // be a symlink (a raw metadata() lookup stops at the first symlink
                // it walks through), so look for one to expand.
                None => match self.expand_first_symlink(&current)? {
                    Some(next) => current = next,
                    // Genuinely missing; let the caller report it.
                    None => return Ok((PathBuf::from(current.to_string()), None)),
                },
            }
        }

        // Too many levels of symlinks.
        Err(FsError::InvalidInput)
    }

    /// Walk `path`'s components and, if any is a symlink, return `path` with the
    /// first such symlink replaced by its target. Returns `None` when no
    /// component is a symlink (i.e. the path is already resolved or is genuinely missing).
    fn expand_first_symlink(&self, path: &PathSegments) -> Result<Option<PathSegments>, FsError> {
        let segments: Vec<PathSegment> = path.iter().cloned().collect();
        for i in 0..segments.len() {
            let prefix: PathSegments = segments[..=i].iter().cloned().collect();
            match self.volume().metadata(&prefix) {
                Some(meta) if meta.is_symlink() => {
                    return Ok(Some(self.expand_symlink_at(&segments, i)?));
                }
                // A real directory/file: keep walking.
                Some(_) => {}
                // This component is missing, so the whole path is missing.
                None => return Ok(None),
            }
        }
        Ok(None)
    }

    /// Replace the symlink at `segments[..=i]` with its target, resolving a
    /// relative target against the link's parent and keeping any trailing
    /// components. The result is normalized (so `..` in the target is applied).
    fn expand_symlink_at(
        &self,
        segments: &[PathSegment],
        i: usize,
    ) -> Result<PathSegments, FsError> {
        let link: PathSegments = segments[..=i].iter().cloned().collect();
        let (target, _) = self
            .volume()
            .read_link(&link)
            .ok_or(FsError::EntryNotFound)?;

        // webc paths are always '/'-rooted, so check the string directly rather
        // than Path::is_absolute() (which is host-platform dependent).
        let mut combined = String::new();
        if target.starts_with('/') {
            combined.push_str(&target);
        } else {
            // Resolve relative to the link's parent, segments[..i].
            for segment in &segments[..i] {
                combined.push('/');
                combined.push_str(segment.as_str());
            }
            combined.push('/');
            combined.push_str(&target);
        }
        for segment in &segments[i + 1..] {
            combined.push('/');
            combined.push_str(segment.as_str());
        }

        normalize(Path::new(&combined)).map_err(|_| FsError::InvalidInput)
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
    fn readlink(&self, path: &Path) -> crate::Result<PathBuf> {
        let path = normalize(path).map_err(|_| FsError::InvalidInput)?;

        match self.volume().metadata(&path) {
            Some(meta) if !meta.is_symlink() => Err(FsError::InvalidInput),
            Some(_) => self
                .volume()
                .read_link(&path)
                .map(|(target, _)| PathBuf::from(target))
                .ok_or(FsError::EntryNotFound),
            None => Err(FsError::EntryNotFound),
        }
    }

    fn read_dir(&self, path: &Path) -> Result<crate::ReadDir, FsError> {
        // opendir follows symlinks, including a symlinked directory.
        let (resolved, meta) = self.resolve_symlinks(path)?;
        let meta = meta.map(compat_meta).ok_or(FsError::EntryNotFound)?;

        if !meta.is_dir() {
            return Err(FsError::BaseNotDirectory);
        }

        // List the resolved directory, but keep the caller's path as the entry
        // prefix (like `std::fs::read_dir`).
        let display_path = normalize(path).map_err(|_| FsError::InvalidInput)?;
        let resolved = normalize(resolved.as_path()).map_err(|_| FsError::InvalidInput)?;

        let mut entries = Vec::new();

        for (name, _, meta) in self
            .volume()
            .read_dir(&resolved)
            .ok_or(FsError::EntryNotFound)?
        {
            let path = PathBuf::from(display_path.join(name).to_string());
            entries.push(DirEntry {
                path,
                metadata: Ok(compat_meta(meta)),
            });
        }

        Ok(ReadDir::new(entries))
    }

    fn create_dir(&self, path: &Path) -> Result<(), FsError> {
        // These entry-existence checks operate on the path itself, not a symlink
        // target, so they use lstat (symlink_metadata) semantics.

        // the directory shouldn't exist yet
        if self.symlink_metadata(path).is_ok() {
            return Err(FsError::AlreadyExists);
        }

        // it's parent should exist
        let parent = path.parent().unwrap_or_else(|| Path::new("/"));

        match self.symlink_metadata(parent) {
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
        // The original directory should exist. rmdir operates on the entry
        // itself (a symlink is not a directory), so use lstat semantics.
        let meta = self.symlink_metadata(path)?;

        // and it should be a directory
        if !meta.is_dir() {
            return Err(FsError::BaseNotDirectory);
        }

        // but we are a readonly filesystem, so you can't modify anything
        Err(FsError::PermissionDenied)
    }

    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<(), FsError>> {
        Box::pin(async {
            // rename operates on the named entries themselves, so use lstat
            // semantics rather than following a trailing symlink.

            // The original file should exist
            let _ = self.symlink_metadata(from)?;

            // we also want to make sure the destination's folder exists, too
            let dest_parent = to.parent().unwrap_or_else(|| Path::new("/"));
            let parent_meta = self.symlink_metadata(dest_parent)?;
            if !parent_meta.is_dir() {
                return Err(FsError::BaseNotDirectory);
            }

            // but we are a readonly filesystem, so you can't modify anything
            Err(FsError::PermissionDenied)
        })
    }

    fn metadata(&self, path: &Path) -> Result<Metadata, FsError> {
        // `stat` semantics: follow symlinks and report the target.
        let (_, meta) = self.resolve_symlinks(path)?;
        meta.map(compat_meta).ok_or(FsError::EntryNotFound)
    }

    fn symlink_metadata(&self, path: &Path) -> crate::Result<Metadata> {
        // `lstat` semantics: report the entry itself, without following a
        // trailing symlink.
        let path = normalize(path).map_err(|_| FsError::InvalidInput)?;

        self.volume()
            .metadata(path)
            .map(compat_meta)
            .ok_or(FsError::EntryNotFound)
    }

    fn remove_file(&self, path: &Path) -> Result<(), FsError> {
        // unlink removes the entry itself; it does not follow a trailing
        // symlink, so use lstat semantics.
        let meta = self.symlink_metadata(path)?;

        if !meta.is_file() {
            return Err(FsError::NotAFile);
        }

        Err(FsError::PermissionDenied)
    }

    fn new_open_options(&self) -> crate::OpenOptions<'_> {
        crate::OpenOptions::new(self)
    }
}

impl FileOpener for WebcVolumeFileSystem {
    fn open(
        &self,
        path: &Path,
        conf: &OpenOptionsConfig,
    ) -> crate::Result<Box<dyn crate::VirtualFile + Send + Sync + 'static>> {
        // Follow symlinks so opening (and exec'ing) a symlinked file resolves to
        // its target, matching a real filesystem.
        let (resolved, resolved_meta) = self.resolve_symlinks(path)?;
        let path = resolved.as_path();
        if let Some(parent) = path.parent() {
            let parent_meta = self.metadata(parent)?;
            if !parent_meta.is_dir() {
                return Err(FsError::BaseNotDirectory);
            }
        }

        let timestamps = match resolved_meta {
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
        WebcMetadata::Symlink {
            target_length,
            timestamps,
        } => Metadata {
            ft: FileType {
                symlink: true,
                ..Default::default()
            },
            len: target_length.try_into().unwrap(),
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
    use std::collections::BTreeMap;
    use std::convert::TryFrom;
    use tokio::io::AsyncReadExt;
    use wasmer_package::utils::from_bytes;
    use webc::PathSegment;

    const PYTHON_WEBC: &[u8] =
        include_bytes!("../../../wasmer-test-files/examples/python-0.1.0.wasmer");

    fn symlink_fs() -> WebcVolumeFileSystem {
        let timestamps = webc::v3::Timestamps::default();
        let dir = webc::v3::write::Directory::new(
            BTreeMap::from_iter([
                (
                    PathSegment::parse("target.txt").unwrap(),
                    webc::v3::write::DirEntry::File(webc::v3::write::FileEntry::borrowed(
                        b"target", timestamps,
                    )),
                ),
                (
                    PathSegment::parse("link").unwrap(),
                    webc::v3::write::DirEntry::Symlink(webc::v3::write::SymlinkEntry::borrowed(
                        "target.txt",
                        timestamps,
                    )),
                ),
            ]),
            timestamps,
        );
        let manifest = webc::metadata::Manifest::default();
        let mut writer = webc::v3::write::Writer::new(webc::v3::ChecksumAlgorithm::Sha256)
            .write_manifest(&manifest)
            .unwrap()
            .write_atoms(BTreeMap::new())
            .unwrap();
        writer.write_volume("atom", dir).unwrap();
        let webc = writer.finish(webc::v3::SignatureAlgorithm::None).unwrap();
        let container = from_bytes(webc).unwrap();
        let volume = container.volumes()["atom"].clone();

        WebcVolumeFileSystem::new(volume)
    }

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
    fn symlink_metadata_and_readlink() {
        let fs = symlink_fs();

        let link = fs.symlink_metadata("/link".as_ref()).unwrap();
        assert!(link.ft.is_symlink());
        assert_eq!(link.len(), "target.txt".len() as u64);
        assert_eq!(
            fs.readlink("/link".as_ref()).unwrap(),
            Path::new("target.txt")
        );
        // open() follows the symlink to its target (unlike symlink_metadata),
        // so opening the link succeeds and yields the target file.
        assert!(
            fs.new_open_options().read(true).open("/link").is_ok(),
            "open() should follow the symlink to its target file",
        );

        let entries: Vec<_> = fs
            .read_dir("/".as_ref())
            .unwrap()
            .map(|entry| entry.unwrap())
            .collect();
        let link_entry = entries
            .iter()
            .find(|entry| entry.path == Path::new("/link"))
            .unwrap();
        assert!(link_entry.metadata().unwrap().ft.is_symlink());

        assert_eq!(
            fs.readlink("/target.txt".as_ref()).unwrap_err(),
            FsError::InvalidInput
        );
        assert_eq!(
            fs.readlink("/missing".as_ref()).unwrap_err(),
            FsError::EntryNotFound
        );
    }

    /// A volume exercising the various symlink shapes `resolve_symlinks` handles:
    /// relative, multi-hop, absolute, `..`-relative, a symlinked intermediate
    /// directory, and a loop.
    ///
    /// ```text
    /// /a.txt                       "content-a"
    /// /rel    -> a.txt             (relative, single hop)
    /// /hop1   -> hop2 -> a.txt     (relative, two hops)
    /// /loop1  -> loop2 -> loop1    (cycle)
    /// /bin/real.wasm               "\0asm-real"
    /// /libexec/git-core/git -> ../../bin/real.wasm
    /// /bindir -> bin              (symlinked directory)
    /// ```
    fn follow_symlinks_fs() -> WebcVolumeFileSystem {
        use webc::v3::write::{DirEntry, Directory, FileEntry, SymlinkEntry};

        let ts = webc::v3::Timestamps::default();
        let file = |bytes: &'static [u8]| DirEntry::File(FileEntry::borrowed(bytes, ts));
        let link = |target: &'static str| DirEntry::Symlink(SymlinkEntry::borrowed(target, ts));
        let seg = |s: &str| PathSegment::parse(s).unwrap();

        let git_core = Directory::new(
            BTreeMap::from_iter([(seg("git"), link("../../bin/real.wasm"))]),
            ts,
        );
        let libexec = Directory::new(
            BTreeMap::from_iter([(seg("git-core"), DirEntry::Dir(git_core))]),
            ts,
        );
        let bin = Directory::new(
            BTreeMap::from_iter([(seg("real.wasm"), file(b"\0asm-real"))]),
            ts,
        );
        let root = Directory::new(
            BTreeMap::from_iter([
                (seg("a.txt"), file(b"content-a")),
                (seg("rel"), link("a.txt")),
                (seg("hop1"), link("hop2")),
                (seg("hop2"), link("a.txt")),
                (seg("loop1"), link("loop2")),
                (seg("loop2"), link("loop1")),
                (seg("bin"), DirEntry::Dir(bin)),
                (seg("libexec"), DirEntry::Dir(libexec)),
                (seg("bindir"), link("bin")),
            ]),
            ts,
        );

        let manifest = webc::metadata::Manifest::default();
        let mut writer = webc::v3::write::Writer::new(webc::v3::ChecksumAlgorithm::Sha256)
            .write_manifest(&manifest)
            .unwrap()
            .write_atoms(BTreeMap::new())
            .unwrap();
        writer.write_volume("atom", root).unwrap();
        let webc = writer.finish(webc::v3::SignatureAlgorithm::None).unwrap();
        let container = from_bytes(webc).unwrap();
        let volume = container.volumes()["atom"].clone();

        WebcVolumeFileSystem::new(volume)
    }

    #[tokio::test]
    async fn open_follows_symlinks() {
        let fs = follow_symlinks_fs();

        async fn read(fs: &WebcVolumeFileSystem, path: &str) -> Vec<u8> {
            let mut f = fs
                .new_open_options()
                .read(true)
                .open(path)
                .unwrap_or_else(|e| panic!("opening {path}: {e:?}"));
            let mut buffer = Vec::new();
            f.read_to_end(&mut buffer).await.unwrap();
            buffer
        }

        // Relative single-hop and multi-hop chains resolve to the same file.
        assert_eq!(read(&fs, "/rel").await, b"content-a");
        assert_eq!(read(&fs, "/hop1").await, b"content-a");
        // The motivating case: a `..`-relative link deep in the tree.
        assert_eq!(read(&fs, "/libexec/git-core/git").await, b"\0asm-real");
        // A symlink in an intermediate directory component is followed too.
        assert_eq!(read(&fs, "/bindir/real.wasm").await, b"\0asm-real");
    }

    #[test]
    fn open_symlink_loop_fails() {
        let fs = follow_symlinks_fs();

        assert_eq!(
            fs.new_open_options().read(true).open("/loop1").unwrap_err(),
            FsError::InvalidInput,
        );
    }

    #[test]
    fn open_symlink_chain_respects_max_depth() {
        use webc::v3::write::{DirEntry, Directory, FileEntry, SymlinkEntry};

        // A volume with `link0 -> link1 -> ... -> link{n-1} -> target.txt`, i.e.
        // `n` symlinks to follow before reaching the file.
        fn chain_fs(n: usize) -> WebcVolumeFileSystem {
            let ts = webc::v3::Timestamps::default();
            let seg = |s: &str| PathSegment::parse(s).unwrap();

            let mut children = BTreeMap::new();
            children.insert(
                seg("target.txt"),
                DirEntry::File(FileEntry::borrowed(b"target", ts)),
            );
            for i in 0..n {
                let target = if i + 1 == n {
                    "target.txt".to_string()
                } else {
                    format!("link{}", i + 1)
                };
                children.insert(
                    seg(&format!("link{i}")),
                    DirEntry::Symlink(SymlinkEntry::owned(target, ts)),
                );
            }

            let manifest = webc::metadata::Manifest::default();
            let mut writer = webc::v3::write::Writer::new(webc::v3::ChecksumAlgorithm::Sha256)
                .write_manifest(&manifest)
                .unwrap()
                .write_atoms(BTreeMap::new())
                .unwrap();
            writer
                .write_volume("atom", Directory::new(children, ts))
                .unwrap();
            let webc = writer.finish(webc::v3::SignatureAlgorithm::None).unwrap();
            let container = from_bytes(webc).unwrap();
            WebcVolumeFileSystem::new(container.volumes()["atom"].clone())
        }

        // A chain of MAX_SYMLINK_DEPTH (40) links resolves (matches Linux
        // MAXSYMLINKS)...
        assert!(
            chain_fs(40)
                .new_open_options()
                .read(true)
                .open("/link0")
                .is_ok(),
            "a 40-link chain should resolve",
        );

        // ...but one more link is too many.
        assert_eq!(
            chain_fs(41)
                .new_open_options()
                .read(true)
                .open("/link0")
                .unwrap_err(),
            FsError::InvalidInput,
        );
    }

    #[test]
    fn metadata_follows_symlinks() {
        let fs = symlink_fs();

        // metadata() follows the link to its target file (stat semantics)...
        let target = fs.metadata("/link".as_ref()).unwrap();
        assert!(target.is_file());
        assert_eq!(target.len(), "target".len() as u64);

        // ...while symlink_metadata() reports the link itself (lstat semantics).
        let link = fs.symlink_metadata("/link".as_ref()).unwrap();
        assert!(link.ft.is_symlink());
        assert_eq!(link.len(), "target.txt".len() as u64);
    }

    #[test]
    fn read_dir_follows_symlinked_directory() {
        let fs = follow_symlinks_fs();

        // /bindir -> bin, so stat sees a directory...
        assert!(fs.metadata("/bindir".as_ref()).unwrap().is_dir());

        // ...and read_dir() lists the target's contents, keeping the caller's
        // path as the prefix.
        let entries: Vec<_> = fs
            .read_dir("/bindir".as_ref())
            .unwrap()
            .map(|entry| entry.unwrap().path)
            .collect();
        assert_eq!(entries, vec![PathBuf::from("/bindir/real.wasm")]);
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
