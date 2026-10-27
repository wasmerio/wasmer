//! Filesystems mounted nowhere, which give the objects behind pipes, sockets and event
//! descriptors an inode and a path, as Linux's pipefs, sockfs and anon_inodefs do. `fstat`
//! reports that inode, `/proc/<pid>/fd` names it (`pipe:[ino]`, `socket:[ino]`,
//! `anon_inode:[eventfd]`), and a pipe can be opened again through it.

use std::{
    any::Any,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use async_trait::async_trait;
use wasixv2_syscalls::Errno;
use wasixv2_vfs::{
    Attr, Clock, DirSink, FileType, Filesystem, FsInfo, FsResult, Handle, Mount, NAME_MAX, Node,
    OpenOptions, Path, SetAttr, StatFs, TimeSet, Vfs,
};

/// `PIPEFS_MAGIC`, `SOCKFS_MAGIC` and `ANON_INODE_FS_MAGIC`.
pub(crate) const PIPEFS_MAGIC: u32 = 0x5049_5045;
const SOCKFS_MAGIC: u32 = 0x534f_434b;
pub(crate) const ANON_INODE_FS_MAGIC: u32 = 0x0904_1934;
/// `MQUEUE_MAGIC`.
const MQUEUE_MAGIC: u32 = 0x1980_0202;

/// The internal mounts behind objects without a name.
pub(crate) struct Pseudo {
    pipe: (Arc<Mount>, Arc<PseudoFs>),
    socket: (Arc<Mount>, Arc<PseudoFs>),
    anon: Arc<Mount>,
    /// The kernel's own mount of the message queue filesystem (`mq_mnt`).
    mqueue: (Arc<Mount>, Arc<PseudoFs>),
    /// The one inode every anonymous file shares (`anon_inode_inode`); only its name differs.
    anon_inode: Arc<dyn Node>,
}

impl Pseudo {
    pub(crate) fn new(vfs: &Vfs, clock: Arc<dyn Clock>) -> Self {
        let mount = |kind| {
            let fs = Arc::new(PseudoFs::new(kind, clock.clone()));
            (vfs.kern_mount(fs.clone()), fs)
        };
        let pipe = mount(Kind::Pipe);
        let socket = mount(Kind::Socket);
        let (anon, anon_fs) = mount(Kind::Anon);
        let mqueue = mount(Kind::Mqueue);
        // Anonymous inodes have no file type (`alloc_anon_inode`); the VFS needs one, and
        // `statx` masks it off as `anon_inode_getattr` does. The one inode is made at boot, so
        // root owns it.
        let anon_inode = anon_fs.new_node(FileType::Regular, 0o600, (0, 0));
        Self {
            pipe,
            socket,
            anon,
            mqueue,
            anon_inode,
        }
    }

    /// The inode of a new pipe (`get_pipe_inode`), owned by `owner`, the creator's fsuid and
    /// fsgid.
    pub(crate) fn pipe(&self, owner: (u32, u32)) -> Path {
        let (mount, fs) = &self.pipe;
        mount.pseudo_path(fs.new_node(FileType::Fifo, 0o600, owner), b"")
    }

    /// The inode of a new socket (`sock_alloc`), owned by `owner`, the creator's fsuid and
    /// fsgid.
    pub(crate) fn socket(&self, owner: (u32, u32)) -> Path {
        let (mount, fs) = &self.socket;
        mount.pseudo_path(fs.new_node(FileType::Socket, 0o777, owner), b"")
    }

    /// The inode of a new POSIX message queue (`mqueue_get_inode`), owned by `owner` with the
    /// permission bits `mode`.
    pub(crate) fn mqueue_inode(&self, owner: (u32, u32), mode: u32) -> Arc<dyn Node> {
        self.mqueue.1.new_node(FileType::Regular, mode, owner)
    }

    /// Where the queue `name` with the inode `inode` is, as an open descriptor names it.
    pub(crate) fn mqueue(&self, inode: Arc<dyn Node>, name: &[u8]) -> Path {
        self.mqueue.0.pseudo_path(inode, name)
    }

    /// A path for an anonymous file of the class `name`, such as `[eventfd]`
    /// (`anon_inode_getfile`).
    pub(crate) fn anon(&self, name: &str) -> Path {
        self.anon
            .pseudo_path(self.anon_inode.clone(), name.as_bytes())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Pipe,
    Socket,
    Anon,
    Mqueue,
}

struct PseudoFs {
    kind: Kind,
    next_ino: AtomicU64,
    clock: Arc<dyn Clock>,
    root: Arc<PseudoNode>,
}

impl PseudoFs {
    fn new(kind: Kind, clock: Arc<dyn Clock>) -> Self {
        let root = PseudoNode::new(1, FileType::Directory, 0o755, (0, 0), kind, clock.clone());
        Self {
            kind,
            next_ino: AtomicU64::new(2),
            clock,
            root: Arc::new(root),
        }
    }

    /// A new inode of this filesystem, owned by the uid and gid `owner`.
    fn new_node(&self, file_type: FileType, mode: u32, owner: (u32, u32)) -> Arc<dyn Node> {
        let ino = self.next_ino.fetch_add(1, Ordering::Relaxed);
        Arc::new(PseudoNode::new(
            ino,
            file_type,
            mode,
            owner,
            self.kind,
            self.clock.clone(),
        ))
    }
}

#[async_trait(?Send)]
impl Filesystem for PseudoFs {
    fn info(&self) -> FsInfo {
        let (name, magic) = match self.kind {
            Kind::Pipe => ("pipefs", PIPEFS_MAGIC),
            Kind::Socket => ("sockfs", SOCKFS_MAGIC),
            Kind::Anon => ("anon_inodefs", ANON_INODE_FS_MAGIC),
            Kind::Mqueue => ("mqueue", MQUEUE_MAGIC),
        };
        FsInfo {
            name,
            magic,
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

    async fn statfs(&self) -> FsResult<StatFs> {
        Ok(StatFs {
            block_size: 4096,
            name_max: NAME_MAX as u32,
            fragment_size: 4096,
            ..StatFs::default()
        })
    }

    /// `pipefs_dname`, `sockfs_dname` and `anon_inodefs_dname`; a queue is named by its path in
    /// the queue filesystem.
    fn dname(&self, name: &[u8], node: &dyn Node) -> Option<Vec<u8>> {
        Some(match self.kind {
            Kind::Pipe => format!("pipe:[{}]", node.ino()).into_bytes(),
            Kind::Socket => format!("socket:[{}]", node.ino()).into_bytes(),
            Kind::Anon => [b"anon_inode:", name].concat(),
            Kind::Mqueue => [b"/", name].concat(),
        })
    }
}

struct PseudoNode {
    kind: Kind,
    clock: Arc<dyn Clock>,
    attr: Mutex<Attr>,
}

impl PseudoNode {
    fn new(
        ino: u64,
        file_type: FileType,
        mode: u32,
        (uid, gid): (u32, u32),
        kind: Kind,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let now = clock.now();
        Self {
            kind,
            clock,
            attr: Mutex::new(Attr {
                ino,
                file_type,
                mode,
                nlink: 1,
                uid,
                gid,
                rdev: Default::default(),
                size: 0,
                blocks: 0,
                blksize: 4096,
                atime: now,
                mtime: now,
                ctime: now,
                btime: None,
                dev: None,
            }),
        }
    }
}

#[async_trait(?Send)]
impl Node for PseudoNode {
    fn ino(&self) -> u64 {
        self.attr.lock().unwrap().ino
    }

    fn file_type(&self) -> FileType {
        self.attr.lock().unwrap().file_type
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn getattr(&self) -> FsResult<Attr> {
        Ok(self.attr.lock().unwrap().clone())
    }

    /// `simple_setattr` for pipes and sockets; anonymous inodes refuse
    /// (`anon_inode_setattr`).
    async fn setattr(&self, set: &SetAttr) -> FsResult<Attr> {
        if self.kind == Kind::Anon {
            return Err(Errno::EOPNOTSUPP);
        }
        let now = self.clock.now();
        let mut attr = self.attr.lock().unwrap();
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

    async fn lookup(&self, _: &[u8]) -> FsResult<Arc<dyn Node>> {
        match self.file_type() {
            FileType::Directory => Err(Errno::ENOENT),
            _ => Err(Errno::ENOTDIR),
        }
    }

    /// Only the empty root opens; the objects themselves are never opened as files
    /// (`no_open`), except that the kernel attaches a pipe to its inode.
    async fn open(&self, _: OpenOptions) -> FsResult<Box<dyn Handle>> {
        match self.file_type() {
            FileType::Directory => Ok(Box::new(EmptyDir)),
            _ => Err(Errno::ENXIO),
        }
    }
}

struct EmptyDir;

#[async_trait(?Send)]
impl Handle for EmptyDir {
    async fn readdir(&self, _: u64, _: &mut dyn DirSink) -> FsResult<()> {
        Ok(())
    }
}
