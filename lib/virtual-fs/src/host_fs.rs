use crate::{
    DirEntry, FileType, FsError, Metadata, OpenOptions, OpenOptionsConfig, ReadDir, Result,
    VirtualFile,
};
use bytes::{Buf, Bytes};
use futures::future::BoxFuture;
#[cfg(feature = "enable-serde")]
use serde::{de, Deserialize, Serialize};
use std::convert::TryInto;
use std::fs;
use std::io::{self, Seek};
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs as tfs;
use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite, ReadBuf};
use tokio::runtime::Handle;

#[derive(Debug, Clone)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct FileSystem {
    #[cfg_attr(feature = "enable-serde", serde(skip, default = "default_handle"))]
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

impl FileSystem {
    pub fn new(handle: Handle, root: impl Into<PathBuf>) -> Result<Self> {
        let root = canonicalize(&root.into())?;

        Ok(FileSystem { handle, root })
    }
}

impl FileSystem {
    fn prepare_path(&self, path: &Path) -> PathBuf {
        let path = normalize_path(path);

        let path = if !path.starts_with(&self.root) {
            let path = path.strip_prefix("/").unwrap_or(&path);

            self.root.join(path)
        } else {
            path.to_owned()
        };

        debug_assert!(path.starts_with(&self.root));
        path
    }
}

impl crate::FileSystem for FileSystem {
    fn readlink(&self, path: &Path) -> Result<PathBuf> {
        let path = self.prepare_path(path);

        fs::read_link(path).map_err(Into::into)
    }

    fn read_dir(&self, path: &Path) -> Result<ReadDir> {
        let path = self.prepare_path(path);

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

                let metadata = entry.metadata()?;

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
        let path = self.prepare_path(path);

        if path.parent().is_none() {
            return Err(FsError::BaseNotDirectory);
        }

        fs::create_dir(path).map_err(Into::into)
    }

    fn remove_dir(&self, path: &Path) -> Result<()> {
        let path = self.prepare_path(path);

        if path.parent().is_none() {
            return Err(FsError::BaseNotDirectory);
        }

        // https://github.com/rust-lang/rust/issues/86442
        // DirectoryNotEmpty is not implemented consistently
        if path.is_dir() && self.read_dir(&path).map(|s| !s.is_empty()).unwrap_or(false) {
            return Err(FsError::DirectoryNotEmpty);
        }
        fs::remove_dir(path).map_err(Into::into)
    }

    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            use filetime::{set_file_mtime, FileTime};
            let norm_from = normalize_path(from);
            let norm_to = normalize_path(to);

            if norm_from.parent().is_none() {
                return Err(FsError::BaseNotDirectory);
            }
            if norm_to.parent().is_none() {
                return Err(FsError::BaseNotDirectory);
            }

            let from = self.prepare_path(from);
            let to = self.prepare_path(to);

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
        let path = self.prepare_path(path);

        if path.parent().is_none() {
            return Err(FsError::BaseNotDirectory);
        }

        fs::remove_file(path).map_err(Into::into)
    }

    fn new_open_options(&self) -> OpenOptions {
        OpenOptions::new(self)
    }

    fn metadata(&self, path: &Path) -> Result<Metadata> {
        let path = self.prepare_path(path);

        fs::metadata(path)
            .and_then(TryInto::try_into)
            .map_err(Into::into)
    }

    fn symlink_metadata(&self, path: &Path) -> Result<Metadata> {
        let path = self.prepare_path(path);

        fs::symlink_metadata(path)
            .and_then(TryInto::try_into)
            .map_err(Into::into)
    }

    fn mount(
        &self,
        _name: String,
        _path: &Path,
        _fs: Box<dyn crate::FileSystem + Send + Sync>,
    ) -> Result<()> {
        Err(FsError::Unsupported)
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
                .and_then(|time| {
                    time.duration_since(UNIX_EPOCH)
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                })
                .map_or(0, |time| time.as_nanos() as u64),
            created: self
                .created()
                .and_then(|time| {
                    time.duration_since(UNIX_EPOCH)
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                })
                .map_or(0, |time| time.as_nanos() as u64),
            modified: self
                .modified()
                .and_then(|time| {
                    time.duration_since(UNIX_EPOCH)
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                })
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
        let path = self.prepare_path(path);

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
            .map_err(Into::into)
            .map(|file| {
                Box::new(File::new(
                    self.handle.clone(),
                    file,
                    path.to_owned(),
                    read,
                    write,
                    append,
                )) as Box<dyn VirtualFile + Send + Sync + 'static>
            })
    }
}

/// A thin wrapper around `std::fs::File`
#[derive(Debug)]
#[cfg_attr(feature = "enable-serde", derive(Serialize))]
pub struct File {
    #[cfg_attr(feature = "enable-serde", serde(skip, default = "default_handle"))]
    handle: Handle,
    #[cfg_attr(feature = "enable-serde", serde(skip))]
    inner: tfs::File,
    #[cfg_attr(feature = "enable-serde", serde(skip_serializing))]
    inner_std: fs::File,
    pub host_path: PathBuf,
    #[cfg(feature = "enable-serde")]
    flags: u16,
}

#[cfg(feature = "enable-serde")]
impl<'de> Deserialize<'de> for File {
    fn deserialize<D>(deserializer: D) -> std::result::Result<File, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "snake_case")]
        enum Field {
            HostPath,
            Flags,
        }

        struct FileVisitor;

        impl<'de> de::Visitor<'de> for FileVisitor {
            type Value = File;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("struct File")
            }

            fn visit_seq<V>(self, mut seq: V) -> std::result::Result<Self::Value, V::Error>
            where
                V: de::SeqAccess<'de>,
            {
                let host_path = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let flags = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?;
                let inner = fs::OpenOptions::new()
                    .read(flags & File::READ != 0)
                    .write(flags & File::WRITE != 0)
                    .append(flags & File::APPEND != 0)
                    .open(&host_path)
                    .map_err(|_| de::Error::custom("Could not open file on this system"))?;
                Ok(File {
                    handle: Handle::current(),
                    inner: tokio::fs::File::from_std(inner.try_clone().unwrap()),
                    inner_std: inner,
                    host_path,
                    flags,
                })
            }

            fn visit_map<V>(self, mut map: V) -> std::result::Result<Self::Value, V::Error>
            where
                V: de::MapAccess<'de>,
            {
                let mut host_path = None;
                let mut flags = None;
                while let Some(key) = map.next_key()? {
                    match key {
                        Field::HostPath => {
                            if host_path.is_some() {
                                return Err(de::Error::duplicate_field("host_path"));
                            }
                            host_path = Some(map.next_value()?);
                        }
                        Field::Flags => {
                            if flags.is_some() {
                                return Err(de::Error::duplicate_field("flags"));
                            }
                            flags = Some(map.next_value()?);
                        }
                    }
                }
                let host_path = host_path.ok_or_else(|| de::Error::missing_field("host_path"))?;
                let flags = flags.ok_or_else(|| de::Error::missing_field("flags"))?;
                let inner = fs::OpenOptions::new()
                    .read(flags & File::READ != 0)
                    .write(flags & File::WRITE != 0)
                    .append(flags & File::APPEND != 0)
                    .open(&host_path)
                    .map_err(|_| de::Error::custom("Could not open file on this system"))?;
                Ok(File {
                    handle: Handle::current(),
                    inner: tokio::fs::File::from_std(inner.try_clone().unwrap()),
                    inner_std: inner,
                    host_path,
                    flags,
                })
            }
        }

        const FIELDS: &[&str] = &["host_path", "flags"];
        deserializer.deserialize_struct("File", FIELDS, FileVisitor)
    }
}

impl File {
    const READ: u16 = 1;
    const WRITE: u16 = 2;
    const APPEND: u16 = 4;

    /// creates a new host file from a `std::fs::File` and a path
    pub fn new(
        handle: Handle,
        file: fs::File,
        host_path: PathBuf,
        read: bool,
        write: bool,
        append: bool,
    ) -> Self {
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

        let async_file = tfs::File::from_std(file.try_clone().unwrap());
        Self {
            handle,
            inner_std: file,
            inner: async_file,
            host_path,
            #[cfg(feature = "enable-serde")]
            flags: _flags,
        }
    }

    fn metadata(&self) -> std::fs::Metadata {
        // FIXME: no unwrap!
        self.inner_std.metadata().unwrap()
    }
}

//#[cfg_attr(feature = "enable-serde", typetag::serde)]
#[async_trait::async_trait]
impl VirtualFile for File {
    fn last_accessed(&self) -> u64 {
        self.metadata()
            .accessed()
            .ok()
            .and_then(|ct| ct.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|ct| ct.as_nanos() as u64)
            .unwrap_or(0)
    }

    fn last_modified(&self) -> u64 {
        self.metadata()
            .modified()
            .ok()
            .and_then(|ct| ct.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|ct| ct.as_nanos() as u64)
            .unwrap_or(0)
    }

    fn created_time(&self) -> u64 {
        self.metadata()
            .created()
            .ok()
            .and_then(|ct| ct.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|ct| ct.as_nanos() as u64)
            .unwrap_or(0)
    }

    fn set_times(&mut self, atime: Option<u64>, mtime: Option<u64>) -> crate::Result<()> {
        let atime = atime.map(|t| filetime::FileTime::from_unix_time(t as i64, 0));
        let mtime = mtime.map(|t| filetime::FileTime::from_unix_time(t as i64, 0));

        filetime::set_file_handle_times(&self.inner_std, atime, mtime)
            .map_err(|_| crate::FsError::IOError)
    }

    fn size(&self) -> u64 {
        self.metadata().len()
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
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct Stdout {
    #[cfg_attr(feature = "enable-serde", serde(skip, default = "default_handle"))]
    handle: Handle,
    #[cfg_attr(feature = "enable-serde", serde(skip, default = "default_stdout"))]
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

//#[cfg_attr(feature = "enable-serde", typetag::serde)]
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
        Poll::Ready(Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stdout",
        )))
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
        Err(io::Error::new(io::ErrorKind::Other, "can not seek stdout"))
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Err(io::Error::new(
            io::ErrorKind::Other,
            "can not seek stdout",
        )))
    }
}

/// A wrapper type around Stderr that implements `VirtualFile`.
#[derive(Debug)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct Stderr {
    #[cfg_attr(feature = "enable-serde", serde(skip, default = "default_handle"))]
    handle: Handle,
    #[cfg_attr(feature = "enable-serde", serde(skip, default = "default_stderr"))]
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
        Poll::Ready(Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stderr",
        )))
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
        Err(io::Error::new(io::ErrorKind::Other, "can not seek stderr"))
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Err(io::Error::new(
            io::ErrorKind::Other,
            "can not seek stderr",
        )))
    }
}

//#[cfg_attr(feature = "enable-serde", typetag::serde)]
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
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct Stdin {
    read_buffer: Arc<std::sync::Mutex<Option<Bytes>>>,
    #[cfg_attr(feature = "enable-serde", serde(skip, default = "default_handle"))]
    handle: Handle,
    #[cfg_attr(feature = "enable-serde", serde(skip, default = "default_stdin"))]
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
        Poll::Ready(Err(io::Error::new(
            io::ErrorKind::Other,
            "can not wrote to stdin",
        )))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Err(io::Error::new(
            io::ErrorKind::Other,
            "can not flush stdin",
        )))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Err(io::Error::new(
            io::ErrorKind::Other,
            "can not wrote to stdin",
        )))
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(Err(io::Error::new(
            io::ErrorKind::Other,
            "can not wrote to stdin",
        )))
    }
}

impl AsyncSeek for Stdin {
    fn start_seek(self: Pin<&mut Self>, _position: io::SeekFrom) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Other, "can not seek stdin"))
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Err(io::Error::new(
            io::ErrorKind::Other,
            "can not seek stdin",
        )))
    }
}

//#[cfg_attr(feature = "enable-serde", typetag::serde)]
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
    use crate::FsError;
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
        // it seems created is not evailable on musl, at least on CI testing.
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
}
