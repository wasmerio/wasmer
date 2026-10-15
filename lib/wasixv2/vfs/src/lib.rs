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
mod notify;
mod ops;
mod types;

use std::sync::{
    Arc,
    atomic::{AtomicU32, AtomicU64, Ordering},
};

pub use cred::{Cap, Cred, MAY_EXEC, MAY_READ, MAY_WRITE, generic_permission};
pub use dentry::Dentry;
pub use driver::{Filesystem, Handle, MAX_DIR_COOKIE, Node};
pub use file::{DIR_POS_EOD, File};
pub use inode::{Bound, Inode};
pub use locks::{FlockOp, LockKey, LockKind, LockManager, LockOwner, RangeLock};
pub use mount::{Mount, MountFlags, Namespace, Path, Superblock};
pub use namei::{Last, LookupFlags, MAX_SYMLINKS};
pub use notify::{FsEvent, FsWatcher, Object as FsObject};
pub use ops::{Ctx, OpenHow, XATTR_NAME_MAX, XATTR_SIZE_MAX};
pub use types::*;
pub use wasixv2_abi::Errno;

/// Longest path, including its terminating NUL, that a syscall accepts.
pub const PATH_MAX: usize = 4096;
/// Longest single path component.
pub const NAME_MAX: usize = 255;

/// The VFS of one machine: its mount tree, lock tables and watchers.
pub struct Vfs {
    pub(crate) ns: Namespace,
    next_id: AtomicU64,
    /// Minor numbers of anonymous devices (Linux's `get_anon_bdev`).
    next_anon: AtomicU32,
    pub locks: LockManager,
    watcher: std::sync::RwLock<Option<Arc<dyn FsWatcher>>>,
}

impl Vfs {
    /// A VFS whose root is `root`.
    pub fn new(root: Arc<dyn Filesystem>) -> Arc<Self> {
        let next_id = AtomicU64::new(1);
        let next_anon = AtomicU32::new(1);
        let sb = new_superblock(&next_id, &next_anon, root);
        let root_dentry = Dentry::root(&sb, sb.fs.root());
        let mount = Arc::new(Mount {
            id: next_id.fetch_add(1, Ordering::Relaxed),
            root: root_dentry,
            flags: MountFlags::default(),
            parent: std::sync::Mutex::new(None),
        });
        Arc::new(Self {
            ns: Namespace::new(mount),
            next_id,
            next_anon,
            locks: LockManager::default(),
            watcher: std::sync::RwLock::new(None),
        })
    }

    pub fn namespace(&self) -> &Namespace {
        &self.ns
    }

    /// The root of the mount tree.
    pub fn root(&self) -> Path {
        self.ns.root()
    }

    pub(crate) fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Mounts `fs` on the directory `at` (resolved from the tree's root with full privileges),
    /// as the embedder configures the machine. With `create`, missing directories along `at` are
    /// made first, as `mkdir -p` would.
    pub async fn mount(
        &self,
        at: &[u8],
        fs: Arc<dyn Filesystem>,
        flags: MountFlags,
        create: bool,
    ) -> FsResult<Arc<Mount>> {
        let sb = new_superblock(&self.next_id, &self.next_anon, fs);
        let root = Dentry::root(&sb, sb.fs.root());
        self.attach(at, root, flags, create).await
    }

    /// Bind-mounts the tree at `from` on `at` (`mount --bind`).
    pub async fn bind_mount(
        &self,
        from: &[u8],
        at: &[u8],
        flags: MountFlags,
        create: bool,
    ) -> FsResult<Arc<Mount>> {
        let cred = Cred::root();
        let root = self.root();
        let cx = Ctx {
            cred: &cred,
            root: &root,
            umask: 0o022,
        };
        let source = self.lookup(&cx, &root, from, LookupFlags::follow()).await?;
        self.attach(at, source.dentry, flags, create).await
    }

    async fn attach(
        &self,
        at: &[u8],
        root: Arc<Dentry>,
        flags: MountFlags,
        create: bool,
    ) -> FsResult<Arc<Mount>> {
        let cred = Cred::root();
        let ns_root = self.root();
        let cx = Ctx {
            cred: &cred,
            root: &ns_root,
            umask: 0o022,
        };
        if create {
            self.mkdir_all(&cx, at).await?;
        }
        let target = self
            .lookup(&cx, &ns_root, at, LookupFlags::follow())
            .await?;
        if target.dentry.is_dir() != root.is_dir() {
            return Err(if root.is_dir() {
                Errno::ENOTDIR
            } else {
                Errno::EISDIR
            });
        }
        let mount = Arc::new(Mount {
            id: self.next_id(),
            root,
            flags,
            parent: std::sync::Mutex::new(None),
        });
        self.ns.attach(mount.clone(), &target);
        Ok(mount)
    }

    /// Detaches the topmost mount on `at` (resolved with full privileges).
    pub async fn umount(&self, at: &[u8]) -> FsResult<()> {
        let cred = Cred::root();
        let root = self.root();
        let cx = Ctx {
            cred: &cred,
            root: &root,
            umask: 0,
        };
        let path = self.lookup(&cx, &root, at, LookupFlags::follow()).await?;
        if !path.is_mount_root() {
            return Err(Errno::EINVAL);
        }
        let mountpoint = path.mnt.parent().ok_or(Errno::EBUSY)?;
        self.ns.detach(&mountpoint).ok_or(Errno::EINVAL)?;
        Ok(())
    }

    /// Installs the watcher that receives change notifications.
    pub fn set_watcher(&self, watcher: Option<Arc<dyn FsWatcher>>) {
        *self.watcher.write().unwrap() = watcher;
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

    pub(crate) fn notify(&self, event: FsEvent<'_>) {
        if let Some(watcher) = &*self.watcher.read().unwrap() {
            watcher.event(&event);
        }
    }

    /// Reports an event on the object `dentry` names, to its own watches and then to its
    /// parent directory's, with its name.
    pub(crate) fn notify_object(&self, dentry: &Arc<Dentry>, event: FsEvent<'_>) {
        let Some(watcher) = &*self.watcher.read().unwrap() else {
            return;
        };
        watcher.event(&event);
        if dentry.is_unlinked() {
            return;
        }
        if let Some(parent) = dentry.parent()
            && !Arc::ptr_eq(&parent, dentry)
        {
            let dir = notify::Object {
                sb: parent.sb().id,
                ino: parent.node().ino(),
            };
            watcher.child_event(dir, &dentry.name(), &event);
        }
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
    next_id: &AtomicU64,
    next_anon: &AtomicU32,
    fs: Arc<dyn Filesystem>,
) -> Arc<Superblock> {
    let id = next_id.fetch_add(1, Ordering::Relaxed);
    let info = fs.info();
    let dev = info.dev.unwrap_or_else(|| anon_dev(next_anon));
    Arc::new(Superblock {
        id,
        dev,
        fs,
        info,
        rename_lock: futures_util::lock::Mutex::new(()),
        inodes: Default::default(),
        sub_devs: Default::default(),
    })
}
