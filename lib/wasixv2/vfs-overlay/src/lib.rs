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
//! that, and replacing an entry by a whiteout is two steps.

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
    AllocMode, Attr, Device, DirEntry, DirSink, FileType, Filesystem, FsInfo, FsResult, Handle,
    NewKind, NewNode, Node, OpenOptions, RenameMode, SeekHole, SetAttr, StatFs, TimeSet, XattrSet,
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
            }),
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
        let layers = self.shared.upper.iter().chain(&self.shared.lowers);
        FsInfo {
            magic: OVERLAYFS_SUPER_MAGIC,
            name_max: layers.map(|fs| fs.info().name_max).max().unwrap_or(0),
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
    /// Serialises namespace and metadata changes, standing in for the parent's `i_rwsem` and
    /// the copy-up lock: several of them are multi-step on the upper layer.
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
        *overflow.entry((layer, ino)).or_insert_with(|| {
            OVERFLOW_LAYER << LAYER_SHIFT | self.next_overflow.fetch_add(1, Ordering::Relaxed)
        })
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

    /// The overlay object for `found`, reached as `name` in `parent`.
    fn node(&self, found: Vec<Real>, parent: &Arc<OverlayNode>, name: &[u8]) -> Arc<OverlayNode> {
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
            return node;
        }
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
            ino: self.ino_for(&all),
            file_type,
            this: this.clone(),
            shared,
            state: Mutex::new(State {
                upper,
                lowers,
                parent: Some(parent.clone()),
                name: name.to_vec(),
                key: Some(key.clone()),
            }),
        });
        self.nodes
            .lock()
            .unwrap()
            .insert(key, Arc::downgrade(&node));
        node
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
            node.copy_up_one().await?;
        }
        Ok(self.upper_node().expect("copied up"))
    }

    /// Copies this object up into its parent's upper directory, which exists.
    async fn copy_up_one(&self) -> FsResult<()> {
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
        let kind = match self.file_type {
            FileType::Regular => NewKind::Regular,
            FileType::Directory => NewKind::Directory,
            FileType::Symlink => NewKind::Symlink(&target),
            FileType::Fifo => NewKind::Fifo,
            FileType::Socket => NewKind::Socket,
            FileType::CharDevice => NewKind::CharDevice(attr.rdev),
            FileType::BlockDevice => NewKind::BlockDevice(attr.rdev),
        };
        let new = NewNode {
            kind,
            mode: attr.mode,
            uid: attr.uid,
            gid: attr.gid,
        };
        // A regular file is filled while unnamed where possible, so a failed or interrupted
        // copy never shadows the lower file with a partial one.
        let (upper, named) = match kind {
            NewKind::Regular => match parent_upper.create_unnamed(&new).await {
                Ok(file) => (file, false),
                Err(Errno::EOPNOTSUPP) => (parent_upper.create(&name, &new).await?, true),
                Err(err) => return Err(err),
            },
            _ => (parent_upper.create(&name, &new).await?, true),
        };
        let mut result = fill_copy(&lower, &upper, &attr).await;
        if result.is_ok() && !named {
            result = parent_upper.link(&name, &upper).await;
        }
        if let Err(err) = result {
            if named {
                let _ = match self.is_dir() {
                    true => parent_upper.rmdir(&name).await,
                    false => parent_upper.unlink(&name).await,
                };
            }
            return Err(err);
        }
        // Creating the entry stamped the parent; it keeps the times it had (as Linux).
        let _ = parent_upper.setattr(&restore_times(&parent_attr)).await;

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
            state.key.replace(Key::Upper(upper.ino()))
        };
        self.shared
            .origins
            .lock()
            .unwrap()
            .insert(upper.ino(), self.ino);
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
        let file = upper
            .create(
                name,
                &NewNode {
                    kind: NewKind::Regular,
                    ..device
                },
            )
            .await?;
        if let Err(err) = set_overlay_xattr(&file, WHITEOUT, b"y").await {
            let _ = upper.unlink(name).await;
            return Err(err);
        }
        Ok(())
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

    fn moved(&self, parent: &Arc<OverlayNode>, name: &[u8]) {
        let mut state = self.state.lock().unwrap();
        state.parent = Some(parent.clone());
        state.name = name.to_vec();
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
        let _guard = self.shared.write_lock.lock().await;
        let upper = self.arc().copy_up().await?;
        let attr = upper.setattr(attr).await?;
        Ok(self.fix_attr(attr))
    }

    async fn lookup(&self, name: &[u8]) -> FsResult<Arc<dyn Node>> {
        self.check_dir()?;
        let found = self.find(name).await?;
        if found.is_empty() {
            return Err(Errno::ENOENT);
        }
        Ok(self.shared.node(found, &self.arc(), name))
    }

    async fn create(&self, name: &[u8], new: &NewNode<'_>) -> FsResult<Arc<dyn Node>> {
        self.check_dir()?;
        let _guard = self.shared.write_lock.lock().await;
        if !self.find(name).await?.is_empty() {
            return Err(Errno::EEXIST);
        }
        let upper = self.arc().copy_up().await?;
        let over_whiteout = self.clear_whiteout(&upper, name).await?;
        let created = upper.create(name, new).await;
        // A directory replacing a whiteout must not merge with what the whiteout hid.
        let created = match created {
            Ok(child) if over_whiteout && new.kind == NewKind::Directory => {
                match set_overlay_xattr(&child, OPAQUE, b"y").await {
                    Ok(()) => Ok(child),
                    Err(err) => {
                        let _ = upper.rmdir(name).await;
                        Err(err)
                    }
                }
            }
            other => other,
        };
        let child = match created {
            Ok(child) => child,
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
        Ok(self.shared.node(found, &self.arc(), name))
    }

    async fn create_unnamed(&self, new: &NewNode<'_>) -> FsResult<Arc<dyn Node>> {
        self.check_dir()?;
        let _guard = self.shared.write_lock.lock().await;
        let upper = self.arc().copy_up().await?;
        let child = upper.create_unnamed(new).await?;
        let found = vec![Real {
            layer: UPPER,
            node: child,
            xwhiteouts: false,
        }];
        Ok(self.shared.node(found, &self.arc(), b""))
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
        if top.layer == UPPER {
            upper.unlink(name).await?;
            self.shared.forget_if_gone(&top.node).await;
        }
        if positive {
            self.make_whiteout(&upper, name).await?;
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
        let child = self.shared.node(found, &self.arc(), name);
        child.check_dir()?;
        if !child.merged_entries().await?.is_empty() {
            return Err(Errno::ENOTEMPTY);
        }
        let upper = self.arc().copy_up().await?;
        let positive = self.lower_positive(name).await?;
        if in_upper {
            child.clear_upper_whiteouts().await?;
            let child_upper = child.upper_node().expect("in the upper layer");
            upper.rmdir(name).await?;
            self.shared.forget_if_gone(&child_upper).await;
        }
        if positive {
            self.make_whiteout(&upper, name).await?;
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
        let source = self.shared.node(found, &old_parent, name);
        // Without `redirect_dir`, a directory with lower parts cannot move (as Linux).
        if source.is_dir() && source.has_lowers() {
            return Err(Errno::EXDEV);
        }
        let found = new_parent.find(new_name).await?;
        let target = match found.is_empty() {
            true => None,
            false => Some(self.shared.node(found, &new_parent, new_name)),
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
        old_upper.rename(name, &new_upper, new_name, mode).await?;

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
            if source_positive {
                old_parent.make_whiteout(&old_upper, name).await?;
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
        let (real, in_upper) = if options.write {
            let _guard = self.shared.write_lock.lock().await;
            (self.arc().copy_up().await?, true)
        } else {
            let in_upper = self.upper().is_some();
            (self.real(), in_upper)
        };
        let handle = real.open(options).await?;
        Ok(Box::new(FileHandle {
            node: self.arc(),
            options,
            real: Mutex::new((in_upper, Arc::from(handle))),
        }))
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
        let _guard = self.shared.write_lock.lock().await;
        let upper = self.arc().copy_up().await?;
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
        let _guard = self.shared.write_lock.lock().await;
        let upper = self.arc().copy_up().await?;
        upper.removexattr(name).await
    }
}

/// Copies data, extended attributes and times from `lower` to its fresh upper copy `upper`.
async fn fill_copy(lower: &Arc<dyn Node>, upper: &Arc<dyn Node>, attr: &Attr) -> FsResult<()> {
    if attr.file_type == FileType::Regular {
        copy_data(lower, upper, attr.size).await?;
    }
    copy_xattrs(lower, upper).await?;
    let mut set = restore_times(attr);
    if attr.file_type == FileType::Regular {
        set.size = Some(attr.size);
    }
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

/// Copies the data ranges of a regular file, leaving holes as holes.
async fn copy_data(lower: &Arc<dyn Node>, upper: &Arc<dyn Node>, size: u64) -> FsResult<()> {
    let src = lower
        .open(OpenOptions {
            read: true,
            noatime: true,
            ..OpenOptions::default()
        })
        .await?;
    let dst = upper
        .open(OpenOptions {
            write: true,
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

    async fn seek_hole(&self, offset: u64, size: u64, which: SeekHole) -> FsResult<u64> {
        self.current().await?.seek_hole(offset, size, which).await
    }
}

#[cfg(test)]
mod tests;
