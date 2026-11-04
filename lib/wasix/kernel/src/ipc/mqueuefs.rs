//! The `mqueue` filesystem (`ipc/mqueue.c`): one instance per IPC namespace, whose root holds
//! the namespace's POSIX message queues as files.
//!
//! The kernel keeps the instance mounted outside the mount tree for `mq_open` and `mq_unlink`
//! (`mq_mnt`), and `mount -t mqueue` mounts the same instance, the mounter's IPC namespace's
//! (`get_tree_keyed`), so every mount of it and the calls see the same queues. The root is
//! sticky and world-writable (`S_ISVTX | S_IRWXUGO`), owned by whoever made the namespace.
//! Creating a regular file there makes a queue with the default attributes
//! (`mqueue_create`), removing one is `mq_unlink`, and a queue's file opens as the queue: it
//! reads as its status line and polls as `mq_open`'s descriptors do.
//!
//! The instance holds its namespace weakly (`s_fs_info`): once the namespace is gone, a mount
//! that outlives it still shows the queues that were there, but no new one can be made
//! (`EACCES`).

use std::{
    any::Any,
    collections::BTreeMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

use async_trait::async_trait;
use wasixv2_abi::RlimitResource;
use wasixv2_syscalls::{Errno, TaskId};
use wasixv2_vfs::{
    Attr, Cap, Clock, DirEntry, DirSink, FileType, Filesystem, FsInfo, FsResult, Handle, NAME_MAX,
    NewKind, NewNode, Node, OpenOptions, RenameMode, SetAttr, StatFs, TimeSet,
};

use super::mqueue::{Mqueue, Mqueues, QueueOwner};
use crate::{TaskTable, sync::MutexExt};

/// `MQUEUE_MAGIC`.
pub(crate) const MQUEUE_MAGIC: u32 = 0x1980_0202;
/// `FILENT_SIZE`: the size every queue's file shows.
const FILENT_SIZE: u64 = 80;
/// `DIRENT_SIZE`: what each name adds to the root's size, which starts at two of them.
const DIRENT_SIZE: u64 = 20;

/// An IPC namespace's `mqueue` filesystem.
pub(crate) struct MqueueFs {
    /// The namespace whose queues it holds, while it exists (`s_fs_info`).
    ns: Weak<Mqueues>,
    /// The tasks, to learn who makes a queue (`current`).
    tasks: Weak<TaskTable>,
    clock: Arc<dyn Clock>,
    next_ino: AtomicU64,
    root: Arc<RootNode>,
}

impl MqueueFs {
    /// The filesystem of the namespace `ns`, whose root `creator` owns (`mqueue_fill_super`).
    pub(crate) fn new(
        ns: &Arc<Mqueues>,
        tasks: Weak<TaskTable>,
        clock: Arc<dyn Clock>,
        creator: (u32, u32),
    ) -> Arc<Self> {
        Arc::new_cyclic(|fs: &Weak<Self>| {
            let now = clock.now();
            let root = RootNode {
                fs: fs.clone(),
                names: Mutex::default(),
                attr: Mutex::new(Attr {
                    ino: 1,
                    file_type: FileType::Directory,
                    mode: 0o1777,
                    nlink: 2,
                    uid: creator.0,
                    gid: creator.1,
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
            };
            Self {
                ns: Arc::downgrade(ns),
                tasks,
                clock,
                next_ino: AtomicU64::new(2),
                root: Arc::new(root),
            }
        })
    }

    /// The queue named `name`, if the namespace has one.
    pub(crate) fn lookup(&self, name: &[u8]) -> Option<Arc<Mqueue>> {
        let names = self.root.names.lock_or_recover();
        names.get(name).cloned()
    }

    /// The root's metadata, which `mq_open` and `mq_unlink` check as the directory's.
    pub(crate) fn root_attr(&self) -> Attr {
        self.root.attr.lock_or_recover().clone()
    }

    /// The queue `name`, and whether it is new: the one there, or with `create` one `make`
    /// makes, named while the root is held, as `do_mq_open` holds it; `ENOENT` for none.
    pub(crate) fn lookup_or_create(
        &self,
        name: &[u8],
        create: bool,
        make: impl FnOnce() -> Result<Arc<Mqueue>, Errno>,
    ) -> Result<(Arc<Mqueue>, bool), Errno> {
        let mut names = self.root.names.lock_or_recover();
        if let Some(queue) = names.get(name) {
            return Ok((queue.clone(), false));
        }
        if !create {
            return Err(Errno::ENOENT);
        }
        let queue = make()?;
        names.insert(name.to_vec(), queue.clone());
        drop(names);
        self.root.touch(&self.clock);
        Ok((queue, true))
    }

    /// Names the new queue `queue` `name`; `EEXIST` if the name is taken.
    pub(crate) fn link(&self, name: &[u8], queue: Arc<Mqueue>) -> Result<(), Errno> {
        let mut names = self.root.names.lock_or_recover();
        if names.contains_key(name) {
            return Err(Errno::EEXIST);
        }
        names.insert(name.to_vec(), queue);
        drop(names);
        self.root.touch(&self.clock);
        Ok(())
    }

    /// Removes the name `name` (`mqueue_unlink`): the queue lives on while it is open.
    pub(crate) fn unlink(&self, name: &[u8]) -> Result<Arc<Mqueue>, Errno> {
        let queue = self
            .root
            .names
            .lock_or_recover()
            .remove(name)
            .ok_or(Errno::ENOENT)?;
        self.root.touch(&self.clock);
        queue.unlinked();
        Ok(queue)
    }

    /// A new inode's metadata: a regular file owned by `owner` with the permission bits
    /// `mode` (`mqueue_get_inode`).
    pub(crate) fn new_attr(&self, owner: (u32, u32), mode: u32) -> Attr {
        let now = self.clock.now();
        Attr {
            ino: self.next_ino.fetch_add(1, Ordering::Relaxed),
            file_type: FileType::Regular,
            mode: mode & 0o7777,
            nlink: 1,
            uid: owner.0,
            gid: owner.1,
            rdev: Default::default(),
            size: FILENT_SIZE,
            blocks: 0,
            blksize: 4096,
            atime: now,
            mtime: now,
            ctime: now,
            btime: None,
            dev: None,
        }
    }

    pub(crate) fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }

    /// `mqueue_create`: a queue with the default attributes, made by the task `task` as a
    /// file of the filesystem is created, with the owner and mode the VFS decided. `EACCES`
    /// once the namespace is gone.
    fn create_queue(&self, new: &NewNode<'_>) -> Result<Arc<Mqueue>, Errno> {
        let ns = self.ns.upgrade().ok_or(Errno::EACCES)?;
        let owner = self.queue_owner(new)?;
        Mqueues::create(&ns, self, &owner, new.mode, None)
    }

    /// Who makes a queue through the filesystem: the task `new.task`, charged by its real uid
    /// against its `RLIMIT_MSGQUEUE`, as owner of what `new` says. The embedder has no limits.
    fn queue_owner(&self, new: &NewNode<'_>) -> Result<QueueOwner, Errno> {
        let owner = (new.uid, new.gid);
        if new.task == 0 {
            return Ok(QueueOwner::unlimited(owner));
        }
        let tasks = self.tasks.upgrade().ok_or(Errno::EACCES)?;
        let state = tasks.read_or_recover();
        let task = state.tasks.get(&TaskId(new.task)).ok_or(Errno::ESRCH)?;
        let cred = task.cred.clone();
        let limit = task
            .process
            .resources
            .lock_or_recover()
            .soft(RlimitResource::RLIMIT_MSGQUEUE);
        Ok(QueueOwner {
            owner,
            uid: cred.user.real.0,
            resource: cred.capable_init(Cap::SysResource),
            limit,
        })
    }
}

#[async_trait(?Send)]
impl Filesystem for MqueueFs {
    fn info(&self) -> FsInfo {
        FsInfo {
            name: "mqueue",
            magic: MQUEUE_MAGIC,
            name_max: NAME_MAX,
            read_only: false,
            dev: None,
            has_btime: false,
            // `mq_open` and `mq_unlink` change names beside the VFS.
            stable: false,
        }
    }

    fn root(&self) -> Arc<dyn Node> {
        self.root.clone()
    }

    /// `simple_statfs`.
    async fn statfs(&self) -> FsResult<StatFs> {
        Ok(StatFs {
            block_size: 4096,
            name_max: NAME_MAX as u32,
            fragment_size: 4096,
            ..StatFs::default()
        })
    }

    /// A queue opened with `mq_open` is named by its path in the filesystem.
    fn dname(&self, name: &[u8], _node: &dyn Node) -> Option<Vec<u8>> {
        Some([b"/", name].concat())
    }
}

/// The root directory, which names the queues.
struct RootNode {
    fs: Weak<MqueueFs>,
    names: Mutex<BTreeMap<Vec<u8>, Arc<Mqueue>>>,
    attr: Mutex<Attr>,
}

impl RootNode {
    fn fs(&self) -> FsResult<Arc<MqueueFs>> {
        self.fs.upgrade().ok_or(Errno::ENOENT)
    }

    /// A name came or went: the directory's times move (`simple_inode_init_ts`).
    fn touch(&self, clock: &Arc<dyn Clock>) {
        let now = clock.now();
        let mut attr = self.attr.lock_or_recover();
        attr.atime = now;
        attr.mtime = now;
        attr.ctime = now;
    }
}

/// `simple_setattr` on `attr` at `now`.
fn set_attr(attr: &mut Attr, set: &SetAttr, now: wasixv2_vfs::Timestamp) {
    let time = |t: TimeSet| match t {
        TimeSet::At(t) => t,
        TimeSet::Now => now,
    };
    if let Some(mode) = set.mode {
        attr.mode = mode & 0o7777;
    }
    if let Some(uid) = set.uid {
        attr.uid = uid;
    }
    if let Some(gid) = set.gid {
        attr.gid = gid;
    }
    if let Some(size) = set.size {
        attr.size = size;
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
}

#[async_trait(?Send)]
impl Node for RootNode {
    fn ino(&self) -> u64 {
        1
    }

    fn file_type(&self) -> FileType {
        FileType::Directory
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    /// Its size grows by `DIRENT_SIZE` with each queue it names.
    async fn getattr(&self) -> FsResult<Attr> {
        let names = self.names.lock_or_recover().len() as u64;
        Ok(Attr {
            size: (2 + names) * DIRENT_SIZE,
            ..self.attr.lock_or_recover().clone()
        })
    }

    async fn setattr(&self, set: &SetAttr) -> FsResult<Attr> {
        let now = self.fs()?.clock.now();
        let mut attr = self.attr.lock_or_recover();
        let set = SetAttr {
            size: None,
            ..set.clone()
        };
        set_attr(&mut attr, &set, now);
        Ok(attr.clone())
    }

    /// `simple_lookup`.
    async fn lookup(&self, name: &[u8]) -> FsResult<Arc<dyn Node>> {
        let queue = self.fs()?.lookup(name).ok_or(Errno::ENOENT)?;
        Ok(Arc::new(QueueNode(queue)))
    }

    /// `mqueue_create`; nothing but queues is made here (`EPERM`, as for a directory without
    /// `mkdir`, `mknod` or `symlink`).
    async fn create(&self, name: &[u8], new: &NewNode<'_>) -> FsResult<Arc<dyn Node>> {
        if !matches!(new.kind, NewKind::Regular) {
            return Err(Errno::EPERM);
        }
        let fs = self.fs()?;
        let queue = fs.create_queue(new)?;
        fs.link(name, queue.clone())?;
        Ok(Arc::new(QueueNode(queue)))
    }

    async fn link(&self, _name: &[u8], _target: &Arc<dyn Node>) -> FsResult<()> {
        Err(Errno::EPERM)
    }

    /// `mqueue_unlink`.
    async fn unlink(&self, name: &[u8]) -> FsResult<()> {
        self.fs()?.unlink(name).map(drop)
    }

    async fn rmdir(&self, _name: &[u8]) -> FsResult<()> {
        Err(Errno::EPERM)
    }

    async fn rename(
        &self,
        _name: &[u8],
        _new_dir: &Arc<dyn Node>,
        _new_name: &[u8],
        _mode: RenameMode,
    ) -> FsResult<()> {
        Err(Errno::EPERM)
    }

    async fn open(&self, _options: OpenOptions) -> FsResult<Box<dyn Handle>> {
        Ok(Box::new(RootHandle(self.fs()?)))
    }
}

/// The open root, listed afresh on every read (`dcache_readdir`).
struct RootHandle(Arc<MqueueFs>);

#[async_trait(?Send)]
impl Handle for RootHandle {
    async fn readdir(&self, cookie: u64, sink: &mut dyn DirSink) -> FsResult<()> {
        let names: Vec<(Vec<u8>, u64)> = {
            let names = self.0.root.names.lock_or_recover();
            names
                .iter()
                .map(|(name, queue)| (name.clone(), queue.ino()))
                .collect()
        };
        for (index, (name, ino)) in names.iter().enumerate().skip(cookie as usize) {
            let entry = DirEntry {
                name,
                ino: *ino,
                file_type: FileType::Regular,
                next: index as u64 + 1,
            };
            if !sink.push(entry) {
                break;
            }
        }
        Ok(())
    }
}

/// A queue's file. Opening it gives the queue itself (see [`super::mqueue::queue_of`]).
pub(crate) struct QueueNode(pub(crate) Arc<Mqueue>);

#[async_trait(?Send)]
impl Node for QueueNode {
    fn ino(&self) -> u64 {
        self.0.ino()
    }

    fn file_type(&self) -> FileType {
        FileType::Regular
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn getattr(&self) -> FsResult<Attr> {
        Ok(self.0.attr())
    }

    async fn setattr(&self, set: &SetAttr) -> FsResult<Attr> {
        Ok(self.0.set_attr(|attr, now| set_attr(attr, set, now)))
    }

    /// The kernel opens the queue in place of what this gives ([`super::mqueue::queue_of`]).
    async fn open(&self, _options: OpenOptions) -> FsResult<Box<dyn Handle>> {
        Ok(Box::new(QueueHandle))
    }
}

struct QueueHandle;

#[async_trait(?Send)]
impl Handle for QueueHandle {}
