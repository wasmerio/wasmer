//! Unix-ish translation helpers for the Wasmer VFS.
//!
//! This crate centralizes POSIX/WASI translation logic so it is not duplicated
//! across Wasix syscalls or host filesystem adapters.

#[cfg(feature = "wasi")]
pub mod dirents;
#[cfg(feature = "wasi")]
pub mod errno;
#[cfg(feature = "wasi")]
pub mod filetype;
#[cfg(feature = "wasi")]
pub mod open_flags;
pub mod io_error;

#[cfg(feature = "wasi")]
pub use dirents::{encode_wasi_dirents, EncodeDirentsResult, WASI_DIRENT_ALIGN, WASI_DIRENT_HEADER_SIZE};
#[cfg(feature = "wasi")]
pub use errno::{vfs_error_kind_str, vfs_error_kind_to_wasi_errno, vfs_error_to_wasi_errno};
#[cfg(feature = "wasi")]
pub use filetype::vfs_filetype_to_wasi;
#[cfg(feature = "wasi")]
pub use open_flags::wasi_open_to_vfs_options;
pub use io_error::io_error_to_vfs_error_kind;

#[cfg(test)]
mod tests;
