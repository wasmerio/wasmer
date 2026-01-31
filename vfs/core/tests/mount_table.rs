use std::sync::Arc;

use vfs_core::inode::make_vfs_inode;
use vfs_core::mount::{MountTable, UnmountFlags};
use vfs_core::node::{FsHandle, FsNode};
use vfs_core::path_types::{VfsName, VfsPath, VfsPathBuf};
use vfs_core::provider::{AsyncFsFromSync, VfsRuntime};
use vfs_core::{
    BackendInodeId, MountId, VfsCapabilities, VfsError, VfsErrorKind, VfsFileMode, VfsFileType,
    VfsMetadata, VfsResult, VfsTimespec,
};
use vfs_rt::InlineTestRuntime;

struct DummyNode {
    inode: BackendInodeId,
}

impl DummyNode {
    fn new(inode: BackendInodeId) -> Self {
        Self { inode }
    }

    fn unsupported<T>(&self, op: &'static str) -> VfsResult<T> {
        Err(VfsError::new(VfsErrorKind::NotSupported, op))
    }
}

impl FsNode for DummyNode {
    fn inode(&self) -> BackendInodeId {
        self.inode
    }

    fn file_type(&self) -> VfsFileType {
        VfsFileType::Directory
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(VfsMetadata {
            inode: make_vfs_inode(MountId::from_index(0), self.inode()),
            file_type: self.file_type(),
            mode: VfsFileMode(0o755),
            nlink: 1,
            uid: 0,
            gid: 0,
            size: 0,
            atime: VfsTimespec { secs: 0, nanos: 0 },
            mtime: VfsTimespec { secs: 0, nanos: 0 },
            ctime: VfsTimespec { secs: 0, nanos: 0 },
            rdev_major: 0,
            rdev_minor: 0,
        })
    }

    fn set_metadata(&self, _set: vfs_core::node::SetMetadata) -> VfsResult<()> {
        self.unsupported("dummy.set_metadata")
    }

    fn lookup(&self, _name: &VfsName) -> VfsResult<Arc<dyn FsNode>> {
        self.unsupported("dummy.lookup")
    }

    fn create_file(
        &self,
        _name: &VfsName,
        _opts: vfs_core::node::CreateFile,
    ) -> VfsResult<Arc<dyn FsNode>> {
        self.unsupported("dummy.create_file")
    }

    fn mkdir(
        &self,
        _name: &VfsName,
        _opts: vfs_core::node::MkdirOptions,
    ) -> VfsResult<Arc<dyn FsNode>> {
        self.unsupported("dummy.mkdir")
    }

    fn unlink(&self, _name: &VfsName, _opts: vfs_core::node::UnlinkOptions) -> VfsResult<()> {
        self.unsupported("dummy.unlink")
    }

    fn rmdir(&self, _name: &VfsName) -> VfsResult<()> {
        self.unsupported("dummy.rmdir")
    }

    fn read_dir(
        &self,
        _cursor: Option<vfs_core::node::VfsDirCookie>,
        _max: usize,
    ) -> VfsResult<vfs_core::node::ReadDirBatch> {
        self.unsupported("dummy.read_dir")
    }

    fn rename(
        &self,
        _old_name: &VfsName,
        _new_parent: &dyn FsNode,
        _new_name: &VfsName,
        _opts: vfs_core::node::RenameOptions,
    ) -> VfsResult<()> {
        self.unsupported("dummy.rename")
    }

    fn link(&self, _existing: &dyn FsNode, _new_name: &VfsName) -> VfsResult<()> {
        self.unsupported("dummy.link")
    }

    fn symlink(&self, _new_name: &VfsName, _target: &VfsPath) -> VfsResult<()> {
        self.unsupported("dummy.symlink")
    }

    fn readlink(&self) -> VfsResult<VfsPathBuf> {
        self.unsupported("dummy.readlink")
    }

    fn open(&self, _opts: vfs_core::flags::OpenOptions) -> VfsResult<Arc<dyn FsHandle>> {
        self.unsupported("dummy.open")
    }
}

struct DummyFs {
    root: Arc<dyn FsNode>,
}

impl DummyFs {
    fn new() -> Self {
        Self {
            root: Arc::new(DummyNode::new(
                BackendInodeId::new(1).expect("non-zero inode"),
            )),
        }
    }
}

impl vfs_core::Fs for DummyFs {
    fn provider_name(&self) -> &'static str {
        "dummy"
    }

    fn capabilities(&self) -> VfsCapabilities {
        VfsCapabilities::NONE
    }

    fn root(&self) -> Arc<dyn FsNode> {
        self.root.clone()
    }
}

fn inode(raw: u64) -> BackendInodeId {
    BackendInodeId::new(raw).expect("non-zero inode")
}

fn dummy_fs_pair() -> (Arc<dyn vfs_core::Fs>, Arc<dyn vfs_core::FsAsync>) {
    let fs: Arc<dyn vfs_core::Fs> = Arc::new(DummyFs::new());
    let runtime: Arc<dyn VfsRuntime> = Arc::new(InlineTestRuntime);
    let fs_async: Arc<dyn vfs_core::FsAsync> = Arc::new(AsyncFsFromSync::new(fs.clone(), runtime));
    (fs, fs_async)
}

#[test]
fn mount_enter_mapping() {
    let (root_fs, root_async) = dummy_fs_pair();
    let mount_table = MountTable::new(root_fs, root_async).expect("mount table");
    let root_mount = MountId::from_index(0);

    let mountpoint_inode = make_vfs_inode(root_mount, inode(10));
    let (child_fs, child_async) = dummy_fs_pair();
    let child_mount = mount_table
        .mount(
            root_mount,
            mountpoint_inode,
            child_fs,
            child_async,
            inode(1),
            vfs_core::provider::MountFlags::empty(),
        )
        .expect("mount");

    let inner = mount_table.snapshot();
    assert_eq!(
        inner.mount_by_mountpoint.get(&mountpoint_inode),
        Some(&child_mount)
    );
    assert_eq!(
        MountTable::enter_if_mountpoint(&inner, root_mount, mountpoint_inode),
        Some(child_mount)
    );
}

#[test]
fn parent_of_mount_root_crosses_boundary() {
    let (root_fs, root_async) = dummy_fs_pair();
    let mount_table = MountTable::new(root_fs, root_async).expect("mount table");
    let root_mount = MountId::from_index(0);

    let mountpoint_inode = make_vfs_inode(root_mount, inode(11));
    let (child_fs, child_async) = dummy_fs_pair();
    let child_mount = mount_table
        .mount(
            root_mount,
            mountpoint_inode,
            child_fs,
            child_async,
            inode(2),
            vfs_core::provider::MountFlags::empty(),
        )
        .expect("mount");

    let inner = mount_table.snapshot();
    let (parent, mountpoint) =
        MountTable::parent_of_mount_root(&inner, child_mount).expect("parent");
    assert_eq!(parent, root_mount);
    assert_eq!(mountpoint, mountpoint_inode);
}

#[test]
fn unmount_busy_fails() {
    let (root_fs, root_async) = dummy_fs_pair();
    let mount_table = MountTable::new(root_fs, root_async).expect("mount table");
    let root_mount = MountId::from_index(0);

    let mountpoint_inode = make_vfs_inode(root_mount, inode(12));
    let (child_fs, child_async) = dummy_fs_pair();
    let child_mount = mount_table
        .mount(
            root_mount,
            mountpoint_inode,
            child_fs,
            child_async,
            inode(3),
            vfs_core::provider::MountFlags::empty(),
        )
        .expect("mount");

    let _guard = mount_table.guard(child_mount).expect("mount guard");
    let err = mount_table
        .unmount(child_mount, UnmountFlags::None)
        .expect_err("unmount should be busy");
    assert_eq!(err.kind(), VfsErrorKind::Busy);
    assert_eq!(err.context(), "unmount.busy");
}

#[test]
fn detach_hides_mount_and_reclaims() {
    let (root_fs, root_async) = dummy_fs_pair();
    let mount_table = MountTable::new(root_fs, root_async).expect("mount table");
    let root_mount = MountId::from_index(0);

    let mountpoint_inode = make_vfs_inode(root_mount, inode(13));
    let (child_fs, child_async) = dummy_fs_pair();
    let child_mount = mount_table
        .mount(
            root_mount,
            mountpoint_inode,
            child_fs,
            child_async,
            inode(4),
            vfs_core::provider::MountFlags::empty(),
        )
        .expect("mount");

    {
        let guard = mount_table.guard(child_mount).expect("mount guard");
        mount_table
            .unmount(child_mount, UnmountFlags::Detach)
            .expect("detach");

        let inner = mount_table.snapshot();
        assert!(!inner.mount_by_mountpoint.contains_key(&mountpoint_inode));
        let err = mount_table
            .guard(child_mount)
            .expect_err("detached guard fails");
        assert_eq!(err.kind(), VfsErrorKind::NotFound);
        assert_eq!(err.context(), "mount.detached");

        drop(guard);
    }

    let inner = mount_table.snapshot();
    assert!(
        inner
            .mounts
            .get(child_mount.index())
            .and_then(|slot| slot.as_ref())
            .is_none()
    );
}

#[test]
fn unmount_with_children_fails() {
    let (root_fs, root_async) = dummy_fs_pair();
    let mount_table = MountTable::new(root_fs, root_async).expect("mount table");
    let root_mount = MountId::from_index(0);

    let mountpoint_inode = make_vfs_inode(root_mount, inode(14));
    let (child_fs, child_async) = dummy_fs_pair();
    let child_mount = mount_table
        .mount(
            root_mount,
            mountpoint_inode,
            child_fs,
            child_async,
            inode(5),
            vfs_core::provider::MountFlags::empty(),
        )
        .expect("mount");

    let child_mountpoint = make_vfs_inode(child_mount, inode(15));
    let (grand_fs, grand_async) = dummy_fs_pair();
    let _grandchild_mount = mount_table
        .mount(
            child_mount,
            child_mountpoint,
            grand_fs,
            grand_async,
            inode(6),
            vfs_core::provider::MountFlags::empty(),
        )
        .expect("mount grandchild");

    let err = mount_table
        .unmount(child_mount, UnmountFlags::None)
        .expect_err("unmount should fail with children");
    assert_eq!(err.kind(), VfsErrorKind::Busy);
    assert_eq!(err.context(), "unmount.has_children");
}
