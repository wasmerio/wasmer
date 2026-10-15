//! In-memory filesystem driver, the default writable root (tmpfs, after Linux's `mm/shmem.c`
//! and `fs/libfs.c`).
//!
//! File data is stored sparsely in fixed-size blocks, so a write far past the end costs one block
//! and the hole reads as zeroes. Directory cookies are insertion offsets that are never reused,
//! as tmpfs's `simple_offset` map: a reader resuming at a cookie sees every entry that existed for
//! the whole read exactly once, however the directory changes in between.

use std::{
    any::Any,
    collections::{BTreeMap, HashMap},
    sync::{
        Arc, Mutex, MutexGuard, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

use async_trait::async_trait;
use wasixv2_abi::Errno;
use wasixv2_vfs::{
    AllocMode, Attr, Clock, Device, DirEntry, DirSink, FileType, Filesystem, FsInfo, FsResult,
    Handle, MAX_DIR_COOKIE, NAME_MAX, NewKind, NewNode, Node, OpenOptions, RenameMode, SeekHole,
    SetAttr, StatFs, TMPFS_MAGIC, TimeSet, Timestamp, XattrSet,
};

const BLOCK: u64 = 4096;
/// Largest file size, as Linux's `MAX_LFS_FILESIZE` on 64-bit.
const MAX_SIZE: u64 = i64::MAX as u64;
/// tmpfs's `BOGO_DIRENT_SIZE`: what each entry adds to a directory's size.
const DIRENT_SIZE: u64 = 20;
/// Blocks `statfs` reports in total. tmpfs has no fixed size here.
// TODO: an optional size limit (`size=` mount option) enforced with ENOSPC.
const TOTAL_BLOCKS: u64 = 1 << 22;
/// relatime: refresh `atime` at least this often even when it is newer than `mtime`.
const RELATIME_SECS: i64 = 24 * 60 * 60;

/// A memory filesystem.
pub struct MemFs {
    shared: Arc<Shared>,
    root: Arc<MemNode>,
}

struct Shared {
    clock: Arc<dyn Clock>,
    next_ino: AtomicU64,
    blocks_used: AtomicU64,
    inodes: AtomicU64,
}

impl Shared {
    fn now(&self) -> Timestamp {
        self.clock.now()
    }

    fn time(&self, set: TimeSet) -> Timestamp {
        match set {
            TimeSet::Now => self.now(),
            TimeSet::At(time) => time,
        }
    }
}

impl MemFs {
    /// An empty filesystem whose root is owned by `uid`/`gid` with permission bits `mode`.
    pub fn new(clock: Arc<dyn Clock>, mode: u32, uid: u32, gid: u32) -> Self {
        let shared = Arc::new(Shared {
            clock,
            next_ino: AtomicU64::new(1),
            blocks_used: AtomicU64::new(0),
            inodes: AtomicU64::new(0),
        });
        let root = MemNode::new(
            &shared,
            &NewNode {
                kind: NewKind::Directory,
                mode,
                uid,
                gid,
            },
        );
        // The root's `..` is itself.
        root.meta().nlink = 2;
        Self { shared, root }
    }
}

#[async_trait(?Send)]
impl Filesystem for MemFs {
    fn info(&self) -> FsInfo {
        FsInfo {
            magic: TMPFS_MAGIC,
            name_max: NAME_MAX,
            read_only: false,
            dev: None,
            has_btime: true,
        }
    }

    fn root(&self) -> Arc<dyn Node> {
        self.root.clone()
    }

    async fn statfs(&self) -> FsResult<StatFs> {
        let used = self.shared.blocks_used.load(Ordering::Relaxed);
        let inodes = self.shared.inodes.load(Ordering::Relaxed);
        let free = TOTAL_BLOCKS.saturating_sub(used);
        Ok(StatFs {
            block_size: BLOCK as u32,
            blocks: TOTAL_BLOCKS,
            blocks_free: free,
            blocks_avail: free,
            files: TOTAL_BLOCKS,
            files_free: TOTAL_BLOCKS.saturating_sub(inodes),
            name_max: NAME_MAX as u32,
            fragment_size: BLOCK as u32,
        })
    }
}

/// One object: file, directory, symlink or special file.
pub struct MemNode {
    ino: u64,
    file_type: FileType,
    this: Weak<MemNode>,
    shared: Arc<Shared>,
    meta: Mutex<Meta>,
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
}

enum Content {
    File(Mutex<Blocks>),
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
    fn new(shared: &Arc<Shared>, new: &NewNode<'_>) -> Arc<Self> {
        let now = shared.now();
        let (content, rdev) = match new.kind {
            NewKind::Regular => (Content::File(Mutex::default()), Device::default()),
            NewKind::Directory => (Content::Dir(Mutex::default()), Device::default()),
            NewKind::Symlink(target) => (Content::Symlink(target.to_vec()), Device::default()),
            NewKind::CharDevice(dev) | NewKind::BlockDevice(dev) => (Content::Special, dev),
            NewKind::Fifo | NewKind::Socket => (Content::Special, Device::default()),
        };
        shared.inodes.fetch_add(1, Ordering::Relaxed);
        Arc::new_cyclic(|this| Self {
            ino: shared.next_ino.fetch_add(1, Ordering::Relaxed),
            file_type: new.kind.file_type(),
            this: this.clone(),
            shared: shared.clone(),
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
            }),
            content,
        })
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

    fn blocks(&self) -> FsResult<MutexGuard<'_, Blocks>> {
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
        if meta.atime <= meta.mtime
            || meta.atime <= meta.ctime
            || now.sec - meta.atime.sec >= RELATIME_SECS
        {
            meta.atime = now;
        }
    }

    fn drop_link(&self) {
        let mut meta = self.meta();
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
            Content::File(blocks) => blocks.lock().unwrap().size,
            Content::Dir(dir) => dir.lock().unwrap().by_name.len() as u64 * DIRENT_SIZE,
            Content::Symlink(target) => target.len() as u64,
            Content::Special => 0,
        }
    }

    fn allocated(&self) -> u64 {
        match &self.content {
            Content::File(blocks) => blocks.lock().unwrap().blocks.len() as u64,
            _ => 0,
        }
    }

    fn account(&self, before: u64) {
        let after = self.allocated();
        if after >= before {
            self.shared
                .blocks_used
                .fetch_add(after - before, Ordering::Relaxed);
        } else {
            self.shared
                .blocks_used
                .fetch_sub(before - after, Ordering::Relaxed);
        }
    }
}

impl Drop for MemNode {
    fn drop(&mut self) {
        let blocks = self.allocated();
        self.shared.blocks_used.fetch_sub(blocks, Ordering::Relaxed);
        self.shared.inodes.fetch_sub(1, Ordering::Relaxed);
    }
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

    async fn getattr(&self) -> FsResult<Attr> {
        let size = self.size();
        let blocks = self.allocated() * (BLOCK / 512);
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
        if let Some(size) = attr.size {
            if size > MAX_SIZE {
                return Err(Errno::EFBIG);
            }
            let before = self.allocated();
            self.blocks()?.truncate(size);
            self.account(before);
        }
        {
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
            if let Some(time) = attr.atime {
                meta.atime = self.shared.time(time);
            }
            if let Some(time) = attr.mtime {
                meta.mtime = self.shared.time(time);
            }
            if let Some(time) = attr.ctime {
                meta.ctime = self.shared.time(time);
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
        let node = MemNode::new(&self.shared, new);
        dir.insert(name, node.clone())?;
        drop(dir);
        if new.kind == NewKind::Directory {
            node.meta().nlink = 2;
            self.meta().nlink += 1;
        }
        self.touch();
        Ok(node)
    }

    async fn create_unnamed(&self, new: &NewNode<'_>) -> FsResult<Arc<dyn Node>> {
        drop(self.dir()?);
        let node = MemNode::new(&self.shared, new);
        node.meta().nlink = 0;
        Ok(node)
    }

    async fn link(&self, name: &[u8], target: &Arc<dyn Node>) -> FsResult<()> {
        let target = Self::downcast(target)?;
        let mut dir = self.dir()?;
        if dir.by_name.contains_key(name) {
            return Err(Errno::EEXIST);
        }
        dir.insert(name, target.clone())?;
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
        meta.xattrs.insert(name.to_vec(), value.to_vec());
        meta.ctime = now;
        Ok(())
    }

    async fn listxattr(&self) -> FsResult<Vec<Vec<u8>>> {
        Ok(self.meta().xattrs.keys().cloned().collect())
    }

    async fn removexattr(&self, name: &[u8]) -> FsResult<()> {
        let now = self.shared.now();
        let mut meta = self.meta();
        meta.xattrs.remove(name).ok_or(Errno::ENODATA)?;
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

#[async_trait(?Send)]
impl Handle for MemHandle {
    async fn read_at(&self, buf: &mut [u8], offset: u64) -> FsResult<usize> {
        let n = self.node.blocks()?.read(buf, offset);
        self.read_done();
        Ok(n)
    }

    async fn write_at(&self, data: &[u8], offset: u64) -> FsResult<usize> {
        let before = self.node.allocated();
        let n = self.node.blocks()?.write(data, offset)?;
        self.node.account(before);
        self.node.touch();
        Ok(n)
    }

    async fn append(&self, data: &[u8]) -> FsResult<(u64, usize)> {
        let before = self.node.allocated();
        let result = {
            let mut blocks = self.node.blocks()?;
            let at = blocks.size;
            blocks.write(data, at).map(|n| (at, n))
        }?;
        self.node.account(before);
        self.node.touch();
        Ok(result)
    }

    async fn allocate(&self, mode: AllocMode, offset: u64, len: u64) -> FsResult<()> {
        let end = offset
            .checked_add(len)
            .filter(|&end| end <= MAX_SIZE)
            .ok_or(Errno::EFBIG)?;
        let before = self.node.allocated();
        {
            let mut blocks = self.node.blocks()?;
            match mode {
                AllocMode::Allocate | AllocMode::AllocateKeepSize => {
                    blocks.allocate(offset, end);
                    if mode == AllocMode::Allocate {
                        blocks.size = blocks.size.max(end);
                    }
                }
                AllocMode::PunchHole => blocks.punch(offset, end),
                AllocMode::ZeroRange { keep_size } => {
                    blocks.punch(offset, end);
                    blocks.allocate(offset, end);
                    if !keep_size {
                        blocks.size = blocks.size.max(end);
                    }
                }
            }
        }
        self.node.account(before);
        self.node.touch();
        Ok(())
    }

    async fn seek_hole(&self, offset: u64, _size: u64, which: SeekHole) -> FsResult<u64> {
        self.node.blocks()?.seek_hole(offset, which)
    }

    async fn readdir(&self, cookie: u64, sink: &mut dyn DirSink) -> FsResult<()> {
        // Collected first so the sink never runs under the directory lock.
        let entries: Vec<(u64, Vec<u8>, u64, FileType)> = {
            let dir = self.node.dir()?;
            dir.by_cookie
                .range(cookie.max(1)..)
                .map(|(&c, (name, node))| (c, name.clone(), node.ino, node.file_type))
                .collect()
        };
        for (c, name, ino, file_type) in entries {
            let entry = DirEntry {
                name: &name,
                ino,
                file_type,
                next: c + 1,
            };
            if !sink.push(entry) {
                break;
            }
        }
        self.read_done();
        Ok(())
    }
}

#[derive(Default)]
struct Blocks {
    size: u64,
    blocks: BTreeMap<u64, Box<[u8]>>,
}

impl Blocks {
    fn read(&self, buf: &mut [u8], offset: u64) -> usize {
        let end = self.size.min(offset.saturating_add(buf.len() as u64));
        let mut pos = offset;
        while pos < end {
            let (index, start) = (pos / BLOCK, (pos % BLOCK) as usize);
            let len = ((BLOCK - start as u64).min(end - pos)) as usize;
            let out = &mut buf[(pos - offset) as usize..][..len];
            match self.blocks.get(&index) {
                Some(block) => out.copy_from_slice(&block[start..start + len]),
                None => out.fill(0),
            }
            pos += len as u64;
        }
        end.saturating_sub(offset) as usize
    }

    fn write(&mut self, data: &[u8], offset: u64) -> FsResult<usize> {
        let end = offset
            .checked_add(data.len() as u64)
            .filter(|&end| end <= MAX_SIZE)
            .ok_or(Errno::EFBIG)?;
        let mut pos = offset;
        while pos < end {
            let (index, start) = (pos / BLOCK, (pos % BLOCK) as usize);
            let len = ((BLOCK - start as u64).min(end - pos)) as usize;
            let block = self.block(index);
            block[start..start + len].copy_from_slice(&data[(pos - offset) as usize..][..len]);
            pos += len as u64;
        }
        self.size = self.size.max(end);
        Ok(data.len())
    }

    fn block(&mut self, index: u64) -> &mut Box<[u8]> {
        self.blocks
            .entry(index)
            .or_insert_with(|| vec![0; BLOCK as usize].into_boxed_slice())
    }

    fn truncate(&mut self, size: u64) {
        let first_gone = size.div_ceil(BLOCK);
        self.blocks.split_off(&first_gone);
        // Zero the tail of the last block so a later extension reads zeroes there.
        if let Some(block) = self.blocks.get_mut(&(size / BLOCK)) {
            block[(size % BLOCK) as usize..].fill(0);
        }
        self.size = size;
    }

    fn allocate(&mut self, start: u64, end: u64) {
        if start >= end {
            return;
        }
        for index in start / BLOCK..=(end - 1) / BLOCK {
            self.block(index);
        }
    }

    /// Zeroes `start..end`, freeing whole blocks.
    fn punch(&mut self, start: u64, end: u64) {
        let end = end.min(self.size.max(start));
        let mut pos = start;
        while pos < end {
            let (index, offset) = (pos / BLOCK, (pos % BLOCK) as usize);
            let len = (BLOCK - offset as u64).min(end - pos) as usize;
            if len as u64 == BLOCK {
                self.blocks.remove(&index);
            } else if let Some(block) = self.blocks.get_mut(&index) {
                block[offset..offset + len].fill(0);
            }
            pos += len as u64;
        }
    }

    /// `SEEK_DATA`/`SEEK_HOLE` over allocated blocks, as tmpfs answers from its page cache.
    fn seek_hole(&self, offset: u64, which: SeekHole) -> FsResult<u64> {
        if offset >= self.size {
            return Err(Errno::ENXIO);
        }
        let index = offset / BLOCK;
        match which {
            SeekHole::Data => {
                let (&first, _) = self.blocks.range(index..).next().ok_or(Errno::ENXIO)?;
                let at = (first * BLOCK).max(offset);
                if at >= self.size {
                    return Err(Errno::ENXIO);
                }
                Ok(at)
            }
            SeekHole::Hole => {
                let mut i = index;
                while self.blocks.contains_key(&i) {
                    i += 1;
                }
                Ok((i * BLOCK).max(offset).min(self.size))
            }
        }
    }
}

#[cfg(test)]
mod tests;
