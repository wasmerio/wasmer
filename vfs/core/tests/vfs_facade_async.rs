use std::sync::Arc;

use vfs_core::inode::make_vfs_inode;
use vfs_core::mount::MountTable;
use vfs_core::provider::{AsyncFsFromSync, VfsRuntime, VfsRuntimeExt};
use vfs_core::{
    AllowAllPolicy, FsAsync, FsSync, MountId, OpenFlags, OpenOptions, ResolveFlags, StatOptions,
    Vfs, VfsBaseDirAsync, VfsConfig, VfsContext, VfsCred, VfsDirHandle, VfsDirHandleAsync,
    VfsHandleId, VfsPath, VfsResult,
};
use vfs_mem::MemFs;
use vfs_rt::InlineTestRuntime;

struct AsyncFixture {
    vfs: Vfs,
    ctx: VfsContext,
    runtime: Arc<dyn VfsRuntime>,
}

fn setup_async() -> AsyncFixture {
    let runtime: Arc<dyn VfsRuntime> = Arc::new(InlineTestRuntime);
    let fs_sync: Arc<dyn FsSync> = Arc::new(MemFs::new());
    let fs_async: Arc<dyn FsAsync> =
        Arc::new(AsyncFsFromSync::new(fs_sync.clone(), runtime.clone()));
    let mount_table = Arc::new(MountTable::new(fs_sync.clone(), fs_async.clone()).expect("mount"));
    let vfs = Vfs::new(mount_table.clone());
    let guard = mount_table
        .guard(MountId::from_index(0))
        .expect("mount guard");
    let root = fs_sync.root();
    let cwd = VfsDirHandle::new(
        VfsHandleId(1),
        guard.clone(),
        make_vfs_inode(MountId::from_index(0), root.inode()),
        root,
        None,
    );
    let async_root = runtime.block_on(fs_async.root()).expect("async root");
    let cwd_async = VfsDirHandleAsync::new(
        VfsHandleId(2),
        guard,
        make_vfs_inode(MountId::from_index(0), async_root.inode()),
        async_root,
        None,
    );
    let ctx = VfsContext::new(
        VfsCred::root(),
        cwd,
        Arc::new(VfsConfig::default()),
        Arc::new(AllowAllPolicy),
    )
    .with_async_cwd(cwd_async);
    AsyncFixture { vfs, ctx, runtime }
}

fn open_create() -> OpenOptions {
    OpenOptions {
        flags: OpenFlags::READ | OpenFlags::WRITE | OpenFlags::CREATE,
        mode: None,
        resolve: ResolveFlags::empty(),
    }
}

#[test]
fn openat_async_read_write() -> VfsResult<()> {
    let fixture = setup_async();
    fixture.runtime.block_on(async {
        let handle = fixture
            .vfs
            .openat_async(
                &fixture.ctx,
                VfsBaseDirAsync::Cwd,
                VfsPath::new(b"file"),
                open_create(),
            )
            .await?;
        handle.write(b"hi").await?;
        handle.seek(std::io::SeekFrom::Start(0)).await?;
        let mut buf = [0u8; 2];
        let read = handle.read(&mut buf).await?;
        assert_eq!(read, 2);
        assert_eq!(&buf, b"hi");
        Ok::<(), vfs_core::VfsError>(())
    })?;
    Ok(())
}

#[test]
fn statat_async_basic() -> VfsResult<()> {
    let fixture = setup_async();
    fixture.runtime.block_on(async {
        fixture
            .vfs
            .mkdirat_async(
                &fixture.ctx,
                VfsBaseDirAsync::Cwd,
                VfsPath::new(b"dir"),
                vfs_core::MkdirOptions {
                    mode: None,
                    resolve: ResolveFlags::empty(),
                },
            )
            .await?;
        let _handle = fixture
            .vfs
            .openat_async(
                &fixture.ctx,
                VfsBaseDirAsync::Cwd,
                VfsPath::new(b"dir/file"),
                open_create(),
            )
            .await?;
        let meta = fixture
            .vfs
            .statat_async(
                &fixture.ctx,
                VfsBaseDirAsync::Cwd,
                VfsPath::new(b"dir/file"),
                StatOptions {
                    resolve: ResolveFlags::empty(),
                    follow: true,
                    require_dir_if_trailing_slash: false,
                },
            )
            .await?;
        assert_eq!(meta.file_type, vfs_core::VfsFileType::RegularFile);
        Ok::<(), vfs_core::VfsError>(())
    })?;
    Ok(())
}
