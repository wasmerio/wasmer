mod config;
mod fs;
mod handle;
mod node;
mod platform;
mod provider;

pub use config::HostFsConfig;
pub use provider::HostFsProvider;

use vfs_core::{VfsError, VfsErrorKind, VfsResult};

pub(crate) fn map_io_error(context: &'static str, err: std::io::Error) -> VfsError {
    let kind = io_error_kind(&err);
    VfsError::with_source(kind, context, err)
}

pub(crate) fn io_result<T>(context: &'static str, result: std::io::Result<T>) -> VfsResult<T> {
    result.map_err(|err| map_io_error(context, err))
}

pub(crate) fn readonly_error(context: &'static str) -> VfsError {
    VfsError::new(VfsErrorKind::ReadOnlyFs, context)
}

#[cfg(unix)]
fn io_error_kind(err: &std::io::Error) -> VfsErrorKind {
    vfs_unix::io_error_to_vfs_error_kind(err)
}

#[cfg(not(unix))]
fn io_error_kind(err: &std::io::Error) -> VfsErrorKind {
    match err.kind() {
        std::io::ErrorKind::NotFound => VfsErrorKind::NotFound,
        std::io::ErrorKind::PermissionDenied => VfsErrorKind::PermissionDenied,
        std::io::ErrorKind::AlreadyExists => VfsErrorKind::AlreadyExists,
        std::io::ErrorKind::InvalidInput => VfsErrorKind::InvalidInput,
        std::io::ErrorKind::Unsupported => VfsErrorKind::NotSupported,
        _ => VfsErrorKind::Io,
    }
}
