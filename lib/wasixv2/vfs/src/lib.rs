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
mod locks;
mod mount;
mod namei;
mod notify;
mod ops;
mod types;

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

pub use cred::{Cap, Cred, MAY_EXEC, MAY_READ, MAY_WRITE, generic_permission};
pub use dentry::Dentry;
pub use driver::{Filesystem, Handle, MAX_DIR_COOKIE, Node};
pub use file::{DIR_POS_EOD, File};
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
    pub locks: LockManager,
    watcher: std::sync::RwLock<Option<Arc<dyn FsWatcher>>>,
}

impl Vfs {
    /// A VFS whose root is `root`.
    pub fn new(root: Arc<dyn Filesystem>) -> Arc<Self> {
        let next_id = AtomicU64::new(1);
        let sb = new_superblock(&next_id, root);
        let root_dentry = Dentry::root(sb.clone(), sb.fs.root());
        let mount = Arc::new(Mount {
            id: next_id.fetch_add(1, Ordering::Relaxed),
            root: root_dentry,
            flags: MountFlags::default(),
            parent: std::sync::Mutex::new(None),
        });
        Arc::new(Self {
            ns: Namespace::new(mount),
            next_id,
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
        let sb = new_superblock(&self.next_id, fs);
        let root = Dentry::root(sb.clone(), sb.fs.root());
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

    pub(crate) fn notify(&self, event: FsEvent<'_>) {
        if let Some(watcher) = &*self.watcher.read().unwrap() {
            watcher.event(&event);
        }
    }
}

fn new_superblock(next_id: &AtomicU64, fs: Arc<dyn Filesystem>) -> Arc<Superblock> {
    let id = next_id.fetch_add(1, Ordering::Relaxed);
    let info = fs.info();
    // Anonymous devices have major 0, like the ones Linux hands tmpfs.
    let dev = info.dev.unwrap_or(Device {
        major: 0,
        minor: id as u32 & 0xfffff,
    });
    Arc::new(Superblock {
        id,
        dev,
        fs,
        info,
        rename_lock: futures_util::lock::Mutex::new(()),
    })
}
