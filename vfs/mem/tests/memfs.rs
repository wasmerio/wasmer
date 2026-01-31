use std::sync::Arc;

use vfs_core::flags::{OpenFlags, OpenOptions, ResolveFlags};
use vfs_core::node::{CreateFile, MkdirOptions, RenameOptions, UnlinkOptions};
use vfs_core::path_types::{VfsName, VfsPath};
use vfs_core::provider::{MountFlags, MountRequest};
use vfs_core::provider_registry::FsProviderRegistry;
use vfs_core::traits_sync::FsSync;
use vfs_core::{VfsErrorKind, VfsResult};
use vfs_mem::{MemFsConfig, MemFsProvider};

fn name(bytes: &[u8]) -> VfsName<'_> {
    VfsName::new(bytes).expect("name")
}

fn open_rw() -> OpenOptions {
    OpenOptions {
        flags: OpenFlags::READ | OpenFlags::WRITE,
        mode: None,
        resolve: ResolveFlags::empty(),
    }
}

fn assert_err_kind<T>(result: VfsResult<T>, kind: VfsErrorKind) {
    match result {
        Ok(_) => panic!("expected error"),
        Err(err) => assert_eq!(err.kind(), kind),
    }
}

#[test]
fn provider_registry_mounts_memfs() {
    let registry = FsProviderRegistry::new();
    registry
        .register_provider("mem", Arc::new(MemFsProvider))
        .expect("register");

    let config = MemFsConfig::default();
    let req = MountRequest {
        target_path: VfsPath::new(b"/"),
        flags: MountFlags::empty(),
        config: &config,
    };
    let fs = registry.create_fs_sync("mem", req).expect("mount");
    assert_eq!(fs.provider_name(), "mem");
}

#[test]
fn file_crud_roundtrip() {
    let fs = vfs_mem::MemFs::new();
    let root = fs.root();
    let file = root
        .create_file(&name(b"file"), CreateFile::default())
        .expect("create");
    let handle = file.open(open_rw()).expect("open");

    handle.write_at(0, b"hello").expect("write");
    let mut buf = [0u8; 5];
    let read = handle.read_at(0, &mut buf).expect("read");
    assert_eq!(read, 5);
    assert_eq!(&buf, b"hello");

    handle.set_len(2).expect("set_len");
    assert_eq!(handle.len().expect("len"), 2);
}

#[test]
fn directory_semantics_and_readdir_cursor() {
    let fs = vfs_mem::MemFs::new();
    let root = fs.root();
    root.mkdir(&name(b"dir"), MkdirOptions::default())
        .expect("mkdir");

    let dir = root.lookup(&name(b"dir")).expect("lookup");
    assert_err_kind(dir.rmdir(&name(b"missing")), VfsErrorKind::NotFound);

    root.create_file(&name(b"b"), CreateFile::default())
        .expect("create");
    root.create_file(&name(b"a"), CreateFile::default())
        .expect("create");
    root.create_file(&name(b"c"), CreateFile::default())
        .expect("create");

    let batch = root.read_dir(None, 2).expect("read_dir");
    assert_eq!(batch.entries.len(), 2);
    assert_eq!(batch.entries[0].name.as_bytes(), b"a");
    assert_eq!(batch.entries[1].name.as_bytes(), b"b");
    let next = batch.next.expect("cursor");

    let batch = root.read_dir(Some(next), 2).expect("read_dir");
    assert_eq!(batch.entries.len(), 1);
    assert_eq!(batch.entries[0].name.as_bytes(), b"c");
    assert!(batch.next.is_none());
}

#[test]
fn rename_and_exchange_behavior() {
    let fs = vfs_mem::MemFs::new();
    let root = fs.root();
    root.create_file(&name(b"a"), CreateFile::default())
        .expect("create");
    root.create_file(&name(b"b"), CreateFile::default())
        .expect("create");

    assert_err_kind(
        root.rename(
            &name(b"a"),
            root.as_ref(),
            &name(b"b"),
            RenameOptions {
                noreplace: true,
                exchange: false,
            },
        ),
        VfsErrorKind::AlreadyExists,
    );

    assert_err_kind(
        root.rename(
            &name(b"a"),
            root.as_ref(),
            &name(b"b"),
            RenameOptions {
                noreplace: false,
                exchange: true,
            },
        ),
        VfsErrorKind::NotSupported,
    );

    root.rename(
        &name(b"a"),
        root.as_ref(),
        &name(b"c"),
        RenameOptions::default(),
    )
    .expect("rename");
    assert!(root.lookup(&name(b"c")).is_ok());
}

#[test]
fn symlink_roundtrip() {
    let fs = vfs_mem::MemFs::new();
    let root = fs.root();
    root.symlink(&name(b"link"), VfsPath::new(b"/target"))
        .expect("symlink");
    let link = root.lookup(&name(b"link")).expect("lookup");
    let target = link.readlink().expect("readlink");
    assert_eq!(target.as_bytes(), b"/target");
}

#[test]
fn hardlink_and_unlink_lifetime() {
    let fs = vfs_mem::MemFs::new();
    let root = fs.root();
    let file = root
        .create_file(&name(b"file"), CreateFile::default())
        .expect("create");
    let handle = file.open(open_rw()).expect("open");
    handle.write_at(0, b"data").expect("write");

    root.link(file.as_ref(), &name(b"file2")).expect("link");
    let meta = file.metadata().expect("meta");
    assert_eq!(meta.nlink, 2);

    root.unlink(&name(b"file"), UnlinkOptions { must_be_dir: false })
        .expect("unlink");
    let file2 = root.lookup(&name(b"file2")).expect("lookup");
    let meta = file2.metadata().expect("meta");
    assert_eq!(meta.nlink, 1);
    assert_err_kind(root.lookup(&name(b"file")), VfsErrorKind::NotFound);

    root.unlink(&name(b"file2"), UnlinkOptions { must_be_dir: false })
        .expect("unlink");

    let mut buf = [0u8; 4];
    let read = handle.read_at(0, &mut buf).expect("read");
    assert_eq!(read, 4);
    assert_eq!(&buf, b"data");
}
