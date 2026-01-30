//! WASI open flags → VFS `OpenOptions` translation.
//!
//! This module is the only place that should translate WASI open-like flags
//! into `vfs-core` flags. Callers must not duplicate this logic.

use vfs_core::{OpenFlags, OpenOptions, ResolveFlags};
use wasmer_wasix_types::types::file::__WASI_LOOKUP_SYMLINK_FOLLOW;
use wasmer_wasix_types::wasi::{Fdflags, LookupFlags, Oflags};

/// Convert WASI/WASIX open-like inputs into VFS `OpenOptions`.
///
/// Note: rights-based read/write decisions are applied by the caller. This
/// function only translates flag semantics (create, trunc, append, etc.).
pub fn wasi_open_to_vfs_options(
    oflags: Oflags,
    fdflags: Fdflags,
    lookupflags: Option<LookupFlags>,
) -> OpenOptions {
    let mut flags = OpenFlags::empty();

    if oflags.contains(Oflags::CREATE) {
        flags |= OpenFlags::CREATE;
    }
    if oflags.contains(Oflags::EXCL) {
        flags |= OpenFlags::EXCL;
    }
    if oflags.contains(Oflags::TRUNC) {
        flags |= OpenFlags::TRUNC;
    }
    if oflags.contains(Oflags::DIRECTORY) {
        flags |= OpenFlags::DIRECTORY;
    }

    if fdflags.contains(Fdflags::APPEND) {
        flags |= OpenFlags::APPEND;
    }
    if fdflags.contains(Fdflags::NONBLOCK) {
        flags |= OpenFlags::NONBLOCK;
    }
    if fdflags.contains(Fdflags::SYNC) || fdflags.contains(Fdflags::RSYNC) {
        flags |= OpenFlags::SYNC;
    }
    if fdflags.contains(Fdflags::DSYNC) {
        flags |= OpenFlags::DSYNC;
    }

    let follow_symlinks = lookupflags
        .map(|flags| (flags & __WASI_LOOKUP_SYMLINK_FOLLOW) != 0)
        .unwrap_or(true);

    let mut resolve = ResolveFlags::empty();
    if !follow_symlinks {
        resolve |= ResolveFlags::NO_SYMLINK_FOLLOW;
    }

    OpenOptions {
        flags,
        mode: None,
        resolve,
    }
}
