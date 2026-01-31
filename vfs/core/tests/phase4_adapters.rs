use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::join;
use vfs_core::flags::{OpenFlags, OpenOptions, ResolveFlags};
use vfs_core::inode::make_vfs_inode;
use vfs_core::node::{
    CreateFile, FsHandleAsync, FsNodeAsync, MkdirOptions, ReadDirBatch, RenameOptions,
    SetMetadata, UnlinkOptions, VfsDirCookie,
};
use vfs_core::path_types::{VfsName, VfsPath, VfsPathBuf};
use vfs_core::provider::{AsyncFsFromSync, SyncFsFromAsync, VfsRuntime, VfsRuntimeExt};
use vfs_core::{
    BackendInodeId, FsAsync, FsSync, MountId, VfsBaseDirAsync, VfsCapabilities, VfsConfig,
    VfsContext, VfsCred, VfsDirHandle, VfsDirHandleAsync, VfsError, VfsErrorKind, VfsFileMode,
    VfsFileType, VfsHandleAsync, VfsHandleId, VfsMetadata, VfsResult, VfsTimespec,
};
use vfs_mem::MemFs;
use std::any::Any;
use std::future::Future;
use std::pin::Pin;

struct TestRuntime;

impl VfsRuntime for TestRuntime {
    fn spawn_blocking_boxed(
        &self,
        f: Box<dyn FnOnce() -> Box<dyn Any + Send> + Send>,
    ) -> Pin<Box<dyn Future<Output = Box<dyn Any + Send>> + Send>> {
        Box::pin(async move {
            let handle = std::thread::spawn(f);
            match handle.join() {
                Ok(value) => value,
                Err(err) => std::panic::resume_unwind(err),
            }
        })
    }

    fn block_on_boxed<'a>(
        &'a self,
        fut: Pin<Box<dyn Future<Output = Box<dyn Any + Send>> + Send + 'a>>,
    ) -> Box<dyn Any + Send> {
        futures::executor::block_on(fut)
    }
}

#[test]
fn sync_backend_works_through_async_adapter() {
    let runtime: Arc<dyn VfsRuntime> = Arc::new(TestRuntime);
    let fs_sync: Arc<dyn FsSync> = Arc::new(MemFs::new());
    let fs_async: Arc<dyn FsAsync> = Arc::new(AsyncFsFromSync::new(fs_sync.clone(), runtime.clone()));

    runtime
        .block_on(async {
            let root = fs_async.root().await?;
            let name = VfsName::new(b"file").expect("name");
            let node = root
                .create_file(&name, CreateFile::default())
                .await?;

            let opts = OpenOptions {
                flags: OpenFlags::READ | OpenFlags::WRITE | OpenFlags::CREATE,
                mode: None,
                resolve: ResolveFlags::empty(),
            };
            let handle = node.open(opts).await?;
            let data = b"hello";
            handle.write_at(0, data).await?;
            let mut buf = vec![0u8; data.len()];
            handle.read_at(0, &mut buf).await?;
            assert_eq!(buf, data);

            let link = VfsName::new(b"link").expect("link");
            root.symlink(&link, VfsPath::new(b"file")).await?;
            let link_node = root.lookup(&link).await?;
            let target = link_node.readlink().await?;
            assert_eq!(target.as_bytes(), b"file");

            Ok::<(), VfsError>(())
        })
        .expect("async adapter");
}

#[derive(Debug)]
struct AsyncTestNode {
    inode: BackendInodeId,
    file_type: VfsFileType,
    children: Mutex<HashMap<Vec<u8>, Arc<AsyncTestNode>>>,
}

impl AsyncTestNode {
    fn dir(inode: BackendInodeId) -> Self {
        Self {
            inode,
            file_type: VfsFileType::Directory,
            children: Mutex::new(HashMap::new()),
        }
    }

    fn file(inode: BackendInodeId) -> Self {
        Self {
            inode,
            file_type: VfsFileType::RegularFile,
            children: Mutex::new(HashMap::new()),
        }
    }

    fn unsupported<T>(&self, op: &'static str) -> VfsResult<T> {
        Err(VfsError::new(VfsErrorKind::NotSupported, op))
    }
}

#[async_trait]
impl FsNodeAsync for AsyncTestNode {
    fn inode(&self) -> BackendInodeId {
        self.inode
    }

    fn file_type(&self) -> VfsFileType {
        self.file_type
    }

    async fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(VfsMetadata {
            inode: make_vfs_inode(MountId::from_index(0), self.inode),
            file_type: self.file_type,
            mode: VfsFileMode(0o644),
            uid: 0,
            gid: 0,
            nlink: 1,
            size: 0,
            atime: VfsTimespec { secs: 0, nanos: 0 },
            mtime: VfsTimespec { secs: 0, nanos: 0 },
            ctime: VfsTimespec { secs: 0, nanos: 0 },
            rdev_major: 0,
            rdev_minor: 0,
        })
    }

    async fn set_metadata(&self, _set: SetMetadata) -> VfsResult<()> {
        self.unsupported("async_test.set_metadata")
    }

    async fn lookup(&self, name: &VfsName) -> VfsResult<Arc<dyn FsNodeAsync>> {
        let children = self.children.lock().unwrap();
        children
            .get(name.as_bytes())
            .cloned()
            .map(|node| node as Arc<dyn FsNodeAsync>)
            .ok_or_else(|| VfsError::new(VfsErrorKind::NotFound, "async_test.lookup"))
    }

    async fn create_file(
        &self,
        _name: &VfsName,
        _opts: CreateFile,
    ) -> VfsResult<Arc<dyn FsNodeAsync>> {
        self.unsupported("async_test.create_file")
    }

    async fn mkdir(
        &self,
        _name: &VfsName,
        _opts: MkdirOptions,
    ) -> VfsResult<Arc<dyn FsNodeAsync>> {
        self.unsupported("async_test.mkdir")
    }

    async fn unlink(&self, _name: &VfsName, _opts: UnlinkOptions) -> VfsResult<()> {
        self.unsupported("async_test.unlink")
    }

    async fn rmdir(&self, _name: &VfsName) -> VfsResult<()> {
        self.unsupported("async_test.rmdir")
    }

    async fn read_dir(
        &self,
        _cursor: Option<VfsDirCookie>,
        _max: usize,
    ) -> VfsResult<ReadDirBatch> {
        self.unsupported("async_test.read_dir")
    }

    async fn rename(
        &self,
        _old_name: &VfsName,
        _new_parent: &dyn FsNodeAsync,
        _new_name: &VfsName,
        _opts: RenameOptions,
    ) -> VfsResult<()> {
        self.unsupported("async_test.rename")
    }

    async fn link(&self, _existing: &dyn FsNodeAsync, _new_name: &VfsName) -> VfsResult<()> {
        self.unsupported("async_test.link")
    }

    async fn symlink(&self, _new_name: &VfsName, _target: &VfsPath) -> VfsResult<()> {
        self.unsupported("async_test.symlink")
    }

    async fn readlink(&self) -> VfsResult<VfsPathBuf> {
        self.unsupported("async_test.readlink")
    }

    async fn open(&self, _opts: OpenOptions) -> VfsResult<Arc<dyn FsHandleAsync>> {
        self.unsupported("async_test.open")
    }
}

struct AsyncTestFs {
    root: Arc<AsyncTestNode>,
}

#[async_trait]
impl FsAsync for AsyncTestFs {
    fn provider_name(&self) -> &'static str {
        "async-test"
    }

    fn capabilities(&self) -> VfsCapabilities {
        VfsCapabilities::NONE
    }

    async fn root(&self) -> VfsResult<Arc<dyn FsNodeAsync>> {
        Ok(self.root.clone())
    }
}

#[test]
fn async_backend_works_through_sync_adapter() {
    let runtime: Arc<dyn VfsRuntime> = Arc::new(TestRuntime);
    let root = Arc::new(AsyncTestNode::dir(
        BackendInodeId::new(1).expect("inode"),
    ));
    let child = Arc::new(AsyncTestNode::file(
        BackendInodeId::new(2).expect("inode"),
    ));
    root.children
        .lock()
        .unwrap()
        .insert(b"child".to_vec(), child);
    let fs_async = Arc::new(AsyncTestFs { root });
    let fs_sync: Arc<dyn FsSync> = Arc::new(SyncFsFromAsync::new(fs_async, runtime));

    let root_sync = fs_sync.root();
    let name = VfsName::new(b"child").expect("name");
    let child = root_sync.lookup(&name).expect("lookup child");
    assert_eq!(child.file_type(), VfsFileType::RegularFile);
}

#[derive(Debug)]
struct AsyncMemHandle {
    inode: BackendInodeId,
    file_type: VfsFileType,
    data: Mutex<Vec<u8>>,
}

impl AsyncMemHandle {
    fn new(inode: BackendInodeId, file_type: VfsFileType) -> Self {
        Self {
            inode,
            file_type,
            data: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl FsHandleAsync for AsyncMemHandle {
    async fn read_at(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        let data = self.data.lock().unwrap();
        let start = offset as usize;
        if start >= data.len() {
            return Ok(0);
        }
        let end = usize::min(data.len(), start + buf.len());
        let count = end - start;
        buf[..count].copy_from_slice(&data[start..end]);
        Ok(count)
    }

    async fn write_at(&self, offset: u64, buf: &[u8]) -> VfsResult<usize> {
        let mut data = self.data.lock().unwrap();
        let start = offset as usize;
        if start > data.len() {
            data.resize(start, 0);
        }
        let end = start + buf.len();
        if end > data.len() {
            data.resize(end, 0);
        }
        data[start..end].copy_from_slice(buf);
        Ok(buf.len())
    }

    async fn flush(&self) -> VfsResult<()> {
        Ok(())
    }

    async fn fsync(&self) -> VfsResult<()> {
        Ok(())
    }

    async fn get_metadata(&self) -> VfsResult<VfsMetadata> {
        let size = self.data.lock().unwrap().len() as u64;
        Ok(VfsMetadata {
            inode: make_vfs_inode(MountId::from_index(0), self.inode),
            file_type: self.file_type,
            mode: VfsFileMode(0o666),
            uid: 0,
            gid: 0,
            nlink: 1,
            size,
            atime: VfsTimespec { secs: 0, nanos: 0 },
            mtime: VfsTimespec { secs: 0, nanos: 0 },
            ctime: VfsTimespec { secs: 0, nanos: 0 },
            rdev_major: 0,
            rdev_minor: 0,
        })
    }

    async fn set_len(&self, len: u64) -> VfsResult<()> {
        let mut data = self.data.lock().unwrap();
        data.resize(len as usize, 0);
        Ok(())
    }
}

#[test]
fn async_ofd_shares_offsets() {
    let runtime: Arc<dyn VfsRuntime> = Arc::new(TestRuntime);
    let fs_sync: Arc<dyn FsSync> = Arc::new(MemFs::new());
    let fs_async: Arc<dyn FsAsync> = Arc::new(AsyncFsFromSync::new(fs_sync.clone(), runtime));
    let mount_table =
        vfs_core::mount::MountTable::new(fs_sync, fs_async).expect("mount table");
    let guard = mount_table
        .guard(MountId::from_index(0))
        .expect("mount guard");

    let inode = BackendInodeId::new(10).expect("inode");
    let vfs_inode = make_vfs_inode(MountId::from_index(0), inode);
    let backend = Arc::new(AsyncMemHandle::new(inode, VfsFileType::RegularFile));
    let flags = OpenFlags::READ | OpenFlags::WRITE;
    let handle = Arc::new(VfsHandleAsync::new(
        VfsHandleId(1),
        guard,
        vfs_inode,
        VfsFileType::RegularFile,
        backend,
        flags,
        vfs_ratelimit::LimiterChain::default(),
    ));

    let handle_b = handle.clone();
    let write_a = async {
        handle.write(b"aa").await.expect("write a");
    };
    let write_b = async {
        handle_b.write(b"bb").await.expect("write b");
    };
    futures::executor::block_on(async {
        join!(write_a, write_b);
    });

    let mut buf = [0u8; 4];
    futures::executor::block_on(async {
        handle.pread_at(0, &mut buf).await.expect("read");
    });
    assert_eq!(&buf, b"aabb");
    assert_eq!(handle.tell(), 4);
}

#[test]
fn async_path_walker_parity_nofollow() {
    let runtime: Arc<dyn VfsRuntime> = Arc::new(TestRuntime);
    let fs_sync: Arc<dyn FsSync> = Arc::new(MemFs::new());
    let fs_async: Arc<dyn FsAsync> = Arc::new(AsyncFsFromSync::new(fs_sync.clone(), runtime.clone()));
    let mount_table =
        vfs_core::mount::MountTable::new(fs_sync.clone(), fs_async.clone()).expect("mount table");

    let root = fs_sync.root();
    let name = VfsName::new(b"target").expect("name");
    root.create_file(&name, CreateFile::default())
        .expect("create file");
    let link = VfsName::new(b"link").expect("link");
    root.symlink(&link, VfsPath::new(b"target"))
        .expect("symlink");

    let guard = mount_table
        .guard(MountId::from_index(0))
        .expect("mount guard");
    let cwd = VfsDirHandle::new(
        VfsHandleId(1),
        guard.clone(),
        make_vfs_inode(MountId::from_index(0), root.inode()),
        root.clone(),
        None,
    );
    let async_root = runtime
        .block_on(fs_async.root())
        .expect("async root");
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
        Arc::new(vfs_core::AllowAllPolicy),
    )
    .with_async_cwd(cwd_async);
    let walker = vfs_core::PathWalkerAsync::new(Arc::new(mount_table));

    let mut flags = vfs_core::WalkFlags::new(&ctx);
    flags.follow_final_symlink = false;
    let resolved = runtime
        .block_on(walker.resolve(vfs_core::path_walker::ResolutionRequestAsync {
            ctx: &ctx,
            base: VfsBaseDirAsync::Cwd,
            path: VfsPath::new(b"link"),
            flags,
        }))
        .expect("nofollow resolve");
    assert_eq!(resolved.node.file_type(), VfsFileType::Symlink);
}
