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
pub mod io_error;
#[cfg(feature = "wasi")]
pub mod open_flags;

#[cfg(feature = "wasi")]
pub use dirents::{
    EncodeDirentsResult, WASI_DIRENT_ALIGN, WASI_DIRENT_HEADER_SIZE, encode_wasi_dirents,
};
#[cfg(feature = "wasi")]
pub use errno::{vfs_error_kind_str, vfs_error_kind_to_wasi_errno, vfs_error_to_wasi_errno};
#[cfg(feature = "wasi")]
pub use filetype::vfs_filetype_to_wasi;
pub use io_error::io_error_to_vfs_error_kind;
#[cfg(feature = "wasi")]
pub use open_flags::wasi_open_to_vfs_options;

#[cfg(test)]
mod tests;
