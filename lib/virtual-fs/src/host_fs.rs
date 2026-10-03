use crate::{
    DirEntry, FileType, FsError, Metadata, OpenOptions, OpenOptionsConfig, ReadDir, Result,
    VirtualFile,
};
use bytes::{Buf, Bytes};
use futures::future::BoxFuture;
use std::convert::TryInto;
use std::fs;
use std::io::{self, Seek};
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::UNIX_EPOCH;
use tokio::fs as tfs;
use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite, ReadBuf};
use tokio::runtime::Handle;

#[derive(Debug, Clone)]
pub struct FileSystem {
    handle: Handle,
    root: PathBuf,
}

#[allow(dead_code)]
fn default_handle() -> Handle {
    Handle::current()
}

pub fn canonicalize(path: &Path) -> Result<PathBuf> {
    if !path.exists() {
        return Err(FsError::InvalidInput);
    }
    dunce::canonicalize(path).map_err(Into::into)
}

// Copied from cargo
// https://github.com/rust-lang/cargo/blob/fede83ccf973457de319ba6fa0e36ead454d2e20/src/cargo/util/paths.rs#L61
pub fn normalize_path(path: &Path) -> PathBuf {
    let mut components = path.components().peekable();
    let mut ret = if let Some(c @ Component::Prefix(..)) = components.peek().cloned() {
        components.next();
        PathBuf::from(c.as_os_str())
    } else {
        PathBuf::new()
    };

    for component in components {
        match component {
            Component::Prefix(..) => unreachable!(),
            Component::RootDir => {
                ret.push(component.as_os_str());
            }
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

fn path_suffix_to_guest_absolute(stripped: &Path) -> PathBuf {
    let mut stripped = stripped.to_string_lossy().into_owned();
    if std::path::MAIN_SEPARATOR == '\\' {
        stripped = stripped.replace('\\', "/");
    }

    PathBuf::from(format!("/{}", stripped.trim_start_matches('/')))
}

fn strip_host_root(root: &Path, target: &Path) -> Option<PathBuf> {
    target
        .strip_prefix(root)
        .ok()
        .map(path_suffix_to_guest_absolute)
}

fn host_root_relative_target(root: &Path, target: PathBuf) -> PathBuf {
    if root == Path::new("/") || !target.is_absolute() {
        return target;
    }

    if let Some(target) = strip_host_root(root, &target) {
        return target;
    }

    if let Ok(canonical_target) = canonicalize(&target)
        && let Some(target) = strip_host_root(root, &canonical_target)
    {
        return target;
    }

    target
}

impl FileSystem {
    pub fn new(handle: Handle, root: impl Into<PathBuf>) -> Result<Self> {
        let root = canonicalize(&root.into())?;

        Ok(FileSystem { handle, root })
    }

    fn prepare_path(&self, path: &Path) -> Result<PathBuf> {
        let path = normalize_path(path);

        if matches!(path.components().next(), Some(Component::Prefix(..))) {
            return Err(FsError::InvalidInput);
        }

        if self.root != Path::new("/") && path.starts_with(&self.root) {
            return Err(FsError::InvalidInput);
        }

        let path = path.strip_prefix("/").unwrap_or(&path);
        let path = self.root.join(path);

        debug_assert!(path.starts_with(&self.root));
        Ok(path)
    }
}

impl crate::FileSystem for FileSystem {
    fn readlink(&self, path: &Path) -> Result<PathBuf> {
        let path = self.prepare_path(path)?;

        let target = fs::read_link(path)?;
        Ok(host_root_relative_target(&self.root, target))
    }

    fn read_dir(&self, path: &Path) -> Result<ReadDir> {
        let path = self.prepare_path(path)?;

        let read_dir = fs::read_dir(path)?;
        let mut data = read_dir
            .map(|entry| {
                let entry = entry?;

                let path = entry
                    .path()
                    .strip_prefix(&self.root)
                    .map_err(|_| FsError::InvalidData)?
                    .to_owned();
                let path = Path::new("/").join(path);

                let metadata = fs::symlink_metadata(entry.path())?;

                Ok(DirEntry {
                    path,
                    metadata: Ok(metadata.try_into()?),
                })
            })
            .collect::<std::result::Result<Vec<DirEntry>, io::Error>>()
            .map_err::<FsError, _>(Into::into)?;
        data.sort_by(|a, b| a.path.file_name().cmp(&b.path.file_name()));
        Ok(ReadDir::new(data))
    }

    fn create_dir(&self, path: &Path) -> Result<()> {
        let path = self.prepare_path(path)?;

        if path.parent().is_none() {
            return Err(FsError::BaseNotDirectory);
        }

        fs::create_dir(path).map_err(Into::into)
    }

    fn remove_dir(&self, path: &Path) -> Result<()> {
        let path = self.prepare_path(path)?;

        if path.parent().is_none() {
            return Err(FsError::BaseNotDirectory);
        }

        // https://github.com/rust-lang/rust/issues/86442
        // DirectoryNotEmpty is not implemented consistently
        if path.is_dir()
            && fs::read_dir(&path)
                .map(|mut s| s.next().is_some())
                .unwrap_or(false)
        {
            return Err(FsError::DirectoryNotEmpty);
        }
        fs::remove_dir(path).map_err(Into::into)
    }

    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            use filetime::{FileTime, set_file_mtime};
            let norm_from = normalize_path(from);
            let norm_to = normalize_path(to);

            if norm_from.parent().is_none() {
                return Err(FsError::BaseNotDirectory);
            }
            if norm_to.parent().is_none() {
                return Err(FsError::BaseNotDirectory);
            }

            let from = self.prepare_path(from)?;
            let to = self.prepare_path(to)?;

            if !from.exists() {
                return Err(FsError::EntryNotFound);
            }
            let from_parent = from.parent().unwrap();
            let to_parent = to.parent().unwrap();
            if !from_parent.exists() {
                return Err(FsError::EntryNotFound);
            }
            if !to_parent.exists() {
                return Err(FsError::EntryNotFound);
            }
            let result = if from_parent != to_parent {
                let _ = std::fs::create_dir_all(to_parent);
                if from.is_dir() {
                    fs_extra::move_items(
                        &[&from],
                        &to,
                        &fs_extra::dir::CopyOptions {
                            copy_inside: true,
                            ..Default::default()
                        },
                    )
                    .map(|_| ())
                    .map_err(|_| FsError::UnknownError)?;
                    let _ = fs_extra::remove_items(&[&from]);
                    Ok(())
                } else {
                    fs::copy(&from, &to).map(|_| ()).map_err(FsError::from)?;
                    fs::remove_file(&from).map(|_| ()).map_err(Into::into)
                }
            } else {
                fs::rename(&from, &to).map_err(Into::into)
            };
            let _ = set_file_mtime(&to, FileTime::now()).map(|_| ());
            result
        })
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        let path = self.prepare_path(path)?;

        if path.parent().is_none() {
            return Err(FsError::BaseNotDirectory);
        }

        fs::remove_file(path).map_err(Into::into)
    }

    fn new_open_options(&self) -> OpenOptions<'_> {
        OpenOptions::new(self)
    }

    fn metadata(&self, path: &Path) -> Result<Metadata> {
        let path = self.prepare_path(path)?;

        fs::metadata(path)
            .and_then(TryInto::try_into)
            .map_err(Into::into)
    }

    fn symlink_metadata(&self, path: &Path) -> Result<Metadata> {
        let path = self.prepare_path(path)?;

        fs::symlink_metadata(path)
            .and_then(TryInto::try_into)
            .map_err(Into::into)
    }
}

impl TryInto<Metadata> for std::fs::Metadata {
    type Error = io::Error;

    fn try_into(self) -> std::result::Result<Metadata, Self::Error> {
        let filetype = self.file_type();
        let (char_device, block_device, socket, fifo) = {
            #[cfg(unix)]
            {
                use std::os::unix::fs::FileTypeExt;
                (
                    filetype.is_char_device(),
                    filetype.is_block_device(),
                    filetype.is_socket(),
                    filetype.is_fifo(),
                )
            }
            #[cfg(not(unix))]
            {
                (false, false, false, false)
            }
        };

        Ok(Metadata {
            ft: FileType {
                dir: filetype.is_dir(),
                file: filetype.is_file(),
                symlink: filetype.is_symlink(),
                char_device,
                block_device,
                socket,
                fifo,
            },
            accessed: self
                .accessed()
                .and_then(|time| time.duration_since(UNIX_EPOCH).map_err(io::Error::other))
                .map_or(0, |time| time.as_nanos() as u64),
            created: self
                .created()
                .and_then(|time| time.duration_since(UNIX_EPOCH).map_err(io::Error::other))
                .map_or(0, |time| time.as_nanos() as u64),
            modified: self
                .modified()
                .and_then(|time| time.duration_since(UNIX_EPOCH).map_err(io::Error::other))
                .map_or(0, |time| time.as_nanos() as u64),
            len: self.len(),
        })
    }
}

impl crate::FileOpener for FileSystem {
    fn open(
        &self,
        path: &Path,
        conf: &OpenOptionsConfig,
    ) -> Result<Box<dyn VirtualFile + Send + Sync + 'static>> {
        let path = self.prepare_path(path)?;

        // TODO: handle create implying write, etc.
        let read = conf.read();
        let write = conf.write();

        // according to Rust's stdlib, specifying both truncate and append is nonsensical,
        // and it will return an error if we try to open a file with both flags set.
        // in order to prevent this, and stay compatible with native binaries, we just ignore
        // the append flag if truncate is set. the rationale behind this decision is that
        // truncate is going to be applied first and append is going to be ignored anyway.
        let append = if conf.truncate { false } else { conf.append() };

        let mut oo = fs::OpenOptions::new();
        oo.read(conf.read())
            .write(conf.write())
            .create_new(conf.create_new())
            .create(conf.create())
            .append(append)
            .truncate(conf.truncate())
            .open(&path)
            .and_then(|file| {
                File::try_new(
                    self.handle.clone(),
                    file,
                    path.to_owned(),
                    read,
                    write,
                    append,
                )
            })
            .map(|file| Box::new(file) as Box<dyn VirtualFile + Send + Sync + 'static>)
            .map_err(Into::into)
    }
}

/// A thin wrapper around `std::fs::File`
#[derive(Debug)]
pub struct File {
    handle: Handle,
    inner: tfs::File,
    inner_std: fs::File,
    pub host_path: PathBuf,
}

impl File {
    const READ: u16 = 1;
    const WRITE: u16 = 2;
    const APPEND: u16 = 4;

    /// creates a new host file from a `std::fs::File` and a path
    ///
    /// # Panics
    ///
    /// Panics if the file handle can not be duplicated, e.g. because the
    /// process ran out of file descriptors. Use [`File::try_new`] to handle
    /// that case.
    pub fn new(
        handle: Handle,
        file: fs::File,
        host_path: PathBuf,
        read: bool,
        write: bool,
        append: bool,
    ) -> Self {
        Self::try_new(handle, file, host_path, read, write, append)
            .expect("failed to duplicate host file handle")
    }

    /// creates a new host file from a `std::fs::File` and a path, failing if
    /// the file handle can not be duplicated
    pub fn try_new(
        handle: Handle,
        file: fs::File,
        host_path: PathBuf,
        read: bool,
        write: bool,
        append: bool,
    ) -> io::Result<Self> {
        let mut _flags = 0;

        if read {
            _flags |= Self::READ;
        }

        if write {
            _flags |= Self::WRITE;
        }

        if append {
            _flags |= Self::APPEND;
        }

        let async_file = tfs::File::from_std(file.try_clone()?);
        Ok(Self {
            handle,
            inner_std: file,
            inner: async_file,
            host_path,
        })
    }

    /// Metadata for the infallible [`VirtualFile`] accessors.
    ///
    /// The host can fail `fstat` at any time (e.g. ESTALE on network storage),
    /// which must not crash the runtime. Callers that need to surface the
    /// error use [`VirtualFile::metadata`] instead.
    fn metadata_or_default(&self) -> Metadata {
        VirtualFile::metadata(self).unwrap_or_else(|error| {
            tracing::debug!(
                host_path = %self.host_path.display(),
                %error,
                "failed to read host file metadata",
            );
            Metadata::default()
        })
    }
}

#[async_trait::async_trait]
impl VirtualFile for File {
    fn last_accessed(&self) -> u64 {
        self.metadata_or_default().accessed
    }

    fn last_modified(&self) -> u64 {
        self.metadata_or_default().modified
    }

    fn created_time(&self) -> u64 {
        self.metadata_or_default().created
    }

    fn set_times(&mut self, atime: Option<u64>, mtime: Option<u64>) -> crate::Result<()> {
        let atime = atime.map(|t| filetime::FileTime::from_unix_time(t as i64, 0));
        let mtime = mtime.map(|t| filetime::FileTime::from_unix_time(t as i64, 0));

        filetime::set_file_handle_times(&self.inner_std, atime, mtime)
            .map_err(|_| crate::FsError::IOError)
    }

    fn size(&self) -> u64 {
        self.metadata_or_default().len
    }

    fn metadata(&self) -> Result<Metadata> {
        self.inner_std
            .metadata()
            .and_then(TryInto::try_into)
            .map_err(Into::into)
    }

    fn set_len(&mut self, new_size: u64) -> crate::Result<()> {
        fs::File::set_len(&self.inner_std, new_size).map_err(Into::into)
    }

    fn unlink(&mut self) -> Result<()> {
        fs::remove_file(&self.host_path).map_err(Into::into)
    }

    fn get_special_fd(&self) -> Option<u32> {
        None
    }

    fn poll_read_ready(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        let cursor = match self.inner_std.stream_position() {
            Ok(a) => a,
            Err(err) => return Poll::Ready(Err(err)),
        };
        let end = match self.inner_std.seek(io::SeekFrom::End(0)) {
            Ok(a) => a,
            Err(err) => return Poll::Ready(Err(err)),
        };
        let _ = self.inner_std.seek(io::SeekFrom::Start(cursor));

        let remaining = end - cursor;
        Poll::Ready(Ok(remaining as usize))
    }

    fn poll_write_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(8192))
    }
}

impl AsyncRead for File {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let _guard = Handle::try_current().map_err(|_| self.handle.enter());
        let inner = Pin::new(&mut self.inner);
        inner.poll_read(cx, buf)
    }
}

impl AsyncWrite for File {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let _guard = Handle::try_current().map_err(|_| self.handle.enter());
        let inner = Pin::new(&mut self.inner);
        inner.poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let _guard = Handle::try_current().map_err(|_| self.handle.enter());
        let inner = Pin::new(&mut self.inner);
        inner.poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let _guard = Handle::try_current().map_err(|_| self.handle.enter());
        let inner = Pin::new(&mut self.inner);
        inner.poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let _guard = Handle::try_current().map_err(|_| self.handle.enter());
        let inner = Pin::new(&mut self.inner);
        inner.poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

impl AsyncSeek for File {
    fn start_seek(mut self: Pin<&mut Self>, position: io::SeekFrom) -> io::Result<()> {
        let _guard = Handle::try_current().map_err(|_| self.handle.enter());
        let inner = Pin::new(&mut self.inner);
        inner.start_seek(position)
    }

    fn poll_complete(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        let _guard = Handle::try_current().map_err(|_| self.handle.enter());
        let inner = Pin::new(&mut self.inner);
        inner.poll_complete(cx)
    }
}

impl Drop for File {
    fn drop(&mut self) {
        tracing::trace!(?self.host_path, "Closing host file");
    }
}

/// A wrapper type around Stdout that implements `VirtualFile`.
#[derive(Debug)]
pub struct Stdout {
    handle: Handle,
    inner: tokio::io::Stdout,
}
#[allow(dead_code)]
fn default_stdout() -> tokio::io::Stdout {
    tokio::io::stdout()
}
impl Default for Stdout {
    fn default() -> Self {
        Self {
            handle: Handle::current(),
            inner: tokio::io::stdout(),
        }
    }
}

/// Default size for write buffers.
///
/// Chosen to be both sufficiently large, and a multiple of the default page
/// size on most systems.
///
/// This value has limited meaning, since it is only used for buffer size hints,
/// and those hints are often ignored.
const DEFAULT_BUF_SIZE_HINT: usize = 8 * 1024;

#[async_trait::async_trait]
impl VirtualFile for Stdout {
    fn last_accessed(&self) -> u64 {
        0
    }

    fn last_modified(&self) -> u64 {
        0
    }

    fn created_time(&self) -> u64 {
        0
    }

    fn size(&self) -> u64 {
        0
    }

    fn set_len(&mut self, _new_size: u64) -> crate::Result<()> {
        Ok(())
    }

    fn unlink(&mut self) -> Result<()> {
        Ok(())
    }

    fn get_special_fd(&self) -> Option<u32> {
        Some(1)
    }

    fn poll_read_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(0))
    }

    fn poll_write_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(DEFAULT_BUF_SIZE_HINT))
    }
}

impl AsyncRead for Stdout {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Ready(Err(io::Error::other("can not read from stdout")))
    }
}

impl AsyncWrite for Stdout {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let _guard = Handle::try_current().map_err(|_| self.handle.enter());
        let inner = Pin::new(&mut self.inner);
        inner.poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let _guard = Handle::try_current().map_err(|_| self.handle.enter());
        let inner = Pin::new(&mut self.inner);
        inner.poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let _guard = Handle::try_current().map_err(|_| self.handle.enter());
        let inner = Pin::new(&mut self.inner);
        inner.poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let _guard = Handle::try_current().map_err(|_| self.handle.enter());
        let inner = Pin::new(&mut self.inner);
        inner.poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

impl AsyncSeek for Stdout {
    fn start_seek(self: Pin<&mut Self>, _position: io::SeekFrom) -> io::Result<()> {
        Err(io::Error::other("can not seek stdout"))
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Err(io::Error::other("can not seek stdout")))
    }
}

/// A wrapper type around Stderr that implements `VirtualFile`.
#[derive(Debug)]
pub struct Stderr {
    handle: Handle,
    inner: tokio::io::Stderr,
}
#[allow(dead_code)]
fn default_stderr() -> tokio::io::Stderr {
    tokio::io::stderr()
}
impl Default for Stderr {
    fn default() -> Self {
        Self {
            handle: Handle::current(),
            inner: tokio::io::stderr(),
        }
    }
}

impl AsyncRead for Stderr {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Ready(Err(io::Error::other("can not read from stderr")))
    }
}

impl AsyncWrite for Stderr {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let _guard = Handle::try_current().map_err(|_| self.handle.enter());
        let inner = Pin::new(&mut self.inner);
        inner.poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let _guard = Handle::try_current().map_err(|_| self.handle.enter());
        let inner = Pin::new(&mut self.inner);
        inner.poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let _guard = Handle::try_current().map_err(|_| self.handle.enter());
        let inner = Pin::new(&mut self.inner);
        inner.poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let _guard = Handle::try_current().map_err(|_| self.handle.enter());
        let inner = Pin::new(&mut self.inner);
        inner.poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

impl AsyncSeek for Stderr {
    fn start_seek(self: Pin<&mut Self>, _position: io::SeekFrom) -> io::Result<()> {
        Err(io::Error::other("can not seek stderr"))
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Err(io::Error::other("can not seek stderr")))
    }
}

#[async_trait::async_trait]
impl VirtualFile for Stderr {
    fn last_accessed(&self) -> u64 {
        0
    }

    fn last_modified(&self) -> u64 {
        0
    }

    fn created_time(&self) -> u64 {
        0
    }

    fn size(&self) -> u64 {
        0
    }

    fn set_len(&mut self, _new_size: u64) -> crate::Result<()> {
        Ok(())
    }

    fn unlink(&mut self) -> Result<()> {
        Ok(())
    }

    fn get_special_fd(&self) -> Option<u32> {
        Some(2)
    }

    fn poll_read_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(0))
    }

    fn poll_write_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(8192))
    }
}

/// A wrapper type around Stdin that implements `VirtualFile`.
#[derive(Debug)]
pub struct Stdin {
    read_buffer: Arc<std::sync::Mutex<Option<Bytes>>>,
    handle: Handle,
    inner: tokio::io::Stdin,
}
#[allow(dead_code)]
fn default_stdin() -> tokio::io::Stdin {
    tokio::io::stdin()
}
impl Default for Stdin {
    fn default() -> Self {
        Self {
            handle: Handle::current(),
            read_buffer: Arc::new(std::sync::Mutex::new(None)),
            inner: tokio::io::stdin(),
        }
    }
}

impl AsyncRead for Stdin {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let max_size = buf.remaining();
        {
            let mut read_buffer = self.read_buffer.lock().unwrap();
            if let Some(read_buffer) = read_buffer.as_mut() {
                let buf_len = read_buffer.len();
                if buf_len > 0 {
                    let read = buf_len.min(max_size);
                    buf.put_slice(&read_buffer[..read]);
                    read_buffer.advance(read);
                    return Poll::Ready(Ok(()));
                }
            }
        }

        let _guard = Handle::try_current().map_err(|_| self.handle.enter());
        let inner = Pin::new(&mut self.inner);
        inner.poll_read(cx, buf)
    }
}

impl AsyncWrite for Stdin {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(Err(io::Error::other("can not wrote to stdin")))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Err(io::Error::other("can not flush stdin")))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Err(io::Error::other("can not wrote to stdin")))
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(Err(io::Error::other("can not wrote to stdin")))
    }
}

impl AsyncSeek for Stdin {
    fn start_seek(self: Pin<&mut Self>, _position: io::SeekFrom) -> io::Result<()> {
        Err(io::Error::other("can not seek stdin"))
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Err(io::Error::other("can not seek stdin")))
    }
}

#[async_trait::async_trait]
impl VirtualFile for Stdin {
    fn last_accessed(&self) -> u64 {
        0
    }
    fn last_modified(&self) -> u64 {
        0
    }
    fn created_time(&self) -> u64 {
        0
    }
    fn size(&self) -> u64 {
        0
    }
    fn set_len(&mut self, _new_size: u64) -> crate::Result<()> {
        Ok(())
    }
    fn unlink(&mut self) -> Result<()> {
        Ok(())
    }
    fn get_special_fd(&self) -> Option<u32> {
        Some(0)
    }
    fn poll_read_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        {
            let read_buffer = self.read_buffer.lock().unwrap();
            if let Some(read_buffer) = read_buffer.as_ref() {
                let buf_len = read_buffer.len();
                if buf_len > 0 {
                    return Poll::Ready(Ok(buf_len));
                }
            }
        }

        let _guard = Handle::try_current().map_err(|_| self.handle.enter());
        let inner = Pin::new(&mut self.inner);

        let mut buf = [0u8; 8192];
        let mut read_buf = ReadBuf::new(&mut buf[..]);
        match inner.poll_read(cx, &mut read_buf) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            Poll::Ready(Ok(())) => {
                let buf = read_buf.filled();
                let buf_len = buf.len();

                let mut read_buffer = self.read_buffer.lock().unwrap();
                read_buffer.replace(Bytes::from(buf.to_vec()));
                Poll::Ready(Ok(buf_len))
            }
        }
    }
    fn poll_write_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(0))
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    use tokio::runtime::Handle;

    use super::FileSystem;
    use crate::FileSystem as FileSystemTrait;
    use crate::{FsError, VirtualFile};
    use std::io;
    use std::path::Path;

    #[tokio::test]
    async fn test_new_filesystem() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("foo2.txt"), b"").unwrap();

        let fs = FileSystem::new(Handle::current(), temp.path()).expect("get filesystem");
        assert!(
            fs.read_dir(Path::new("/")).is_ok(),
            "NativeFS can read root"
        );
        assert!(
            fs.new_open_options()
                .read(true)
                .open(Path::new("/foo2.txt"))
                .is_ok(),
            "created foo2.txt"
        );
    }

    #[tokio::test]
    async fn test_create_dir() {
        let temp: TempDir = TempDir::new().unwrap();
        let fs = FileSystem::new(Handle::current(), temp.path()).expect("get filesystem");

        assert_eq!(
            fs.create_dir(Path::new("../")),
            Err(FsError::AlreadyExists),
            "creating a directory out of bounds",
        );

        assert_eq!(
            fs.create_dir(Path::new("/foo")),
            Ok(()),
            "creating a directory",
        );

        assert!(
            temp.path().join("foo").exists(),
            "foo dir exists in host_fs"
        );

        let cur_dir = read_dir_names(&fs, "/");

        if !cur_dir.contains(&"foo".to_string()) {
            panic!("cur_dir does not contain foo: {cur_dir:#?}");
        }

        assert!(
            cur_dir.contains(&"foo".to_string()),
            "the root is updated and well-defined"
        );

        assert_eq!(
            fs.create_dir(Path::new("foo/bar")),
            Ok(()),
            "creating a sub-directory",
        );

        assert!(
            temp.path().join("foo").join("bar").exists(),
            "foo dir exists in host_fs"
        );

        let foo_dir = read_dir_names(&fs, Path::new("/foo"));

        assert!(
            foo_dir.contains(&"bar".to_string()),
            "the foo directory is updated and well-defined"
        );

        let bar_dir = read_dir_names(&fs, Path::new("/foo/bar"));

        assert!(
            bar_dir.is_empty(),
            "the foo directory is updated and well-defined"
        );
    }

    #[tokio::test]
    async fn test_remove_dir() {
        let temp: TempDir = TempDir::new().unwrap();
        let fs = FileSystem::new(Handle::current(), temp.path()).expect("get filesystem");

        assert_eq!(
            fs.remove_dir(Path::new("/foo")),
            Err(FsError::EntryNotFound),
            "cannot remove a directory that doesn't exist",
        );

        assert_eq!(
            fs.create_dir(Path::new("foo")),
            Ok(()),
            "creating a directory",
        );

        assert_eq!(
            fs.create_dir(Path::new("foo/bar")),
            Ok(()),
            "creating a sub-directory",
        );

        assert!(temp.path().join("foo/bar").exists(), "./foo/bar exists");

        assert_eq!(
            fs.remove_dir(Path::new("foo")),
            Err(FsError::DirectoryNotEmpty),
            "removing a directory that has children",
        );

        assert_eq!(
            fs.remove_dir(Path::new("foo/bar")),
            Ok(()),
            "removing a sub-directory",
        );

        assert_eq!(
            fs.remove_dir(Path::new("foo")),
            Ok(()),
            "removing a directory",
        );

        let cur_dir = read_dir_names(&fs, "/");

        assert!(
            !cur_dir.contains(&"foo".to_string()),
            "the foo directory still exists"
        );
    }

    fn read_dir_names(fs: &FileSystem, path: impl AsRef<Path>) -> Vec<String> {
        fs.read_dir(path.as_ref())
            .unwrap()
            .filter_map(|entry| Some(entry.ok()?.file_name().to_str()?.to_string()))
            .collect::<Vec<_>>()
    }

    #[tokio::test]
    async fn test_rename() {
        let temp: TempDir = TempDir::new().unwrap();
        let fs = FileSystem::new(Handle::current(), temp.path()).expect("get filesystem");
        std::fs::create_dir_all(temp.path().join("foo").join("qux")).unwrap();
        let foo = Path::new("foo");
        let bar = Path::new("bar");
        let foo_realpath = temp.path().join(foo);
        let bar_realpath = temp.path().join(bar);

        assert_eq!(
            fs.rename(Path::new("/"), Path::new("/bar")).await,
            Err(FsError::BaseNotDirectory),
            "renaming a directory that has no parent",
        );
        assert_eq!(
            fs.rename(Path::new("/foo"), Path::new("/")).await,
            Err(FsError::BaseNotDirectory),
            "renaming to a directory that has no parent",
        );

        assert_eq!(
            fs.rename(foo, &foo.join("bar").join("baz"),).await,
            Err(FsError::EntryNotFound),
            "renaming to a directory that has parent that doesn't exist",
        );

        // On Windows, rename "to" must not be an existing directory
        #[cfg(not(target_os = "windows"))]
        assert_eq!(fs.create_dir(bar), Ok(()));

        assert_eq!(
            fs.rename(foo, bar).await,
            Ok(()),
            "renaming to a directory that has parent that exists",
        );

        assert!(
            fs.new_open_options()
                .write(true)
                .create_new(true)
                .open(bar.join("hello1.txt"))
                .is_ok(),
            "creating a new file (`hello1.txt`)",
        );
        assert!(
            fs.new_open_options()
                .write(true)
                .create_new(true)
                .open(bar.join("hello2.txt"))
                .is_ok(),
            "creating a new file (`hello2.txt`)",
        );

        let cur_dir = read_dir_names(&fs, Path::new("/"));

        assert!(
            !cur_dir.contains(&"foo".to_string()),
            "the foo directory still exists"
        );

        assert!(
            cur_dir.contains(&"bar".to_string()),
            "the bar directory still exists"
        );

        let bar_dir = read_dir_names(&fs, bar);

        if !bar_dir.contains(&"qux".to_string()) {
            println!("qux does not exist: {bar_dir:?}")
        }

        let qux_dir = read_dir_names(&fs, bar.join("qux"));

        assert!(qux_dir.is_empty(), "the qux directory is empty");

        assert!(
            bar_realpath.join("hello1.txt").exists(),
            "the /bar/hello1.txt file exists"
        );

        assert!(
            bar_realpath.join("hello2.txt").exists(),
            "the /bar/hello2.txt file exists"
        );

        assert_eq!(fs.create_dir(foo), Ok(()), "create ./foo again");

        assert_eq!(
            fs.rename(&bar.join("hello2.txt"), &foo.join("world2.txt"))
                .await,
            Ok(()),
            "renaming (and moving) a file",
        );

        assert_eq!(
            fs.rename(foo, &bar.join("baz")).await,
            Ok(()),
            "renaming a directory",
        );

        assert_eq!(
            fs.rename(&bar.join("hello1.txt"), &bar.join("world1.txt"))
                .await,
            Ok(()),
            "renaming a file (in the same directory)",
        );

        assert!(bar_realpath.exists(), "./bar exists");
        assert!(bar_realpath.join("baz").exists(), "./bar/baz exists");
        assert!(!foo_realpath.exists(), "foo does not exist anymore");
        assert!(
            bar_realpath.join("baz/world2.txt").exists(),
            "/bar/baz/world2.txt exists"
        );
        assert!(
            bar_realpath.join("world1.txt").exists(),
            "/bar/world1.txt (ex hello1.txt) exists"
        );
        assert!(
            !bar_realpath.join("hello1.txt").exists(),
            "hello1.txt was moved"
        );
        assert!(
            !bar_realpath.join("hello2.txt").exists(),
            "hello2.txt was moved"
        );
        assert!(
            bar_realpath.join("baz/world2.txt").exists(),
            "world2.txt was moved to the correct place"
        );
    }

    #[tokio::test]
    async fn test_metadata() {
        use std::thread::sleep;
        use std::time::Duration;

        let temp = TempDir::new().unwrap();

        let fs = FileSystem::new(Handle::current(), temp.path()).expect("get filesystem");

        let root_metadata = fs.metadata(Path::new("/")).unwrap();

        assert!(root_metadata.ft.dir);
        // it seems created is not available on musl, at least on CI testing.
        #[cfg(not(target_env = "musl"))]
        assert_eq!(root_metadata.accessed, root_metadata.created);
        #[cfg(not(target_env = "musl"))]
        assert_eq!(root_metadata.modified, root_metadata.created);
        assert!(root_metadata.modified > 0);

        let foo = Path::new("foo");

        assert_eq!(fs.create_dir(foo), Ok(()));

        let foo_metadata = fs.metadata(foo);
        assert!(foo_metadata.is_ok());
        let foo_metadata = foo_metadata.unwrap();

        assert!(foo_metadata.ft.dir);
        #[cfg(not(target_env = "musl"))]
        assert_eq!(foo_metadata.accessed, foo_metadata.created);
        #[cfg(not(target_env = "musl"))]
        assert_eq!(foo_metadata.modified, foo_metadata.created);
        assert!(foo_metadata.modified > 0);

        sleep(Duration::from_secs(3));

        let bar = Path::new("bar");

        assert_eq!(fs.rename(foo, bar).await, Ok(()));

        let bar_metadata = fs.metadata(bar).unwrap();
        assert!(bar_metadata.ft.dir);
        assert!(bar_metadata.accessed >= foo_metadata.accessed);
        assert_eq!(bar_metadata.created, foo_metadata.created);
        assert!(bar_metadata.modified > foo_metadata.modified);

        let root_metadata = fs.metadata(bar).unwrap();
        assert!(
            root_metadata.modified > foo_metadata.modified,
            "the parent modified time was updated"
        );
    }

    #[tokio::test]
    async fn test_rejects_host_absolute_paths_inside_root() {
        let temp = TempDir::new().unwrap();
        // Some platforms (e.g. mac) symlink /tmp to /private/tmp, so we need to canonicalize
        // the path to get the real one, making sure the guest and host paths line up.
        let temp_canon = super::canonicalize(temp.path()).expect("canonicalize temp dir");

        let file_path = temp_canon.join("foo.txt");
        std::fs::write(&file_path, b"hello").unwrap();

        let fs = FileSystem::new(Handle::current(), &temp_canon).expect("get filesystem");

        assert_eq!(fs.metadata(&file_path), Err(FsError::InvalidInput));
        assert!(matches!(
            fs.new_open_options().read(true).open(&file_path),
            Err(FsError::InvalidInput)
        ));
    }

    #[tokio::test]
    async fn test_remove_file() {
        let temp = TempDir::new().unwrap();
        let fs = FileSystem::new(Handle::current(), temp.path()).expect("get filesystem");

        assert!(
            fs.new_open_options()
                .write(true)
                .create_new(true)
                .open(Path::new("foo.txt"))
                .is_ok(),
            "creating a new file",
        );

        assert!(read_dir_names(&fs, Path::new("/")).contains(&"foo.txt".to_string()));

        assert!(temp.path().join("foo.txt").is_file());

        assert_eq!(
            fs.remove_file(Path::new("foo.txt")),
            Ok(()),
            "removing a file that exists",
        );

        assert!(!temp.path().join("foo.txt").exists());

        assert_eq!(
            fs.remove_file(Path::new("foo.txt")),
            Err(FsError::EntryNotFound),
            "removing a file that doesn't exists",
        );
    }

    #[tokio::test]
    async fn test_readdir() {
        let temp = TempDir::new().unwrap();
        let fs = FileSystem::new(Handle::current(), temp.path()).expect("get filesystem");

        assert_eq!(fs.create_dir(Path::new("foo")), Ok(()), "creating `foo`");
        assert_eq!(
            fs.create_dir(Path::new("foo/sub")),
            Ok(()),
            "creating `sub`"
        );
        assert_eq!(fs.create_dir(Path::new("bar")), Ok(()), "creating `bar`");
        assert_eq!(fs.create_dir(Path::new("baz")), Ok(()), "creating `bar`");
        assert!(
            fs.new_open_options()
                .write(true)
                .create_new(true)
                .open(Path::new("a.txt"))
                .is_ok(),
            "creating `a.txt`",
        );
        assert!(
            fs.new_open_options()
                .write(true)
                .create_new(true)
                .open(Path::new("b.txt"))
                .is_ok(),
            "creating `b.txt`",
        );

        let readdir = fs.read_dir(Path::new("/"));

        assert!(
            readdir.is_ok(),
            "reading the directory `{}`",
            Path::new("/").display()
        );

        let mut readdir = readdir.unwrap();

        let next = readdir.next().unwrap().unwrap();
        assert!(next.path.ends_with("a.txt"), "checking entry #1");
        assert!(next.metadata().unwrap().is_file(), "checking entry #1");

        let next = readdir.next().unwrap().unwrap();
        assert!(next.path.ends_with("b.txt"), "checking entry #2");
        assert!(next.metadata().unwrap().is_file(), "checking entry #2");

        let next = readdir.next().unwrap().unwrap();
        assert!(next.path.ends_with("bar"), "checking entry #3");
        assert!(next.metadata().unwrap().is_dir(), "checking entry #3");

        let next = readdir.next().unwrap().unwrap();
        assert!(next.path.ends_with("baz"), "checking entry #4");
        assert!(next.metadata().unwrap().is_dir(), "checking entry #4");

        let next = readdir.next().unwrap().unwrap();
        assert!(next.path.ends_with("foo"), "checking entry #5");
        assert!(next.metadata().unwrap().is_dir(), "checking entry #5");

        if let Some(s) = readdir.next() {
            panic!("next: {s:?}");
        }
    }

    /// Swaps the std handle of a host [`super::File`] for an fd that can never
    /// be open, so `fstat` fails (EBADF) like it does with ESTALE or EIO on
    /// network storage. Restores the real handle on drop.
    #[cfg(unix)]
    struct FailingFstat<'a> {
        file: &'a mut super::File,
        original: Option<std::fs::File>,
    }

    #[cfg(unix)]
    impl<'a> FailingFstat<'a> {
        fn new(file: &'a mut super::File) -> Self {
            use std::os::fd::FromRawFd;
            // Above the kernel's fd limit, so no file can ever be open there.
            // SAFETY: the bogus handle is only used for fstat and never closed.
            let bogus = unsafe { std::fs::File::from_raw_fd(i32::MAX) };
            let original = std::mem::replace(&mut file.inner_std, bogus);
            Self {
                file,
                original: Some(original),
            }
        }
    }

    #[cfg(unix)]
    impl Drop for FailingFstat<'_> {
        fn drop(&mut self) {
            if let Some(original) = self.original.take() {
                std::mem::forget(std::mem::replace(&mut self.file.inner_std, original));
            }
        }
    }

    #[cfg(unix)]
    fn open_host_file(temp: &TempDir, contents: &[u8]) -> super::File {
        let path = temp.path().join("file.txt");
        std::fs::write(&path, contents).unwrap();
        let file = std::fs::File::open(&path).unwrap();
        super::File::try_new(Handle::current(), file, path, true, false, false).unwrap()
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_file_accessors_do_not_panic_when_fstat_fails() {
        let temp = TempDir::new().unwrap();
        let mut file = open_host_file(&temp, b"hello");

        let broken = FailingFstat::new(&mut file);
        assert_eq!(broken.file.size(), 0);
        assert_eq!(broken.file.last_accessed(), 0);
        assert_eq!(broken.file.last_modified(), 0);
        assert_eq!(broken.file.created_time(), 0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_file_metadata_reports_fstat_failure() {
        let temp = TempDir::new().unwrap();
        let mut file = open_host_file(&temp, b"hello");

        let metadata = VirtualFile::metadata(&file).unwrap();
        assert!(metadata.is_file());
        assert_eq!(metadata.len(), 5);
        assert_ne!(metadata.modified(), 0);

        let broken = FailingFstat::new(&mut file);
        assert!(VirtualFile::metadata(&*broken.file).is_err());
    }

    #[test]
    fn test_stale_file_handle_error_mapping() {
        let stale = io::Error::from(io::ErrorKind::StaleNetworkFileHandle);
        assert_eq!(FsError::from(stale), FsError::StaleFileHandle);
        assert_eq!(
            io::Error::from(FsError::StaleFileHandle).kind(),
            io::ErrorKind::StaleNetworkFileHandle
        );

        #[cfg(unix)]
        assert_eq!(
            FsError::from(io::Error::from_raw_os_error(libc::ESTALE)),
            FsError::StaleFileHandle
        );
    }
}
