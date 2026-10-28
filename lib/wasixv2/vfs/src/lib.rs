//! The single VFS: mount trees, path walking, the name cache, permission checks, file locks,
//! change notification, and the inode-based traits filesystem drivers implement.
//!
//! Drivers ([`Filesystem`], [`Node`], [`Handle`]) only store data. Everything a program can
//! observe about paths and permissions is decided here, following Linux's `fs/` code; the kernel
//! decodes syscall arguments and calls [`Vfs`].

mod cred;
mod dentry;
mod driver;
mod file;
mod inode;
mod locks;
mod mount;
mod namei;
mod namespace;
mod notify;
mod ops;
mod pnode;
mod seals;
mod sync;
mod types;

use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

pub use cred::{Cap, Cred, MAY_EXEC, MAY_READ, MAY_WRITE, generic_permission};
pub use dentry::Dentry;
pub use driver::{Filesystem, Handle, Link, MAX_DIR_COOKIE, Node, SharedContents};
pub use file::{DIR_POS_EOD, File};
pub use inode::{Bound, Inode};
pub use locks::{FlockOp, LockKey, LockKind, LockManager, LockOwner, RangeLock};
pub use mount::{MntNs, Mount, MountFlags, Path, Superblock};
use mount::{Tree, TreeSeq};
pub use namei::{Last, LookupFlags, MAX_SYMLINKS};
pub use namespace::{MountAttr, MountInfo, MountOp, UmountFlags};
pub use notify::{FsEvent, FsWatcher, Object as FsObject};
pub use ops::{Ctx, OpenHow, XATTR_NAME_MAX, XATTR_SIZE_MAX};
pub use pnode::Propagation;
pub use seals::WRITE_SEALS;
pub use types::*;
pub use wasixv2_abi::Errno;

/// Longest path, including its terminating NUL, that a syscall accepts.
pub const PATH_MAX: usize = 4096;
/// Longest single path component.
pub const NAME_MAX: usize = 255;

/// The VFS of one machine: its mount table, lock tables and watchers.
pub struct Vfs {
    pub(crate) tree: Arc<Tree>,
    /// The initial mount namespace, which the embedder's calls change.
    ns: Arc<MntNs>,
    pub(crate) tree_seq: Arc<TreeSeq>,
    /// Minor numbers of anonymous devices (Linux's `get_anon_bdev`).
    next_anon: AtomicU32,
    pub locks: LockManager,
    watcher: std::sync::OnceLock<Arc<dyn FsWatcher>>,
}

impl Vfs {
    /// A VFS whose root is `root`.
    pub fn new(root: Arc<dyn Filesystem>) -> Arc<Self> {
        let tree_seq = Arc::new(TreeSeq::default());
        let tree = Tree::new(tree_seq.clone());
        let next_anon = AtomicU32::new(1);
        let sb = new_superblock(&tree, &next_anon, root);
        let root_dentry = Dentry::root(&sb, sb.fs.root());
        let devname = sb.info.name.as_bytes().to_vec();
        let mount = Arc::new(Mount::new(
            tree.next_id(),
            root_dentry,
            MountFlags::default(),
            devname,
        ));
        let ns = MntNs::new(&tree, mount.clone());
        tree.lock().add_link(&mount, ns.id());
        Arc::new(Self {
            tree,
            ns,
            tree_seq,
            next_anon,
            locks: LockManager::default(),
            watcher: std::sync::OnceLock::new(),
        })
    }

    /// The initial mount namespace.
    pub fn namespace(&self) -> &Arc<MntNs> {
        &self.ns
    }

    /// The root of the initial namespace's tree.
    pub fn root(&self) -> Path {
        self.ns.root()
    }

    /// A fresh identifier, unique in this VFS: an open file's ([`File::id`]), which owns its
    /// `flock` and OFD locks.
    pub fn next_id(&self) -> u64 {
        self.tree.next_id()
    }

    pub(crate) fn new_superblock(&self, fs: Arc<dyn Filesystem>) -> Arc<Superblock> {
        new_superblock(&self.tree, &self.next_anon, fs)
    }

    /// Mounts `fs` on the directory `at` of the initial namespace (resolved from its root with
    /// full privileges), as the embedder configures the machine. With `create`, missing
    /// directories along `at` are made first, as `mkdir -p` would.
    pub async fn mount(
        &self,
        at: &[u8],
        fs: Arc<dyn Filesystem>,
        flags: MountFlags,
        create: bool,
    ) -> FsResult<()> {
        let source = fs.info().name.as_bytes().to_vec();
        let target = self.embedder_target(at, create).await?;
        self.do_mount(&self.ns, &target, MountOp::New { fs, source, flags })
    }

    /// Mounts `fs` outside the mount tree (`kern_mount`), for filesystems whose objects have no
    /// name: those behind pipes, sockets and event descriptors. Their paths come from
    /// [`Mount::pseudo_path`].
    pub fn kern_mount(&self, fs: Arc<dyn Filesystem>) -> Arc<Mount> {
        let sb = self.new_superblock(fs);
        let devname = sb.info.name.as_bytes().to_vec();
        Arc::new(Mount::new(
            self.next_id(),
            Dentry::root(&sb, sb.fs.root()),
            MountFlags::default(),
            devname,
        ))
    }

    /// Bind-mounts the tree at `from` on `at` (`mount --bind`), in the initial namespace, with
    /// `flags` for the new mount.
    pub async fn bind_mount(
        &self,
        from: &[u8],
        at: &[u8],
        flags: MountFlags,
        create: bool,
    ) -> FsResult<()> {
        let cred = Cred::root();
        let root = self.root();
        let cx = Ctx {
            cred: &cred,
            root: &root,
            umask: 0o022,
            task: 0,
        };
        let source = self.lookup(&cx, &root, from, LookupFlags::follow()).await?;
        let target = self.embedder_target(at, create).await?;
        let bind = MountOp::Bind {
            source,
            recursive: false,
        };
        self.do_mount(&self.ns, &target, bind)?;
        let mounted = self.embedder_target(at, false).await?;
        self.do_mount(&self.ns, &mounted, MountOp::Reconfigure { flags })
    }

    /// `at`, resolved from the initial namespace's root with full privileges, made first
    /// with `create`.
    async fn embedder_target(&self, at: &[u8], create: bool) -> FsResult<Path> {
        let cred = Cred::root();
        let root = self.root();
        let cx = Ctx {
            cred: &cred,
            root: &root,
            umask: 0o022,
            task: 0,
        };
        if create {
            self.mkdir_all(&cx, at).await?;
        }
        self.lookup(&cx, &root, at, LookupFlags::follow()).await
    }

    /// Detaches the topmost mount on `at` (resolved with full privileges), whoever uses it.
    pub async fn umount(&self, at: &[u8]) -> FsResult<()> {
        let path = self.embedder_target(at, false).await?;
        if !path.is_mount_root() {
            return Err(Errno::EINVAL);
        }
        if path.mnt.parent().is_none() {
            return Err(Errno::EBUSY);
        }
        let root = self.root();
        let detach = UmountFlags {
            detach: true,
            ..UmountFlags::default()
        };
        self.do_umount(&self.ns, &root, &path, detach)
    }

    /// Installs the watcher that receives change notifications. A VFS has at most one, set
    /// before it is used, so reporting a change never takes a lock.
    pub fn set_watcher(&self, watcher: Arc<dyn FsWatcher>) {
        assert!(
            self.watcher.set(watcher).is_ok(),
            "the VFS watcher is installed once"
        );
    }

    /// The `st_dev` of an object the driver says lives on its own device `dev`: an anonymous
    /// device of the VFS's, the same one each time, so host device numbers never reach the
    /// guest or collide with the VFS's own.
    pub(crate) fn sub_dev(&self, sb: &Superblock, dev: Device) -> Device {
        *sb.sub_devs
            .lock()
            .unwrap()
            .entry(dev)
            .or_insert_with(|| anon_dev(&self.next_anon))
    }

    /// The watcher, if it watches anything: reads and writes of unwatched files then cost no
    /// more than this check (`fsnotify`'s early return on an empty mask).
    fn watching(&self) -> Option<&Arc<dyn FsWatcher>> {
        self.watcher.get().filter(|watcher| watcher.watching())
    }

    pub(crate) fn notify(&self, event: FsEvent<'_>) {
        if let Some(watcher) = self.watching() {
            watcher.event(&event);
        }
    }

    /// Reports an event on the object `dentry` names, to its own watches and then to its
    /// parent directory's, with its name; for a removed name, the directory it was in
    /// (`fsnotify_parent` on a `d_unlinked` dentry).
    pub(crate) fn notify_object(&self, dentry: &Arc<Dentry>, event: FsEvent<'_>) {
        let Some(watcher) = self.watching() else {
            return;
        };
        let unlinked = dentry.is_unlinked();
        match unlinked {
            true => watcher.unlinked_event(&event),
            false => watcher.event(&event),
        }
        dentry.with_parent(|parent, name| {
            let dir = notify::Object {
                sb: parent.sb().id,
                ino: parent.node().ino(),
            };
            watcher.child_event(dir, name, &event, unlinked);
        });
    }
}

/// A new anonymous device. They have major 0, like the ones Linux hands tmpfs.
fn anon_dev(next_anon: &AtomicU32) -> Device {
    Device {
        major: 0,
        minor: next_anon.fetch_add(1, Ordering::Relaxed) & 0xfffff,
    }
}

fn new_superblock(tree: &Tree, next_anon: &AtomicU32, fs: Arc<dyn Filesystem>) -> Arc<Superblock> {
    let id = tree.next_id();
    let info = fs.info();
    let dev = info.dev.unwrap_or_else(|| anon_dev(next_anon));
    Arc::new(Superblock {
        id,
        dev,
        fs,
        read_only: std::sync::atomic::AtomicBool::new(info.read_only),
        info,
        rename_lock: futures_util::lock::Mutex::new(()),
        tree_seq: tree.seq.clone(),
        inodes: Default::default(),
        sub_devs: Default::default(),
    })
}
