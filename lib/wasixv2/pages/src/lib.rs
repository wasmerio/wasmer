//! Shared pages: host memory that the linear memories of several processes map at once, as
//! Linux's shmem objects back `MAP_SHARED|MAP_ANONYMOUS`, SysV segments and tmpfs files
//! (`mm/shmem.c`).
//!
//! Wasm cannot share part of one module's memory with another module, but the host can: every
//! linear memory is one host mapping reserved to its maximum that never moves, so a range of it
//! can be replaced with a `MAP_SHARED` mapping of a host object. Each process that maps the
//! object then reads and writes the very same pages, with no copy and no synchronisation point,
//! and a futex keyed by the object and offset works across them.
//!
//! [`SharedPages`] is the object as the kernel sees it. [`Memfd`] is the host object where one
//! exists: a `memfd_create` file on Linux, an unlinked `shm_open` object on macOS. Elsewhere
//! there is none, and the kernel keeps its copies synchronised at defined points instead.
//! [`map_at`] and [`map_private_at`] are what the execution layer does with an object's
//! descriptor inside a linear memory.
//!
//! A separate crate because the VFS, the syscall interface, the kernel, the host and the
//! execution layer all need it, and none of them may depend on another for it.

use std::{
    io,
    sync::atomic::{AtomicU64, Ordering},
};

#[cfg(unix)]
use std::os::fd::BorrowedFd;

#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
mod memfd;
#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
pub use memfd::Memfd;
#[cfg(unix)]
mod map;
#[cfg(unix)]
pub use map::{map_at, map_private_at};

/// A shared memory object: bytes several processes map, which stay as long as anyone holds the
/// object.
///
/// Nothing may map the object past its [`size`](SharedPages::size): a host access to a page
/// wholly past the end of a file mapping raises `SIGBUS` in the host. Whoever maps it therefore
/// keeps the size at or above the end of every mapping.
pub trait SharedPages: Send + Sync {
    /// Names the object in futex keys; never reused by another object of this host process.
    fn id(&self) -> u64;

    /// The object's size in bytes.
    fn size(&self) -> u64;

    /// Sets the object's size. Pages past a smaller size are freed; pages a larger size adds
    /// read zero.
    fn resize(&self, size: u64) -> io::Result<()>;

    /// Fills `buf` from `offset`; bytes past the end read zero.
    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<()>;

    /// Writes all of `data` at `offset`, which with the data must lie inside the object.
    fn write_at(&self, data: &[u8], offset: u64) -> io::Result<()>;

    /// Makes `len` bytes from `offset` read zero again, freeing their pages where the host can
    /// (`FALLOC_FL_PUNCH_HOLE`). The size does not change.
    fn punch_hole(&self, offset: u64, len: u64) -> io::Result<()>;

    /// Bytes of host memory the object holds now (`shm_rss`).
    fn allocated(&self) -> u64;

    /// The descriptor that maps the object, if the host can map it into linear memory.
    #[cfg(unix)]
    fn fd(&self) -> Option<BorrowedFd<'_>> {
        None
    }
}

/// A fresh [`SharedPages::id`].
pub fn new_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}
