use crate::wasi_open_to_vfs_options;
use vfs_core::{OpenFlags, OpenOptions, ResolveFlags};
use wasmer_wasix_types::types::file::__WASI_LOOKUP_SYMLINK_FOLLOW;
use wasmer_wasix_types::wasi::{Fdflags, Oflags};

#[test]
fn translates_basic_open_flags() {
    let opts = wasi_open_to_vfs_options(
        Oflags::CREATE | Oflags::EXCL | Oflags::TRUNC | Oflags::DIRECTORY,
        Fdflags::APPEND | Fdflags::NONBLOCK | Fdflags::DSYNC,
        Some(__WASI_LOOKUP_SYMLINK_FOLLOW),
    );

    let expected = OpenOptions {
        flags: OpenFlags::CREATE
            | OpenFlags::EXCL
            | OpenFlags::TRUNC
            | OpenFlags::DIRECTORY
            | OpenFlags::APPEND
            | OpenFlags::NONBLOCK
            | OpenFlags::DSYNC,
        mode: None,
        resolve: ResolveFlags::empty(),
    };

    assert_eq!(opts, expected);
}

#[test]
fn translates_lookup_flags_to_nofollow() {
    let opts = wasi_open_to_vfs_options(Oflags::empty(), Fdflags::empty(), Some(0));
    assert!(opts.resolve.contains(ResolveFlags::NO_SYMLINK_FOLLOW));

    let opts = wasi_open_to_vfs_options(
        Oflags::empty(),
        Fdflags::empty(),
        Some(__WASI_LOOKUP_SYMLINK_FOLLOW),
    );
    assert!(!opts.resolve.contains(ResolveFlags::NO_SYMLINK_FOLLOW));
}

#[test]
fn maps_rsync_to_sync() {
    let opts = wasi_open_to_vfs_options(Oflags::empty(), Fdflags::RSYNC, None);
    assert!(opts.flags.contains(OpenFlags::SYNC));
}
