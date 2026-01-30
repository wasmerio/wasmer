//! VFS → WASI errno translation.
//!
//! This is the single source of truth for mapping `vfs-core` error kinds to
//! `wasi::Errno`. Callers must not duplicate this mapping elsewhere.

use vfs_core::{VfsError, VfsErrorKind};
use wasmer_wasix_types::wasi::Errno;

/// Convert a VFS error to a WASI errno (single source of truth).
pub fn vfs_error_to_wasi_errno(err: &VfsError) -> Errno {
    vfs_error_kind_to_wasi_errno(err.kind())
}

/// Convert a VFS error kind to WASI errno (useful in tests and fast paths).
pub fn vfs_error_kind_to_wasi_errno(kind: VfsErrorKind) -> Errno {
    match kind {
        VfsErrorKind::NotFound => Errno::Noent,
        VfsErrorKind::NotDir => Errno::Notdir,
        VfsErrorKind::IsDir => Errno::Isdir,
        VfsErrorKind::AlreadyExists => Errno::Exist,
        VfsErrorKind::DirNotEmpty => Errno::Notempty,
        VfsErrorKind::PermissionDenied => Errno::Access,
        VfsErrorKind::OperationNotPermitted => Errno::Perm,
        VfsErrorKind::InvalidInput => Errno::Inval,
        VfsErrorKind::TooManySymlinks => Errno::Loop,
        VfsErrorKind::NotSupported => Errno::Notsup,
        VfsErrorKind::CrossDevice => Errno::Xdev,
        VfsErrorKind::Busy => Errno::Busy,
        VfsErrorKind::ReadOnlyFs => Errno::Rofs,
        VfsErrorKind::WouldBlock => Errno::Again,
        VfsErrorKind::Interrupted => Errno::Intr,
        VfsErrorKind::TimedOut => Errno::Timedout,
        VfsErrorKind::Cancelled => Errno::Canceled,
        VfsErrorKind::BrokenPipe => Errno::Pipe,
        VfsErrorKind::NoSpace => Errno::Nospc,
        VfsErrorKind::QuotaExceeded => Errno::Dquot,
        VfsErrorKind::TooManyOpenFiles => Errno::Mfile,
        VfsErrorKind::TooManyOpenFilesSystem => Errno::Nfile,
        VfsErrorKind::TooManyLinks => Errno::Mlink,
        VfsErrorKind::NameTooLong => Errno::Nametoolong,
        VfsErrorKind::FileTooLarge => Errno::Fbig,
        VfsErrorKind::NoMemory => Errno::Nomem,
        VfsErrorKind::Overflow => Errno::Overflow,
        VfsErrorKind::Range => Errno::Range,
        VfsErrorKind::NotSeekable => Errno::Spipe,
        VfsErrorKind::NotImplemented => Errno::Nosys,
        VfsErrorKind::Stale => Errno::Stale,
        VfsErrorKind::Deadlock => Errno::Deadlk,
        VfsErrorKind::TextFileBusy => Errno::Txtbsy,
        VfsErrorKind::NoDevice => Errno::Nodev,
        VfsErrorKind::NoAddress => Errno::Nxio,
        VfsErrorKind::BadHandle => Errno::Badf,
        VfsErrorKind::Internal => Errno::Io,
        VfsErrorKind::Io => Errno::Io,
        _ => Errno::Io,
    }
}

/// Stable string name for a VFS error kind (logging/telemetry only).
pub fn vfs_error_kind_str(kind: VfsErrorKind) -> &'static str {
    match kind {
        VfsErrorKind::NotFound => "not_found",
        VfsErrorKind::NotDir => "not_dir",
        VfsErrorKind::IsDir => "is_dir",
        VfsErrorKind::AlreadyExists => "already_exists",
        VfsErrorKind::DirNotEmpty => "dir_not_empty",
        VfsErrorKind::PermissionDenied => "permission_denied",
        VfsErrorKind::OperationNotPermitted => "operation_not_permitted",
        VfsErrorKind::InvalidInput => "invalid_input",
        VfsErrorKind::TooManySymlinks => "too_many_symlinks",
        VfsErrorKind::NotSupported => "not_supported",
        VfsErrorKind::CrossDevice => "cross_device",
        VfsErrorKind::Busy => "busy",
        VfsErrorKind::ReadOnlyFs => "read_only_fs",
        VfsErrorKind::WouldBlock => "would_block",
        VfsErrorKind::Interrupted => "interrupted",
        VfsErrorKind::TimedOut => "timed_out",
        VfsErrorKind::Cancelled => "cancelled",
        VfsErrorKind::BrokenPipe => "broken_pipe",
        VfsErrorKind::NoSpace => "no_space",
        VfsErrorKind::QuotaExceeded => "quota_exceeded",
        VfsErrorKind::TooManyOpenFiles => "too_many_open_files",
        VfsErrorKind::TooManyOpenFilesSystem => "too_many_open_files_system",
        VfsErrorKind::TooManyLinks => "too_many_links",
        VfsErrorKind::NameTooLong => "name_too_long",
        VfsErrorKind::FileTooLarge => "file_too_large",
        VfsErrorKind::NoMemory => "no_memory",
        VfsErrorKind::Overflow => "overflow",
        VfsErrorKind::Range => "range",
        VfsErrorKind::NotSeekable => "not_seekable",
        VfsErrorKind::NotImplemented => "not_implemented",
        VfsErrorKind::Stale => "stale",
        VfsErrorKind::Deadlock => "deadlock",
        VfsErrorKind::TextFileBusy => "text_file_busy",
        VfsErrorKind::NoDevice => "no_device",
        VfsErrorKind::NoAddress => "no_address",
        VfsErrorKind::BadHandle => "bad_handle",
        VfsErrorKind::Internal => "internal",
        VfsErrorKind::Io => "io",
        _ => "unknown",
    }
}
