use std::error::Error;
use std::fmt;

pub type VfsResult<T> = Result<T, VfsError>;

/// Semantically meaningful VFS error kinds.
///
/// This is "errno-like" (Linux/POSIX semantics), but it is not a 1:1 mirror of host errno values.
/// Mapping to host errno or WASI errno is handled in `vfs-unix` as a single source of truth.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VfsErrorKind {
    NotFound,      // ENOENT
    NotDir,        // ENOTDIR
    IsDir,         // EISDIR
    AlreadyExists, // EEXIST
    DirNotEmpty,   // ENOTEMPTY

    PermissionDenied,      // EACCES
    OperationNotPermitted, // EPERM
    InvalidInput,          // EINVAL
    TooManySymlinks,       // ELOOP
    NotSupported,          // ENOTSUP / EOPNOTSUPP
    CrossDevice,           // EXDEV

    Busy,        // EBUSY
    ReadOnlyFs,  // EROFS
    WouldBlock,  // EAGAIN / EWOULDBLOCK
    Interrupted, // EINTR
    TimedOut,    // ETIMEDOUT
    Cancelled,   // ECANCELED
    BrokenPipe,  // EPIPE

    NoSpace,                // ENOSPC
    QuotaExceeded,          // EDQUOT
    TooManyOpenFiles,       // EMFILE
    TooManyOpenFilesSystem, // ENFILE
    TooManyLinks,           // EMLINK
    NameTooLong,            // ENAMETOOLONG
    FileTooLarge,           // EFBIG
    NoMemory,               // ENOMEM
    Overflow,               // EOVERFLOW
    Range,                  // ERANGE
    NotSeekable,            // ESPIPE

    NotImplemented, // ENOSYS
    Stale,          // ESTALE
    Deadlock,       // EDEADLK
    TextFileBusy,   // ETXTBSY
    NoDevice,       // ENODEV
    NoAddress,      // ENXIO
    BadHandle,      // EBADF

    Internal, // (maps to EIO in `vfs-unix`)
    Io,       // generic fallback
}

impl fmt::Display for VfsErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            VfsErrorKind::NotFound => "not found",
            VfsErrorKind::NotDir => "not a directory",
            VfsErrorKind::IsDir => "is a directory",
            VfsErrorKind::AlreadyExists => "already exists",
            VfsErrorKind::DirNotEmpty => "directory not empty",
            VfsErrorKind::PermissionDenied => "permission denied",
            VfsErrorKind::OperationNotPermitted => "operation not permitted",
            VfsErrorKind::InvalidInput => "invalid input",
            VfsErrorKind::TooManySymlinks => "too many symlinks",
            VfsErrorKind::NotSupported => "operation not supported",
            VfsErrorKind::CrossDevice => "cross-device operation",
            VfsErrorKind::Busy => "resource busy",
            VfsErrorKind::ReadOnlyFs => "read-only filesystem",
            VfsErrorKind::WouldBlock => "operation would block",
            VfsErrorKind::Interrupted => "interrupted",
            VfsErrorKind::TimedOut => "timed out",
            VfsErrorKind::Cancelled => "cancelled",
            VfsErrorKind::BrokenPipe => "broken pipe",
            VfsErrorKind::NoSpace => "no space left on device",
            VfsErrorKind::QuotaExceeded => "quota exceeded",
            VfsErrorKind::TooManyOpenFiles => "too many open files",
            VfsErrorKind::TooManyOpenFilesSystem => "system-wide open file limit reached",
            VfsErrorKind::TooManyLinks => "too many links",
            VfsErrorKind::NameTooLong => "name too long",
            VfsErrorKind::FileTooLarge => "file too large",
            VfsErrorKind::NoMemory => "out of memory",
            VfsErrorKind::Overflow => "value too large",
            VfsErrorKind::Range => "result out of range",
            VfsErrorKind::NotSeekable => "illegal seek",
            VfsErrorKind::NotImplemented => "function not implemented",
            VfsErrorKind::Stale => "stale file handle",
            VfsErrorKind::Deadlock => "resource deadlock avoided",
            VfsErrorKind::TextFileBusy => "text file busy",
            VfsErrorKind::NoDevice => "no such device",
            VfsErrorKind::NoAddress => "no such device or address",
            VfsErrorKind::BadHandle => "bad handle",
            VfsErrorKind::Internal => "internal error",
            VfsErrorKind::Io => "io error",
        };
        f.write_str(msg)
    }
}

/// VFS error value with a stable semantic kind and a cheap static context string.
pub struct VfsError {
    kind: VfsErrorKind,
    context: &'static str,
    source: Option<Box<dyn Error + Send + Sync + 'static>>,
}

impl VfsError {
    #[inline]
    pub fn new(kind: VfsErrorKind, context: &'static str) -> Self {
        Self {
            kind,
            context,
            source: None,
        }
    }

    #[inline]
    pub fn with_source(
        kind: VfsErrorKind,
        context: &'static str,
        source: impl Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind,
            context,
            source: Some(Box::new(source)),
        }
    }

    #[inline]
    pub fn kind(&self) -> VfsErrorKind {
        self.kind
    }

    #[inline]
    pub fn context(&self) -> &'static str {
        self.context
    }
}

impl fmt::Debug for VfsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VfsError")
            .field("kind", &self.kind)
            .field("context", &self.context)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for VfsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (context: {})", self.kind, self.context)
    }
}

impl Error for VfsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_deref().map(|e| e as _)
    }
}

impl From<std::io::Error> for VfsError {
    fn from(value: std::io::Error) -> Self {
        VfsError::with_source(VfsErrorKind::Io, "io", value)
    }
}
