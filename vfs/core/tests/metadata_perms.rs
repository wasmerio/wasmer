use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use vfs_core::inode::make_vfs_inode;
use vfs_core::node::{FsHandle, FsNode};
use vfs_core::policy::{PosixPolicy, VfsMutationOp};
use vfs_core::{
    BackendInodeId, MountId, OpenFlags, VfsBaseDir, VfsConfig, VfsContext, VfsCred, VfsDirHandle,
    VfsErrorKind, VfsFileMode, VfsFileType, VfsHandleId, VfsMetadata, VfsPath, VfsResult,
    VfsTimespec,
};

#[derive(Debug)]
struct TestNode {
    inode: BackendInodeId,
    file_type: VfsFileType,
    metadata: Mutex<VfsMetadata>,
    children: Mutex<HashMap<Vec<u8>, Arc<TestNode>>>,
}

impl TestNode {
    fn dir(inode: BackendInodeId, mode: u32, uid: u32, gid: u32) -> Self {
        Self {
            inode,
            file_type: VfsFileType::Directory,
            metadata: Mutex::new(VfsMetadata {
                inode: make_vfs_inode(MountId::from_index(0), inode),
                file_type: VfsFileType::Directory,
                mode: VfsFileMode(mode),
                nlink: 1,
                uid,
                gid,
                size: 0,
                atime: VfsTimespec { secs: 0, nanos: 0 },
                mtime: VfsTimespec { secs: 0, nanos: 0 },
                ctime: VfsTimespec { secs: 0, nanos: 0 },
                rdev_major: 0,
                rdev_minor: 0,
            }),
            children: Mutex::new(HashMap::new()),
        }
    }

    fn file(inode: BackendInodeId, mode: u32, uid: u32, gid: u32) -> Self {
        Self {
            inode,
            file_type: VfsFileType::RegularFile,
            metadata: Mutex::new(VfsMetadata {
                inode: make_vfs_inode(MountId::from_index(0), inode),
                file_type: VfsFileType::RegularFile,
                mode: VfsFileMode(mode),
                nlink: 1,
                uid,
                gid,
                size: 0,
                atime: VfsTimespec { secs: 0, nanos: 0 },
                mtime: VfsTimespec { secs: 0, nanos: 0 },
                ctime: VfsTimespec { secs: 0, nanos: 0 },
                rdev_major: 0,
                rdev_minor: 0,
            }),
            children: Mutex::new(HashMap::new()),
        }
    }

    fn unsupported<T>(&self, op: &'static str) -> VfsResult<T> {
        Err(vfs_core::VfsError::new(vfs_core::VfsErrorKind::NotSupported, op))
    }
}

impl FsNode for TestNode {
    fn inode(&self) -> BackendInodeId {
        self.inode
    }

    fn file_type(&self) -> VfsFileType {
        self.file_type
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(self.metadata.lock().unwrap().clone())
    }

    fn set_metadata(&self, _set: vfs_core::VfsSetMetadata) -> VfsResult<()> {
        self.unsupported("test.set_metadata")
    }

    fn lookup(&self, name: &vfs_core::VfsName) -> VfsResult<Arc<dyn FsNode>> {
        let children = self.children.lock().unwrap();
        children
            .get(name.as_bytes())
            .cloned()
            .map(|node| node as Arc<dyn FsNode>)
            .ok_or_else(|| vfs_core::VfsError::new(vfs_core::VfsErrorKind::NotFound, "test.lookup"))
    }

    fn create_file(&self, _name: &vfs_core::VfsName, _opts: vfs_core::node::CreateFile) -> VfsResult<Arc<dyn FsNode>> {
        self.unsupported("test.create_file")
    }

    fn mkdir(&self, _name: &vfs_core::VfsName, _opts: vfs_core::node::MkdirOptions) -> VfsResult<Arc<dyn FsNode>> {
        self.unsupported("test.mkdir")
    }

    fn unlink(&self, _name: &vfs_core::VfsName, _opts: vfs_core::node::UnlinkOptions) -> VfsResult<()> {
        self.unsupported("test.unlink")
    }

    fn rmdir(&self, _name: &vfs_core::VfsName) -> VfsResult<()> {
        self.unsupported("test.rmdir")
    }

    fn read_dir(
        &self,
        _cursor: Option<vfs_core::node::DirCursor>,
        _max: usize,
    ) -> VfsResult<vfs_core::node::ReadDirBatch> {
        self.unsupported("test.read_dir")
    }

    fn rename(
        &self,
        _old_name: &vfs_core::VfsName,
        _new_parent: &dyn FsNode,
        _new_name: &vfs_core::VfsName,
        _opts: vfs_core::node::RenameOptions,
    ) -> VfsResult<()> {
        self.unsupported("test.rename")
    }

    fn open(&self, _opts: vfs_core::flags::OpenOptions) -> VfsResult<Arc<dyn FsHandle>> {
        self.unsupported("test.open")
    }

    fn link(&self, _existing: &dyn FsNode, _new_name: &vfs_core::VfsName) -> VfsResult<()> {
        self.unsupported("test.link")
    }

    fn symlink(&self, _new_name: &vfs_core::VfsName, _target: &vfs_core::VfsPath) -> VfsResult<()> {
        self.unsupported("test.symlink")
    }

    fn readlink(&self) -> VfsResult<vfs_core::VfsPathBuf> {
        self.unsupported("test.readlink")
    }
}

struct TestFs {
    root: Arc<TestNode>,
    nodes: Mutex<HashMap<BackendInodeId, Arc<TestNode>>>,
    next_inode: AtomicU64,
}

impl TestFs {
    fn new() -> Arc<Self> {
        let root = Arc::new(TestNode::dir(
            BackendInodeId::new(1).expect("non-zero inode"),
            0o755,
            1000,
            1000,
        ));
        let mut nodes = HashMap::new();
        nodes.insert(root.inode, root.clone());
        Arc::new(Self {
            root,
            nodes: Mutex::new(nodes),
            next_inode: AtomicU64::new(2),
        })
    }

    fn alloc_inode(&self) -> BackendInodeId {
        let raw = self.next_inode.fetch_add(1, Ordering::SeqCst);
        BackendInodeId::new(raw).expect("non-zero inode")
    }

    fn register(&self, node: Arc<TestNode>) {
        self.nodes.lock().unwrap().insert(node.inode, node);
    }

    fn add_dir(&self, parent: &Arc<TestNode>, name: &[u8], mode: u32, uid: u32, gid: u32) -> Arc<TestNode> {
        let node = Arc::new(TestNode::dir(self.alloc_inode(), mode, uid, gid));
        parent
            .children
            .lock()
            .unwrap()
            .insert(name.to_vec(), node.clone());
        self.register(node.clone());
        node
    }

    fn add_file(&self, parent: &Arc<TestNode>, name: &[u8], mode: u32, uid: u32, gid: u32) -> Arc<TestNode> {
        let node = Arc::new(TestNode::file(self.alloc_inode(), mode, uid, gid));
        parent
            .children
            .lock()
            .unwrap()
            .insert(name.to_vec(), node.clone());
        self.register(node.clone());
        node
    }
}

impl vfs_core::Fs for TestFs {
    fn provider_name(&self) -> &'static str {
        "test"
    }

    fn capabilities(&self) -> vfs_core::VfsCapabilities {
        vfs_core::VfsCapabilities::NONE
    }

    fn root(&self) -> Arc<dyn FsNode> {
        self.root.clone()
    }

    fn node_by_inode(&self, inode: BackendInodeId) -> Option<Arc<dyn FsNode>> {
        self.nodes
            .lock()
            .unwrap()
            .get(&inode)
            .cloned()
            .map(|node| node as Arc<dyn FsNode>)
    }
}

fn make_ctx(policy: PosixPolicy, cwd: VfsDirHandle) -> VfsContext {
    VfsContext::new(
        VfsCred {
            uid: 2000,
            gid: 2000,
            groups: smallvec::SmallVec::new(),
            umask: 0,
        },
        cwd,
        Arc::new(VfsConfig::default()),
        Arc::new(policy),
    )
}

fn make_dir_handle(
    mount_table: &vfs_core::mount::MountTable,
    mount: MountId,
    node: Arc<dyn FsNode>,
    id: u64,
) -> VfsDirHandle {
    let guard = mount_table.guard(mount).expect("mount guard");
    VfsDirHandle::new(
        VfsHandleId(id),
        guard,
        make_vfs_inode(mount, node.inode()),
        node,
        None,
    )
}

#[test]
fn traverse_requires_exec() {
    let fs = TestFs::new();
    let root = fs.root.clone();
    let dir = fs.add_dir(&root, b"a", 0o000, 1000, 1000);
    fs.add_file(&dir, b"b", 0o644, 1000, 1000);

    let mount_table = vfs_core::mount::MountTable::new(fs).expect("mount table");
    let cwd = make_dir_handle(&mount_table, MountId::from_index(0), root, 1);
    let ctx = make_ctx(PosixPolicy::new(true, false), cwd);
    let walker = vfs_core::PathWalker::new(Arc::new(mount_table));

    let err = match walker.resolve(vfs_core::path_walker::ResolutionRequest {
        ctx: &ctx,
        base: VfsBaseDir::Cwd,
        path: VfsPath::new(b"a/b"),
        flags: vfs_core::path_walker::WalkFlags::new(&ctx),
    }) {
        Ok(_) => panic!("should deny traversal"),
        Err(err) => err,
    };

    assert_eq!(err.kind(), VfsErrorKind::PermissionDenied);
}

#[test]
fn open_requires_read_write_bits() {
    let policy = PosixPolicy::new(true, false);
    let meta = VfsMetadata {
        inode: make_vfs_inode(MountId::from_index(0), BackendInodeId::new(2).expect("inode")),
        file_type: VfsFileType::RegularFile,
        mode: VfsFileMode(0o400),
        nlink: 1,
        uid: 1000,
        gid: 1000,
        size: 0,
        atime: VfsTimespec { secs: 0, nanos: 0 },
        mtime: VfsTimespec { secs: 0, nanos: 0 },
        ctime: VfsTimespec { secs: 0, nanos: 0 },
        rdev_major: 0,
        rdev_minor: 0,
    };

    let fs: Arc<dyn vfs_core::Fs> = TestFs::new();
    let mount_table = vfs_core::mount::MountTable::new(fs).expect("mount table");
    let cwd = make_dir_handle(&mount_table, MountId::from_index(0), Arc::new(TestNode::dir(BackendInodeId::new(3).expect("inode"), 0o755, 1000, 1000)), 2);
    let ctx = make_ctx(policy, cwd);

    let err = ctx
        .policy
        .check_open(&ctx, &meta, OpenFlags::READ)
        .expect_err("non-owner read should be denied");
    assert_eq!(err.kind(), VfsErrorKind::PermissionDenied);

    let mut owner_ctx = ctx.clone();
    owner_ctx.cred.uid = 1000;
    owner_ctx.cred.gid = 1000;
    owner_ctx
        .policy
        .check_open(&owner_ctx, &meta, OpenFlags::READ)
        .expect("owner read should be allowed");
}

#[test]
fn create_requires_write_and_exec_on_parent() {
    let policy = PosixPolicy::new(true, false);
    let parent_meta = VfsMetadata {
        inode: make_vfs_inode(MountId::from_index(0), BackendInodeId::new(4).expect("inode")),
        file_type: VfsFileType::Directory,
        mode: VfsFileMode(0o555),
        nlink: 1,
        uid: 1000,
        gid: 1000,
        size: 0,
        atime: VfsTimespec { secs: 0, nanos: 0 },
        mtime: VfsTimespec { secs: 0, nanos: 0 },
        ctime: VfsTimespec { secs: 0, nanos: 0 },
        rdev_major: 0,
        rdev_minor: 0,
    };

    let fs: Arc<dyn vfs_core::Fs> = TestFs::new();
    let mount_table = vfs_core::mount::MountTable::new(fs).expect("mount table");
    let cwd = make_dir_handle(&mount_table, MountId::from_index(0), Arc::new(TestNode::dir(BackendInodeId::new(5).expect("inode"), 0o755, 1000, 1000)), 3);
    let ctx = make_ctx(policy, cwd);

    let err = ctx
        .policy
        .check_mutation(&ctx, &parent_meta, VfsMutationOp::CreateFile)
        .expect_err("create should be denied without write");
    assert_eq!(err.kind(), VfsErrorKind::PermissionDenied);
}

#[test]
fn chmod_affects_later_opens() {
    let policy = PosixPolicy::new(true, false);
    let mut meta = VfsMetadata {
        inode: make_vfs_inode(MountId::from_index(0), BackendInodeId::new(6).expect("inode")),
        file_type: VfsFileType::RegularFile,
        mode: VfsFileMode(0o600),
        nlink: 1,
        uid: 1000,
        gid: 1000,
        size: 0,
        atime: VfsTimespec { secs: 0, nanos: 0 },
        mtime: VfsTimespec { secs: 0, nanos: 0 },
        ctime: VfsTimespec { secs: 0, nanos: 0 },
        rdev_major: 0,
        rdev_minor: 0,
    };

    let fs: Arc<dyn vfs_core::Fs> = TestFs::new();
    let mount_table = vfs_core::mount::MountTable::new(fs).expect("mount table");
    let cwd = make_dir_handle(&mount_table, MountId::from_index(0), Arc::new(TestNode::dir(BackendInodeId::new(7).expect("inode"), 0o755, 1000, 1000)), 4);
    let ctx = make_ctx(policy, cwd);

    let err = ctx
        .policy
        .check_open(&ctx, &meta, OpenFlags::READ)
        .expect_err("read should be denied before chmod");
    assert_eq!(err.kind(), VfsErrorKind::PermissionDenied);

    meta.mode = VfsFileMode(0o644);
    ctx.policy
        .check_open(&ctx, &meta, OpenFlags::READ)
        .expect("read should be allowed after chmod");
}
