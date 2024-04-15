#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

#[cfg(test)]
#[macro_use]
extern crate pretty_assertions;

use futures::future::BoxFuture;
use std::any::Any;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use thiserror::Error;

pub mod arc_box_file;
pub mod arc_file;
pub mod arc_fs;
pub mod buffer_file;
pub mod builder;
pub mod combine_file;
pub mod cow_file;
pub mod dual_write_file;
pub mod empty_fs;
#[cfg(feature = "host-fs")]
pub mod host_fs;
pub mod mem_fs;
pub mod null_file;
pub mod passthru_fs;
pub mod random_file;
pub mod special_file;
pub mod tmp_fs;
pub mod union_fs;
pub mod zero_file;
// tty_file -> see wasmer_wasi::tty_file
mod filesystems;
pub(crate) mod ops;
mod overlay_fs;
pub mod pipe;
#[cfg(feature = "host-fs")]
mod scoped_directory_fs;
mod static_file;
#[cfg(feature = "static-fs")]
pub mod static_fs;
mod trace_fs;
#[cfg(feature = "webc-fs")]
pub mod webc_fs;
#[cfg(feature = "webc-fs")]
mod webc_volume_fs;

pub mod limiter;

pub use arc_box_file::*;
pub use arc_file::*;
pub use arc_fs::*;
pub use buffer_file::*;
pub use builder::*;
pub use combine_file::*;
pub use cow_file::*;
pub use dual_write_file::*;
pub use empty_fs::*;
pub use filesystems::FileSystems;
pub use null_file::*;
pub use overlay_fs::OverlayFileSystem;
pub use passthru_fs::*;
pub use pipe::*;
#[cfg(feature = "host-fs")]
pub use scoped_directory_fs::ScopedDirectoryFileSystem;
pub use special_file::*;
pub use static_file::StaticFile;
pub use tmp_fs::*;
pub use trace_fs::TraceFileSystem;
pub use union_fs::*;
#[cfg(feature = "webc-fs")]
pub use webc_volume_fs::WebcVolumeFileSystem;
pub use zero_file::*;

pub type Result<T> = std::result::Result<T, FsError>;

// re-exports
pub use tokio::io::ReadBuf;
pub use tokio::io::{AsyncRead, AsyncReadExt};
pub use tokio::io::{AsyncSeek, AsyncSeekExt};
pub use tokio::io::{AsyncWrite, AsyncWriteExt};

pub trait ClonableVirtualFile: VirtualFile + Clone {}

pub use ops::{copy_reference, copy_reference_ext};

pub trait FileSystem: fmt::Debug + Send + Sync + 'static + Upcastable {
    fn read_dir(&self, path: &Path) -> Result<ReadDir>;
    fn create_dir(&self, path: &Path) -> Result<()>;
    fn remove_dir(&self, path: &Path) -> Result<()>;
    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>>;
    fn metadata(&self, path: &Path) -> Result<Metadata>;
    /// This method gets metadata without following symlinks in the path.
    /// Currently identical to `metadata` because symlinks aren't implemented
    /// yet.
    fn symlink_metadata(&self, path: &Path) -> Result<Metadata> {
        self.metadata(path)
    }
    fn remove_file(&self, path: &Path) -> Result<()>;

    fn new_open_options(&self) -> OpenOptions;
}

impl dyn FileSystem + 'static {
    #[inline]
    pub fn downcast_ref<T: 'static>(&'_ self) -> Option<&'_ T> {
        self.upcast_any_ref().downcast_ref::<T>()
    }
    #[inline]
    pub fn downcast_mut<T: 'static>(&'_ mut self) -> Option<&'_ mut T> {
        self.upcast_any_mut().downcast_mut::<T>()
    }
}

#[async_trait::async_trait]
impl<D, F> FileSystem for D
where
    D: Deref<Target = F> + std::fmt::Debug + Send + Sync + 'static,
    F: FileSystem + ?Sized,
{
    fn read_dir(&self, path: &Path) -> Result<ReadDir> {
        (**self).read_dir(path)
    }

    fn create_dir(&self, path: &Path) -> Result<()> {
        (**self).create_dir(path)
    }

    fn remove_dir(&self, path: &Path) -> Result<()> {
        (**self).remove_dir(path)
    }

    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { (**self).rename(from, to).await })
    }

    fn metadata(&self, path: &Path) -> Result<Metadata> {
        (**self).metadata(path)
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        (**self).remove_file(path)
    }

    fn new_open_options(&self) -> OpenOptions {
        (**self).new_open_options()
    }
}

pub trait FileOpener {
    fn open(
        &self,
        path: &Path,
        conf: &OpenOptionsConfig,
    ) -> Result<Box<dyn VirtualFile + Send + Sync + 'static>>;
}

#[derive(Debug, Clone)]
pub struct OpenOptionsConfig {
    pub read: bool,
    pub write: bool,
    pub create_new: bool,
    pub create: bool,
    pub append: bool,
    pub truncate: bool,
}

impl OpenOptionsConfig {
    /// Returns the minimum allowed rights, given the rights of the parent directory
    pub fn minimum_rights(&self, parent_rights: &Self) -> Self {
        Self {
            read: parent_rights.read && self.read,
            write: parent_rights.write && self.write,
            create_new: parent_rights.create_new && self.create_new,
            create: parent_rights.create && self.create,
            append: parent_rights.append && self.append,
            truncate: parent_rights.truncate && self.truncate,
        }
    }

    pub const fn read(&self) -> bool {
        self.read
    }

    pub const fn write(&self) -> bool {
        self.write
    }

    pub const fn create_new(&self) -> bool {
        self.create_new
    }

    pub const fn create(&self) -> bool {
        self.create
    }

    pub const fn append(&self) -> bool {
        self.append
    }

    pub const fn truncate(&self) -> bool {
        self.truncate
    }

    /// Would a file opened with this [`OpenOptionsConfig`] change files on the
    /// filesystem.
    pub const fn would_mutate(&self) -> bool {
        let OpenOptionsConfig {
            read: _,
            write,
            create_new,
            create,
            append,
            truncate,
        } = *self;
        append || write || create || create_new || truncate
    }
}

impl<'a> fmt::Debug for OpenOptions<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.conf.fmt(f)
    }
}

pub struct OpenOptions<'a> {
    opener: &'a dyn FileOpener,
    conf: OpenOptionsConfig,
}

impl<'a> OpenOptions<'a> {
    pub fn new(opener: &'a dyn FileOpener) -> Self {
        Self {
            opener,
            conf: OpenOptionsConfig {
                read: false,
                write: false,
                create_new: false,
                create: false,
                append: false,
                truncate: false,
            },
        }
    }

    pub fn get_config(&self) -> OpenOptionsConfig {
        self.conf.clone()
    }

    /// Use an existing [`OpenOptionsConfig`] to configure this [`OpenOptions`].
    pub fn options(&mut self, options: OpenOptionsConfig) -> &mut Self {
        self.conf = options;
        self
    }

    /// Sets the option for read access.
    ///
    /// This option, when true, will indicate that the file should be
    /// `read`-able if opened.
    pub fn read(&mut self, read: bool) -> &mut Self {
        self.conf.read = read;
        self
    }

    /// Sets the option for write access.
    ///
    /// This option, when true, will indicate that the file should be
    /// `write`-able if opened.
    ///
    /// If the file already exists, any write calls on it will overwrite its
    /// contents, without truncating it.
    pub fn write(&mut self, write: bool) -> &mut Self {
        self.conf.write = write;
        self
    }

    /// Sets the option for the append mode.
    ///
    /// This option, when true, means that writes will append to a file instead
    /// of overwriting previous contents.
    /// Note that setting `.write(true).append(true)` has the same effect as
    /// setting only `.append(true)`.
    pub fn append(&mut self, append: bool) -> &mut Self {
        self.conf.append = append;
        self
    }

    /// Sets the option for truncating a previous file.
    ///
    /// If a file is successfully opened with this option set it will truncate
    /// the file to 0 length if it already exists.
    ///
    /// The file must be opened with write access for truncate to work.
    pub fn truncate(&mut self, truncate: bool) -> &mut Self {
        self.conf.truncate = truncate;
        self
    }

    /// Sets the option to create a new file, or open it if it already exists.
    pub fn create(&mut self, create: bool) -> &mut Self {
        self.conf.create = create;
        self
    }

    /// Sets the option to create a new file, failing if it already exists.
    pub fn create_new(&mut self, create_new: bool) -> &mut Self {
        self.conf.create_new = create_new;
        self
    }

    pub fn open<P: AsRef<Path>>(
        &mut self,
        path: P,
    ) -> Result<Box<dyn VirtualFile + Send + Sync + 'static>> {
        self.opener.open(path.as_ref(), &self.conf)
    }
}

/// This trait relies on your file closing when it goes out of scope via `Drop`
//#[cfg_attr(feature = "enable-serde", typetag::serde)]
pub trait VirtualFile:
    fmt::Debug + AsyncRead + AsyncWrite + AsyncSeek + Unpin + Upcastable + Send
{
    /// the last time the file was accessed in nanoseconds as a UNIX timestamp
    fn last_accessed(&self) -> u64;

    /// the last time the file was modified in nanoseconds as a UNIX timestamp
    fn last_modified(&self) -> u64;

    /// the time at which the file was created in nanoseconds as a UNIX timestamp
    fn created_time(&self) -> u64;

    /// the size of the file in bytes
    fn size(&self) -> u64;

    /// Change the size of the file, if the `new_size` is greater than the current size
    /// the extra bytes will be allocated and zeroed
    fn set_len(&mut self, new_size: u64) -> Result<()>;

    /// Request deletion of the file
    fn unlink(&mut self) -> Result<()>;

    /// Indicates if the file is opened or closed. This function must not block
    /// Defaults to a status of being constantly open
    fn is_open(&self) -> bool {
        true
    }

    /// Used for "special" files such as `stdin`, `stdout` and `stderr`.
    /// Always returns the same file descriptor (0, 1 or 2). Returns `None`
    /// on normal files
    fn get_special_fd(&self) -> Option<u32> {
        None
    }

    /// Writes to this file using an mmap offset and reference
    /// (this method only works for mmap optimized file systems)
    fn write_from_mmap(&mut self, _offset: u64, _len: u64) -> std::io::Result<()> {
        Err(std::io::ErrorKind::Unsupported.into())
    }

    /// This method will copy a file from a source to this destination where
    /// the default is to do a straight byte copy however file system implementors
    /// may optimize this to do a zero copy
    fn copy_reference(
        &mut self,
        mut src: Box<dyn VirtualFile + Send + Sync + 'static>,
    ) -> BoxFuture<'_, std::io::Result<()>> {
        Box::pin(async move {
            let bytes_written = tokio::io::copy(&mut src, self).await?;
            tracing::trace!(bytes_written, "Copying file into host filesystem",);
            Ok(())
        })
    }

    /// Polls the file for when there is data to be read
    fn poll_read_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>>;

    /// Polls the file for when it is available for writing
    fn poll_write_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>>;
}

// Implementation of `Upcastable` taken from https://users.rust-lang.org/t/why-does-downcasting-not-work-for-subtraits/33286/7 .
/// Trait needed to get downcasting from `VirtualFile` to work.
pub trait Upcastable {
    fn upcast_any_ref(&'_ self) -> &'_ dyn Any;
    fn upcast_any_mut(&'_ mut self) -> &'_ mut dyn Any;
    fn upcast_any_box(self: Box<Self>) -> Box<dyn Any>;
}

impl<T: Any + fmt::Debug + 'static> Upcastable for T {
    #[inline]
    fn upcast_any_ref(&'_ self) -> &'_ dyn Any {
        self
    }
    #[inline]
    fn upcast_any_mut(&'_ mut self) -> &'_ mut dyn Any {
        self
    }
    #[inline]
    fn upcast_any_box(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

/// Determines the mode that stdio handlers will operate in
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum StdioMode {
    /// Stdio will be piped to a file descriptor
    Piped,
    /// Stdio will inherit the file handlers of its parent
    Inherit,
    /// Stdio will be dropped
    Null,
    /// Stdio will be sent to the log handler
    Log,
}

/// Error type for external users
#[derive(Error, Copy, Clone, Debug, PartialEq, Eq)]
pub enum FsError {
    /// The fd given as a base was not a directory so the operation was not possible
    #[error("fd not a directory")]
    BaseNotDirectory,
    /// Expected a file but found not a file
    #[error("fd not a file")]
    NotAFile,
    /// The fd given was not usable
    #[error("invalid fd")]
    InvalidFd,
    /// File exists
    #[error("file exists")]
    AlreadyExists,
    /// The filesystem has failed to lock a resource.
    #[error("lock error")]
    Lock,
    /// Something failed when doing IO. These errors can generally not be handled.
    /// It may work if tried again.
    #[error("io error")]
    IOError,
    /// The address was in use
    #[error("address is in use")]
    AddressInUse,
    /// The address could not be found
    #[error("address could not be found")]
    AddressNotAvailable,
    /// A pipe was closed
    #[error("broken pipe (was closed)")]
    BrokenPipe,
    /// The connection was aborted
    #[error("connection aborted")]
    ConnectionAborted,
    /// The connection request was refused
    #[error("connection refused")]
    ConnectionRefused,
    /// The connection was reset
    #[error("connection reset")]
    ConnectionReset,
    /// The operation was interrupted before it could finish
    #[error("operation interrupted")]
    Interrupted,
    /// Invalid internal data, if the argument data is invalid, use `InvalidInput`
    #[error("invalid internal data")]
    InvalidData,
    /// The provided data is invalid
    #[error("invalid input")]
    InvalidInput,
    /// Could not perform the operation because there was not an open connection
    #[error("connection is not open")]
    NotConnected,
    /// The requested file or directory could not be found
    #[error("entry not found")]
    EntryNotFound,
    /// The requested device couldn't be accessed
    #[error("can't access device")]
    NoDevice,
    /// Caller was not allowed to perform this operation
    #[error("permission denied")]
    PermissionDenied,
    /// The operation did not complete within the given amount of time
    #[error("time out")]
    TimedOut,
    /// Found EOF when EOF was not expected
    #[error("unexpected eof")]
    UnexpectedEof,
    /// Operation would block, this error lets the caller know that they can try again
    #[error("blocking operation. try again")]
    WouldBlock,
    /// A call to write returned 0
    #[error("write returned 0")]
    WriteZero,
    /// Directory not Empty
    #[error("directory not empty")]
    DirectoryNotEmpty,
    #[error("storage full")]
    StorageFull,
    /// Some other unhandled error. If you see this, it's probably a bug.
    #[error("unknown error found")]
    UnknownError,
}

impl From<io::Error> for FsError {
    fn from(io_error: io::Error) -> Self {
        match io_error.kind() {
            io::ErrorKind::AddrInUse => FsError::AddressInUse,
            io::ErrorKind::AddrNotAvailable => FsError::AddressNotAvailable,
            io::ErrorKind::AlreadyExists => FsError::AlreadyExists,
            io::ErrorKind::BrokenPipe => FsError::BrokenPipe,
            io::ErrorKind::ConnectionAborted => FsError::ConnectionAborted,
            io::ErrorKind::ConnectionRefused => FsError::ConnectionRefused,
            io::ErrorKind::ConnectionReset => FsError::ConnectionReset,
            io::ErrorKind::Interrupted => FsError::Interrupted,
            io::ErrorKind::InvalidData => FsError::InvalidData,
            io::ErrorKind::InvalidInput => FsError::InvalidInput,
            io::ErrorKind::NotConnected => FsError::NotConnected,
            io::ErrorKind::NotFound => FsError::EntryNotFound,
            io::ErrorKind::PermissionDenied => FsError::PermissionDenied,
            io::ErrorKind::TimedOut => FsError::TimedOut,
            io::ErrorKind::UnexpectedEof => FsError::UnexpectedEof,
            io::ErrorKind::WouldBlock => FsError::WouldBlock,
            io::ErrorKind::WriteZero => FsError::WriteZero,
            // NOTE: Add this once the "io_error_more" Rust feature is stabilized
            // io::ErrorKind::StorageFull => FsError::StorageFull,
            io::ErrorKind::Other => FsError::IOError,
            // if the following triggers, a new error type was added to this non-exhaustive enum
            _ => FsError::UnknownError,
        }
    }
}

impl From<FsError> for io::Error {
    fn from(val: FsError) -> Self {
        let kind = match val {
            FsError::AddressInUse => io::ErrorKind::AddrInUse,
            FsError::AddressNotAvailable => io::ErrorKind::AddrNotAvailable,
            FsError::AlreadyExists => io::ErrorKind::AlreadyExists,
            FsError::BrokenPipe => io::ErrorKind::BrokenPipe,
            FsError::ConnectionAborted => io::ErrorKind::ConnectionAborted,
            FsError::ConnectionRefused => io::ErrorKind::ConnectionRefused,
            FsError::ConnectionReset => io::ErrorKind::ConnectionReset,
            FsError::Interrupted => io::ErrorKind::Interrupted,
            FsError::InvalidData => io::ErrorKind::InvalidData,
            FsError::InvalidInput => io::ErrorKind::InvalidInput,
            FsError::NotConnected => io::ErrorKind::NotConnected,
            FsError::EntryNotFound => io::ErrorKind::NotFound,
            FsError::PermissionDenied => io::ErrorKind::PermissionDenied,
            FsError::TimedOut => io::ErrorKind::TimedOut,
            FsError::UnexpectedEof => io::ErrorKind::UnexpectedEof,
            FsError::WouldBlock => io::ErrorKind::WouldBlock,
            FsError::WriteZero => io::ErrorKind::WriteZero,
            FsError::IOError => io::ErrorKind::Other,
            FsError::BaseNotDirectory => io::ErrorKind::Other,
            FsError::NotAFile => io::ErrorKind::Other,
            FsError::InvalidFd => io::ErrorKind::Other,
            FsError::Lock => io::ErrorKind::Other,
            FsError::NoDevice => io::ErrorKind::Other,
            FsError::DirectoryNotEmpty => io::ErrorKind::Other,
            FsError::UnknownError => io::ErrorKind::Other,
            FsError::StorageFull => io::ErrorKind::Other,
            // NOTE: Add this once the "io_error_more" Rust feature is stabilized
            // FsError::StorageFull => io::ErrorKind::StorageFull,
        };
        kind.into()
    }
}

#[derive(Debug)]
pub struct ReadDir {
    // TODO: to do this properly we need some kind of callback to the core FS abstraction
    data: Vec<DirEntry>,
    index: usize,
}

impl ReadDir {
    pub fn new(data: Vec<DirEntry>) -> Self {
        Self { data, index: 0 }
    }
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub path: PathBuf,
    // weird hack, to fix this we probably need an internal trait object or callbacks or something
    pub metadata: Result<Metadata>,
}

impl DirEntry {
    pub fn path(&self) -> PathBuf {
        self.path.clone()
    }

    pub fn metadata(&self) -> Result<Metadata> {
        self.metadata.clone()
    }

    pub fn file_type(&self) -> Result<FileType> {
        let metadata = self.metadata.clone()?;
        Ok(metadata.file_type())
    }

    pub fn file_name(&self) -> OsString {
        self.path
            .file_name()
            .unwrap_or(self.path.as_os_str())
            .to_owned()
    }

    pub fn is_white_out(&self) -> Option<PathBuf> {
        ops::is_white_out(&self.path)
    }
}

#[allow(clippy::len_without_is_empty)] // Clippy thinks it's an iterator.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
// TODO: review this, proper solution would probably use a trait object internally
pub struct Metadata {
    pub ft: FileType,
    pub accessed: u64,
    pub created: u64,
    pub modified: u64,
    pub len: u64,
}

impl Metadata {
    pub fn is_file(&self) -> bool {
        self.ft.is_file()
    }

    pub fn is_dir(&self) -> bool {
        self.ft.is_dir()
    }

    pub fn accessed(&self) -> u64 {
        self.accessed
    }

    pub fn created(&self) -> u64 {
        self.created
    }

    pub fn modified(&self) -> u64 {
        self.modified
    }

    pub fn file_type(&self) -> FileType {
        self.ft.clone()
    }

    pub fn len(&self) -> u64 {
        self.len
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
// TODO: review this, proper solution would probably use a trait object internally
pub struct FileType {
    pub dir: bool,
    pub file: bool,
    pub symlink: bool,
    // TODO: the following 3 only exist on unix in the standard FS API.
    // We should mirror that API and extend with that trait too.
    pub char_device: bool,
    pub block_device: bool,
    pub socket: bool,
    pub fifo: bool,
}

impl FileType {
    pub fn new_dir() -> Self {
        Self {
            dir: true,
            ..Default::default()
        }
    }

    pub fn new_file() -> Self {
        Self {
            file: true,
            ..Default::default()
        }
    }

    pub fn is_dir(&self) -> bool {
        self.dir
    }
    pub fn is_file(&self) -> bool {
        self.file
    }
    pub fn is_symlink(&self) -> bool {
        self.symlink
    }
    pub fn is_char_device(&self) -> bool {
        self.char_device
    }
    pub fn is_block_device(&self) -> bool {
        self.block_device
    }
    pub fn is_socket(&self) -> bool {
        self.socket
    }
    pub fn is_fifo(&self) -> bool {
        self.fifo
    }
}

impl Iterator for ReadDir {
    type Item = Result<DirEntry>;

    fn next(&mut self) -> Option<Result<DirEntry>> {
        if let Some(v) = self.data.get(self.index).cloned() {
            self.index += 1;
            return Some(Ok(v));
        }
        None
    }
}
