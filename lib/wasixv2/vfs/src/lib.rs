//! The single VFS: mount trees, path walking, the name cache, permission checks, file locks,
//! change notification, and the inode-based traits filesystem drivers implement.
//!
//! Drivers ([`Filesystem`], [`Node`], [`Handle`]) only store data. Everything a program can
//! observe about paths and permissions is decided here, following Linux's `fs/` code; the kernel
//! decodes syscall arguments and calls [`Vfs`].

mod cred;
mod dentry;
mod driver;
mod export;
mod file;
mod fileattr;
mod inode;
mod locks;
mod mount;
mod namei;
mod namespace;
mod notify;
mod ops;
mod pnode;
mod posix_acl;
mod quota;
mod seals;
mod sync;
mod types;

use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

pub use cred::{
    Cap, Cred, InitialUserNs, MAY_EXEC, MAY_READ, MAY_WRITE, UserNs, generic_permission,
    generic_permission_acl, initial_user_ns, same_user_ns,
};
pub use dentry::Dentry;
pub use driver::{Filesystem, Handle, Link, MAX_DIR_COOKIE, Node, SharedContents};
pub use export::{HandleKind, MAX_HANDLE_SZ};
pub use file::{DIR_POS_EOD, File};
pub use fileattr::{
    FS_APPEND_FL, FS_IMMUTABLE_FL, FS_NOATIME_FL, FS_NODUMP_FL, FS_XFLAG_RDONLY_MASK,
    FS_XFLAGS_MASK, FileKattr,
};
pub use inode::{Bound, Inode};
pub use locks::{
    FlockOp, LeaseHolder, LeaseWait, LockKey, LockKind, LockManager, LockOwner, RangeLock,
};
pub use mount::{MNT_UNIQUE_ID_OFFSET, MntNs, Mount, MountFlags, Path, Superblock};
use mount::{Tree, TreeSeq};
pub use namei::{Last, LookupFlags, MAX_SYMLINKS};
pub use namespace::{MountAttr, MountInfo, MountOp, UmountFlags};
pub use notify::{EventInfo, FsEvent, FsWatcher, MountEvent, Object as FsObject, Perm, PermAnswer};
pub use ops::{Ctx, FMODE_EXEC, OpenHow, XATTR_NAME_MAX, XATTR_SIZE_MAX};
pub use pnode::Propagation;
pub use posix_acl::{
    AclEntry, AclKind, PosixAcl, XATTR_NAME_POSIX_ACL_ACCESS, XATTR_NAME_POSIX_ACL_DEFAULT,
};
pub use quota::QuotaOps;
pub use seals::WRITE_SEALS;
use sync::MutexExt;
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
    /// Every filesystem instance in use (`super_blocks`).
    supers: std::sync::Mutex<Vec<std::sync::Weak<Superblock>>>,
    pub locks: LockManager,
    watcher: std::sync::OnceLock<Arc<dyn FsWatcher>>,
}

impl Vfs {
    /// A VFS whose root is `root`.
    pub fn new(root: Arc<dyn Filesystem>) -> Arc<Self> {
        let tree_seq = Arc::new(TreeSeq::default());
        let tree = Tree::new(tree_seq.clone());
        let next_anon = AtomicU32::new(1);
        let sb = new_superblock(&tree, &next_anon, root, initial_user_ns());
        let root_dentry = Dentry::root(&sb, sb.fs.root());
        *sb.root.lock_or_recover() = Arc::downgrade(&root_dentry);
        let devname = sb.info.name.as_bytes().to_vec();
        let mount = Arc::new(Mount::new(
            tree.next_id(),
            root_dentry,
            MountFlags::default(),
            devname,
        ));
        let ns = {
            let mut state = tree.lock();
            let ns = MntNs::new(&tree, &mut state, mount.clone(), None, initial_user_ns());
            state.add_link(&mount, ns.id());
            ns
        };
        Arc::new(Self {
            tree,
            ns,
            tree_seq,
            next_anon,
            supers: std::sync::Mutex::new(vec![Arc::downgrade(&sb)]),
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

    /// The root dentry of `fs`'s instance: the one in use if `fs` is mounted already (`sget`
    /// of a filesystem with a single instance, as devtmpfs), else a new one, which belongs to
    /// the user namespace `owner`. Returns whether it is new.
    pub(crate) fn fs_root(
        &self,
        fs: Arc<dyn Filesystem>,
        owner: Arc<dyn UserNs>,
    ) -> (Arc<Dentry>, bool) {
        let mut supers = self.supers.lock_or_recover();
        supers.retain(|sb| sb.strong_count() > 0);
        let same = |sb: &Superblock| std::ptr::addr_eq(Arc::as_ptr(&sb.fs), Arc::as_ptr(&fs));
        if let Some(sb) = supers
            .iter()
            .filter_map(|sb| sb.upgrade())
            .find(|sb| same(sb))
        {
            let mut root = sb.root.lock_or_recover();
            if let Some(dentry) = root.upgrade() {
                return (dentry, false);
            }
            let dentry = Dentry::root(&sb, sb.fs.root());
            *root = Arc::downgrade(&dentry);
            return (dentry, false);
        }
        let sb = new_superblock(&self.tree, &self.next_anon, fs, owner);
        let dentry = Dentry::root(&sb, sb.fs.root());
        *sb.root.lock_or_recover() = Arc::downgrade(&dentry);
        supers.push(Arc::downgrade(&sb));
        (dentry, true)
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
        let op = MountOp::New {
            fs,
            source,
            flags,
            sb_read_only: false,
            owner: initial_user_ns(),
        };
        self.do_mount(&self.ns, &target, op)
    }

    /// Mounts `fs` outside the mount tree (`kern_mount`), for filesystems whose objects have no
    /// name: those behind pipes, sockets and event descriptors. Their paths come from
    /// [`Mount::pseudo_path`].
    pub fn kern_mount(&self, fs: Arc<dyn Filesystem>) -> Arc<Mount> {
        self.kern_mount_owned(fs, initial_user_ns())
    }

    /// [`Vfs::kern_mount`] of a filesystem that belongs to the user namespace `owner`
    /// (`fc->user_ns`), as an IPC namespace's message queue filesystem belongs to the
    /// namespace's owner.
    pub fn kern_mount_owned(&self, fs: Arc<dyn Filesystem>, owner: Arc<dyn UserNs>) -> Arc<Mount> {
        let (root, _) = self.fs_root(fs, owner);
        let devname = root.sb().info.name.as_bytes().to_vec();
        Arc::new(Mount::new(
            self.next_id(),
            root,
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
            leases: None,
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
            leases: None,
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
        assert!(self.tree.watcher.set(watcher.clone()).is_ok());
        assert!(
            self.watcher.set(watcher).is_ok(),
            "the VFS watcher is installed once"
        );
    }

    /// The device of the filesystem instance with id `id`, while it is in use.
    pub fn sb_dev(&self, id: u64) -> Option<Device> {
        self.superblock(id).map(|sb| sb.dev)
    }

    /// The filesystem instance with id `id`, while it is in use.
    pub(crate) fn superblock(&self, id: u64) -> Option<Arc<Superblock>> {
        let supers = self.supers.lock_or_recover();
        supers
            .iter()
            .filter_map(|sb| sb.upgrade())
            .find(|sb| sb.id == id)
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
            let info = notify::EventInfo {
                parent: None,
                path: None,
                is_dir: false,
            };
            watcher.fsnotify(&event, &info);
        }
    }

    /// Reports a data-access event on the object `path` names, as [`Vfs::notify_object`]
    /// does, with the path it was used through (`fsnotify_file`).
    pub(crate) fn notify_path(&self, path: &Path, event: FsEvent<'_>) {
        self.notify_dentry(&path.dentry, Some(path), event);
    }

    /// `fsnotify_open_perm` and `fsnotify_file_area_perm`: the watches' answer to `perm` on
    /// `path`, waited for as `cx` waits (killable only); allowed when nothing watches or the
    /// caller cannot wait.
    pub async fn permission_event(&self, cx: &Ctx<'_>, path: &Path, perm: Perm) -> FsResult<()> {
        let Some(wait) = cx.leases else {
            return Ok(());
        };
        let Some(answer) = self.watching().and_then(|w| w.permission(path, perm)) else {
            return Ok(());
        };
        wait.wait_killable(answer).await
    }

    /// Reports an event on the object `dentry` names, to its own watches and then to its
    /// parent directory's, with its name; for a removed name, the directory it was in
    /// (`fsnotify_parent` on a `d_unlinked` dentry).
    pub(crate) fn notify_object(&self, dentry: &Arc<Dentry>, event: FsEvent<'_>) {
        self.notify_dentry(dentry, None, event);
    }

    fn notify_dentry(&self, dentry: &Arc<Dentry>, path: Option<&Path>, event: FsEvent<'_>) {
        let Some(watcher) = self.watching() else {
            return;
        };
        let unlinked = dentry.is_unlinked();
        match unlinked {
            true => watcher.unlinked_event(&event),
            false => watcher.event(&event),
        }
        let mut parent = None;
        dentry.with_parent(|dir, name| {
            let dir = notify::Object {
                sb: dir.sb().id,
                ino: dir.node().ino(),
            };
            watcher.child_event(dir, name, &event, unlinked);
            parent = Some((dir, name.to_vec()));
        });
        let info = notify::EventInfo {
            parent: parent.as_ref().map(|(dir, name)| (*dir, name.as_slice())),
            path,
            is_dir: dentry.is_dir(),
        };
        watcher.fsnotify(&event, &info);
    }
}

/// A new anonymous device. They have major 0, like the ones Linux hands tmpfs.
fn anon_dev(next_anon: &AtomicU32) -> Device {
    Device {
        major: 0,
        minor: next_anon.fetch_add(1, Ordering::Relaxed) & 0xfffff,
    }
}

fn new_superblock(
    tree: &Tree,
    next_anon: &AtomicU32,
    fs: Arc<dyn Filesystem>,
    owner: Arc<dyn UserNs>,
) -> Arc<Superblock> {
    let id = tree.next_id();
    let info = fs.info();
    let dev = info.dev.unwrap_or_else(|| anon_dev(next_anon));
    Arc::new(Superblock {
        id,
        dev,
        fs,
        read_only: std::sync::atomic::AtomicBool::new(info.read_only),
        info,
        owner,
        rename_lock: futures_util::lock::Mutex::new(()),
        tree_seq: tree.seq.clone(),
        inodes: Default::default(),
        sub_devs: Default::default(),
        root: Default::default(),
        writers: Default::default(),
    })
}
