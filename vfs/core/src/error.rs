use std::borrow::Cow;
use std::fmt;

/// Core VFS error surface.
///
/// This is intentionally "errno-like": it represents semantic failure modes that can be mapped to
/// WASI/OS errors in a single place (e.g. `vfs-unix`).
#[derive(Debug)]
pub enum VfsError {
    // Common POSIX-ish errors.
    NotFound,               // ENOENT
    NotDir,                 // ENOTDIR
    IsDir,                  // EISDIR
    AlreadyExists,          // EEXIST
    NotEmpty,               // ENOTEMPTY
    PermissionDenied,       // EACCES
    OperationNotPermitted,  // EPERM
    InvalidInput,           // EINVAL
    WouldBlock,             // EAGAIN / EWOULDBLOCK
    Interrupted,            // EINTR
    TimedOut,               // ETIMEDOUT
    Cancelled,              // ECANCELED
    BrokenPipe,             // EPIPE
    Busy,                   // EBUSY
    ReadOnlyFs,             // EROFS
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
    CrossDeviceLink,        // EXDEV
    TooManySymlinks,        // ELOOP
    NotSupported,           // ENOTSUP / EOPNOTSUPP
    NotImplemented,         // ENOSYS
    Stale,                  // ESTALE (best-effort; backend-dependent)
    Deadlock,               // EDEADLK
    TextFileBusy,           // ETXTBSY
    NoDevice,               // ENODEV
    NoAddress,              // ENXIO
    /// File descriptor / handle is invalid.
    BadHandle, // EBADF

    /// Raw IO error from a backend or host API.
    ///
    /// Prefer using a more specific variant when possible so errno mapping is stable.
    Io(std::io::Error),

    /// Generic message for internal invariants, unclassified backend errors, etc.
    Message(Cow<'static, str>),
}

impl VfsError {
    pub fn message(msg: impl Into<Cow<'static, str>>) -> Self {
        Self::Message(msg.into())
    }
}

impl fmt::Display for VfsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VfsError::NotFound => write!(f, "not found"),
            VfsError::NotDir => write!(f, "not a directory"),
            VfsError::IsDir => write!(f, "is a directory"),
            VfsError::AlreadyExists => write!(f, "already exists"),
            VfsError::NotEmpty => write!(f, "directory not empty"),
            VfsError::PermissionDenied => write!(f, "permission denied"),
            VfsError::OperationNotPermitted => write!(f, "operation not permitted"),
            VfsError::InvalidInput => write!(f, "invalid input"),
            VfsError::WouldBlock => write!(f, "operation would block"),
            VfsError::Interrupted => write!(f, "interrupted"),
            VfsError::TimedOut => write!(f, "timed out"),
            VfsError::Cancelled => write!(f, "cancelled"),
            VfsError::BrokenPipe => write!(f, "broken pipe"),
            VfsError::Busy => write!(f, "resource busy"),
            VfsError::ReadOnlyFs => write!(f, "read-only filesystem"),
            VfsError::NoSpace => write!(f, "no space left on device"),
            VfsError::QuotaExceeded => write!(f, "quota exceeded"),
            VfsError::TooManyOpenFiles => write!(f, "too many open files"),
            VfsError::TooManyOpenFilesSystem => write!(f, "system-wide open file limit reached"),
            VfsError::TooManyLinks => write!(f, "too many links"),
            VfsError::NameTooLong => write!(f, "name too long"),
            VfsError::FileTooLarge => write!(f, "file too large"),
            VfsError::NoMemory => write!(f, "out of memory"),
            VfsError::Overflow => write!(f, "value too large"),
            VfsError::Range => write!(f, "result out of range"),
            VfsError::NotSeekable => write!(f, "illegal seek"),
            VfsError::CrossDeviceLink => write!(f, "cross-device link"),
            VfsError::TooManySymlinks => write!(f, "too many symlinks"),
            VfsError::NotSupported => write!(f, "operation not supported"),
            VfsError::NotImplemented => write!(f, "function not implemented"),
            VfsError::Stale => write!(f, "stale file handle"),
            VfsError::Deadlock => write!(f, "resource deadlock avoided"),
            VfsError::TextFileBusy => write!(f, "text file busy"),
            VfsError::NoDevice => write!(f, "no such device"),
            VfsError::NoAddress => write!(f, "no such device or address"),
            VfsError::BadHandle => write!(f, "bad handle"),
            VfsError::Io(err) => write!(f, "io error: {err}"),
            VfsError::Message(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for VfsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            VfsError::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for VfsError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

pub type VfsResult<T> = Result<T, VfsError>;
