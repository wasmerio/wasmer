use std::sync::Arc;

use vfs_core::inode::make_vfs_inode;
use vfs_core::mount::MountTable;
use vfs_core::provider::{AsyncFsFromSync, VfsRuntime};
use vfs_core::{
    AllowAllPolicy, FsAsync, FsSync, MountId, OpenFlags, OpenOptions, ReadDirOptions, ResolveFlags,
    StatOptions, Vfs, VfsBaseDir, VfsConfig, VfsContext, VfsCred, VfsDirHandle, VfsErrorKind,
    VfsHandleId, VfsPath, VfsResult,
};
use vfs_mem::MemFs;
use vfs_rt::InlineTestRuntime;

fn setup_vfs() -> (Vfs, VfsContext, Arc<MountTable>) {
    let runtime: Arc<dyn VfsRuntime> = Arc::new(InlineTestRuntime);
    let fs_sync: Arc<dyn FsSync> = Arc::new(MemFs::new());
    let fs_async: Arc<dyn FsAsync> = Arc::new(AsyncFsFromSync::new(fs_sync.clone(), runtime));
    let mount_table = Arc::new(MountTable::new(fs_sync.clone(), fs_async).expect("mount table"));
    let vfs = Vfs::new(mount_table.clone());
    let guard = mount_table
        .guard(MountId::from_index(0))
        .expect("mount guard");
    let root = fs_sync.root();
    let cwd = VfsDirHandle::new(
        VfsHandleId(1),
        guard,
        make_vfs_inode(MountId::from_index(0), root.inode()),
        root,
        None,
    );
    let ctx = VfsContext::new(
        VfsCred::root(),
        cwd,
        Arc::new(VfsConfig::default()),
        Arc::new(AllowAllPolicy),
    );
    (vfs, ctx, mount_table)
}

fn open_create() -> OpenOptions {
    OpenOptions {
        flags: OpenFlags::READ | OpenFlags::WRITE | OpenFlags::CREATE,
        mode: None,
        resolve: ResolveFlags::empty(),
    }
}

#[test]
fn statat_basic() -> VfsResult<()> {
    let (vfs, ctx, _) = setup_vfs();
    vfs.mkdirat(
        &ctx,
        VfsBaseDir::Cwd,
        VfsPath::new(b"dir"),
        vfs_core::MkdirOptions {
            mode: None,
            resolve: ResolveFlags::empty(),
        },
    )?;
    let _handle = vfs.openat(
        &ctx,
        VfsBaseDir::Cwd,
        VfsPath::new(b"dir/file"),
        open_create(),
    )?;
    let meta = vfs.statat(
        &ctx,
        VfsBaseDir::Cwd,
        VfsPath::new(b"dir/file"),
        StatOptions {
            resolve: ResolveFlags::empty(),
            follow: true,
            require_dir_if_trailing_slash: false,
        },
    )?;
    assert_eq!(meta.file_type, vfs_core::VfsFileType::RegularFile);
    Ok(())
}

#[test]
fn openat_ofd_read_write() -> VfsResult<()> {
    let (vfs, ctx, _) = setup_vfs();
    let handle = vfs.openat(&ctx, VfsBaseDir::Cwd, VfsPath::new(b"file"), open_create())?;
    handle.write(b"hello")?;
    handle.seek(std::io::SeekFrom::Start(0))?;
    let mut buf = [0u8; 5];
    let read = handle.read(&mut buf)?;
    assert_eq!(read, 5);
    assert_eq!(&buf, b"hello");
    Ok(())
}

#[test]
fn mkdirat_and_unlinkat() -> VfsResult<()> {
    let (vfs, ctx, _) = setup_vfs();
    vfs.mkdirat(
        &ctx,
        VfsBaseDir::Cwd,
        VfsPath::new(b"a"),
        vfs_core::MkdirOptions {
            mode: None,
            resolve: ResolveFlags::empty(),
        },
    )?;
    let err = vfs
        .unlinkat(
            &ctx,
            VfsBaseDir::Cwd,
            VfsPath::new(b"a"),
            vfs_core::UnlinkOptions {
                resolve: ResolveFlags::empty(),
            },
        )
        .unwrap_err();
    assert!(matches!(
        err.kind(),
        VfsErrorKind::IsDir | VfsErrorKind::NotSupported
    ));

    let _handle = vfs.openat(
        &ctx,
        VfsBaseDir::Cwd,
        VfsPath::new(b"a/file"),
        open_create(),
    )?;
    vfs.unlinkat(
        &ctx,
        VfsBaseDir::Cwd,
        VfsPath::new(b"a/file"),
        vfs_core::UnlinkOptions {
            resolve: ResolveFlags::empty(),
        },
    )?;
    Ok(())
}

#[test]
fn renameat_moves_entry() -> VfsResult<()> {
    let (vfs, ctx, _) = setup_vfs();
    for dir in [b"a", b"b"] {
        vfs.mkdirat(
            &ctx,
            VfsBaseDir::Cwd,
            VfsPath::new(dir),
            vfs_core::MkdirOptions {
                mode: None,
                resolve: ResolveFlags::empty(),
            },
        )?;
    }
    let _handle = vfs.openat(
        &ctx,
        VfsBaseDir::Cwd,
        VfsPath::new(b"a/file"),
        open_create(),
    )?;
    vfs.renameat(
        &ctx,
        VfsBaseDir::Cwd,
        VfsPath::new(b"a/file"),
        VfsBaseDir::Cwd,
        VfsPath::new(b"b/file2"),
        vfs_core::RenameOptions {
            flags: vfs_core::RenameFlags::empty(),
            resolve: ResolveFlags::empty(),
        },
    )?;
    let meta = vfs.statat(
        &ctx,
        VfsBaseDir::Cwd,
        VfsPath::new(b"b/file2"),
        StatOptions {
            resolve: ResolveFlags::empty(),
            follow: true,
            require_dir_if_trailing_slash: false,
        },
    )?;
    assert_eq!(meta.file_type, vfs_core::VfsFileType::RegularFile);
    let err = vfs
        .statat(
            &ctx,
            VfsBaseDir::Cwd,
            VfsPath::new(b"a/file"),
            StatOptions {
                resolve: ResolveFlags::empty(),
                follow: true,
                require_dir_if_trailing_slash: false,
            },
        )
        .unwrap_err();
    assert_eq!(err.kind(), VfsErrorKind::NotFound);
    Ok(())
}

#[test]
fn readlink_and_symlink() -> VfsResult<()> {
    let (vfs, ctx, _) = setup_vfs();
    let _handle = vfs.openat(
        &ctx,
        VfsBaseDir::Cwd,
        VfsPath::new(b"target"),
        open_create(),
    )?;
    vfs.symlinkat(
        &ctx,
        VfsBaseDir::Cwd,
        VfsPath::new(b"link"),
        VfsPath::new(b"target"),
        vfs_core::SymlinkOptions {
            resolve: ResolveFlags::empty(),
        },
    )?;
    let target = vfs.readlinkat(
        &ctx,
        VfsBaseDir::Cwd,
        VfsPath::new(b"link"),
        vfs_core::ReadlinkOptions {
            resolve: ResolveFlags::empty(),
        },
    )?;
    assert_eq!(target.as_bytes(), b"target");
    Ok(())
}

#[test]
fn opendirat_yields_dir_handle() -> VfsResult<()> {
    let (vfs, ctx, _) = setup_vfs();
    vfs.mkdirat(
        &ctx,
        VfsBaseDir::Cwd,
        VfsPath::new(b"dir"),
        vfs_core::MkdirOptions {
            mode: None,
            resolve: ResolveFlags::empty(),
        },
    )?;
    let _handle = vfs.openat(
        &ctx,
        VfsBaseDir::Cwd,
        VfsPath::new(b"dir/file1"),
        open_create(),
    )?;
    let dir_handle = vfs.opendirat(
        &ctx,
        VfsBaseDir::Cwd,
        VfsPath::new(b"dir"),
        OpenOptions {
            flags: OpenFlags::DIRECTORY | OpenFlags::READ,
            mode: None,
            resolve: ResolveFlags::empty(),
        },
    )?;
    let batch = dir_handle.node().read_dir(None, 10)?;
    let names: Vec<Vec<u8>> = batch
        .entries
        .iter()
        .map(|e| e.name.as_bytes().to_vec())
        .collect();
    assert!(names.iter().any(|name| name.as_slice() == b"file1"));
    Ok(())
}

#[test]
fn dir_streams_are_usable() -> VfsResult<()> {
    let (vfs, ctx, _) = setup_vfs();
    vfs.mkdirat(
        &ctx,
        VfsBaseDir::Cwd,
        VfsPath::new(b"dir"),
        vfs_core::MkdirOptions {
            mode: None,
            resolve: ResolveFlags::empty(),
        },
    )?;
    for file in [b"a", b"b"] {
        let mut path = Vec::from(b"dir/".as_slice());
        path.extend_from_slice(file);
        let _handle = vfs.openat(&ctx, VfsBaseDir::Cwd, VfsPath::new(&path), open_create())?;
    }
    let dir_handle = vfs.opendirat(
        &ctx,
        VfsBaseDir::Cwd,
        VfsPath::new(b"dir"),
        OpenOptions {
            flags: OpenFlags::DIRECTORY | OpenFlags::READ,
            mode: None,
            resolve: ResolveFlags::empty(),
        },
    )?;
    let stream = vfs.readdir(&ctx, &dir_handle, ReadDirOptions)?;
    let stream_id = stream.clone();
    let first = vfs.readdir_next(&ctx, &stream, 1)?;
    assert_eq!(first.entries.len(), 1);
    let second = vfs.readdir_next(&ctx, &stream, 1)?;
    assert_eq!(second.entries.len(), 1);
    vfs.readdir_close(stream)?;
    let err = vfs.readdir_next(&ctx, &stream_id, 1).unwrap_err();
    assert_eq!(err.kind(), VfsErrorKind::BadHandle);
    Ok(())
}
