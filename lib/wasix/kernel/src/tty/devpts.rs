//! The pty filesystem, Linux's devpts (`fs/devpts/inode.c`), mounted at `/dev/pts`: `ptmx`,
//! and a node for each pty slave whose master is open, named by its index and owned by the
//! process that opened the master. The kernel has one instance, as the initial devpts
//! instance: every mount shows the same ptys.
//!
//! A slave opens only through its node here (`devpts_get_priv`): a node with its number made
//! elsewhere opens nothing.

use std::{
    any::Any,
    collections::BTreeMap,
    sync::{Arc, Mutex, Weak},
};

use async_trait::async_trait;
use wasixv2_syscalls::Errno;
use wasixv2_vfs::{
    Attr, Clock, DirEntry, DirSink, FileType, Filesystem, FsInfo, FsResult, Handle, NAME_MAX, Node,
    OpenOptions, SetAttr, StatFs, TimeSet,
};

use super::{Tty, pty::PTMX};
use crate::{Kernel, sync::MutexExt};

/// `DEVPTS_SUPER_MAGIC`.
pub(super) const DEVPTS_SUPER_MAGIC: u32 = 0x1cd1;
/// `NR_UNIX98_PTY_DEFAULT`, `/proc/sys/kernel/pty/max`: how many ptys there may be.
const PTY_MAX: usize = 4096;
/// `DEVPTS_DEFAULT_MODE`: a slave's node is its owner's alone.
const PTY_MODE: u32 = 0o600;
/// `DEVPTS_DEFAULT_PTMX_MODE`: devpts' own `ptmx` opens for nobody but root; `/dev/ptmx` is
/// the one to use.
const PTMX_MODE: u32 = 0;
/// The root's and `ptmx`'s inode numbers; a slave's is its index plus 3.
const ROOT_INO: u64 = 1;
const PTMX_INO: u64 = 2;

/// A slave's node, and the slave it keeps while the master is open (the dentry's
/// `d_fsdata`).
struct Entry {
    node: Arc<PtsNode>,
    _slave: Arc<Tty>,
}

/// The ptys' indexes and the nodes of their slaves (`pts_fs_info`).
pub(crate) struct Ptys {
    /// The indexes in use, with the slave's node while its master is open.
    slots: Mutex<BTreeMap<u32, Option<Entry>>>,
    root: Arc<PtsNode>,
    ptmx: Arc<PtsNode>,
    clock: Arc<dyn Clock>,
}

impl Ptys {
    pub(crate) fn new(clock: Arc<dyn Clock>) -> Arc<Self> {
        Arc::new_cyclic(|ptys: &Weak<Ptys>| {
            let node = |ino, file_type, mode, rdev, kind| {
                Arc::new(PtsNode::new(
                    ino,
                    file_type,
                    mode,
                    rdev,
                    (0, 0),
                    kind,
                    ptys.clone(),
                    &*clock,
                ))
            };
            // `devpts_fill_super`: the root is `S_IRUGO | S_IXUGO | S_IWUSR`.
            let root = node(
                ROOT_INO,
                FileType::Directory,
                0o755,
                Default::default(),
                Kind::Root,
            );
            let ptmx = node(PTMX_INO, FileType::CharDevice, PTMX_MODE, PTMX, Kind::Ptmx);
            Self {
                slots: Mutex::default(),
                root,
                ptmx,
                clock: clock.clone(),
            }
        })
    }

    /// `devpts_new_index`: the lowest free index; `ENOSPC` once the limit is reached.
    pub(super) fn new_index(&self) -> Result<u32, Errno> {
        let mut slots = self.slots.lock_or_recover();
        // `atomic_inc_return(&pty_count) >= pty_limit`.
        if slots.len() + 1 >= PTY_MAX {
            return Err(Errno::ENOSPC);
        }
        let index = (0..)
            .zip(slots.keys())
            .find(|(free, used)| free != *used)
            .map_or(slots.len() as u32, |(free, _)| free);
        slots.insert(index, None);
        Ok(index)
    }

    /// `devpts_pty_new`: the node of the slave `slave` at `index`, owned by `owner`.
    pub(super) fn add(self: &Arc<Self>, index: u32, slave: &Arc<Tty>, owner: (u32, u32)) {
        let node = PtsNode::new(
            u64::from(index) + 3,
            FileType::CharDevice,
            PTY_MODE,
            slave.device(),
            owner,
            Kind::Slave(Arc::downgrade(slave)),
            Arc::downgrade(self),
            &*self.clock,
        );
        let entry = Entry {
            node: Arc::new(node),
            _slave: slave.clone(),
        };
        self.slots.lock_or_recover().insert(index, Some(entry));
    }

    /// `devpts_pty_kill`: the slave's node goes, as its master closed.
    pub(super) fn kill(&self, index: u32) {
        let entry = self
            .slots
            .lock_or_recover()
            .get_mut(&index)
            .and_then(Option::take);
        // The slave may go with it, outside the lock.
        drop(entry);
    }

    /// `devpts_kill_index`: neither side is open any more.
    pub(super) fn free(&self, index: u32) {
        self.slots.lock_or_recover().remove(&index);
    }

    /// The slave a node opens (`devpts_get_priv`): only a node of this filesystem, of a pty
    /// whose master is open.
    pub(crate) fn slave(&self, node: &dyn Node) -> Option<Arc<Tty>> {
        let node: &PtsNode = node.as_any().downcast_ref()?;
        match &node.kind {
            Kind::Slave(slave) => slave.upgrade(),
            _ => None,
        }
    }

    fn node(&self, index: u32) -> Option<Arc<PtsNode>> {
        let slots = self.slots.lock_or_recover();
        Some(slots.get(&index)?.as_ref()?.node.clone())
    }

    /// The nodes there are, by index.
    fn nodes(&self) -> Vec<(u32, Arc<PtsNode>)> {
        let slots = self.slots.lock_or_recover();
        slots
            .iter()
            .filter_map(|(&index, entry)| Some((index, entry.as_ref()?.node.clone())))
            .collect()
    }
}

impl Kernel {
    /// The pty filesystem, to mount at `/dev/pts`.
    pub fn devpts(&self) -> Arc<dyn Filesystem> {
        Arc::new(DevptsFs(self.ptys.clone()))
    }
}

struct DevptsFs(Arc<Ptys>);

#[async_trait(?Send)]
impl Filesystem for DevptsFs {
    fn info(&self) -> FsInfo {
        FsInfo {
            name: "devpts",
            magic: DEVPTS_SUPER_MAGIC,
            name_max: NAME_MAX,
            read_only: false,
            dev: None,
            has_btime: false,
            // Slaves come and go with their masters.
            stable: false,
        }
    }

    fn root(&self) -> Arc<dyn Node> {
        self.0.root.clone()
    }

    async fn statfs(&self) -> FsResult<StatFs> {
        Ok(StatFs {
            block_size: 4096,
            name_max: NAME_MAX as u32,
            fragment_size: 4096,
            ..StatFs::default()
        })
    }
}

enum Kind {
    Root,
    Ptmx,
    /// A slave's node; weak, as the slave belongs to its pair.
    Slave(Weak<Tty>),
}

pub(crate) struct PtsNode {
    attr: Mutex<Attr>,
    kind: Kind,
    ptys: Weak<Ptys>,
}

impl PtsNode {
    #[allow(clippy::too_many_arguments)]
    fn new(
        ino: u64,
        file_type: FileType,
        mode: u32,
        rdev: wasixv2_vfs::Device,
        (uid, gid): (u32, u32),
        kind: Kind,
        ptys: Weak<Ptys>,
        clock: &dyn Clock,
    ) -> Self {
        let now = clock.now();
        Self {
            attr: Mutex::new(Attr {
                ino,
                file_type,
                mode,
                nlink: match file_type {
                    FileType::Directory => 2,
                    _ => 1,
                },
                uid,
                gid,
                rdev,
                size: 0,
                blocks: 0,
                blksize: 1024,
                atime: now,
                mtime: now,
                ctime: now,
                btime: None,
                dev: None,
            }),
            kind,
            ptys,
        }
    }
}

#[async_trait(?Send)]
impl Node for PtsNode {
    fn ino(&self) -> u64 {
        self.attr.lock_or_recover().ino
    }

    fn file_type(&self) -> FileType {
        self.attr.lock_or_recover().file_type
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn getattr(&self) -> FsResult<Attr> {
        Ok(self.attr.lock_or_recover().clone())
    }

    /// `simple_setattr`: `chmod` and `chown` of a slave, as `grantpt` does.
    async fn setattr(&self, set: &SetAttr) -> FsResult<Attr> {
        let now = self.ptys.upgrade().map(|ptys| ptys.clock.now());
        let mut attr = self.attr.lock_or_recover();
        let time = |t: TimeSet| match (t, now) {
            (TimeSet::At(t), _) => t,
            (TimeSet::Now, Some(now)) => now,
            (TimeSet::Now, None) => attr.ctime,
        };
        let (atime, mtime, ctime) = (
            set.atime.map(time),
            set.mtime.map(time),
            set.ctime.map(time),
        );
        if let Some(mode) = set.mode {
            attr.mode = mode;
        }
        if let Some(uid) = set.uid {
            attr.uid = uid;
        }
        if let Some(gid) = set.gid {
            attr.gid = gid;
        }
        if let Some(t) = atime {
            attr.atime = t;
        }
        if let Some(t) = mtime {
            attr.mtime = t;
        }
        if let Some(t) = ctime {
            attr.ctime = t;
        }
        Ok(attr.clone())
    }

    async fn lookup(&self, name: &[u8]) -> FsResult<Arc<dyn Node>> {
        if !matches!(self.kind, Kind::Root) {
            return Err(Errno::ENOTDIR);
        }
        let ptys = self.ptys.upgrade().ok_or(Errno::ENOENT)?;
        if name == b"ptmx" {
            return Ok(ptys.ptmx.clone());
        }
        // A slave's name is its index in decimal, without leading zeros.
        let index = std::str::from_utf8(name)
            .ok()
            .filter(|name| !name.starts_with('0') || *name == "0")
            .and_then(|name| name.parse::<u32>().ok())
            .ok_or(Errno::ENOENT)?;
        Ok(ptys.node(index).ok_or(Errno::ENOENT)?)
    }

    /// The root lists; the devices open through their drivers, never here.
    async fn open(&self, _: OpenOptions) -> FsResult<Box<dyn Handle>> {
        match self.kind {
            Kind::Root => Ok(Box::new(RootDir(self.ptys.clone()))),
            _ => Err(Errno::ENXIO),
        }
    }
}

/// The open root: `ptmx`, then the slaves by index, listed afresh on every read.
struct RootDir(Weak<Ptys>);

#[async_trait(?Send)]
impl Handle for RootDir {
    async fn readdir(&self, cookie: u64, sink: &mut dyn DirSink) -> FsResult<()> {
        let Some(ptys) = self.0.upgrade() else {
            return Ok(());
        };
        if cookie == 0 {
            let entry = DirEntry {
                name: b"ptmx",
                ino: PTMX_INO,
                file_type: FileType::CharDevice,
                next: 1,
            };
            if !sink.push(entry) {
                return Ok(());
            }
        }
        for (index, node) in ptys.nodes() {
            let pos = u64::from(index) + 1;
            if pos < cookie {
                continue;
            }
            let name = index.to_string();
            let entry = DirEntry {
                name: name.as_bytes(),
                ino: node.ino(),
                file_type: FileType::CharDevice,
                next: pos + 1,
            };
            if !sink.push(entry) {
                break;
            }
        }
        Ok(())
    }
}
