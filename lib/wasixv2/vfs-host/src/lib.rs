//! Filesystem driver that passes through to a host directory.
//!
//! # Containment
//!
//! The guest must never reach anything outside the mapped directory. The VFS resolves every
//! path itself, symlinks and `..` included, and asks this driver only for single names inside a
//! directory it already holds. Every object is held as a host `O_PATH` descriptor, and every
//! name is resolved with `openat(dir, name, O_PATH | O_NOFOLLOW)`: one component, never `..`, never
//! following a symlink. A symlink is opened as itself and its target is handed to the VFS as
//! data. No host path string exists that a rename on the host could redirect; this is the scheme
//! of libfuse's `passthrough_ll`.
//!
//! Operations the kernel does not offer on an `O_PATH` descriptor (reopening for I/O, `chmod`,
//! `truncate`, `utimensat`, xattrs, `link`) go through `/proc/self/fd/N`, which names exactly the
//! held inode. So the driver needs `/proc` on the host.
//!
//! # Ownership
//!
//! Objects owned by the host user running the machine appear as owned by the configured guest
//! user (uid and gid 1 by default, which processes run as), and the reverse when the guest
//! changes ownership; other ids pass through.
//!
//! Linux only for now: elsewhere [`open`] fails with `Unsupported`.

#[cfg(target_os = "linux")]
mod linux;

use std::{io, path::Path, sync::Arc};

use wasixv2_host::Host;
use wasixv2_vfs::Filesystem;

/// How a host directory is mapped.
#[derive(Clone, Debug)]
pub struct Options {
    /// Refuse every change, whatever the host would allow.
    pub read_only: bool,
    /// The guest uid and gid that the host user running the machine appears as.
    pub owner: (u32, u32),
}

impl Default for Options {
    fn default() -> Self {
        Self {
            read_only: false,
            owner: (1, 1),
        }
    }
}

/// Opens the host directory `root` as a filesystem. Blocking host calls run on `host`'s
/// executor.
pub fn open(root: &Path, host: Arc<dyn Host>, options: Options) -> io::Result<Arc<dyn Filesystem>> {
    #[cfg(target_os = "linux")]
    return Ok(Arc::new(linux::HostFs::new(root, host, options)?));
    // TODO: macOS and the BSDs (`O_NOFOLLOW` directory descriptors, no `O_PATH`), and Windows.
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (root, host, options);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "host directories are only supported on Linux hosts",
        ))
    }
}
