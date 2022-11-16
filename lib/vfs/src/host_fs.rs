use crate::{
    DirEntry, FileDescriptor, FileType, FsError, Metadata, OpenOptions, OpenOptionsConfig, ReadDir,
    Result, VirtualFile,
};
#[cfg(feature = "enable-serde")]
use serde::{de, Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite, AsyncSeek};
use std::convert::TryInto;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::fs;
use tokio::fs as tfs;
use std::io::{self};
#[cfg(unix)]
use std::os::unix::io::{AsRawFd, RawFd};
#[cfg(windows)]
use std::os::windows::io::{AsRawHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::debug;

trait TryIntoFileDescriptor {
    type Error;

    fn try_into_filedescriptor(&self) -> std::result::Result<FileDescriptor, Self::Error>;
}

#[cfg(unix)]
impl<T> TryIntoFileDescriptor for T
where
    T: AsRawFd,
{
    type Error = FsError;

    fn try_into_filedescriptor(&self) -> std::result::Result<FileDescriptor, Self::Error> {
        Ok(FileDescriptor(
            self.as_raw_fd()
                .try_into()
                .map_err(|_| FsError::InvalidFd)?,
        ))
    }
}

#[cfg(unix)]
impl TryInto<RawFd> for FileDescriptor {
    type Error = FsError;

    fn try_into(self) -> std::result::Result<RawFd, Self::Error> {
        self.0.try_into().map_err(|_| FsError::InvalidFd)
    }
}

#[cfg(windows)]
impl<T> TryIntoFileDescriptor for T
where
    T: AsRawHandle,
{
    type Error = FsError;

    fn try_into_filedescriptor(&self) -> std::result::Result<FileDescriptor, Self::Error> {
        Ok(FileDescriptor(self.as_raw_handle() as usize))
    }
}

#[cfg(windows)]
impl TryInto<RawHandle> for FileDescriptor {
    type Error = FsError;

    fn try_into(self) -> std::result::Result<RawHandle, Self::Error> {
        Ok(self.0 as RawHandle)
    }
}

#[derive(Debug, Default, Clone)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct FileSystem;

impl crate::FileSystem for FileSystem {
    fn read_dir(&self, path: &Path) -> Result<ReadDir> {
        let read_dir = fs::read_dir(path)?;
        let data = read_dir
            .map(|entry| {
                let entry = entry?;
                let metadata = entry.metadata()?;
                Ok(DirEntry {
                    path: entry.path(),
                    metadata: Ok(metadata.try_into()?),
                })
            })
            .collect::<std::result::Result<Vec<DirEntry>, io::Error>>()
            .map_err::<FsError, _>(Into::into)?;
        Ok(ReadDir::new(data))
    }

    fn create_dir(&self, path: &Path) -> Result<()> {
        fs::create_dir(path).map_err(Into::into)
    }

    fn remove_dir(&self, path: &Path) -> Result<()> {
        fs::remove_dir(path).map_err(Into::into)
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        fs::rename(from, to).map_err(Into::into)
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        fs::remove_file(path).map_err(Into::into)
    }

    fn new_open_options(&self) -> OpenOptions {
        OpenOptions::new(Box::new(FileOpener))
    }

    fn metadata(&self, path: &Path) -> Result<Metadata> {
        fs::metadata(path)
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

#[derive(Debug, Clone)]
pub struct FileOpener;

impl crate::FileOpener for FileOpener {
    fn open(
        &mut self,
        path: &Path,
        conf: &OpenOptionsConfig,
    ) -> Result<Box<dyn VirtualFile + Send + Sync + 'static>> {
        // TODO: handle create implying write, etc.
        let read = conf.read();
        let write = conf.write();
        let append = conf.append();
        let mut oo = fs::OpenOptions::new();
        oo.read(conf.read())
            .write(conf.write())
            .create_new(conf.create_new())
            .create(conf.create())
            .append(conf.append())
            .truncate(conf.truncate())
            .open(path)
            .map_err(Into::into)
            .map(|file| {
                Box::new(File::new(file, path.to_owned(), read, write, append))
                    as Box<dyn VirtualFile + Send + Sync + 'static>
            })
    }
}

/// A thin wrapper around `std::fs::File`
#[derive(Debug)]
#[cfg_attr(feature = "enable-serde", derive(Serialize))]
pub struct File {
    #[cfg_attr(feature = "enable-serde", serde(skip_serializing))]
    inner_std: fs::File,
    inner: tfs::File,
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
                    inner,
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
                    inner,
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
    pub fn new(file: fs::File, host_path: PathBuf, read: bool, write: bool, append: bool) -> Self {
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
            inner_std: file,
            inner: async_file,
            host_path,
            #[cfg(feature = "enable-serde")]
            flags: _flags,
        }
    }

    fn metadata(&self) -> std::fs::Metadata {
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

    fn size(&self) -> u64 {
        self.metadata().len()
    }

    fn set_len(&mut self, new_size: u64) -> Result<()> {
        fs::File::set_len(&mut self.inner_std, new_size).map_err(Into::into)
    }

    fn unlink(&mut self) -> Result<()> {
        fs::remove_file(&self.host_path).map_err(Into::into)
    }
    fn get_special_fd(&self) -> Option<u32> {
        None
    }
}

impl AsyncRead
for File {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let inner = Pin::new(&mut self.inner);
        inner.poll_read(cx, buf)
    }
}

impl AsyncWrite
for File {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let inner = Pin::new(&mut self.inner);
        inner.poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>
    ) -> Poll<io::Result<()>> {
        let inner = Pin::new(&mut self.inner);
        inner.poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>
    ) -> Poll<io::Result<()>> {
        let inner = Pin::new(&mut self.inner);
        inner.poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let inner = Pin::new(&mut self.inner);
        inner.poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

impl AsyncSeek
for File {
    fn start_seek(
        mut self: Pin<&mut Self>,
        position: io::SeekFrom
    ) -> io::Result<()> {
        let inner = Pin::new(&mut self.inner);
        inner.start_seek(position)
    }

    fn poll_complete(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>
    ) -> Poll<io::Result<u64>> {
        let inner = Pin::new(&mut self.inner);
        inner.poll_complete(cx)
    }
}

#[allow(dead_code)]
#[cfg(unix)]
fn host_file_bytes_available(host_fd: FileDescriptor) -> Result<usize> {
    let mut bytes_found: libc::c_int = 0;
    let result = unsafe { libc::ioctl(host_fd.try_into()?, libc::FIONREAD, &mut bytes_found) };

    match result {
        // success
        0 => Ok(bytes_found.try_into().unwrap_or(0)),
        libc::EBADF => Err(FsError::InvalidFd),
        libc::EFAULT => Err(FsError::InvalidData),
        libc::EINVAL => Err(FsError::InvalidInput),
        _ => Err(FsError::IOError),
    }
}

#[allow(dead_code)]
#[cfg(not(unix))]
fn host_file_bytes_available(_host_fd: FileDescriptor) -> Result<usize> {
    unimplemented!("host_file_bytes_available not yet implemented for non-Unix-like targets.  This probably means the program tried to use wasi::poll_oneoff")
}

/// A wrapper type around Stdout that implements `VirtualFile` and
/// `Serialize` + `Deserialize`.
#[derive(Debug, Default)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct Stdout;

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

    fn set_len(&mut self, _new_size: u64) -> Result<()> {
        debug!("Calling VirtualFile::set_len on stdout; this is probably a bug");
        Err(FsError::PermissionDenied)
    }

    fn unlink(&mut self) -> Result<()> {
        Ok(())
    }

    #[cfg(feature = "sys")]
    fn get_fd(&self) -> Option<FileDescriptor> {
        io::stdout().try_into_filedescriptor().ok()
    }

    #[cfg(feature = "js")]
    fn get_fd(&self) -> Option<FileDescriptor> {
        None
    }

    fn get_special_fd(&self) -> Option<u32> {
        Some(1)
    }
}

impl AsyncRead
for Stdout {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Ready(
            Err(io::Error::new(
                io::ErrorKind::Other,
                "can not read from stdout",
            ))
        )
    }
}

impl AsyncWrite
for Stdout {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut inner = tokio::io::stdout();
        let inner = Pin::new(&mut inner);
        inner.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut inner = tokio::io::stdout();
        let inner = Pin::new(&mut inner);
        inner.poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut inner = tokio::io::stdout();
        let inner = Pin::new(&mut inner);
        inner.poll_shutdown(cx)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let mut inner = tokio::io::stdout();
        let inner = Pin::new(&mut inner);
        inner.poll_write_vectored(cx, bufs)
    }
}

impl AsyncSeek
for Stdout {
    fn start_seek(self: Pin<&mut Self>, _position: io::SeekFrom) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Other, "can not seek stdout"))
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(
            Err(io::Error::new(io::ErrorKind::Other, "can not seek stdout"))
        )
    }
}

/// A wrapper type around Stderr that implements `VirtualFile` and
/// `Serialize` + `Deserialize`.
#[derive(Debug, Default)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct Stderr;

impl AsyncRead
for Stderr {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Ready(
            Err(io::Error::new(
                io::ErrorKind::Other,
                "can not read from stderr",
            ))
        )
    }
}

impl AsyncWrite
for Stderr {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut inner = tokio::io::stderr();
        let inner = Pin::new(&mut inner);
        inner.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut inner = tokio::io::stderr();
        let inner = Pin::new(&mut inner);
        inner.poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut inner = tokio::io::stderr();
        let inner = Pin::new(&mut inner);
        inner.poll_shutdown(cx)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let mut inner = tokio::io::stderr();
        let inner = Pin::new(&mut inner);
        inner.poll_write_vectored(cx, bufs)
    }
}

impl AsyncSeek
for Stderr {
    fn start_seek(self: Pin<&mut Self>, _position: io::SeekFrom) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Other, "can not seek stderr"))
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(
            Err(io::Error::new(io::ErrorKind::Other, "can not seek stderr"))
        )
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

    fn set_len(&mut self, _new_size: u64) -> Result<()> {
        debug!("Calling VirtualFile::set_len on stderr; this is probably a bug");
        Err(FsError::PermissionDenied)
    }

    fn unlink(&mut self) -> Result<()> {
        Ok(())
    }

    #[cfg(feature = "sys")]
    fn get_fd(&self) -> Option<FileDescriptor> {
        io::stderr().try_into_filedescriptor().ok()
    }

    #[cfg(feature = "js")]
    fn get_fd(&self) -> Option<FileDescriptor> {
        None
    }

    fn get_special_fd(&self) -> Option<u32> {
        Some(2)
    }
}

/// A wrapper type around Stdin that implements `VirtualFile` and
/// `Serialize` + `Deserialize`.
#[derive(Debug, Default)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct Stdin;

impl AsyncRead
for Stdin {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let mut inner = tokio::io::stdin();
        let inner = Pin::new(&mut inner);
        inner.poll_read(cx, buf)
    }
}

impl AsyncWrite
for Stdin {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(
            Err(io::Error::new(io::ErrorKind::Other, "can not wrote to stdin"))
        )
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(
            Err(io::Error::new(io::ErrorKind::Other, "can not flush stdin"))
        )
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(
            Err(io::Error::new(io::ErrorKind::Other, "can not wrote to stdin"))
        )
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(
            Err(io::Error::new(io::ErrorKind::Other, "can not wrote to stdin"))
        )
    }
}

impl AsyncSeek
for Stdin {
    fn start_seek(self: Pin<&mut Self>, _position: io::SeekFrom) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Other, "can not seek stdin"))
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(
            Err(io::Error::new(io::ErrorKind::Other, "can not seek stdin"))
        )
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

    fn set_len(&mut self, _new_size: u64) -> Result<()> {
        debug!("Calling VirtualFile::set_len on stdin; this is probably a bug");
        Err(FsError::PermissionDenied)
    }

    fn unlink(&mut self) -> Result<()> {
        Ok(())
    }

    #[cfg(feature = "sys")]
    fn get_fd(&self) -> Option<FileDescriptor> {
        io::stdin().try_into_filedescriptor().ok()
    }

    #[cfg(feature = "js")]
    fn get_fd(&self) -> Option<FileDescriptor> {
        None
    }

    fn get_special_fd(&self) -> Option<u32> {
        Some(0)
    }
}
