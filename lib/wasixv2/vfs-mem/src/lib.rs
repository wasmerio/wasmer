//! In-memory filesystem driver, the default writable root (tmpfs, after Linux's `mm/shmem.c`
//! and `fs/libfs.c`).
//!
//! File data is stored sparsely in fixed-size blocks, so a write far past the end costs one block
//! and the hole reads as zeroes; allocation is tracked as ranges, so `fallocate` and hole
//! punching cost what they touch, not the length of the range. Directory cookies are insertion
//! offsets that are never reused, as tmpfs's `simple_offset` map: a reader resuming at a cookie
//! sees every entry that existed for the whole read exactly once, however the directory changes
//! in between.
//!
//! Like tmpfs, a filesystem has a size limit for data (`size=`) and one for inodes
//! (`nr_inodes=`), by default half of the memory the machine is given (see [`Limits`]). Past
//! them, writes and creation fail with `ENOSPC`.
//!
//! A filesystem given host shared memory ([`MemFs::with_shared_pages`]) keeps each regular
//! file's data in a shared memory object, as tmpfs keeps its files in shmem, so the kernel can
//! map a file's own pages into every process that maps it ([`Node::shared_pages`]).

use std::{
    any::Any,
    collections::{BTreeMap, HashMap},
    io,
    ops::Range,
    sync::{
        Arc, Mutex, MutexGuard, OnceLock, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

use async_trait::async_trait;
use wasixv2_abi::{Errno, FsInodeFlags};
use wasixv2_pages::{Provider, SharedPages};
use wasixv2_vfs::{
    AllocMode, Attr, Clock, Cred, Device, DirEntry, DirSink, FS_APPEND_FL, FS_IMMUTABLE_FL,
    FS_NOATIME_FL, FS_NODUMP_FL, FileId, FileKattr, FileType, Filesystem, FsInfo, FsResult, Handle,
    MAX_DIR_COOKIE, NAME_MAX, NewKind, NewNode, Node, OpenOptions, RenameMode, SeekHole, SetAttr,
    StatFs, TMPFS_MAGIC, TimeSet, Timestamp, XattrSet,
};

mod data;
mod quota;

pub use quota::QuotaLimits;
use quota::{Charge, Quotas};

use data::Data;

const BLOCK: u64 = 4096;
/// Largest file size, as Linux's `MAX_LFS_FILESIZE` on 64-bit.
const MAX_SIZE: u64 = i64::MAX as u64;
/// The handle type `shmem_encode_fh` gives (`FILEID_INO32_GEN`'s value, with three words).
const FILEID_SHMEM: i32 = 1;
/// The inode flags tmpfs shows, keeps, and hands to new objects (`SHMEM_FL_USER_VISIBLE`,
/// `SHMEM_FL_USER_MODIFIABLE`, `SHMEM_FL_INHERITED`); case folding is not offered.
const SHMEM_FL_USER_VISIBLE: u32 = FsInodeFlags::FS_FL_USER_VISIBLE.0;
const SHMEM_FL_USER_MODIFIABLE: u32 = FS_IMMUTABLE_FL | FS_APPEND_FL | FS_NODUMP_FL | FS_NOATIME_FL;
const SHMEM_FL_INHERITED: u32 = FS_NODUMP_FL | FS_NOATIME_FL;
/// tmpfs's `BOGO_DIRENT_SIZE`: what each entry adds to a directory's size.
const DIRENT_SIZE: u64 = 20;
/// tmpfs's `BOGO_INODE_SIZE`: the inode space an inode or an extra link takes.
const INODE_SIZE: u64 = 1024;
/// What an extended attribute takes besides its name and value (`simple_xattr_space`).
const XATTR_OVERHEAD: u64 = 40;
/// relatime: refresh `atime` at least this often even when it is newer than `mtime`.
const RELATIME_SECS: i64 = 24 * 60 * 60;
/// Directory entries `readdir` copies out per hold of the directory's lock.
const READDIR_BATCH: usize = 64;

/// Size limits of a memory filesystem (tmpfs's `size=` and `nr_inodes=`). `None`, or 0 as for
/// tmpfs, is unlimited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Bytes of file data, in whole blocks.
    pub max_bytes: Option<u64>,
    /// Inodes. As tmpfs's `free_ispace`, each extra hard link counts as one too, and extended
    /// attributes take from the same space, 1 KiB per inode.
    pub max_inodes: Option<u64>,
}

impl Limits {
    pub const UNLIMITED: Self = Self {
        max_bytes: None,
        max_inodes: None,
    };

    /// The memory a machine has when the embedder does not say: the 4 GiB a wasm32 guest can
    /// address, which is the total `sysinfo` reports.
    pub const DEFAULT_MEMORY: u64 = 4 << 30;

    /// tmpfs's defaults for a machine with `memory` bytes: half of it for data, and an inode
    /// per two pages of it (`shmem_default_max_blocks`, `shmem_default_max_inodes`).
    pub fn for_memory(memory: u64) -> Self {
        Self {
            max_bytes: Some(memory / 2 / BLOCK * BLOCK),
            max_inodes: Some(memory / BLOCK / 2),
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self::for_memory(Self::DEFAULT_MEMORY)
    }
}

/// A memory filesystem.
pub struct MemFs {
    shared: Arc<Shared>,
    root: Arc<MemNode>,
}

struct Shared {
    clock: Arc<dyn Clock>,
    next_ino: AtomicU64,
    space: Space,
    quotas: Quotas,
    /// Makes the shared memory objects regular files keep their data in, if they do.
    pages: OnceLock<Arc<dyn Provider>>,
    /// The root's permission bits and owner as the filesystem was made with them
    /// (`shmem_sb_info`'s `mode`, `uid` and `gid`), which its options show.
    root_owner: (u32, u32, u32),
    /// Every object that exists, by inode number, for file handles (the inode hash
    /// `shmem_fh_to_dentry` looks in). Numbers are never reused, so the generation is 0.
    nodes: Mutex<HashMap<u64, Weak<MemNode>>>,
}

impl Shared {
    fn now(&self) -> Timestamp {
        self.clock.now()
    }
}

/// The space a filesystem has used and may use (`shmem_sb_info`'s `used_blocks` and
/// `free_ispace`).
pub(crate) struct Space {
    /// The block limit; 0 for none. A remount changes it.
    max_blocks: AtomicU64,
    blocks: AtomicU64,
    /// The inode limit; 0 for none.
    max_inodes: AtomicU64,
    /// Inode space used, in bytes: [`INODE_SIZE`] per inode and extra link, plus xattrs.
    ispace: AtomicU64,
}

impl Space {
    fn new(limits: Limits) -> Self {
        Self {
            max_blocks: AtomicU64::new(limits.max_bytes.map_or(0, |bytes| bytes / BLOCK)),
            blocks: AtomicU64::new(0),
            max_inodes: AtomicU64::new(limits.max_inodes.unwrap_or(0)),
            ispace: AtomicU64::new(0),
        }
    }

    fn max_blocks(&self) -> Option<u64> {
        Some(self.max_blocks.load(Ordering::Relaxed)).filter(|&n| n > 0)
    }

    fn max_inodes(&self) -> Option<u64> {
        Some(self.max_inodes.load(Ordering::Relaxed)).filter(|&n| n > 0)
    }

    /// Takes `n` from `used` unless that would pass `max`.
    fn charge(used: &AtomicU64, max: Option<u64>, n: u64) -> FsResult<()> {
        used.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |now| {
            let next = now.checked_add(n)?;
            max.is_none_or(|max| next <= max).then_some(next)
        })
        .map(drop)
        .map_err(|_| Errno::ENOSPC)
    }

    pub(crate) fn charge_blocks(&self, n: u64) -> FsResult<()> {
        Self::charge(&self.blocks, self.max_blocks(), n)
    }

    pub(crate) fn release_blocks(&self, n: u64) {
        self.blocks.fetch_sub(n, Ordering::Relaxed);
    }

    fn max_ispace(&self) -> Option<u64> {
        self.max_inodes().map(|n| n.saturating_mul(INODE_SIZE))
    }

    fn charge_ispace(&self, bytes: u64) -> FsResult<()> {
        Self::charge(&self.ispace, self.max_ispace(), bytes)
    }

    fn release_ispace(&self, bytes: u64) {
        self.ispace.fetch_sub(bytes, Ordering::Relaxed);
    }

    /// `shmem_reserve_inode`: room for an inode, or for another link to one.
    fn reserve_inode(&self) -> FsResult<()> {
        self.charge_ispace(INODE_SIZE)
    }

    fn free_inode(&self) {
        self.release_ispace(INODE_SIZE);
    }
}

impl MemFs {
    /// An empty filesystem whose root is owned by `uid`/`gid` with permission bits `mode`, with
    /// the default [`Limits`].
    pub fn new(clock: Arc<dyn Clock>, mode: u32, uid: u32, gid: u32) -> Self {
        Self::with_limits(clock, mode, uid, gid, Limits::default())
    }

    /// Like [`MemFs::new`], with the given size limits.
    pub fn with_limits(
        clock: Arc<dyn Clock>,
        mode: u32,
        uid: u32,
        gid: u32,
        limits: Limits,
    ) -> Self {
        Self::with_limits_and_quotas(clock, mode, uid, gid, limits, QuotaLimits::default())
            .expect("an empty filesystem has room for its root")
    }

    /// A tmpfs with quota accounting enabled at mount time (`shmem_fill_super`).
    pub fn with_limits_and_quotas(
        clock: Arc<dyn Clock>,
        mode: u32,
        uid: u32,
        gid: u32,
        limits: Limits,
        quotas: QuotaLimits,
    ) -> FsResult<Self> {
        let shared = Arc::new(Shared {
            clock,
            next_ino: AtomicU64::new(1),
            space: Space::new(limits),
            quotas: Quotas::new(quotas),
            pages: OnceLock::new(),
            root_owner: (mode & 0o7777, uid, gid),
            nodes: Mutex::default(),
        });
        let root = NewNode {
            umask: 0,
            kind: NewKind::Directory,
            mode,
            uid,
            gid,
            task: 0,
            quota_exempt: false,
        };
        // The root's `..` is itself.
        let root = MemNode::new(&shared, &root)?;
        root.meta().nlink = 2;
        Ok(Self { shared, root })
    }

    /// `shmem_reconfigure`: quotas cannot be enabled or their defaults changed on remount.
    pub fn check_quota_remount(&self, options: QuotaLimits) -> FsResult<()> {
        let old = self.shared.quotas.defaults;
        if options.types != 0 && old.types == 0
            || (0..2).any(|i| {
                options.block_hard[i] != 0 && options.block_hard[i] != old.block_hard[i]
                    || options.inode_hard[i] != 0 && options.inode_hard[i] != old.inode_hard[i]
            })
        {
            return Err(Errno::EINVAL);
        }
        Ok(())
    }

    /// Changes the limits a remount names (`shmem_reconfigure`): `max_bytes` and `max_inodes`
    /// where they are `Some`, 0 lifting the limit. A filesystem without a limit cannot be given
    /// one, nor one below what it uses (`EINVAL`). Charges taken meanwhile may pass the new
    /// limit once, as the checks and the change are not atomic together.
    pub fn set_limits(&self, limits: Limits) -> FsResult<()> {
        let space = &self.shared.space;
        let blocks = limits.max_bytes.map(|bytes| bytes.div_ceil(BLOCK));
        if let Some(blocks) = blocks.filter(|&n| n > 0)
            && (space.max_blocks().is_none() || space.blocks.load(Ordering::Relaxed) > blocks)
        {
            return Err(Errno::EINVAL);
        }
        if let Some(inodes) = limits.max_inodes.filter(|&n| n > 0)
            && (space.max_inodes().is_none()
                || space.ispace.load(Ordering::Relaxed) > inodes.saturating_mul(INODE_SIZE))
        {
            return Err(Errno::EINVAL);
        }
        if let Some(blocks) = blocks {
            space.max_blocks.store(blocks, Ordering::Relaxed);
        }
        if let Some(inodes) = limits.max_inodes {
            space.max_inodes.store(inodes, Ordering::Relaxed);
        }
        Ok(())
    }

    /// Keeps the data of regular files created from now on in shared memory objects from
    /// `provider`, which `MAP_SHARED` mappings show directly, as the memfd and `/dev/shm`
    /// tmpfs of a Linux system keep theirs in shmem. A file whose object cannot be made keeps
    /// its data on the heap, and its mappings are copies. Only the first provider counts.
    pub fn with_shared_pages(self, provider: Arc<dyn Provider>) -> Self {
        let _ = self.shared.pages.set(provider);
        self
    }
}

#[async_trait(?Send)]
impl Filesystem for MemFs {
    fn limits_file_size(&self) -> bool {
        true
    }

    fn quota(&self) -> Option<&dyn wasixv2_vfs::QuotaOps> {
        (self.shared.quotas.defaults.types != 0).then_some(self)
    }

    fn info(&self) -> FsInfo {
        FsInfo {
            name: "tmpfs",
            magic: TMPFS_MAGIC,
            name_max: NAME_MAX,
            read_only: false,
            dev: None,
            has_btime: true,
            stable: true,
        }
    }

    fn root(&self) -> Arc<dyn Node> {
        self.root.clone()
    }

    /// `shmem_encode_fh`: the generation, then the inode number's low and high words.
    fn encode_fh(&self, node: &dyn Node) -> Option<FileId> {
        let mut data = node.generation().to_le_bytes().to_vec();
        data.extend_from_slice(&(node.ino() as u32).to_le_bytes());
        data.extend_from_slice(&((node.ino() >> 32) as u32).to_le_bytes());
        Some(FileId {
            kind: FILEID_SHMEM,
            data,
        })
    }

    /// `shmem_fh_to_dentry`: the object while it exists, linked or held open.
    async fn fh_to_node(&self, fid: &FileId) -> FsResult<Arc<dyn Node>> {
        let word = |i: usize| -> Option<u32> {
            let bytes = fid.data.get(i * 4..i * 4 + 4)?;
            Some(u32::from_le_bytes(bytes.try_into().unwrap()))
        };
        let (Some(generation), Some(low), Some(high)) = (word(0), word(1), word(2)) else {
            return Err(Errno::ESTALE);
        };
        let ino = u64::from(high) << 32 | u64::from(low);
        let node = self
            .shared
            .nodes
            .lock()
            .unwrap()
            .get(&ino)
            .and_then(Weak::upgrade);
        match node {
            Some(node) if node.generation() == generation => Ok(node),
            _ => Err(Errno::ESTALE),
        }
    }

    /// As `shmem_statfs`: an unlimited size or inode count reports 0 for it.
    async fn statfs(&self) -> FsResult<StatFs> {
        let space = &self.shared.space;
        let mut stat = StatFs {
            block_size: BLOCK as u32,
            name_max: NAME_MAX as u32,
            fragment_size: BLOCK as u32,
            ..StatFs::default()
        };
        if let Some(max) = space.max_blocks() {
            let free = max.saturating_sub(space.blocks.load(Ordering::Relaxed));
            (stat.blocks, stat.blocks_free, stat.blocks_avail) = (max, free, free);
        }
        if let (Some(max), Some(max_ispace)) = (space.max_inodes(), space.max_ispace()) {
            let free = max_ispace.saturating_sub(space.ispace.load(Ordering::Relaxed));
            (stat.files, stat.files_free) = (max, free / INODE_SIZE);
        }
        Ok(stat)
    }

    /// `shmem_show_options`: the size and inode limits that are not the defaults, and the
    /// root's mode and owner that are not `1777` and root's.
    fn show_options(&self, out: &mut Vec<u8>) {
        let space = &self.shared.space;
        let defaults = Limits::default();
        let max_bytes = space.max_blocks.load(Ordering::Relaxed) * BLOCK;
        if Some(max_bytes) != defaults.max_bytes {
            out.extend_from_slice(format!(",size={}k", max_bytes / 1024).as_bytes());
        }
        let max_inodes = space.max_inodes.load(Ordering::Relaxed);
        if Some(max_inodes) != defaults.max_inodes {
            out.extend_from_slice(format!(",nr_inodes={max_inodes}").as_bytes());
        }
        let quota = self.shared.quotas.defaults;
        for (i, name) in ["usrquota", "grpquota"].into_iter().enumerate() {
            if quota.types & (1 << i) != 0 {
                out.extend_from_slice(format!(",{name}").as_bytes());
            }
            if quota.block_hard[i] != 0 {
                out.extend_from_slice(
                    format!(",{name}_block_hardlimit={}", quota.block_hard[i]).as_bytes(),
                );
            }
            if quota.inode_hard[i] != 0 {
                out.extend_from_slice(
                    format!(",{name}_inode_hardlimit={}", quota.inode_hard[i]).as_bytes(),
                );
            }
        }
        let (mode, uid, gid) = self.shared.root_owner;
        if mode != 0o1777 {
            out.extend_from_slice(format!(",mode={mode:03o}").as_bytes());
        }
        if uid != 0 {
            out.extend_from_slice(format!(",uid={uid}").as_bytes());
        }
        if gid != 0 {
            out.extend_from_slice(format!(",gid={gid}").as_bytes());
        }
    }
}

/// One object: file, directory, symlink or special file.
pub struct MemNode {
    ino: u64,
    file_type: FileType,
    this: Weak<MemNode>,
    shared: Arc<Shared>,
    meta: Mutex<Meta>,
    /// Serializes allocation with quota ownership transfer; always taken before data/meta.
    quota_owner: Mutex<(u32, u32)>,
    content: Content,
}

struct Meta {
    mode: u32,
    uid: u32,
    gid: u32,
    nlink: u32,
    rdev: Device,
    atime: Timestamp,
    mtime: Timestamp,
    ctime: Timestamp,
    btime: Timestamp,
    xattrs: BTreeMap<Vec<u8>, Vec<u8>>,
    /// The inode flags (`shmem_inode_info`'s `fsflags`).
    fsflags: u32,
}

enum Content {
    File(Mutex<Data>),
    Dir(Mutex<Dir>),
    Symlink(Vec<u8>),
    Special,
}

#[derive(Default)]
struct Dir {
    by_name: HashMap<Vec<u8>, u64>,
    by_cookie: BTreeMap<u64, (Vec<u8>, Arc<MemNode>)>,
    next_cookie: u64,
}

impl Dir {
    fn get(&self, name: &[u8]) -> Option<&Arc<MemNode>> {
        let cookie = self.by_name.get(name)?;
        Some(&self.by_cookie[cookie].1)
    }

    fn insert(&mut self, name: &[u8], node: Arc<MemNode>) -> FsResult<()> {
        let cookie = self.next_cookie.max(1);
        if cookie > MAX_DIR_COOKIE {
            return Err(Errno::ENOSPC);
        }
        self.next_cookie = cookie + 1;
        self.by_name.insert(name.to_vec(), cookie);
        self.by_cookie.insert(cookie, (name.to_vec(), node));
        Ok(())
    }

    /// Replaces the object under an existing name, keeping its cookie, as tmpfs keeps the
    /// replaced entry's offset on rename.
    fn replace(&mut self, name: &[u8], node: Arc<MemNode>) -> Arc<MemNode> {
        let cookie = self.by_name[name];
        std::mem::replace(&mut self.by_cookie.get_mut(&cookie).unwrap().1, node)
    }

    fn remove(&mut self, name: &[u8]) -> Option<Arc<MemNode>> {
        let cookie = self.by_name.remove(name)?;
        self.by_cookie.remove(&cookie).map(|(_, node)| node)
    }

    fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }
}

impl MemNode {
    /// A new object, charged to the filesystem's inode space.
    fn new(shared: &Arc<Shared>, new: &NewNode<'_>) -> FsResult<Arc<Self>> {
        shared.space.reserve_inode()?;
        if let Err(err) = shared.quotas.charge(
            (new.uid, new.gid),
            0,
            1,
            shared.now().sec.max(0) as u64,
            new.quota_exempt,
        ) {
            shared.space.free_inode();
            return Err(err);
        }
        let now = shared.now();
        let (content, rdev) = match new.kind {
            NewKind::Regular => {
                let data = Data::new(shared.pages.get().cloned());
                (Content::File(Mutex::new(data)), Device::default())
            }
            NewKind::Directory => (Content::Dir(Mutex::default()), Device::default()),
            NewKind::Symlink(target) => (Content::Symlink(target.to_vec()), Device::default()),
            NewKind::CharDevice(dev) | NewKind::BlockDevice(dev) => (Content::Special, dev),
            NewKind::Fifo | NewKind::Socket => (Content::Special, Device::default()),
        };
        let ino = shared.next_ino.fetch_add(1, Ordering::Relaxed);
        let node = Arc::new_cyclic(|this| Self {
            ino,
            file_type: new.kind.file_type(),
            this: this.clone(),
            shared: shared.clone(),
            quota_owner: Mutex::new((new.uid, new.gid)),
            meta: Mutex::new(Meta {
                mode: new.mode,
                uid: new.uid,
                gid: new.gid,
                nlink: 1,
                rdev,
                atime: now,
                mtime: now,
                ctime: now,
                btime: now,
                xattrs: BTreeMap::new(),
                fsflags: 0,
            }),
            content,
        });
        let weak = Arc::downgrade(&node);
        shared.nodes.lock().unwrap().insert(ino, weak);
        Ok(node)
    }

    /// `__shmem_get_inode`: a new object takes the flags of its directory that are inherited
    /// (`SHMEM_FL_INHERITED`).
    fn inherit_flags(&self, dir: &MemNode) {
        let inherited = dir.meta().fsflags & SHMEM_FL_INHERITED;
        self.meta().fsflags = inherited;
    }

    fn arc(&self) -> Arc<MemNode> {
        self.this.upgrade().expect("a node in use is alive")
    }

    fn meta(&self) -> MutexGuard<'_, Meta> {
        self.meta.lock().unwrap()
    }

    fn dir(&self) -> FsResult<MutexGuard<'_, Dir>> {
        match &self.content {
            Content::Dir(dir) => Ok(dir.lock().unwrap()),
            _ => Err(Errno::ENOTDIR),
        }
    }

    fn data(&self) -> FsResult<MutexGuard<'_, Data>> {
        match &self.content {
            Content::File(blocks) => Ok(blocks.lock().unwrap()),
            Content::Dir(_) => Err(Errno::EISDIR),
            _ => Err(Errno::EINVAL),
        }
    }

    /// Stamps `mtime` and `ctime` with the current time.
    fn touch(&self) {
        let now = self.shared.now();
        let mut meta = self.meta();
        meta.mtime = now;
        meta.ctime = now;
    }

    fn touch_ctime(&self) {
        let now = self.shared.now();
        self.meta().ctime = now;
    }

    /// `touch_atime` with relatime.
    fn accessed(&self) {
        let now = self.shared.now();
        let mut meta = self.meta();
        // `touch_atime`: `S_NOATIME`.
        if meta.fsflags & FS_NOATIME_FL != 0 {
            return;
        }
        if meta.atime <= meta.mtime
            || meta.atime <= meta.ctime
            || now.sec - meta.atime.sec >= RELATIME_SECS
        {
            meta.atime = now;
        }
    }

    /// One name of a non-directory went away. A link beyond the last gives its inode space
    /// back (`shmem_unlink`); the last one's goes with the inode.
    fn drop_link(&self) {
        let mut meta = self.meta();
        if meta.nlink > 1 {
            self.shared.space.free_inode();
        }
        meta.nlink = meta.nlink.saturating_sub(1);
        meta.ctime = self.shared.now();
    }

    fn downcast(node: &Arc<dyn Node>) -> FsResult<Arc<MemNode>> {
        node.as_any()
            .downcast_ref::<MemNode>()
            .map(MemNode::arc)
            .ok_or(Errno::EXDEV)
    }

    fn size(&self) -> u64 {
        match &self.content {
            Content::File(data) => data.lock().unwrap().size,
            Content::Dir(dir) => dir.lock().unwrap().by_name.len() as u64 * DIRENT_SIZE,
            Content::Symlink(target) => target.len() as u64,
            Content::Special => 0,
        }
    }

    fn allocated(&self) -> u64 {
        match &self.content {
            Content::File(data) => data.lock().unwrap().allocated(),
            _ => 0,
        }
    }

    /// Blocks of memory the object holds, for `st_blocks`.
    fn blocks(&self) -> u64 {
        match &self.content {
            Content::File(data) => data.lock().unwrap().blocks(),
            _ => 0,
        }
    }
}

impl Drop for MemNode {
    fn drop(&mut self) {
        self.shared.nodes.lock().unwrap().remove(&self.ino);
        let space = &self.shared.space;
        let owner = *self.quota_owner.lock().unwrap();
        self.shared
            .quotas
            .release(owner, self.allocated() * BLOCK, 1);
        space.release_blocks(self.allocated());
        let xattrs: u64 = self.meta().xattrs.iter().map(xattr_space).sum();
        space.release_ispace(xattrs);
        space.free_inode();
    }
}

/// The inode space an extended attribute takes (`simple_xattr_space`).
fn xattr_space((name, value): (&Vec<u8>, &Vec<u8>)) -> u64 {
    XATTR_OVERHEAD + name.len() as u64 + value.len() as u64
}

/// Locks two directories in inode order, so concurrent renames cannot deadlock.
fn lock_two<'a>(
    a: &'a MemNode,
    b: &'a MemNode,
) -> FsResult<(MutexGuard<'a, Dir>, MutexGuard<'a, Dir>)> {
    if a.ino < b.ino {
        let first = a.dir()?;
        Ok((first, b.dir()?))
    } else {
        let second = b.dir()?;
        Ok((a.dir()?, second))
    }
}

#[async_trait(?Send)]
impl Node for MemNode {
    fn ino(&self) -> u64 {
        self.ino
    }

    fn file_type(&self) -> FileType {
        self.file_type
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn inode_flags(&self) -> u32 {
        self.meta().fsflags
    }

    /// `shmem_fileattr_get`.
    async fn fileattr_get(&self) -> FsResult<FileKattr> {
        Ok(FileKattr::from_flags(
            self.meta().fsflags & SHMEM_FL_USER_VISIBLE,
        ))
    }

    /// `shmem_fileattr_set`: only the flags tmpfs keeps.
    async fn fileattr_set(&self, fa: &FileKattr) -> FsResult<()> {
        if fa.has_fsx() || fa.flags & !SHMEM_FL_USER_MODIFIABLE != 0 {
            return Err(Errno::EOPNOTSUPP);
        }
        let now = self.shared.now();
        let mut meta = self.meta();
        meta.fsflags =
            (meta.fsflags & !SHMEM_FL_USER_MODIFIABLE) | (fa.flags & SHMEM_FL_USER_MODIFIABLE);
        meta.ctime = now;
        Ok(())
    }

    async fn getattr(&self) -> FsResult<Attr> {
        let size = self.size();
        let blocks = self.blocks() * (BLOCK / 512);
        let meta = self.meta();
        Ok(Attr {
            ino: self.ino,
            file_type: self.file_type,
            mode: meta.mode,
            nlink: meta.nlink,
            uid: meta.uid,
            gid: meta.gid,
            rdev: meta.rdev,
            size,
            blocks,
            blksize: BLOCK as u32,
            atime: meta.atime,
            mtime: meta.mtime,
            ctime: meta.ctime,
            btime: Some(meta.btime),
            dev: None,
        })
    }

    async fn setattr(&self, attr: &SetAttr) -> FsResult<Attr> {
        // Linux fs/attr.c:notify_change samples current_time once for the update.
        let now = self.shared.now();
        let time = |set| match set {
            TimeSet::Now => now,
            TimeSet::At(time) => time,
        };
        {
            let mut owner = self.quota_owner.lock().unwrap();
            let charge = Charge {
                shared: &self.shared,
                owner: *owner,
                exempt: attr.quota_exempt,
            };
            if let Some(size) = attr.size {
                if size > MAX_SIZE {
                    return Err(Errno::EFBIG);
                }
                self.data()?.truncate(&charge, size)?;
            }
            {
                let next = (attr.uid.unwrap_or(owner.0), attr.gid.unwrap_or(owner.1));
                self.shared.quotas.transfer(
                    [*owner, next],
                    self.allocated() * BLOCK,
                    now.sec.max(0) as u64,
                    attr.quota_exempt,
                )?;
                *owner = next;
                let mut meta = self.meta();
                if let Some(mode) = attr.mode {
                    meta.mode = mode;
                }
                if let Some(uid) = attr.uid {
                    meta.uid = uid;
                }
                if let Some(gid) = attr.gid {
                    meta.gid = gid;
                }
                if let Some(set) = attr.atime {
                    meta.atime = time(set);
                }
                if let Some(set) = attr.mtime {
                    meta.mtime = time(set);
                }
                if let Some(set) = attr.ctime {
                    meta.ctime = time(set);
                }
            }
        }
        self.getattr().await
    }

    async fn lookup(&self, name: &[u8]) -> FsResult<Arc<dyn Node>> {
        let dir = self.dir()?;
        let node: Arc<dyn Node> = dir.get(name).ok_or(Errno::ENOENT)?.clone();
        Ok(node)
    }

    async fn create(&self, name: &[u8], new: &NewNode<'_>) -> FsResult<Arc<dyn Node>> {
        let mut dir = self.dir()?;
        if dir.by_name.contains_key(name) {
            return Err(Errno::EEXIST);
        }
        let node = MemNode::new(&self.shared, new)?;
        node.inherit_flags(self);
        dir.insert(name, node.clone())?;
        drop(dir);
        if new.kind == NewKind::Directory {
            node.meta().nlink = 2;
            self.meta().nlink += 1;
        }
        self.touch();
        Ok(node)
    }

    async fn create_unnamed(
        &self,
        new: &NewNode<'_>,
        options: OpenOptions,
    ) -> FsResult<(Arc<dyn Node>, Box<dyn Handle>)> {
        drop(self.dir()?);
        let node = MemNode::new(&self.shared, new)?;
        node.inherit_flags(self);
        node.meta().nlink = 0;
        let handle = node.open(options).await?;
        Ok((node, handle))
    }

    async fn link(&self, name: &[u8], target: &Arc<dyn Node>) -> FsResult<()> {
        let target = Self::downcast(target)?;
        let mut dir = self.dir()?;
        if dir.by_name.contains_key(name) {
            return Err(Errno::EEXIST);
        }
        // `shmem_link`: a link to a named file takes inode space; naming an O_TMPFILE file
        // uses what its inode already has.
        let named = target.meta().nlink > 0;
        if named {
            self.shared.space.reserve_inode()?;
        }
        if let Err(errno) = dir.insert(name, target.clone()) {
            if named {
                self.shared.space.free_inode();
            }
            return Err(errno);
        }
        drop(dir);
        target.meta().nlink += 1;
        target.touch_ctime();
        self.touch();
        Ok(())
    }

    async fn unlink(&self, name: &[u8]) -> FsResult<()> {
        let mut dir = self.dir()?;
        match dir.get(name) {
            None => return Err(Errno::ENOENT),
            Some(node) if node.file_type == FileType::Directory => return Err(Errno::EISDIR),
            Some(_) => {}
        }
        let node = dir.remove(name).expect("checked above");
        drop(dir);
        node.drop_link();
        self.touch();
        Ok(())
    }

    async fn rmdir(&self, name: &[u8]) -> FsResult<()> {
        let mut dir = self.dir()?;
        let node = dir.get(name).ok_or(Errno::ENOENT)?;
        if !node.dir()?.is_empty() {
            return Err(Errno::ENOTEMPTY);
        }
        let node = dir.remove(name).expect("checked above");
        drop(dir);
        node.meta().nlink = 0;
        node.touch_ctime();
        self.meta().nlink -= 1;
        self.touch();
        Ok(())
    }

    async fn rename(
        &self,
        name: &[u8],
        new_dir: &Arc<dyn Node>,
        new_name: &[u8],
        mode: RenameMode,
    ) -> FsResult<()> {
        if mode == RenameMode::Whiteout {
            return Err(Errno::EINVAL);
        }
        let new_parent = Self::downcast(new_dir)?;
        let source = self.dir()?.get(name).cloned().ok_or(Errno::ENOENT)?;
        let same_dir = new_parent.ino == self.ino;
        self.rename_locked(&source, name, &new_parent, new_name, mode, same_dir)
    }

    async fn readlink(&self) -> FsResult<Vec<u8>> {
        match &self.content {
            Content::Symlink(target) => {
                self.accessed();
                Ok(target.clone())
            }
            _ => Err(Errno::EINVAL),
        }
    }

    async fn open(&self, options: OpenOptions) -> FsResult<Box<dyn Handle>> {
        match &self.content {
            Content::File(_) | Content::Dir(_) => Ok(Box::new(MemHandle {
                node: self.arc(),
                noatime: options.noatime,
            })),
            _ => Err(Errno::ENXIO),
        }
    }

    fn shared_pages(
        &self,
        range: Range<u64>,
        cred: &Cred,
    ) -> FsResult<Option<Arc<dyn SharedPages>>> {
        if !matches!(self.content, Content::File(_)) {
            return Ok(None);
        }
        let owner = self.quota_owner.lock().unwrap();
        let charge = Charge {
            shared: &self.shared,
            owner: *owner,
            exempt: cred.capable_init(wasixv2_vfs::Cap::SysResource),
        };
        let range = Arc::new(range);
        let pages = self.data()?.shared_pages(&charge, &range)?;
        Ok(pages.map(|pages| {
            Arc::new(MappedPages {
                pages,
                range,
                _node: self.arc(),
            }) as Arc<dyn SharedPages>
        }))
    }

    async fn getxattr(&self, name: &[u8]) -> FsResult<Vec<u8>> {
        self.meta().xattrs.get(name).cloned().ok_or(Errno::ENODATA)
    }

    async fn setxattr(&self, name: &[u8], value: &[u8], how: XattrSet) -> FsResult<()> {
        let now = self.shared.now();
        let mut meta = self.meta();
        let exists = meta.xattrs.contains_key(name);
        match how {
            XattrSet::Create if exists => return Err(Errno::EEXIST),
            XattrSet::Replace if !exists => return Err(Errno::ENODATA),
            _ => {}
        }
        let (name, value) = (name.to_vec(), value.to_vec());
        let space = &self.shared.space;
        space.charge_ispace(xattr_space((&name, &value)))?;
        if let Some(old) = meta.xattrs.insert(name.clone(), value) {
            space.release_ispace(xattr_space((&name, &old)));
        }
        meta.ctime = now;
        Ok(())
    }

    async fn listxattr(&self) -> FsResult<Vec<Vec<u8>>> {
        Ok(self.meta().xattrs.keys().cloned().collect())
    }

    async fn removexattr(&self, name: &[u8]) -> FsResult<()> {
        let now = self.shared.now();
        let mut meta = self.meta();
        let (name, value) = meta.xattrs.remove_entry(name).ok_or(Errno::ENODATA)?;
        self.shared
            .space
            .release_ispace(xattr_space((&name, &value)));
        meta.ctime = now;
        Ok(())
    }
}

impl MemNode {
    /// The body of `rename` (`shmem_rename2`), with both directories locked in inode order.
    fn rename_locked(
        &self,
        source: &Arc<MemNode>,
        name: &[u8],
        new_parent: &Arc<MemNode>,
        new_name: &[u8],
        mode: RenameMode,
        same_dir: bool,
    ) -> FsResult<()> {
        let (mut old_guard, mut new_guard) = if same_dir {
            (self.dir()?, None)
        } else {
            let (a, b) = lock_two(self, new_parent)?;
            (a, Some(b))
        };
        let old: &mut Dir = &mut old_guard;
        let new: Option<&mut Dir> = new_guard.as_deref_mut();
        // Re-check under the locks: the source must still be the object the VFS saw.
        if !old.get(name).is_some_and(|n| Arc::ptr_eq(n, source)) {
            return Err(Errno::ENOENT);
        }
        let source_is_dir = source.file_type == FileType::Directory;
        let target = match &new {
            Some(new) => new.get(new_name).cloned(),
            None => old.get(new_name).cloned(),
        };
        let mut dir_links = (0i32, 0i32); // (old parent, new parent) nlink changes
        match (mode, &target) {
            (RenameMode::Exchange, None) => return Err(Errno::ENOENT),
            (RenameMode::NoReplace, Some(_)) => return Err(Errno::EEXIST),
            (RenameMode::Exchange, Some(target)) => {
                let target_is_dir = target.file_type == FileType::Directory;
                match new {
                    Some(new) => {
                        new.replace(new_name, source.clone());
                        old.replace(name, target.clone());
                        if source_is_dir != target_is_dir {
                            let delta = if source_is_dir { 1 } else { -1 };
                            dir_links = (-delta, delta);
                        }
                    }
                    None => {
                        old.replace(new_name, source.clone());
                        old.replace(name, target.clone());
                    }
                }
                target.touch_ctime();
            }
            (_, target) => {
                if let Some(target) = target {
                    if Arc::ptr_eq(target, source) {
                        return Ok(());
                    }
                    let target_is_dir = target.file_type == FileType::Directory;
                    if target_is_dir && !target.dir()?.is_empty() {
                        return Err(Errno::ENOTEMPTY);
                    }
                    old.remove(name);
                    match new {
                        Some(new) => {
                            new.replace(new_name, source.clone());
                        }
                        None => {
                            old.replace(new_name, source.clone());
                        }
                    }
                    if target_is_dir {
                        target.meta().nlink = 0;
                        dir_links.1 -= 1;
                    } else {
                        target.drop_link();
                    }
                } else {
                    old.remove(name);
                    match new {
                        Some(new) => new.insert(new_name, source.clone())?,
                        None => old.insert(new_name, source.clone())?,
                    }
                }
                if source_is_dir {
                    dir_links.0 -= 1;
                    dir_links.1 += 1;
                }
            }
        }
        drop(old_guard);
        drop(new_guard);
        let adjust = |node: &MemNode, delta: i32| {
            let mut meta = node.meta();
            meta.nlink = (meta.nlink as i32 + delta) as u32;
        };
        if same_dir {
            adjust(self, dir_links.0 + dir_links.1);
        } else {
            adjust(self, dir_links.0);
            adjust(new_parent, dir_links.1);
        }
        source.touch_ctime();
        self.touch();
        if !same_dir {
            new_parent.touch();
        }
        Ok(())
    }
}

/// Keeps a materialized range charged while any mapping can allocate its host pages.
struct MappedPages {
    pages: Arc<dyn SharedPages>,
    range: Arc<Range<u64>>,
    _node: Arc<MemNode>,
}

impl SharedPages for MappedPages {
    fn id(&self) -> u64 {
        self.pages.id()
    }
    fn size(&self) -> u64 {
        self.range.end
    }
    fn resize(&self, _size: u64) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<()> {
        self.pages.read_at(buf, offset)
    }
    fn write_at(&self, data: &[u8], offset: u64) -> io::Result<()> {
        self.pages.write_at(data, offset)
    }
    fn punch_hole(&self, offset: u64, len: u64) -> io::Result<()> {
        self.pages.punch_hole(offset, len)
    }
    fn allocated(&self) -> u64 {
        self.pages.allocated()
    }
    fn seek(&self, offset: u64, which: wasixv2_pages::Seek) -> io::Result<Option<u64>> {
        self.pages.seek(offset, which)
    }
    #[cfg(unix)]
    fn fd(&self) -> Option<std::os::fd::BorrowedFd<'_>> {
        self.pages.fd()
    }
}

/// An open file or directory.
struct MemHandle {
    node: Arc<MemNode>,
    noatime: bool,
}

impl MemHandle {
    fn read_done(&self) {
        if !self.noatime {
            self.node.accessed();
        }
    }
}

impl MemHandle {
    fn write(&self, data: &[u8], offset: Option<u64>, exempt: bool) -> FsResult<(u64, usize)> {
        let owner = self.node.quota_owner.lock().unwrap();
        let charge = Charge {
            shared: &self.node.shared,
            owner: *owner,
            exempt,
        };
        let result = {
            let mut file = self.node.data()?;
            let at = offset.unwrap_or(file.size);
            file.write(&charge, data, at).map(|n| (at, n))
        }?;
        self.node.touch();
        Ok(result)
    }

    fn fallocate(&self, mode: AllocMode, offset: u64, len: u64, exempt: bool) -> FsResult<()> {
        let end = offset
            .checked_add(len)
            .filter(|&end| end <= MAX_SIZE)
            .ok_or(Errno::EFBIG)?;
        let owner = self.node.quota_owner.lock().unwrap();
        let charge = Charge {
            shared: &self.node.shared,
            owner: *owner,
            exempt,
        };
        {
            let mut file = self.node.data()?;
            match mode {
                AllocMode::Allocate | AllocMode::AllocateKeepSize => {
                    file.allocate(&charge, offset, end, mode == AllocMode::Allocate)?;
                }
                AllocMode::PunchHole => file.punch(&charge, offset, end)?,
                AllocMode::ZeroRange { keep_size } => {
                    file.punch(&charge, offset, end)?;
                    file.allocate(&charge, offset, end, !keep_size)?;
                }
            }
        }
        self.node.touch();
        Ok(())
    }
}

#[async_trait(?Send)]
impl Handle for MemHandle {
    fn mmap(&self, _shared: bool) -> FsResult<()> {
        // mm/shmem.c:shmem_mmap_prepare calls file_accessed for every file mapping.
        self.read_done();
        Ok(())
    }

    async fn read_at(&self, buf: &mut [u8], offset: u64) -> FsResult<usize> {
        let n = self.node.data()?.read(buf, offset)?;
        self.read_done();
        Ok(n)
    }

    async fn write_at(&self, data: &[u8], offset: u64) -> FsResult<usize> {
        self.write(data, Some(offset), false).map(|(_, n)| n)
    }

    async fn append(&self, data: &[u8]) -> FsResult<(u64, usize)> {
        self.write(data, None, false)
    }

    async fn write_at_as(
        &self,
        data: &[u8],
        offset: u64,
        cred: &wasixv2_vfs::Cred,
    ) -> FsResult<usize> {
        self.write(
            data,
            Some(offset),
            cred.capable_init(wasixv2_vfs::Cap::SysResource),
        )
        .map(|(_, n)| n)
    }

    async fn append_as(&self, data: &[u8], cred: &wasixv2_vfs::Cred) -> FsResult<(u64, usize)> {
        self.write(data, None, cred.capable_init(wasixv2_vfs::Cap::SysResource))
    }

    async fn allocate(&self, mode: AllocMode, offset: u64, len: u64) -> FsResult<()> {
        self.fallocate(mode, offset, len, false)
    }

    async fn allocate_as(
        &self,
        mode: AllocMode,
        offset: u64,
        len: u64,
        cred: &wasixv2_vfs::Cred,
    ) -> FsResult<()> {
        self.fallocate(
            mode,
            offset,
            len,
            cred.capable_init(wasixv2_vfs::Cap::SysResource),
        )
    }

    async fn set_size(&self, size: u64) -> FsResult<()> {
        if size > MAX_SIZE {
            return Err(Errno::EFBIG);
        }
        let owner = self.node.quota_owner.lock().unwrap();
        let charge = Charge {
            shared: &self.node.shared,
            owner: *owner,
            exempt: false,
        };
        self.node.data()?.truncate(&charge, size)?;
        self.node.touch();
        Ok(())
    }

    async fn seek_hole(&self, offset: u64, _size: u64, which: SeekHole) -> FsResult<u64> {
        self.node.data()?.seek_hole(offset, which)
    }

    async fn readdir(&self, cookie: u64, sink: &mut dyn DirSink) -> FsResult<()> {
        // Copied out in small batches, so the sink never runs under the directory lock and a
        // call costs what the sink takes, not the size of the directory.
        let mut cookie = cookie.max(1);
        loop {
            let batch: Vec<(u64, Vec<u8>, u64, FileType)> = {
                let dir = self.node.dir()?;
                dir.by_cookie
                    .range(cookie..)
                    .take(READDIR_BATCH)
                    .map(|(&c, (name, node))| (c, name.clone(), node.ino, node.file_type))
                    .collect()
            };
            let Some(&(last, ..)) = batch.last() else {
                break;
            };
            let ended = batch.len() < READDIR_BATCH;
            for (c, name, ino, file_type) in batch {
                let entry = DirEntry {
                    name: &name,
                    ino,
                    file_type,
                    next: c + 1,
                };
                if !sink.push(entry) {
                    self.read_done();
                    return Ok(());
                }
            }
            if ended {
                break;
            }
            cookie = last + 1;
        }
        self.read_done();
        Ok(())
    }
}

#[cfg(test)]
mod tests;
