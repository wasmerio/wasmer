//! Host `std::io::Error` normalization for VFS.
//!
//! This helper keeps platform error normalization out of `vfs-core` and out of
//! individual backends. It is best-effort and intended for diagnostics and
//! host filesystem adapters.

use std::io::ErrorKind;
use vfs_core::VfsErrorKind;

/// Best-effort conversion from `std::io::Error` to `VfsErrorKind`.
pub fn io_error_to_vfs_error_kind(e: &std::io::Error) -> VfsErrorKind {
    #[cfg(feature = "host-errno")]
    if let Some(kind) = map_unix_errno(e) {
        return kind;
    }

    match e.kind() {
        ErrorKind::NotFound => VfsErrorKind::NotFound,
        ErrorKind::PermissionDenied => VfsErrorKind::PermissionDenied,
        ErrorKind::AlreadyExists => VfsErrorKind::AlreadyExists,
        ErrorKind::InvalidInput => VfsErrorKind::InvalidInput,
        ErrorKind::BrokenPipe => VfsErrorKind::BrokenPipe,
        ErrorKind::WouldBlock => VfsErrorKind::WouldBlock,
        ErrorKind::Interrupted => VfsErrorKind::Interrupted,
        ErrorKind::TimedOut => VfsErrorKind::TimedOut,
        ErrorKind::Unsupported => VfsErrorKind::NotSupported,
        ErrorKind::OutOfMemory => VfsErrorKind::NoMemory,
        ErrorKind::NotConnected => VfsErrorKind::Io,
        ErrorKind::AddrInUse => VfsErrorKind::Busy,
        ErrorKind::AddrNotAvailable => VfsErrorKind::NoAddress,
        ErrorKind::ConnectionAborted => VfsErrorKind::Io,
        ErrorKind::ConnectionRefused => VfsErrorKind::Io,
        ErrorKind::ConnectionReset => VfsErrorKind::Io,
        ErrorKind::HostUnreachable => VfsErrorKind::Io,
        ErrorKind::NetworkUnreachable => VfsErrorKind::Io,
        ErrorKind::Other => VfsErrorKind::Io,
        ErrorKind::UnexpectedEof => VfsErrorKind::Io,
        ErrorKind::WriteZero => VfsErrorKind::Io,
        _ => VfsErrorKind::Io,
    }
}

#[cfg(feature = "host-errno")]
fn map_unix_errno(e: &std::io::Error) -> Option<VfsErrorKind> {
    let raw = e.raw_os_error()?;
    if raw == libc::ENOTSUP {
        return Some(VfsErrorKind::NotSupported);
    }
    if libc::ENOTSUP != libc::EOPNOTSUPP && raw == libc::EOPNOTSUPP {
        return Some(VfsErrorKind::NotSupported);
    }
    if raw == libc::EAGAIN {
        return Some(VfsErrorKind::WouldBlock);
    }
    if libc::EAGAIN != libc::EWOULDBLOCK && raw == libc::EWOULDBLOCK {
        return Some(VfsErrorKind::WouldBlock);
    }
    let kind = match raw {
        libc::ENOENT => VfsErrorKind::NotFound,
        libc::ENOTDIR => VfsErrorKind::NotDir,
        libc::EISDIR => VfsErrorKind::IsDir,
        libc::EEXIST => VfsErrorKind::AlreadyExists,
        libc::ENOTEMPTY => VfsErrorKind::DirNotEmpty,
        libc::EACCES => VfsErrorKind::PermissionDenied,
        libc::EPERM => VfsErrorKind::OperationNotPermitted,
        libc::EINVAL => VfsErrorKind::InvalidInput,
        libc::ELOOP => VfsErrorKind::TooManySymlinks,
        libc::EXDEV => VfsErrorKind::CrossDevice,
        libc::EBUSY => VfsErrorKind::Busy,
        libc::EROFS => VfsErrorKind::ReadOnlyFs,
        libc::EINTR => VfsErrorKind::Interrupted,
        libc::ETIMEDOUT => VfsErrorKind::TimedOut,
        libc::ECANCELED => VfsErrorKind::Cancelled,
        libc::EPIPE => VfsErrorKind::BrokenPipe,
        libc::ENOSPC => VfsErrorKind::NoSpace,
        libc::EDQUOT => VfsErrorKind::QuotaExceeded,
        libc::EMFILE => VfsErrorKind::TooManyOpenFiles,
        libc::ENFILE => VfsErrorKind::TooManyOpenFilesSystem,
        libc::EMLINK => VfsErrorKind::TooManyLinks,
        libc::ENAMETOOLONG => VfsErrorKind::NameTooLong,
        libc::EFBIG => VfsErrorKind::FileTooLarge,
        libc::ENOMEM => VfsErrorKind::NoMemory,
        libc::EOVERFLOW => VfsErrorKind::Overflow,
        libc::ERANGE => VfsErrorKind::Range,
        libc::ESPIPE => VfsErrorKind::NotSeekable,
        libc::ENOSYS => VfsErrorKind::NotImplemented,
        libc::ESTALE => VfsErrorKind::Stale,
        libc::EDEADLK => VfsErrorKind::Deadlock,
        libc::ETXTBSY => VfsErrorKind::TextFileBusy,
        libc::ENODEV => VfsErrorKind::NoDevice,
        libc::ENXIO => VfsErrorKind::NoAddress,
        libc::EBADF => VfsErrorKind::BadHandle,
        _ => return None,
    };
    Some(kind)
}
