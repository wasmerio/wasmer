//! Overlay filesystem driver: a writable upper layer over read-only lower layers, after Linux's
//! `fs/overlayfs` (`Documentation/filesystems/overlayfs.rst`).
//!
//! Layers are any drivers. A name resolves top-down: the upper layer, then each lower layer in
//! order. A non-directory hides everything below it, directories with the same name merge until
//! one is opaque, and a whiteout hides the name in every layer below. Changing anything that only
//! exists in a lower layer first copies it, and every directory above it, up to the upper layer.
//!
//! On-disk format, compatible with Linux's for a host upper directory:
//! - A whiteout is a 0/0 character device. Where the upper driver cannot create one (a host
//!   directory without `CAP_MKNOD`), it is an empty regular file with an `overlay.whiteout`
//!   xattr in a directory marked `overlay.opaque=x` (Linux's "xwhiteouts").
//! - An opaque directory has `overlay.opaque=y`.
//! - Overlay xattrs are written as `trusted.overlay.*`, falling back to `user.overlay.*` where
//!   trusted xattrs are not allowed; both are read, and both are hidden from programs.
//!
//! Without a `workdir` there is no atomic rename-into-place: copy-up of a regular file goes
//! through an unnamed file (`O_TMPFILE`) linked in when complete where the upper driver supports
//! that, and a whiteout that replaces an entry is made under a temporary name first and renamed
//! over it, so a failure to make one changes nothing.

use std::{
    any::Any,
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

use async_trait::async_trait;
use futures::lock::Mutex as AsyncMutex;
use wasixv2_abi::Errno;
use wasixv2_vfs::{
    AllocMode, Attr, Device, DirEntry, DirSink, FileKattr, FileType, Filesystem, FsInfo, FsResult,
    Handle, NewKind, NewNode, Node, OpenOptions, RenameMode, SeekHole, SetAttr, SharedContents,
    StatFs, TimeSet, XattrSet,
};

/// `OVERLAYFS_SUPER_MAGIC`.
pub const OVERLAYFS_SUPER_MAGIC: u32 = 0x794c_7630;
/// Most lower layers an overlay takes: the layer index lives in the top byte of inode numbers.
pub const MAX_LOWERS: usize = 254;

/// Real inode numbers below this are combined with their layer index ("xino").
const LAYER_SHIFT: u32 = 56;
/// Layer index of inode numbers handed out for real inode numbers too large to combine.
const OVERFLOW_LAYER: u64 = 0xff;
const UPPER: u8 = 0;
const XATTR_PREFIXES: [&[u8]; 2] = [b"trusted.overlay.", b"user.overlay."];
const OPAQUE: &[u8] = b"opaque";
const WHITEOUT: &[u8] = b"whiteout";
const COPY_CHUNK: u64 = 64 * 1024;
/// Permission bits a copy has until it is complete, so the overlay can fill it whatever the
/// final bits are (a host upper checks them for xattrs and reopening).
const COPY_FILE_MODE: u32 = 0o600;
const COPY_DIR_MODE: u32 = 0o700;
/// Temporary names of whiteouts made before they replace an entry.
const WHITEOUT_PREFIX: &[u8] = b"#overlay-whiteout.";

/// An overlay of an optional upper layer over lower layers.
pub struct OverlayFs {
    shared: Arc<Shared>,
    root: Arc<OverlayNode>,
}

impl OverlayFs {
    /// Stacks `upper` (writable; `None` for a read-only overlay) over `lowers`, topmost first.
    ///
    /// # Panics
    ///
    /// With no layer at all, or more than [`MAX_LOWERS`] lower layers.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        upper: Option<Arc<dyn Filesystem>>,
        lowers: Vec<Arc<dyn Filesystem>>,
    ) -> Arc<dyn Filesystem> {
        assert!(
            upper.is_some() || !lowers.is_empty(),
            "an overlay needs a layer"
        );
        assert!(lowers.len() <= MAX_LOWERS, "too many lower layers");
        let shared = Arc::new(Shared {
            upper,
            lowers,
            nodes: Mutex::default(),
            origins: Mutex::default(),
            overflow: Mutex::default(),
            next_overflow: AtomicU64::new(1),
            next_whiteout: AtomicU64::new(0),
            write_lock: AsyncMutex::new(()),
        });
        // Root directories are never opaque; xwhiteouts are checked for conservatively.
        let upper = shared.upper.as_ref().map(|fs| Real {
            layer: UPPER,
            node: fs.root(),
            xwhiteouts: true,
        });
        let lowers: Vec<Real> = shared
            .lowers
            .iter()
            .enumerate()
            .map(|(i, fs)| Real {
                layer: i as u8 + 1,
                node: fs.root(),
                xwhiteouts: true,
            })
            .collect();
        let found: Vec<Real> = upper.iter().chain(&lowers).cloned().collect();
        let root = Arc::new_cyclic(|this| OverlayNode {
            ino: shared.ino_for(&found),
            file_type: FileType::Directory,
            this: this.clone(),
            shared: shared.clone(),
            state: Mutex::new(State {
                upper,
                lowers,
                parent: None,
                name: Vec::new(),
                key: None,
                unlinked: false,
            }),
            copy_lock: AsyncMutex::new(()),
        });
        Arc::new(Self { shared, root })
    }

    fn top(&self) -> &Arc<dyn Filesystem> {
        self.shared.upper.as_ref().unwrap_or(&self.shared.lowers[0])
    }
}

#[async_trait(?Send)]
impl Filesystem for OverlayFs {
    fn info(&self) -> FsInfo {
        let layers = || self.shared.upper.iter().chain(&self.shared.lowers);
        FsInfo {
            name: "overlay",
            magic: OVERLAYFS_SUPER_MAGIC,
            name_max: layers().map(|fs| fs.info().name_max).max().unwrap_or(0),
            // Every change goes through the overlay unless a layer changes underneath it.
            stable: layers().all(|fs| fs.info().stable),
            read_only: self
                .shared
                .upper
                .as_ref()
                .is_none_or(|fs| fs.info().read_only),
            dev: None,
            has_btime: self.top().info().has_btime,
        }
    }

    fn root(&self) -> Arc<dyn Node> {
        self.root.clone()
    }

    async fn statfs(&self) -> FsResult<StatFs> {
        self.top().statfs().await
    }

    async fn sync(&self) -> FsResult<()> {
        match &self.shared.upper {
            Some(upper) => upper.sync().await,
            None => Ok(()),
        }
    }
}

struct Shared {
    upper: Option<Arc<dyn Filesystem>>,
    lowers: Vec<Arc<dyn Filesystem>>,
    /// Live overlay objects, so every name resolving to one object shares its copy-up state.
    nodes: Mutex<HashMap<Key, Weak<OverlayNode>>>,
    /// Upper inode number → overlay inode number of objects copied up from a lower layer, so
    /// `st_ino` survives copy-up (Linux keeps this in `trusted.overlay.origin`).
    origins: Mutex<HashMap<u64, u64>>,
    overflow: Mutex<HashMap<(u8, u64), u64>>,
    next_overflow: AtomicU64,
    next_whiteout: AtomicU64,
    /// Serialises namespace and metadata changes, standing in for the parents' `i_rwsem`:
    /// several of them are multi-step on the upper layer. Copying a file's data up for an open
    /// holds it only to put the finished copy in place.
    write_lock: AsyncMutex<()>,
}

/// Identity of a live overlay object: its upper object once it has one, else the name it was
/// found under. Lower hard links are therefore separate objects, and copying one up breaks the
/// link (Linux's `index=off`).
#[derive(Clone, PartialEq, Eq, Hash)]
enum Key {
    Upper(u64),
    Lower(u64, Vec<u8>),
}

impl Shared {
    /// A unique overlay inode number for real inode `ino` of `layer`.
    fn xino(&self, layer: u8, ino: u64) -> u64 {
        if ino < 1 << LAYER_SHIFT {
            return (layer as u64) << LAYER_SHIFT | ino;
        }
        let mut overflow = self.overflow.lock().unwrap();
        *overflow
            .entry((layer, ino))
            .or_insert_with(|| self.fresh_ino())
    }

    /// The inode number of the object whose real objects are `found` (topmost first): the
    /// origin of a copied-up object, the topmost lower directory of a merged directory, else
    /// the topmost object.
    fn ino_for(&self, found: &[Real]) -> u64 {
        let top = &found[0];
        if top.layer == UPPER {
            if let Some(&ino) = self.origins.lock().unwrap().get(&top.node.ino()) {
                return ino;
            }
            if let Some(lower) = found.get(1) {
                return self.xino(lower.layer, lower.node.ino());
            }
        }
        self.xino(top.layer, top.node.ino())
    }

    /// A number no real inode maps to, for an object that cannot use its real one.
    fn fresh_ino(&self) -> u64 {
        OVERFLOW_LAYER << LAYER_SHIFT | self.next_overflow.fetch_add(1, Ordering::Relaxed)
    }

    /// The overlay object for `found`, reached as `name` in `parent`.
    async fn node(
        &self,
        found: Vec<Real>,
        parent: &Arc<OverlayNode>,
        name: &[u8],
    ) -> FsResult<Arc<OverlayNode>> {
        let top = &found[0];
        let key = match top.layer {
            UPPER => Key::Upper(top.node.ino()),
            _ => Key::Lower(parent.ino, name.to_vec()),
        };
        // Not under the lock: dropping the last reference takes it.
        let existing = self.nodes.lock().unwrap().get(&key).and_then(Weak::upgrade);
        if let Some(node) = existing
            && node.real().ino() == top.node.ino()
        {
            return Ok(node);
        }
        // Lower hard links are separate overlay objects (`index=off`) that diverge on copy-up,
        // and an inode number is the object's identity in the VFS, so each takes a number of
        // its own instead of sharing the real one. Its `d_ino` in a listing stays the real one.
        let aliased = top.layer != UPPER
            && top.node.file_type() != FileType::Directory
            && top.node.getattr().await?.nlink > 1;
        let shared = parent.shared.clone();
        let file_type = top.node.file_type();
        let mut found = found.into_iter();
        let upper = found.next().unwrap();
        let (upper, lowers): (Option<Real>, Vec<Real>) = match upper.layer {
            UPPER => (Some(upper), found.collect()),
            _ => (None, std::iter::once(upper).chain(found).collect()),
        };
        let all: Vec<Real> = upper.iter().chain(&lowers).cloned().collect();
        let node = Arc::new_cyclic(|this| OverlayNode {
            ino: match aliased {
                true => self.fresh_ino(),
                false => self.ino_for(&all),
            },
            file_type,
            this: this.clone(),
            shared,
            state: Mutex::new(State {
                upper,
                lowers,
                parent: Some(parent.clone()),
                name: name.to_vec(),
                key: Some(key.clone()),
                unlinked: false,
            }),
            copy_lock: AsyncMutex::new(()),
        });
        self.nodes
            .lock()
            .unwrap()
            .insert(key, Arc::downgrade(&node));
        Ok(node)
    }

    /// Drops what is remembered about upper object `node` once its last link is gone, as the
    /// upper driver may reuse its inode number.
    async fn forget_if_gone(&self, node: &Arc<dyn Node>) {
        if node.getattr().await.is_ok_and(|attr| attr.nlink > 0) {
            return;
        }
        self.origins.lock().unwrap().remove(&node.ino());
        self.nodes.lock().unwrap().remove(&Key::Upper(node.ino()));
    }

    /// The lower object reached as `name` in `parent` lost that name: a copy-up of it, for a
    /// file still open, must not take the name back (`d_unhashed` in `ovl_copy_up`).
    fn lower_unlinked(&self, parent: &OverlayNode, name: &[u8]) {
        let key = Key::Lower(parent.ino, name.to_vec());
        let node = self.nodes.lock().unwrap().remove(&key);
        if let Some(node) = node.and_then(|weak| weak.upgrade()) {
            let mut state = node.state.lock().unwrap();
            state.unlinked = true;
            state.key = None;
        }
    }

    /// A name for a whiteout that does not replace anything yet.
    fn whiteout_name(&self) -> Vec<u8> {
        let n = self.next_whiteout.fetch_add(1, Ordering::Relaxed);
        [WHITEOUT_PREFIX, n.to_string().as_bytes()].concat()
    }
}

/// A real object in one layer.
#[derive(Clone)]
struct Real {
    /// 0 for the upper layer, `1 + i` for lower layer `i`.
    layer: u8,
    node: Arc<dyn Node>,
    /// A directory that may hold xattr whiteouts (`overlay.opaque=x`).
    xwhiteouts: bool,
}

/// One overlay object (Linux's `ovl_inode`).
pub struct OverlayNode {
    ino: u64,
    file_type: FileType,
    this: Weak<OverlayNode>,
    shared: Arc<Shared>,
    state: Mutex<State>,
    /// Held while this object's data is copied up for an open, so only one copy is made.
    copy_lock: AsyncMutex<()>,
}

struct State {
    upper: Option<Real>,
    /// Directories: every lower directory merged into this one. Others: the lower object until
    /// copy-up, then nothing.
    lowers: Vec<Real>,
    /// Where copy-up puts the object: the directory and name it was last reached through.
    parent: Option<Arc<OverlayNode>>,
    name: Vec<u8>,
    key: Option<Key>,
    /// A lower non-directory whose name was removed: its copy-up stays unnamed.
    unlinked: bool,
}

impl Drop for OverlayNode {
    fn drop(&mut self) {
        let Some(key) = self.state.get_mut().unwrap().key.take() else {
            return;
        };
        let mut nodes = self.shared.nodes.lock().unwrap();
        if nodes.get(&key).is_some_and(|weak| weak.strong_count() == 0) {
            nodes.remove(&key);
        }
    }
}

/// A copy of a lower object, complete but maybe not in place yet.
struct Copy {
    node: Arc<dyn Node>,
    /// Created under its name rather than unnamed.
    named: bool,
    /// A regular file's copy, open with the options `prepare_copy` was given.
    handle: Option<Box<dyn Handle>>,
}

impl OverlayNode {
    fn arc(&self) -> Arc<OverlayNode> {
        self.this.upgrade().expect("a node in use is alive")
    }

    fn downcast(node: &Arc<dyn Node>) -> FsResult<Arc<OverlayNode>> {
        node.as_any()
            .downcast_ref::<OverlayNode>()
            .map(OverlayNode::arc)
            .ok_or(Errno::EXDEV)
    }

    fn is_dir(&self) -> bool {
        self.file_type == FileType::Directory
    }

    fn check_dir(&self) -> FsResult<()> {
        match self.is_dir() {
            true => Ok(()),
            false => Err(Errno::ENOTDIR),
        }
    }

    fn upper(&self) -> Option<Real> {
        self.state.lock().unwrap().upper.clone()
    }

    fn upper_node(&self) -> Option<Arc<dyn Node>> {
        self.upper().map(|real| real.node)
    }

    fn has_lowers(&self) -> bool {
        !self.state.lock().unwrap().lowers.is_empty()
    }

    /// The object that answers reads: the upper one if there is one.
    fn real(&self) -> Arc<dyn Node> {
        let state = self.state.lock().unwrap();
        match &state.upper {
            Some(upper) => upper.node.clone(),
            None => state.lowers[0].node.clone(),
        }
    }

    /// Every real object, topmost first.
    fn layers(&self) -> Vec<Real> {
        let state = self.state.lock().unwrap();
        state.upper.iter().chain(&state.lowers).cloned().collect()
    }

    fn fix_attr(&self, mut attr: Attr) -> Attr {
        attr.ino = self.ino;
        attr.dev = None;
        // A merged directory's link count means nothing; 1 tells `find` so (as Linux).
        if self.is_dir() && self.layers().len() > 1 {
            attr.nlink = 1;
        }
        attr
    }

    /// Resolves `name` across the layers of this directory (`ovl_lookup`); empty if absent.
    async fn find(&self, name: &[u8]) -> FsResult<Vec<Real>> {
        let mut found: Vec<Real> = Vec::new();
        for dir in self.layers() {
            let child = match dir.node.lookup(name).await {
                Ok(child) => child,
                Err(Errno::ENOENT | Errno::ENAMETOOLONG) => continue,
                Err(err) => return Err(err),
            };
            if is_whiteout(&child, dir.xwhiteouts).await? {
                break;
            }
            let is_dir = child.file_type() == FileType::Directory;
            if !found.is_empty() && !is_dir {
                break;
            }
            let (opaque, xwhiteouts) = match is_dir {
                true => dir_flags(&child).await,
                false => (false, false),
            };
            found.push(Real {
                layer: dir.layer,
                node: child,
                xwhiteouts,
            });
            if !is_dir || opaque {
                break;
            }
        }
        Ok(found)
    }

    /// Whether a lower layer of this directory has `name`, so removing it needs a whiteout.
    async fn lower_positive(&self, name: &[u8]) -> FsResult<bool> {
        let lowers = self.state.lock().unwrap().lowers.clone();
        for dir in lowers {
            match dir.node.lookup(name).await {
                Ok(child) => return Ok(!is_whiteout(&child, dir.xwhiteouts).await?),
                Err(Errno::ENOENT | Errno::ENAMETOOLONG) => {}
                Err(err) => return Err(err),
            }
        }
        Ok(false)
    }

    /// The merged listing (`ovl_dir_read_merged`): upper entries shadow lower ones, whiteouts
    /// hide names below them.
    async fn merged_entries(&self) -> FsResult<Vec<Entry>> {
        let layers = self.layers();
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for (i, dir) in layers.iter().enumerate() {
            for (name, ino, file_type) in read_all(&dir.node).await? {
                if !seen.insert(name.clone()) {
                    continue;
                }
                let maybe_whiteout = file_type == FileType::CharDevice
                    || (file_type == FileType::Regular && dir.xwhiteouts);
                if maybe_whiteout {
                    match dir.node.lookup(&name).await {
                        Ok(child) if is_whiteout(&child, dir.xwhiteouts).await? => continue,
                        Ok(_) => {}
                        Err(Errno::ENOENT) => continue,
                        Err(err) => return Err(err),
                    }
                }
                let ino = self
                    .entry_ino(dir, i + 1 < layers.len(), &name, ino, file_type)
                    .await?;
                out.push(Entry {
                    name,
                    ino,
                    file_type,
                });
            }
        }
        Ok(out)
    }

    /// The `d_ino` of an entry, matching the `st_ino` a lookup gives.
    async fn entry_ino(
        &self,
        dir: &Real,
        more_layers: bool,
        name: &[u8],
        ino: u64,
        file_type: FileType,
    ) -> FsResult<u64> {
        if dir.layer == UPPER {
            if let Some(&origin) = self.shared.origins.lock().unwrap().get(&ino) {
                return Ok(origin);
            }
            if file_type == FileType::Directory && more_layers {
                let found = self.find(name).await?;
                if !found.is_empty() {
                    return Ok(self.shared.ino_for(&found));
                }
            }
        }
        Ok(self.shared.xino(dir.layer, ino))
    }

    /// Copies this object and every ancestor missing from the upper layer up, topmost first
    /// (`ovl_copy_up`). Callers hold the write lock.
    async fn copy_up(self: &Arc<Self>) -> FsResult<Arc<dyn Node>> {
        if self.shared.upper.is_none() {
            return Err(Errno::EROFS);
        }
        let mut chain = Vec::new();
        let mut cur = self.clone();
        while cur.upper().is_none() {
            let parent = cur.state.lock().unwrap().parent.clone();
            chain.push(cur);
            cur = parent.expect("the root has an upper directory when the overlay does");
        }
        for node in chain.into_iter().rev() {
            let copy = node.prepare_copy(None, false).await?;
            node.install_copy(copy).await?;
        }
        Ok(self.upper_node().expect("copied up"))
    }

    /// The upper object of a file opened for writing with `options`, copying it up first
    /// (`ovl_open` with write access), and the copy's handle if this made it. The data is
    /// copied into an unnamed file without the overlay lock, which is taken only to put the
    /// copy in place; for `O_TRUNC` the data is not copied at all.
    async fn copy_up_for_write(
        self: &Arc<Self>,
        options: OpenOptions,
    ) -> FsResult<(Arc<dyn Node>, Option<Box<dyn Handle>>)> {
        if let Some(upper) = self.upper_node() {
            return Ok((upper, None));
        }
        let _copying = self.copy_lock.lock().await;
        if let Some(upper) = self.upper_node() {
            return Ok((upper, None));
        }
        let parent = self.state.lock().unwrap().parent.clone();
        let parent = parent.expect("a file has a parent");
        {
            let _guard = self.shared.write_lock.lock().await;
            parent.copy_up().await?;
        }
        let unnamed = self.prepare_copy(Some(options), true).await;
        let _guard = self.shared.write_lock.lock().await;
        // A metadata change may have copied it up meanwhile; ours is unnamed, so it just goes.
        if let Some(upper) = self.upper_node() {
            return Ok((upper, None));
        }
        let mut copy = match unnamed {
            // A copy made under its name is a namespace change, so it is made under the lock.
            Err(Errno::EOPNOTSUPP) => self.prepare_copy(Some(options), false).await?,
            copy => copy?,
        };
        // The copy's own handle: reopening could check bits the copy's opener is exempt from.
        let handle = copy.handle.take();
        self.install_copy(copy).await?;
        Ok((self.upper_node().expect("copied up"), handle))
    }

    /// Makes a complete copy of this lower object in its parent's upper directory, which
    /// exists: named for anything but a regular file the upper driver can create unnamed, or
    /// `EOPNOTSUPP` with `unnamed_only`. A regular file's copy stays open for writing, with
    /// `open`'s other options; for `O_TRUNC` its data is not copied.
    async fn prepare_copy(&self, open: Option<OpenOptions>, unnamed_only: bool) -> FsResult<Copy> {
        let (parent, name, lower) = {
            let state = self.state.lock().unwrap();
            let parent = state.parent.clone().expect("not the root");
            (parent, state.name.clone(), state.lowers[0].node.clone())
        };
        let parent_upper = parent.upper_node().expect("parent copied up first");
        let parent_attr = parent_upper.getattr().await?;
        let attr = lower.getattr().await?;
        let target = match self.file_type {
            FileType::Symlink => lower.readlink().await?,
            _ => Vec::new(),
        };
        let (kind, mode) = match self.file_type {
            FileType::Regular => (NewKind::Regular, COPY_FILE_MODE),
            FileType::Directory => (NewKind::Directory, COPY_DIR_MODE),
            FileType::Symlink => (NewKind::Symlink(&target), attr.mode),
            FileType::Fifo => (NewKind::Fifo, attr.mode),
            FileType::Socket => (NewKind::Socket, attr.mode),
            FileType::CharDevice => (NewKind::CharDevice(attr.rdev), attr.mode),
            FileType::BlockDevice => (NewKind::BlockDevice(attr.rdev), attr.mode),
        };
        let new = NewNode {
            kind,
            mode,
            uid: attr.uid,
            gid: attr.gid,
            task: 0,
        };
        let unlinked = self.state.lock().unwrap().unlinked;
        // A regular file is filled while unnamed where possible, so a failed or interrupted
        // copy never shadows the lower file with a partial one.
        let write = OpenOptions {
            write: true,
            truncate: false,
            ..open.unwrap_or_default()
        };
        let (upper, handle, named) = match kind {
            NewKind::Regular => match parent_upper.create_unnamed(&new, write).await {
                Ok((file, handle)) => (file, Some(handle), false),
                Err(Errno::EOPNOTSUPP) if !unlinked && !unnamed_only => {
                    let (file, handle) = parent_upper.create_open(&name, &new, write).await?;
                    (file, Some(handle), true)
                }
                Err(err) => return Err(err),
            },
            _ => (parent_upper.create(&name, &new).await?, None, true),
        };
        let size = match open {
            Some(options) if options.truncate => 0,
            _ => attr.size,
        };
        if let Err(err) = fill_copy(&lower, &upper, handle.as_deref(), &attr, size).await {
            if named {
                let _ = match self.is_dir() {
                    true => parent_upper.rmdir(&name).await,
                    false => parent_upper.unlink(&name).await,
                };
            }
            return Err(err);
        }
        if named {
            // Creating the entry stamped the parent; it keeps the times it had (as Linux).
            let _ = parent_upper.setattr(&restore_times(&parent_attr)).await;
        }
        Ok(Copy {
            node: upper,
            named,
            handle,
        })
    }

    /// Puts a copy from `prepare_copy` in place: links an unnamed one in where this object was
    /// last reached, unless its name was removed, and makes it this object's upper object.
    async fn install_copy(&self, copy: Copy) -> FsResult<()> {
        let upper = copy.node;
        let (parent, name, unlinked) = {
            let state = self.state.lock().unwrap();
            (state.parent.clone(), state.name.clone(), state.unlinked)
        };
        if !copy.named && !unlinked {
            let parent_upper = parent
                .and_then(|parent| parent.upper_node())
                .expect("parent copied up first");
            let parent_attr = parent_upper.getattr().await?;
            parent_upper.link(&name, &upper).await?;
            let _ = parent_upper.setattr(&restore_times(&parent_attr)).await;
        }

        let old_key = {
            let mut state = self.state.lock().unwrap();
            state.upper = Some(Real {
                layer: UPPER,
                node: upper.clone(),
                xwhiteouts: false,
            });
            if !self.is_dir() {
                state.lowers.clear();
            }
            match unlinked {
                true => None,
                false => state.key.replace(Key::Upper(upper.ino())),
            }
        };
        self.shared
            .origins
            .lock()
            .unwrap()
            .insert(upper.ino(), self.ino);
        if unlinked {
            return Ok(());
        }
        let mut nodes = self.shared.nodes.lock().unwrap();
        if let Some(old) = old_key
            && nodes
                .get(&old)
                .is_some_and(|w| w.as_ptr() == self.this.as_ptr())
        {
            nodes.remove(&old);
        }
        nodes.insert(Key::Upper(upper.ino()), self.this.clone());
        Ok(())
    }

    /// Creates a whiteout for `name` in this directory's upper directory `upper`.
    async fn make_whiteout(&self, upper: &Arc<dyn Node>, name: &[u8]) -> FsResult<()> {
        let device = NewNode {
            kind: NewKind::CharDevice(Device::default()),
            mode: 0,
            uid: 0,
            gid: 0,
            task: 0,
        };
        match upper.create(name, &device).await {
            Ok(_) => return Ok(()),
            Err(Errno::EPERM | Errno::EOPNOTSUPP | Errno::EINVAL) => {}
            Err(err) => return Err(err),
        }
        let marked = self.upper().is_some_and(|real| real.xwhiteouts);
        if !marked {
            set_overlay_xattr(upper, OPAQUE, b"x").await?;
            if let Some(real) = &mut self.state.lock().unwrap().upper {
                real.xwhiteouts = true;
            }
        }
        // Owner-writable, so a host upper lets the owner set the `user.` marker on it.
        let file = NewNode {
            kind: NewKind::Regular,
            mode: COPY_FILE_MODE,
            ..device
        };
        let file = upper.create(name, &file).await?;
        if let Err(err) = set_overlay_xattr(&file, WHITEOUT, b"y").await {
            let _ = upper.unlink(name).await;
            return Err(err);
        }
        Ok(())
    }

    /// Makes a whiteout under a temporary name in upper directory `upper`, for
    /// `install_whiteout` to move over an entry once the entry may go.
    async fn prepare_whiteout(&self, upper: &Arc<dyn Node>) -> FsResult<Vec<u8>> {
        loop {
            let name = self.shared.whiteout_name();
            match self.make_whiteout(upper, &name).await {
                Ok(()) => return Ok(name),
                Err(Errno::EEXIST) => continue,
                Err(err) => return Err(err),
            }
        }
    }

    /// Renames the whiteout `temp` to `name`, replacing what is there.
    async fn install_whiteout(
        &self,
        upper: &Arc<dyn Node>,
        temp: &[u8],
        name: &[u8],
    ) -> FsResult<()> {
        let result = upper.rename(temp, upper, name, RenameMode::Replace).await;
        if result.is_err() {
            let _ = upper.unlink(temp).await;
        }
        result
    }

    /// Removes a whiteout at `name` in upper directory `upper`; whether there was one.
    async fn clear_whiteout(&self, upper: &Arc<dyn Node>, name: &[u8]) -> FsResult<bool> {
        let xwhiteouts = self.upper().is_some_and(|real| real.xwhiteouts);
        match upper.lookup(name).await {
            Ok(child) if is_whiteout(&child, xwhiteouts).await? => {
                upper.unlink(name).await?;
                Ok(true)
            }
            Ok(_) => Err(Errno::EEXIST),
            Err(Errno::ENOENT) => Ok(false),
            Err(err) => Err(err),
        }
    }

    /// Removes the whiteouts in this directory's upper directory before it is removed or
    /// replaced; it must hold nothing else.
    async fn clear_upper_whiteouts(&self) -> FsResult<()> {
        let Some(upper) = self.upper() else {
            return Ok(());
        };
        for (name, _, _) in read_all(&upper.node).await? {
            let child = upper.node.lookup(&name).await?;
            if !is_whiteout(&child, upper.xwhiteouts).await? {
                return Err(Errno::ENOTEMPTY);
            }
            upper.node.unlink(&name).await?;
        }
        Ok(())
    }

    /// The upper object for a change of this object's own metadata, copied up under the
    /// overlay lock if it has none yet.
    async fn upper_for_change(&self) -> FsResult<Arc<dyn Node>> {
        if let Some(upper) = self.upper_node() {
            return Ok(upper);
        }
        let _guard = self.shared.write_lock.lock().await;
        self.arc().copy_up().await
    }

    fn moved(&self, parent: &Arc<OverlayNode>, name: &[u8]) {
        let mut state = self.state.lock().unwrap();
        state.parent = Some(parent.clone());
        state.name = name.to_vec();
    }

    /// Adds `name` in the upper directory, over a whiteout if there is one, and opens it with
    /// `open` at once if given.
    async fn create_entry(
        &self,
        name: &[u8],
        new: &NewNode<'_>,
        open: Option<OpenOptions>,
    ) -> FsResult<(Arc<OverlayNode>, Option<Box<dyn Handle>>)> {
        self.check_dir()?;
        let _guard = self.shared.write_lock.lock().await;
        if !self.find(name).await?.is_empty() {
            return Err(Errno::EEXIST);
        }
        let upper = self.arc().copy_up().await?;
        let over_whiteout = self.clear_whiteout(&upper, name).await?;
        let created = match open {
            Some(options) => upper
                .create_open(name, new, options)
                .await
                .map(|(child, handle)| (child, Some(handle))),
            None => upper.create(name, new).await.map(|child| (child, None)),
        };
        // A directory replacing a whiteout must not merge with what the whiteout hid.
        let created = match created {
            Ok((child, handle)) if over_whiteout && new.kind == NewKind::Directory => {
                match set_overlay_xattr(&child, OPAQUE, b"y").await {
                    Ok(()) => Ok((child, handle)),
                    Err(err) => {
                        let _ = upper.rmdir(name).await;
                        Err(err)
                    }
                }
            }
            other => other,
        };
        let (child, handle) = match created {
            Ok(created) => created,
            Err(err) => {
                if over_whiteout {
                    let _ = self.make_whiteout(&upper, name).await;
                }
                return Err(err);
            }
        };
        let found = vec![Real {
            layer: UPPER,
            node: child,
            xwhiteouts: false,
        }];
        let node = self.shared.node(found, &self.arc(), name).await?;
        Ok((node, handle))
    }
}

#[async_trait(?Send)]
impl Node for OverlayNode {
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
        let attr = self.real().getattr().await?;
        Ok(self.fix_attr(attr))
    }

    async fn setattr(&self, attr: &SetAttr) -> FsResult<Attr> {
        let upper = self.upper_for_change().await?;
        let attr = upper.setattr(attr).await?;
        Ok(self.fix_attr(attr))
    }

    async fn lookup(&self, name: &[u8]) -> FsResult<Arc<dyn Node>> {
        self.check_dir()?;
        let found = self.find(name).await?;
        if found.is_empty() {
            return Err(Errno::ENOENT);
        }
        self.shared
            .node(found, &self.arc(), name)
            .await
            .map(|n| n as _)
    }

    async fn create(&self, name: &[u8], new: &NewNode<'_>) -> FsResult<Arc<dyn Node>> {
        let (node, _) = self.create_entry(name, new, None).await?;
        Ok(node)
    }

    async fn create_open(
        &self,
        name: &[u8],
        new: &NewNode<'_>,
        options: OpenOptions,
    ) -> FsResult<(Arc<dyn Node>, Box<dyn Handle>)> {
        let (node, handle) = self.create_entry(name, new, Some(options)).await?;
        let handle = handle.expect("opened");
        Ok((node.clone(), FileHandle::upper(node, options, handle)))
    }

    async fn create_unnamed(
        &self,
        new: &NewNode<'_>,
        options: OpenOptions,
    ) -> FsResult<(Arc<dyn Node>, Box<dyn Handle>)> {
        self.check_dir()?;
        let _guard = self.shared.write_lock.lock().await;
        let upper = self.arc().copy_up().await?;
        let (child, handle) = upper.create_unnamed(new, options).await?;
        let found = vec![Real {
            layer: UPPER,
            node: child,
            xwhiteouts: false,
        }];
        let node = self.shared.node(found, &self.arc(), b"").await?;
        Ok((node.clone(), FileHandle::upper(node, options, handle)))
    }

    async fn link(&self, name: &[u8], target: &Arc<dyn Node>) -> FsResult<()> {
        self.check_dir()?;
        let target = Self::downcast(target)?;
        let _guard = self.shared.write_lock.lock().await;
        if !self.find(name).await?.is_empty() {
            return Err(Errno::EEXIST);
        }
        let target_upper = target.copy_up().await?;
        let upper = self.arc().copy_up().await?;
        let over_whiteout = self.clear_whiteout(&upper, name).await?;
        if let Err(err) = upper.link(name, &target_upper).await {
            if over_whiteout {
                let _ = self.make_whiteout(&upper, name).await;
            }
            return Err(err);
        }
        Ok(())
    }

    async fn unlink(&self, name: &[u8]) -> FsResult<()> {
        self.check_dir()?;
        let _guard = self.shared.write_lock.lock().await;
        let found = self.find(name).await?;
        let top = found.first().ok_or(Errno::ENOENT)?;
        if top.node.file_type() == FileType::Directory {
            return Err(Errno::EISDIR);
        }
        let upper = self.arc().copy_up().await?;
        let positive = self.lower_positive(name).await?;
        match (top.layer == UPPER, positive) {
            (true, true) => {
                let temp = self.prepare_whiteout(&upper).await?;
                self.install_whiteout(&upper, &temp, name).await?;
            }
            (true, false) => upper.unlink(name).await?,
            (false, _) => {
                self.make_whiteout(&upper, name).await?;
                self.shared.lower_unlinked(self, name);
            }
        }
        if top.layer == UPPER {
            self.shared.forget_if_gone(&top.node).await;
        }
        Ok(())
    }

    async fn rmdir(&self, name: &[u8]) -> FsResult<()> {
        self.check_dir()?;
        let _guard = self.shared.write_lock.lock().await;
        let found = self.find(name).await?;
        if found.is_empty() {
            return Err(Errno::ENOENT);
        }
        let in_upper = found[0].layer == UPPER;
        let child = self.shared.node(found, &self.arc(), name).await?;
        child.check_dir()?;
        if !child.merged_entries().await?.is_empty() {
            return Err(Errno::ENOTEMPTY);
        }
        let upper = self.arc().copy_up().await?;
        let positive = self.lower_positive(name).await?;
        if !in_upper {
            return match positive {
                true => self.make_whiteout(&upper, name).await,
                false => Ok(()),
            };
        }
        // The whiteout, which can fail to be made, exists before anything is removed.
        let temp = match positive {
            true => Some(self.prepare_whiteout(&upper).await?),
            false => None,
        };
        let removed = async {
            child.clear_upper_whiteouts().await?;
            upper.rmdir(name).await
        };
        if let Err(err) = removed.await {
            if let Some(temp) = &temp {
                let _ = upper.unlink(temp).await;
            }
            return Err(err);
        }
        let child_upper = child.upper_node().expect("in the upper layer");
        self.shared.forget_if_gone(&child_upper).await;
        if let Some(temp) = temp {
            self.install_whiteout(&upper, &temp, name).await?;
        }
        Ok(())
    }

    async fn rename(
        &self,
        name: &[u8],
        new_dir: &Arc<dyn Node>,
        new_name: &[u8],
        mode: RenameMode,
    ) -> FsResult<()> {
        self.check_dir()?;
        if mode == RenameMode::Whiteout {
            return Err(Errno::EINVAL);
        }
        let new_parent = Self::downcast(new_dir)?;
        let old_parent = self.arc();
        let _guard = self.shared.write_lock.lock().await;
        let found = self.find(name).await?;
        if found.is_empty() {
            return Err(Errno::ENOENT);
        }
        let source = self.shared.node(found, &old_parent, name).await?;
        // Without `redirect_dir`, a directory with lower parts cannot move (as Linux).
        if source.is_dir() && source.has_lowers() {
            return Err(Errno::EXDEV);
        }
        let found = new_parent.find(new_name).await?;
        let target = match found.is_empty() {
            true => None,
            false => Some(self.shared.node(found, &new_parent, new_name).await?),
        };
        match (mode, &target) {
            (RenameMode::NoReplace, Some(_)) => return Err(Errno::EEXIST),
            (RenameMode::Exchange, None) => return Err(Errno::ENOENT),
            _ => {}
        }
        if let Some(target) = &target {
            if Arc::ptr_eq(target, &source) {
                return Ok(());
            }
            if target.is_dir() {
                if mode == RenameMode::Exchange && target.has_lowers() {
                    return Err(Errno::EXDEV);
                }
                if mode == RenameMode::Replace && !target.merged_entries().await?.is_empty() {
                    return Err(Errno::ENOTEMPTY);
                }
            }
        }
        let source_upper = source.copy_up().await?;
        let old_upper = old_parent.copy_up().await?;
        let new_upper = new_parent.copy_up().await?;
        let target_upper = match (&target, mode) {
            (Some(target), RenameMode::Exchange) => Some(target.copy_up().await?),
            (Some(target), _) => target.upper_node(),
            (None, _) => None,
        };
        match &target {
            None => {
                new_parent.clear_whiteout(&new_upper, new_name).await?;
            }
            Some(target) if target.is_dir() && mode == RenameMode::Replace => {
                target.clear_upper_whiteouts().await?;
            }
            Some(_) => {}
        }
        let source_positive = old_parent.lower_positive(name).await?;
        let target_positive = new_parent.lower_positive(new_name).await?;
        // The whiteout the source leaves exists before the source moves.
        let temp = match source_positive && mode != RenameMode::Exchange {
            true => Some(old_parent.prepare_whiteout(&old_upper).await?),
            false => None,
        };
        if let Err(err) = old_upper.rename(name, &new_upper, new_name, mode).await {
            if let Some(temp) = &temp {
                let _ = old_upper.unlink(temp).await;
            }
            return Err(err);
        }
        if let Some(target) = &target
            && target.upper().is_none()
            && mode != RenameMode::Exchange
        {
            self.shared.lower_unlinked(&new_parent, new_name);
        }

        if mode == RenameMode::Exchange {
            let target = target.as_ref().expect("checked above");
            target.moved(&old_parent, name);
            if target.is_dir() && source_positive {
                set_overlay_xattr(target_upper.as_ref().expect("copied up"), OPAQUE, b"y").await?;
            }
        } else {
            if let Some(replaced) = &target_upper {
                self.shared.forget_if_gone(replaced).await;
            }
            if let Some(temp) = temp {
                old_parent.install_whiteout(&old_upper, &temp, name).await?;
            }
        }
        source.moved(&new_parent, new_name);
        if source.is_dir() && target_positive {
            set_overlay_xattr(&source_upper, OPAQUE, b"y").await?;
        }
        Ok(())
    }

    async fn readlink(&self) -> FsResult<Vec<u8>> {
        self.real().readlink().await
    }

    async fn open(&self, options: OpenOptions) -> FsResult<Box<dyn Handle>> {
        if self.is_dir() {
            return Ok(Box::new(DirHandle {
                node: self.arc(),
                snapshot: Mutex::new(None),
            }));
        }
        let (real, copied, in_upper) = if options.write {
            let (real, copied) = self.arc().copy_up_for_write(options).await?;
            (real, copied, true)
        } else {
            (self.real(), None, self.upper().is_some())
        };
        let handle = match copied {
            Some(handle) => handle,
            None => real.open(options).await?,
        };
        Ok(Box::new(FileHandle {
            node: self.arc(),
            options,
            real: Mutex::new((in_upper, Arc::from(handle))),
        }))
    }

    /// `ovl_fileattr_get`: the real object's.
    fn inode_flags(&self) -> u32 {
        self.real().inode_flags()
    }

    async fn fileattr_get(&self) -> FsResult<FileKattr> {
        self.real().fileattr_get().await
    }

    /// `ovl_fileattr_set`: on the object copied up.
    async fn fileattr_set(&self, fa: &FileKattr) -> FsResult<()> {
        let upper = self.upper_for_change().await?;
        upper.fileattr_set(fa).await
    }

    async fn getxattr(&self, name: &[u8]) -> FsResult<Vec<u8>> {
        if is_private(name) {
            return Err(Errno::EOPNOTSUPP);
        }
        self.real().getxattr(name).await
    }

    async fn setxattr(&self, name: &[u8], value: &[u8], how: XattrSet) -> FsResult<()> {
        if is_private(name) {
            return Err(Errno::EOPNOTSUPP);
        }
        let upper = self.upper_for_change().await?;
        upper.setxattr(name, value, how).await
    }

    async fn listxattr(&self) -> FsResult<Vec<Vec<u8>>> {
        let mut names = self.real().listxattr().await?;
        names.retain(|name| !is_private(name));
        Ok(names)
    }

    async fn removexattr(&self, name: &[u8]) -> FsResult<()> {
        if is_private(name) {
            return Err(Errno::EOPNOTSUPP);
        }
        let upper = self.upper_for_change().await?;
        upper.removexattr(name).await
    }
}

/// Copies `size` bytes of data, the extended attributes, and then the mode and times of `lower`
/// (whose metadata is `attr`) to its fresh upper copy `upper`, opened for writing as `handle`
/// if it is a regular file.
async fn fill_copy(
    lower: &Arc<dyn Node>,
    upper: &Arc<dyn Node>,
    handle: Option<&dyn Handle>,
    attr: &Attr,
    size: u64,
) -> FsResult<()> {
    if let Some(handle) = handle {
        copy_data(lower, handle, size).await?;
        handle.set_size(size).await?;
    }
    copy_xattrs(lower, upper).await?;
    let set = SetAttr {
        mode: (attr.file_type != FileType::Symlink).then_some(attr.mode),
        ..restore_times(attr)
    };
    match upper.setattr(&set).await {
        // Not every host can stamp a symlink.
        Err(_) if attr.file_type == FileType::Symlink => Ok(()),
        other => other.map(drop),
    }
}

fn restore_times(attr: &Attr) -> SetAttr {
    SetAttr {
        atime: Some(TimeSet::At(attr.atime)),
        mtime: Some(TimeSet::At(attr.mtime)),
        ..SetAttr::default()
    }
}

/// Copies the data ranges of a regular file to `dst`, leaving holes as holes.
async fn copy_data(lower: &Arc<dyn Node>, dst: &dyn Handle, size: u64) -> FsResult<()> {
    if size == 0 {
        return Ok(());
    }
    let src = lower
        .open(OpenOptions {
            read: true,
            noatime: true,
            ..OpenOptions::default()
        })
        .await?;
    let mut buf = vec![0; COPY_CHUNK as usize];
    let mut offset = 0;
    while offset < size {
        offset = match src.seek_hole(offset, size, SeekHole::Data).await {
            Ok(data) => data,
            Err(Errno::ENXIO) => break,
            Err(err) => return Err(err),
        };
        let hole = src.seek_hole(offset, size, SeekHole::Hole).await?;
        while offset < hole {
            let want = (hole - offset).min(COPY_CHUNK) as usize;
            let n = src.read_at(&mut buf[..want], offset).await?;
            if n == 0 {
                return Ok(());
            }
            let mut done = 0;
            while done < n {
                done += dst.write_at(&buf[done..n], offset + done as u64).await?;
            }
            offset += n as u64;
        }
    }
    Ok(())
}

async fn copy_xattrs(lower: &Arc<dyn Node>, upper: &Arc<dyn Node>) -> FsResult<()> {
    let names = match lower.listxattr().await {
        Ok(names) => names,
        Err(Errno::EOPNOTSUPP) => return Ok(()),
        Err(err) => return Err(err),
    };
    for name in names.iter().filter(|name| !is_private(name)) {
        let value = match lower.getxattr(name).await {
            Ok(value) => value,
            Err(Errno::ENODATA) => continue,
            Err(err) => return Err(err),
        };
        match upper.setxattr(name, &value, XattrSet::Any).await {
            Ok(()) | Err(Errno::EOPNOTSUPP) => {}
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

fn is_private(name: &[u8]) -> bool {
    XATTR_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

async fn get_overlay_xattr(node: &Arc<dyn Node>, suffix: &[u8]) -> Option<Vec<u8>> {
    for prefix in XATTR_PREFIXES {
        if let Ok(value) = node.getxattr(&[prefix, suffix].concat()).await {
            return Some(value);
        }
    }
    None
}

/// Sets `overlay.<suffix>`: trusted if allowed, else user.
async fn set_overlay_xattr(node: &Arc<dyn Node>, suffix: &[u8], value: &[u8]) -> FsResult<()> {
    let mut result = Ok(());
    for prefix in XATTR_PREFIXES {
        result = node
            .setxattr(&[prefix, suffix].concat(), value, XattrSet::Any)
            .await;
        match result {
            Err(Errno::EPERM | Errno::EOPNOTSUPP) => continue,
            _ => break,
        }
    }
    result
}

/// `(opaque, may hold xattr whiteouts)` of a real directory.
async fn dir_flags(dir: &Arc<dyn Node>) -> (bool, bool) {
    match get_overlay_xattr(dir, OPAQUE).await.as_deref() {
        Some(b"y") => (true, false),
        Some(b"x") => (false, true),
        _ => (false, false),
    }
}

async fn is_whiteout(node: &Arc<dyn Node>, xwhiteouts: bool) -> FsResult<bool> {
    Ok(match node.file_type() {
        FileType::CharDevice => node.getattr().await?.rdev == Device::default(),
        FileType::Regular if xwhiteouts => {
            node.getattr().await?.size == 0 && get_overlay_xattr(node, WHITEOUT).await.is_some()
        }
        _ => false,
    })
}

/// Every entry of a real directory as `(name, ino, type)`.
async fn read_all(dir: &Arc<dyn Node>) -> FsResult<Vec<(Vec<u8>, u64, FileType)>> {
    struct Collect(Vec<(Vec<u8>, u64, FileType)>);
    impl DirSink for Collect {
        fn push(&mut self, entry: DirEntry<'_>) -> bool {
            self.0
                .push((entry.name.to_vec(), entry.ino, entry.file_type));
            true
        }
    }
    let handle = dir
        .open(OpenOptions {
            read: true,
            noatime: true,
            ..OpenOptions::default()
        })
        .await?;
    let mut sink = Collect(Vec::new());
    handle.readdir(0, &mut sink).await?;
    Ok(sink.0)
}

struct Entry {
    name: Vec<u8>,
    ino: u64,
    file_type: FileType,
}

/// An open directory. Its merged listing is read when reading starts at 0 and cookies index into
/// it, as Linux's `ovl_dir_cache`: a listing stays stable while the directory changes.
struct DirHandle {
    node: Arc<OverlayNode>,
    snapshot: Mutex<Option<Arc<Vec<Entry>>>>,
}

#[async_trait(?Send)]
impl Handle for DirHandle {
    async fn readdir(&self, cookie: u64, sink: &mut dyn DirSink) -> FsResult<()> {
        let cached = self.snapshot.lock().unwrap().clone();
        let entries = match cached {
            Some(entries) if cookie != 0 => entries,
            _ => {
                let entries = Arc::new(self.node.merged_entries().await?);
                *self.snapshot.lock().unwrap() = Some(entries.clone());
                entries
            }
        };
        for (i, entry) in entries.iter().enumerate().skip(cookie as usize) {
            let pushed = sink.push(DirEntry {
                name: &entry.name,
                ino: entry.ino,
                file_type: entry.file_type,
                next: i as u64 + 1,
            });
            if !pushed {
                break;
            }
        }
        Ok(())
    }
}

/// An open non-directory. A handle opened for reading before a copy-up switches to the upper
/// copy on its next use (Linux's `ovl_real_fdget` since 4.19).
struct FileHandle {
    node: Arc<OverlayNode>,
    options: OpenOptions,
    /// `(opened on the upper object, handle)`.
    real: Mutex<(bool, Arc<dyn Handle>)>,
}

impl FileHandle {
    /// A handle on an object that is in the upper layer already.
    fn upper(
        node: Arc<OverlayNode>,
        options: OpenOptions,
        handle: Box<dyn Handle>,
    ) -> Box<dyn Handle> {
        Box::new(Self {
            node,
            options,
            real: Mutex::new((true, Arc::from(handle))),
        })
    }

    async fn current(&self) -> FsResult<Arc<dyn Handle>> {
        let (in_upper, handle) = self.real.lock().unwrap().clone();
        if in_upper {
            return Ok(handle);
        }
        let Some(upper) = self.node.upper_node() else {
            return Ok(handle);
        };
        let handle: Arc<dyn Handle> = Arc::from(upper.open(self.options).await?);
        *self.real.lock().unwrap() = (true, handle.clone());
        Ok(handle)
    }
}

#[async_trait(?Send)]
impl Handle for FileHandle {
    async fn read_at(&self, buf: &mut [u8], offset: u64) -> FsResult<usize> {
        self.current().await?.read_at(buf, offset).await
    }

    async fn write_at(&self, data: &[u8], offset: u64) -> FsResult<usize> {
        self.current().await?.write_at(data, offset).await
    }

    async fn append(&self, data: &[u8]) -> FsResult<(u64, usize)> {
        self.current().await?.append(data).await
    }

    async fn sync(&self, data_only: bool) -> FsResult<()> {
        self.current().await?.sync(data_only).await
    }

    async fn allocate(&self, mode: AllocMode, offset: u64, len: u64) -> FsResult<()> {
        self.current().await?.allocate(mode, offset, len).await
    }

    async fn set_size(&self, size: u64) -> FsResult<()> {
        self.current().await?.set_size(size).await
    }

    async fn seek_hole(&self, offset: u64, size: u64, which: SeekHole) -> FsResult<u64> {
        self.current().await?.seek_hole(offset, size, which).await
    }

    /// The lower file's, until a copy-up makes the upper one current.
    async fn shared_contents(&self) -> Option<SharedContents> {
        self.current().await.ok()?.shared_contents().await
    }
}

#[cfg(test)]
mod tests;
