//! A cross-platform Rust API for memory mapped buffers.
// TODO: Refactor this into it's own lib
mod common;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::Mmap;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use self::unix::Mmap;
