//! The namespace filesystem (`fs/nsfs.c`), mounted nowhere: one inode per namespace, which the
//! links in `/proc/<pid>/ns` jump to. An open file of it stands for its namespace, for `setns`
//! and the `NS_GET_*` requests, and its path reads as `kind:[ino]`.

use std::{any::Any, sync::Arc};

use async_trait::async_trait;
use wasixv2_abi::{GuestPtrMut, IoctlRequest, OpenFlags};
use wasixv2_syscalls::{Caller, Errno, LocalFuture, SysResult, write_value};
use wasixv2_vfs::{
    Attr, Clock, DirSink, FileType, Filesystem, FsInfo, FsResult, Handle, Mount, NAME_MAX, Node,
    OpenOptions, Path, SetAttr, StatFs, TimeSet, Vfs,
};
use wasixv2_wait::{PollMask, PollTable, Pollable};

use crate::{
    Kernel,
    file::{Backing, FileObject, Io, OpenFile},
    ns::Namespace,
    sync::MutexExt,
};

/// `NSFS_MAGIC`.
const NSFS_MAGIC: u32 = 0x6e73_6673;

/// The kernel's mount of the namespace filesystem (`nsfs_mnt`).
#[derive(Clone)]
pub(crate) struct Nsfs {
    mount: Arc<Mount>,
    clock: Arc<dyn Clock>,
}

impl Nsfs {
    pub(crate) fn new(vfs: &Vfs, clock: Arc<dyn Clock>) -> Self {
        let root = Arc::new(NsRoot {
            attr: Attr {
                ino: 1,
                file_type: FileType::Directory,
                mode: 0o755,
                nlink: 2,
                ..inode_attr(clock.now())
            },
        });
        Self {
            mount: vfs.kern_mount(Arc::new(NsFs { root })),
            clock,
        }
    }

    /// Where `ns`'s inode is (`ns_get_path`). The inode is the one already in use for the
    /// namespace, if any (`path_from_stashed`).
    pub(crate) fn path(&self, ns: &Namespace) -> Path {
        let mut stashed = ns.common().stashed.lock_or_recover();
        let node = match stashed.upgrade() {
            Some(node) => node,
            None => {
                let node: Arc<dyn Node> = Arc::new(NsNode::new(ns.clone(), self.clock.clone()));
                *stashed = Arc::downgrade(&node);
                node
            }
        };
        self.mount.pseudo_path(node, b"")
    }
}

/// The namespace an nsfs path stands for, if `path` is one (`proc_ns_file`).
pub(crate) fn namespace(path: &Path) -> Option<Namespace> {
    if path.dentry.sb().info.magic != NSFS_MAGIC {
        return None;
    }
    let node = path.dentry.node();
    let node: &NsNode = node.as_any().downcast_ref()?;
    Some(node.ns.clone())
}

/// A new inode's attributes, made at `now` and owned by root (`nsfs_init_inode`).
fn inode_attr(now: wasixv2_vfs::Timestamp) -> Attr {
    Attr {
        ino: 0,
        file_type: FileType::Regular,
        mode: 0o444,
        nlink: 1,
        uid: 0,
        gid: 0,
        rdev: Default::default(),
        size: 0,
        blocks: 0,
        blksize: 4096,
        atime: now,
        mtime: now,
        ctime: now,
        btime: None,
        dev: None,
    }
}

struct NsFs {
    root: Arc<NsRoot>,
}

#[async_trait(?Send)]
impl Filesystem for NsFs {
    fn info(&self) -> FsInfo {
        FsInfo {
            name: "nsfs",
            magic: NSFS_MAGIC,
            name_max: NAME_MAX,
            read_only: false,
            dev: None,
            has_btime: false,
            // Nothing is ever named in it.
            stable: true,
        }
    }

    fn root(&self) -> Arc<dyn Node> {
        self.root.clone()
    }

    /// `ns_dentry_operations`: a namespace file binds, which keeps its namespace.
    fn binds_objects(&self) -> bool {
        true
    }

    async fn statfs(&self) -> FsResult<StatFs> {
        Ok(StatFs {
            block_size: 4096,
            name_max: NAME_MAX as u32,
            fragment_size: 4096,
            ..StatFs::default()
        })
    }

    /// `ns_dname`.
    fn dname(&self, _: &[u8], node: &dyn Node) -> Option<Vec<u8>> {
        let node: &NsNode = node.as_any().downcast_ref()?;
        Some(format!("{}:[{}]", node.ns.kind().name(), node.ns.ino()).into_bytes())
    }
}

/// The filesystem's empty root.
struct NsRoot {
    attr: Attr,
}

#[async_trait(?Send)]
impl Node for NsRoot {
    fn ino(&self) -> u64 {
        self.attr.ino
    }

    fn file_type(&self) -> FileType {
        FileType::Directory
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn getattr(&self) -> FsResult<Attr> {
        Ok(self.attr.clone())
    }

    async fn setattr(&self, _: &SetAttr) -> FsResult<Attr> {
        Err(Errno::EPERM)
    }

    async fn lookup(&self, _: &[u8]) -> FsResult<Arc<dyn Node>> {
        Err(Errno::ENOENT)
    }

    async fn open(&self, _: OpenOptions) -> FsResult<Box<dyn Handle>> {
        Ok(Box::new(EmptyDir))
    }
}

struct EmptyDir;

#[async_trait(?Send)]
impl Handle for EmptyDir {
    async fn readdir(&self, _: u64, _: &mut dyn DirSink) -> FsResult<()> {
        Ok(())
    }
}

/// A namespace's inode: a regular file readable by everyone, numbered as the namespace.
pub(crate) struct NsNode {
    ns: Namespace,
    clock: Arc<dyn Clock>,
    attr: std::sync::Mutex<Attr>,
}

impl NsNode {
    fn new(ns: Namespace, clock: Arc<dyn Clock>) -> Self {
        let attr = Attr {
            ino: u64::from(ns.ino()),
            ..inode_attr(clock.now())
        };
        Self {
            ns,
            clock,
            attr: std::sync::Mutex::new(attr),
        }
    }
}

#[async_trait(?Send)]
impl Node for NsNode {
    fn ino(&self) -> u64 {
        u64::from(self.ns.ino())
    }

    fn file_type(&self) -> FileType {
        FileType::Regular
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn getattr(&self) -> FsResult<Attr> {
        Ok(self.attr.lock_or_recover().clone())
    }

    /// `simple_setattr`: there is no size to change.
    async fn setattr(&self, set: &SetAttr) -> FsResult<Attr> {
        if set.size.is_some() {
            return Err(Errno::EINVAL);
        }
        let now = self.clock.now();
        let mut attr = self.attr.lock_or_recover();
        let time = |t: TimeSet| match t {
            TimeSet::At(t) => t,
            TimeSet::Now => now,
        };
        if let Some(mode) = set.mode {
            attr.mode = mode;
        }
        if let Some(uid) = set.uid {
            attr.uid = uid;
        }
        if let Some(gid) = set.gid {
            attr.gid = gid;
        }
        if let Some(t) = set.atime {
            attr.atime = time(t);
        }
        if let Some(t) = set.mtime {
            attr.mtime = time(t);
        }
        if let Some(t) = set.ctime {
            attr.ctime = time(t);
        }
        Ok(attr.clone())
    }

    /// Opens; the kernel then attaches the namespace ([`NsFile`]), whose file has no data.
    async fn open(&self, _: OpenOptions) -> FsResult<Box<dyn Handle>> {
        Ok(Box::new(NoData))
    }
}

/// What the VFS opens before the kernel attaches the namespace.
struct NoData;

#[async_trait(?Send)]
impl Handle for NoData {}

/// An open namespace file (`ns_file_operations`): no data, only its `ioctl` requests.
pub(crate) struct NsFile(Namespace);

impl NsFile {
    pub(crate) fn new(ns: Namespace) -> Self {
        Self(ns)
    }

    pub(crate) fn namespace(&self) -> &Namespace {
        &self.0
    }
}

impl Pollable for NsFile {
    /// `DEFAULT_POLLMASK` of a file without `poll`.
    fn poll(&self, _: Option<&mut PollTable<'_>>) -> PollMask {
        PollMask::READABLE | PollMask::WRITABLE
    }
}

impl FileObject for NsFile {
    fn read<'a>(&'a self, _: Io<'a>, _: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async { Err(Errno::EINVAL.into()) })
    }

    fn write<'a>(&'a self, _: Io<'a>, _: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async { Err(Errno::EINVAL.into()) })
    }

    fn can_poll(&self) -> bool {
        false
    }
}

impl Kernel {
    /// `ns_ioctl`: the namespace that owns the file's (`NS_GET_USERNS`) or its parent
    /// (`NS_GET_PARENT`) as a new descriptor, the file's kind as its `CLONE_NEW*` flag
    /// (`NS_GET_NSTYPE`), or the uid that made a user namespace (`NS_GET_OWNER_UID`).
    pub(crate) async fn ns_ioctl(
        &self,
        caller: &Caller<'_>,
        file: &NsFile,
        request: IoctlRequest,
        arg: u32,
    ) -> SysResult<i32> {
        let ns = file.namespace();
        match request {
            IoctlRequest::NS_GET_USERNS => {
                let owner = ns.owner(&self.cred(caller).user_ns)?;
                self.open_namespace(caller, Namespace::User(owner))
            }
            IoctlRequest::NS_GET_PARENT => {
                let user = self.cred(caller).user_ns.clone();
                let active = self.process(caller.task()).pid_ns.clone();
                let parent = ns.parent(&user, &active)?;
                self.open_namespace(caller, parent)
            }
            IoctlRequest::NS_GET_NSTYPE => Ok(ns.kind().flag() as i32),
            // Linux `fs/nsfs.c:ns_ioctl`, IDs shared with listns.
            IoctlRequest::NS_GET_ID | IoctlRequest::NS_GET_MNTNS_ID => {
                if request == IoctlRequest::NS_GET_MNTNS_ID && ns.kind() != crate::ns::NsKind::Mnt {
                    return Err(Errno::EINVAL.into());
                }
                write_value(caller.memory(), GuestPtrMut::<u64>::new(arg), &ns.ns_id()).await?;
                Ok(0)
            }
            IoctlRequest::NS_GET_OWNER_UID => {
                let uid = ns.owner_uid(&self.cred(caller).user_ns)?;
                write_value(caller.memory(), GuestPtrMut::<u32>::new(arg), &uid).await?;
                Ok(0)
            }
            _ => Err(Errno::ENOTTY.into()),
        }
    }

    /// `open_namespace`: a new read-only, close-on-exec descriptor for `ns`.
    fn open_namespace(&self, caller: &Caller<'_>, ns: Namespace) -> SysResult<i32> {
        let path = self.pseudo.nsfs.path(&ns);
        let backing = Backing::Stream(Arc::new(NsFile(ns)), path);
        let [fd] =
            self.install_files(caller, [OpenFile::new(backing, OpenFlags::O_RDONLY)], true)?;
        Ok(fd.0)
    }
}
