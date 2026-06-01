// TODO: currently, hard links are broken in the presence or renames.
// It is impossible to fix them with the current setup, since a hard
// link must point to the actual file rather than its path, but the
// only way we can get to a file on a FileSystem instance is by going
// through its respective FileOpener and giving it a path as input.
// TODO: refactor away the InodeVal type
//
// ## FD map / inode lock order
//
// When both locks are needed: acquire `fd_map` before `inode`, never the reverse.
// Do not hold an `inode` lock while waiting on `fd_map`. Mutations that install or
// remove map entries (`insert`, `remove`, `acquire_handle`, `drop_one_handle`) must
// run under `fd_map.write()`. Capture `VirtualFile` handles (or cloned `Fd` data)
// under the map lock before any `await`; never resolve I/O by fd number after dropping
// the lock.

mod fd;
mod fd_list;
mod inode_guard;
mod notification;
mod path_posix;

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        Arc, Mutex, RwLock, Weak,
        atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering},
    },
    task::{Context, Poll},
};

use crate::{
    net::socket::InodeSocketKind,
    state::{Stderr, Stdin, Stdout},
};
use futures::{Future, future::BoxFuture};
#[cfg(feature = "enable-serde")]
use serde_derive::{Deserialize, Serialize};
use tracing::{debug, trace, warn};
use virtual_fs::{
    ArcFileSystem, FileSystem, FsError, MountFileSystem, OpenOptions, OverlayFileSystem,
    TmpFileSystem, VirtualFile, limiter::DynFsMemoryLimiter,
};
use wasmer_config::package::PackageId;
use wasmer_wasix_types::{
    types::{__WASI_STDERR_FILENO, __WASI_STDIN_FILENO, __WASI_STDOUT_FILENO},
    wasi::{
        Errno, Fd as WasiFd, Fdflags, Fdflagsext, Fdstat, Filesize, Filestat, Filetype,
        Preopentype, Prestat, PrestatEnum, Rights, Socktype,
    },
};

pub(crate) use self::fd::VirtualFileLock;
pub use self::fd::{Fd, FdInner, InodeVal, Kind, SymlinkKind};
pub(crate) use self::fd_list::FdList;
pub(crate) use self::inode_guard::{
    InodeValFilePollGuard, InodeValFilePollGuardJoin, InodeValFilePollGuardMode,
    InodeValFileReadGuard, InodeValFileWriteGuard, WasiStateFileGuard,
};
pub use self::notification::NotificationInner;
pub(crate) use self::path_posix::{PosixPath, PosixPathBuf, PosixPathComponent};
use crate::{ALL_RIGHTS, bin_factory::BinaryPackage, state::PreopenedDir};

// POSIX bounds descriptor numbers by the process fd limit (`OPEN_MAX`,
// `RLIMIT_NOFILE` on Linux). Other OSes commonly override the default, so
// use a Linux-like 64k ceiling until WASIX models per-process fd limits.
pub(crate) const MAX_FD: WasiFd = (64 * 1024) - 1;

pub(crate) struct FlushPoller {
    pub(crate) file: VirtualFileLock,
}

/// Result of removing an fd under `fd_map.write()`, with an optional flush target
/// captured before `drop_one_handle` may clear the inode handle.
pub(crate) struct CloseFdOutcome {
    pub skipped_preopen: bool,
    pub removed: bool,
    pub flush_target: Option<VirtualFileLock>,
}

impl CloseFdOutcome {
    fn not_found() -> Self {
        Self {
            skipped_preopen: false,
            removed: false,
            flush_target: None,
        }
    }
}

impl Future for FlushPoller {
    type Output = Result<(), Errno>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut file = self.file.write().unwrap();
        Pin::new(file.as_mut())
            .poll_flush(cx)
            .map_err(|_| Errno::Io)
    }
}

/// the fd value of the virtual root
///
/// Used for interacting with the file system when it has no
/// pre-opened file descriptors at the root level. Normally
/// a WASM process will do this in the libc initialization stage
/// however that does not happen when the WASM process has never
/// been run. Further that logic could change at any time in libc
/// which would then break functionality. Instead we use this fixed
/// file descriptor
///
/// This is especially important for fuse mounting journals which
/// use the same syscalls as a normal WASI application but do not
/// run the libc initialization logic
pub const VIRTUAL_ROOT_FD: WasiFd = 3;

/// The root inode and stdio inodes are the first inodes in the
/// file system tree
pub const FS_STDIN_INO: Inode = Inode(10);
pub const FS_STDOUT_INO: Inode = Inode(11);
pub const FS_STDERR_INO: Inode = Inode(12);
pub const FS_ROOT_INO: Inode = Inode(13);

const STDIN_DEFAULT_RIGHTS: Rights = {
    // This might seem a bit overenineered, but it's the only way I
    // discovered for getting the values in a const environment
    Rights::from_bits_truncate(
        Rights::FD_DATASYNC.bits()
            | Rights::FD_READ.bits()
            | Rights::FD_SYNC.bits()
            | Rights::FD_ADVISE.bits()
            | Rights::FD_FILESTAT_GET.bits()
            | Rights::FD_FDSTAT_SET_FLAGS.bits()
            | Rights::POLL_FD_READWRITE.bits(),
    )
};
const STDOUT_DEFAULT_RIGHTS: Rights = {
    // This might seem a bit overenineered, but it's the only way I
    // discovered for getting the values in a const environment
    Rights::from_bits_truncate(
        Rights::FD_DATASYNC.bits()
            | Rights::FD_SYNC.bits()
            | Rights::FD_WRITE.bits()
            | Rights::FD_ADVISE.bits()
            | Rights::FD_FILESTAT_GET.bits()
            | Rights::FD_FDSTAT_SET_FLAGS.bits()
            | Rights::POLL_FD_READWRITE.bits(),
    )
};
const STDERR_DEFAULT_RIGHTS: Rights = STDOUT_DEFAULT_RIGHTS;

/// A completely arbitrary "big enough" number used as the upper limit for
/// the number of symlinks that can be traversed when resolving a path
pub const MAX_SYMLINKS: u32 = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct Inode(u64);

impl Inode {
    pub fn as_u64(&self) -> u64 {
        self.0
    }

    pub fn from_path(str: &str) -> Self {
        Inode(xxhash_rust::xxh64::xxh64(str.as_bytes(), 0))
    }
}

#[derive(Debug, Clone)]
pub struct InodeGuard {
    ino: Inode,
    inner: Arc<InodeVal>,

    // This exists because self.inner doesn't really represent the
    // number of FDs referencing this InodeGuard. We need that number
    // so we can know when to drop the file handle, which should result
    // in the backing file (which may be a host file) getting closed.
    open_handles: Arc<AtomicI32>,
}
impl InodeGuard {
    pub fn ino(&self) -> Inode {
        self.ino
    }

    pub fn downgrade(&self) -> InodeWeakGuard {
        InodeWeakGuard {
            ino: self.ino,
            open_handles: self.open_handles.clone(),
            inner: Arc::downgrade(&self.inner),
        }
    }

    pub fn ref_cnt(&self) -> usize {
        Arc::strong_count(&self.inner)
    }

    pub fn handle_count(&self) -> u32 {
        self.open_handles.load(Ordering::SeqCst) as u32
    }

    pub fn acquire_handle(&self) {
        let prev_handles = self.open_handles.fetch_add(1, Ordering::SeqCst);
        trace!(ino = %self.ino.0, new_count = %(prev_handles + 1), "acquiring handle for InodeGuard");
    }

    pub fn drop_one_handle(&self) {
        let prev_handles = self.open_handles.fetch_sub(1, Ordering::SeqCst);

        trace!(ino = %self.ino.0, %prev_handles, "dropping handle for InodeGuard");

        // If this wasn't the last handle, nothing else to do...
        if prev_handles > 1 {
            return;
        }

        // ... otherwise, drop the VirtualFile reference
        let mut guard = self.inner.write();

        // Must have at least one open handle before we can drop.
        // This check happens after `inner` is locked so we can
        // poison the lock and keep people from using this (possibly
        // corrupt) InodeGuard.
        if prev_handles != 1 {
            panic!("InodeGuard handle dropped too many times");
        }

        // Re-check the open handles to account for race conditions
        if self.open_handles.load(Ordering::SeqCst) != 0 {
            return;
        }

        let ino = self.ino.0;
        trace!(%ino, "InodeGuard has no more open handles");

        match guard.deref_mut() {
            Kind::File { handle, .. } if handle.is_some() => {
                let file_ref_count = Arc::strong_count(handle.as_ref().unwrap());
                trace!(%file_ref_count, %ino, "dropping file handle");
                drop(handle.take().unwrap());
            }
            Kind::PipeRx { rx } => {
                trace!(%ino, "closing pipe rx");
                rx.close();
            }
            Kind::PipeTx { tx } => {
                trace!(%ino, "closing pipe tx");
                tx.close();
            }
            _ => (),
        }
    }
}
impl std::ops::Deref for InodeGuard {
    type Target = InodeVal;
    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

#[derive(Debug, Clone)]
pub struct InodeWeakGuard {
    ino: Inode,
    // Needed for when we want to upgrade back. We don't exactly
    // care too much when the AtomicI32 is dropped, so this is
    // a strong reference to keep things simple.
    open_handles: Arc<AtomicI32>,
    inner: Weak<InodeVal>,
}
impl InodeWeakGuard {
    pub fn ino(&self) -> Inode {
        self.ino
    }
    pub fn upgrade(&self) -> Option<InodeGuard> {
        Weak::upgrade(&self.inner).map(|inner| InodeGuard {
            ino: self.ino,
            open_handles: self.open_handles.clone(),
            inner,
        })
    }
}

#[derive(Debug)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
struct EphemeralSymlinkEntry {
    path_to_symlink: PathBuf,
    relative_path: PathBuf,
}

#[derive(Debug)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
enum ComponentResolution {
    Create {
        kind: Kind,
        name: String,
        entry_name: String,
        is_ephemeral: bool,
    },
    BackingSymlink {
        file: PathBuf,
        link_value: PathBuf,
        entry_name: String,
    },
    Special {
        kind: Kind,
        name: Cow<'static, str>,
        entry_name: String,
        stat: Filestat,
    },
}

#[derive(Debug)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
struct WasiInodesProtected {
    lookup: HashMap<Inode, Weak<InodeVal>>,
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct WasiInodes {
    protected: Arc<RwLock<WasiInodesProtected>>,
}

impl WasiInodes {
    pub fn new() -> Self {
        Self {
            protected: Arc::new(RwLock::new(WasiInodesProtected {
                lookup: Default::default(),
            })),
        }
    }

    /// adds another value to the inodes
    pub fn add_inode_val(&self, val: InodeVal) -> InodeGuard {
        let val = Arc::new(val);
        let st_ino = {
            let guard = val.stat.read().unwrap();
            guard.st_ino
        };

        let mut guard = self.protected.write().unwrap();
        let ino = Inode(st_ino);
        guard.lookup.insert(ino, Arc::downgrade(&val));

        // every 100 calls we clear out dead weaks
        if guard.lookup.len() % 100 == 1 {
            guard.lookup.retain(|_, v| Weak::strong_count(v) > 0);
        }

        let open_handles = Arc::new(AtomicI32::new(0));

        InodeGuard {
            ino,
            open_handles,
            inner: val,
        }
    }

    /// Get the `VirtualFile` object at stdout mutably
    pub(crate) fn stdout_mut(fd_map: &RwLock<FdList>) -> Result<InodeValFileWriteGuard, FsError> {
        Self::std_dev_get_mut(fd_map, __WASI_STDOUT_FILENO)
    }

    /// Get the `VirtualFile` object at stderr mutably
    pub(crate) fn stderr_mut(fd_map: &RwLock<FdList>) -> Result<InodeValFileWriteGuard, FsError> {
        Self::std_dev_get_mut(fd_map, __WASI_STDERR_FILENO)
    }

    /// Get the `VirtualFile` object at stdin
    /// TODO: Review why this is dead
    #[allow(dead_code)]
    pub(crate) fn stdin(fd_map: &RwLock<FdList>) -> Result<InodeValFileReadGuard, FsError> {
        Self::std_dev_get(fd_map, __WASI_STDIN_FILENO)
    }
    /// Get the `VirtualFile` object at stdin mutably
    pub(crate) fn stdin_mut(fd_map: &RwLock<FdList>) -> Result<InodeValFileWriteGuard, FsError> {
        Self::std_dev_get_mut(fd_map, __WASI_STDIN_FILENO)
    }

    /// Internal helper function to get a standard device handle.
    /// Expects one of `__WASI_STDIN_FILENO`, `__WASI_STDOUT_FILENO`, `__WASI_STDERR_FILENO`.
    fn std_dev_get(fd_map: &RwLock<FdList>, fd: WasiFd) -> Result<InodeValFileReadGuard, FsError> {
        if let Some(fd) = fd_map.read().unwrap().get(fd) {
            let guard = fd.inode.read();
            if let Kind::File {
                handle: Some(handle),
                ..
            } = guard.deref()
            {
                Ok(InodeValFileReadGuard::new(handle))
            } else {
                // Our public API should ensure that this is not possible
                Err(FsError::NotAFile)
            }
        } else {
            // this should only trigger if we made a mistake in this crate
            Err(FsError::NoDevice)
        }
    }
    /// Internal helper function to mutably get a standard device handle.
    /// Expects one of `__WASI_STDIN_FILENO`, `__WASI_STDOUT_FILENO`, `__WASI_STDERR_FILENO`.
    fn std_dev_get_mut(
        fd_map: &RwLock<FdList>,
        fd: WasiFd,
    ) -> Result<InodeValFileWriteGuard, FsError> {
        if let Some(fd) = fd_map.read().unwrap().get(fd) {
            let guard = fd.inode.read();
            if let Kind::File {
                handle: Some(handle),
                ..
            } = guard.deref()
            {
                Ok(InodeValFileWriteGuard::new(handle))
            } else {
                // Our public API should ensure that this is not possible
                Err(FsError::NotAFile)
            }
        } else {
            // this should only trigger if we made a mistake in this crate
            Err(FsError::NoDevice)
        }
    }
}

impl Default for WasiInodes {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct WasiFsRoot {
    root: Arc<MountFileSystem>,
    memory_limiter: Option<DynFsMemoryLimiter>,
}

impl WasiFsRoot {
    pub fn from_mount_fs(root: MountFileSystem) -> Self {
        Self {
            root: Arc::new(root),
            memory_limiter: None,
        }
    }

    pub fn from_filesystem(fs: Arc<dyn FileSystem + Send + Sync>) -> Self {
        let root = MountFileSystem::new();
        root.mount(Path::new("/"), fs)
            .expect("mounting the root fs on an empty mount fs should succeed");

        Self {
            root: Arc::new(root),
            memory_limiter: None,
        }
    }

    pub fn with_memory_limiter_opt(mut self, limiter: Option<DynFsMemoryLimiter>) -> Self {
        self.memory_limiter = limiter;
        self
    }

    pub(crate) fn memory_limiter(&self) -> Option<&DynFsMemoryLimiter> {
        self.memory_limiter.as_ref()
    }

    pub(crate) fn root(&self) -> &Arc<MountFileSystem> {
        &self.root
    }

    pub(crate) fn writable_root(&self) -> Option<TmpFileSystem> {
        let root = self.root.filesystem_at(Path::new("/"))?;
        find_writable_root(root.as_ref())
    }

    pub(crate) fn stack_root_filesystem(
        &self,
        lower: Arc<dyn FileSystem + Send + Sync>,
    ) -> Result<(), FsError> {
        let current = self
            .root
            .filesystem_at(Path::new("/"))
            .ok_or(FsError::EntryNotFound)?;
        let overlay =
            OverlayFileSystem::new(ArcFileSystem::new(current), [ArcFileSystem::new(lower)]);
        self.root.set_mount(Path::new("/"), Arc::new(overlay))
    }
}

fn find_writable_root(fs: &(dyn FileSystem + Send + Sync)) -> Option<TmpFileSystem> {
    if let Some(tmp) = fs.upcast_any_ref().downcast_ref::<TmpFileSystem>() {
        return Some(tmp.clone());
    }

    if let Some(arc_fs) = fs.upcast_any_ref().downcast_ref::<ArcFileSystem>() {
        return find_writable_root(arc_fs.inner().as_ref());
    }

    if let Some(overlay) = fs
        .upcast_any_ref()
        .downcast_ref::<OverlayFileSystem<ArcFileSystem, Vec<Arc<dyn FileSystem + Send + Sync>>>>()
    {
        return find_writable_root(overlay.primary());
    }

    if let Some(overlay) = fs
        .upcast_any_ref()
        .downcast_ref::<OverlayFileSystem<ArcFileSystem, [ArcFileSystem; 1]>>()
    {
        return find_writable_root(overlay.primary());
    }

    None
}

impl FileSystem for WasiFsRoot {
    fn readlink(&self, path: &Path) -> virtual_fs::Result<PathBuf> {
        self.root.readlink(path)
    }

    fn read_dir(&self, path: &Path) -> virtual_fs::Result<virtual_fs::ReadDir> {
        self.root.read_dir(path)
    }

    fn create_dir(&self, path: &Path) -> virtual_fs::Result<()> {
        self.root.create_dir(path)
    }

    fn create_symlink(&self, source: &Path, target: &Path) -> virtual_fs::Result<()> {
        self.root.create_symlink(source, target)
    }

    fn hard_link(&self, source: &Path, target: &Path) -> virtual_fs::Result<()> {
        self.root.hard_link(source, target)
    }

    fn remove_dir(&self, path: &Path) -> virtual_fs::Result<()> {
        self.root.remove_dir(path)
    }

    fn rename<'a>(&'a self, from: &Path, to: &Path) -> BoxFuture<'a, virtual_fs::Result<()>> {
        let from = from.to_owned();
        let to = to.to_owned();
        let this = self.clone();
        Box::pin(async move { this.root.rename(&from, &to).await })
    }

    fn metadata(&self, path: &Path) -> virtual_fs::Result<virtual_fs::Metadata> {
        self.root.metadata(path)
    }

    fn symlink_metadata(&self, path: &Path) -> virtual_fs::Result<virtual_fs::Metadata> {
        self.root.symlink_metadata(path)
    }

    fn remove_file(&self, path: &Path) -> virtual_fs::Result<()> {
        self.root.remove_file(path)
    }

    fn new_open_options(&self) -> OpenOptions<'_> {
        self.root.new_open_options()
    }
}

/// Warning, modifying these fields directly may cause invariants to break and
/// should be considered unsafe.  These fields may be made private in a future release
///
/// Lock order when touching both the fd map and an inode: **`fd_map` first, then
/// `inode`**. Prefer the `*_locked` helpers on [`WasiFs`] (`insert_fd_locked`,
/// `clone_fd_locked`, `close_fd_locked`, `dup2_at`) so handle counts and map slots
/// stay consistent under concurrency.
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct WasiFs {
    //pub repo: Repo,
    pub preopen_fds: RwLock<Vec<u32>>,
    pub fd_map: RwLock<FdList>,
    pub current_dir: Mutex<String>,
    #[cfg_attr(feature = "enable-serde", serde(skip, default))]
    pub root_fs: WasiFsRoot,
    pub root_inode: InodeGuard,
    pub has_unioned: Mutex<HashSet<PackageId>>,
    ephemeral_symlinks: Arc<RwLock<HashMap<PathBuf, EphemeralSymlinkEntry>>>,

    // TODO: remove
    // using an atomic is a hack to enable customization after construction,
    // but it shouldn't be necessary
    // It should not be necessary at all.
    is_wasix: AtomicBool,

    // The preopens when this was initialized
    pub(crate) init_preopens: Vec<PreopenedDir>,
    // The virtual file system preopens when this was initialized
    pub(crate) init_vfs_preopens: Vec<String>,
}

impl WasiFs {
    fn writable_package_mount(
        fs: Arc<dyn FileSystem + Send + Sync>,
        limiter: Option<&DynFsMemoryLimiter>,
    ) -> Arc<dyn FileSystem + Send + Sync> {
        let upper = TmpFileSystem::new();
        if let Some(limiter) = limiter {
            upper.set_memory_limiter(limiter.clone());
        }

        Arc::new(OverlayFileSystem::new(upper, [ArcFileSystem::new(fs)]))
    }

    pub fn is_wasix(&self) -> bool {
        // NOTE: this will only be set once very early in the instance lifetime,
        // so Relaxed should be okay.
        self.is_wasix.load(Ordering::Relaxed)
    }

    pub fn set_is_wasix(&self, is_wasix: bool) {
        self.is_wasix.store(is_wasix, Ordering::SeqCst);
    }

    pub(crate) fn register_ephemeral_symlink(
        &self,
        full_path: PathBuf,
        path_to_symlink: PathBuf,
        relative_path: PathBuf,
    ) {
        let mut guard = self.ephemeral_symlinks.write().unwrap();
        guard.insert(
            PosixPath::from_path(&full_path)
                .normalize_virtual_symlink_key()
                .into_path_buf(),
            EphemeralSymlinkEntry {
                path_to_symlink: PosixPath::from_path(&path_to_symlink)
                    .normalize_virtual_symlink_key()
                    .into_path_buf(),
                relative_path,
            },
        );
    }

    pub(crate) fn ephemeral_symlink_at(&self, full_path: &Path) -> Option<(PathBuf, PathBuf)> {
        let guard = self.ephemeral_symlinks.read().unwrap();
        let key = PosixPath::from_path(full_path)
            .normalize_virtual_symlink_key()
            .into_path_buf();
        let entry = guard.get(&key)?;
        Some((entry.path_to_symlink.clone(), entry.relative_path.clone()))
    }

    pub(crate) fn unregister_ephemeral_symlink(&self, full_path: &Path) {
        let mut guard = self.ephemeral_symlinks.write().unwrap();
        let key = PosixPath::from_path(full_path)
            .normalize_virtual_symlink_key()
            .into_path_buf();
        guard.remove(&key);
    }

    pub(crate) fn move_ephemeral_symlink(
        &self,
        old_full_path: &Path,
        new_full_path: &Path,
        path_to_symlink: PathBuf,
        relative_path: PathBuf,
    ) {
        let old_key = PosixPath::from_path(old_full_path)
            .normalize_virtual_symlink_key()
            .into_path_buf();
        let new_key = PosixPath::from_path(new_full_path)
            .normalize_virtual_symlink_key()
            .into_path_buf();

        let mut guard = self.ephemeral_symlinks.write().unwrap();
        guard.remove(&old_key);
        guard.insert(
            new_key,
            EphemeralSymlinkEntry {
                path_to_symlink: PosixPath::from_path(&path_to_symlink)
                    .normalize_virtual_symlink_key()
                    .into_path_buf(),
                relative_path,
            },
        );
    }

    /// Forking the WasiState is used when either fork or vfork is called
    pub fn fork(&self) -> Self {
        Self {
            preopen_fds: RwLock::new(self.preopen_fds.read().unwrap().clone()),
            fd_map: RwLock::new(self.fd_map.read().unwrap().clone()),
            current_dir: Mutex::new(self.current_dir.lock().unwrap().clone()),
            is_wasix: AtomicBool::new(self.is_wasix.load(Ordering::Acquire)),
            root_fs: self.root_fs.clone(),
            root_inode: self.root_inode.clone(),
            has_unioned: Mutex::new(self.has_unioned.lock().unwrap().clone()),
            ephemeral_symlinks: self.ephemeral_symlinks.clone(),
            init_preopens: self.init_preopens.clone(),
            init_vfs_preopens: self.init_vfs_preopens.clone(),
        }
    }

    /// Closes all file descriptors marked CLOEXEC (except stdio and preopens).
    pub async fn close_cloexec_fds(&self) {
        let flush_targets = {
            let mut fd_map = self.fd_map.write().unwrap();
            let to_close: Vec<WasiFd> = fd_map
                .iter()
                .filter_map(|(k, v)| {
                    if v.inner.fd_flags.contains(Fdflagsext::CLOEXEC)
                        && !v.is_stdio
                        && !v.inode.is_preopened
                    {
                        tracing::trace!(fd = %k, "Closing FD due to CLOEXEC flag");
                        Some(k)
                    } else {
                        None
                    }
                })
                .collect();
            let mut flush_targets = Vec::new();
            for fd in to_close {
                let outcome = Self::close_fd_locked(&mut fd_map, fd);
                if let Some(target) = outcome.flush_target {
                    flush_targets.push(target);
                }
            }
            flush_targets
        };

        for file in flush_targets {
            Self::flush_file_best_effort(file).await;
        }
    }

    /// Closes all file descriptors, flushing captured handles after dropping the map lock.
    pub async fn close_all(&self) {
        let flush_targets = {
            let mut fd_map = self.fd_map.write().unwrap();
            let mut fds: HashSet<WasiFd> = fd_map.keys().collect();
            fds.insert(__WASI_STDOUT_FILENO);
            fds.insert(__WASI_STDERR_FILENO);

            let mut flush_targets = Vec::new();
            for fd in fds {
                let outcome = Self::close_fd_locked(&mut fd_map, fd);
                if let Some(target) = outcome.flush_target {
                    flush_targets.push(target);
                }
            }

            // Preopens skipped by close_fd_locked remain until clear().
            for (_fd, fd_ref) in fd_map.iter().collect::<Vec<_>>() {
                if let Some(target) = Self::file_flush_target(&fd_ref.inode) {
                    flush_targets.push(target);
                }
            }
            fd_map.clear();
            flush_targets
        };

        for file in flush_targets {
            Self::flush_file_best_effort(file).await;
        }
    }

    /// Will conditionally union the binary file system with this one
    /// if it has not already been unioned
    pub async fn conditional_union(
        &self,
        binary: &BinaryPackage,
    ) -> Result<(), virtual_fs::FsError> {
        let Some(package_mounts) = &binary.package_mounts else {
            return Ok(());
        };

        let needs_to_be_unioned = self.has_unioned.lock().unwrap().insert(binary.id.clone());
        if !needs_to_be_unioned {
            return Ok(());
        }

        if let Some(root_layer) = &package_mounts.root_layer {
            self.root_fs
                .stack_root_filesystem(Self::writable_package_mount(
                    root_layer.clone(),
                    self.root_fs.memory_limiter(),
                ))?;
        }

        for mount in &package_mounts.mounts {
            self.root_fs.root().mount_with_source(
                &mount.guest_path,
                &mount.source_path,
                Self::writable_package_mount(mount.fs.clone(), self.root_fs.memory_limiter()),
            )?;
        }

        Ok(())
    }

    /// Created for the builder API. like `new` but with more information
    pub(crate) fn new_with_preopen(
        inodes: &WasiInodes,
        preopens: &[PreopenedDir],
        vfs_preopens: &[String],
        fs_backing: WasiFsRoot,
    ) -> Result<Self, String> {
        let mut wasi_fs = Self::new_init(fs_backing, inodes, FS_ROOT_INO)?;
        wasi_fs.init_preopens = preopens.to_vec();
        wasi_fs.init_vfs_preopens = vfs_preopens.to_vec();
        wasi_fs.create_preopens(inodes, false)?;
        Ok(wasi_fs)
    }

    /// Converts a relative path into an absolute path
    pub(crate) fn relative_path_to_absolute(&self, path: String) -> String {
        if path.starts_with('/') {
            return path;
        }

        let current_dir = self.current_dir.lock().unwrap();
        format!("{}/{}", current_dir.trim_end_matches('/'), path)
    }

    /// Private helper function to init the filesystem, called in `new` and
    /// `new_with_preopen`
    fn new_init(
        fs_backing: WasiFsRoot,
        inodes: &WasiInodes,
        st_ino: Inode,
    ) -> Result<Self, String> {
        debug!("Initializing WASI filesystem");

        let stat = Filestat {
            st_filetype: Filetype::Directory,
            st_ino: st_ino.as_u64(),
            ..Filestat::default()
        };
        let root_kind = Kind::Root {
            entries: HashMap::new(),
        };
        let root_inode = inodes.add_inode_val(InodeVal {
            stat: RwLock::new(stat),
            is_preopened: true,
            name: RwLock::new("/".into()),
            kind: RwLock::new(root_kind),
        });

        let wasi_fs = Self {
            preopen_fds: RwLock::new(vec![]),
            fd_map: RwLock::new(FdList::new()),
            current_dir: Mutex::new("/".to_string()),
            is_wasix: AtomicBool::new(false),
            root_fs: fs_backing,
            root_inode,
            has_unioned: Mutex::new(HashSet::new()),
            ephemeral_symlinks: Arc::new(RwLock::new(HashMap::new())),
            init_preopens: Default::default(),
            init_vfs_preopens: Default::default(),
        };
        wasi_fs.create_stdin(inodes);
        wasi_fs.create_stdout(inodes);
        wasi_fs.create_stderr(inodes);
        wasi_fs.create_rootfd()?;

        Ok(wasi_fs)
    }

    /// This function is like create dir all, but it also opens it.
    /// Function is unsafe because it may break invariants and hasn't been tested.
    /// This is an experimental function and may be removed
    ///
    /// # Safety
    /// - Virtual directories created with this function must not conflict with
    ///   the standard operation of the WASI filesystem.  This is vague and
    ///   unlikely in practice.  [Join the discussion](https://github.com/wasmerio/wasmer/issues/1219)
    ///   for what the newer, safer WASI FS APIs should look like.
    #[allow(dead_code)]
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn open_dir_all(
        &mut self,
        inodes: &WasiInodes,
        base: WasiFd,
        name: String,
        rights: Rights,
        rights_inheriting: Rights,
        flags: Fdflags,
        fd_flags: Fdflagsext,
    ) -> Result<WasiFd, FsError> {
        // TODO: check permissions here? probably not, but this should be
        // an explicit choice, so justify it in a comment when we remove this one
        let mut cur_inode = self.get_fd_inode(base).map_err(fs_error_from_wasi_err)?;

        let path: &Path = Path::new(&name);
        //let n_components = path.components().count();
        for c in path.components() {
            let segment_name = c.as_os_str().to_string_lossy().to_string();
            let guard = cur_inode.read();
            match guard.deref() {
                Kind::Dir { entries, .. } | Kind::Root { entries } => {
                    if let Some(_entry) = entries.get(&segment_name) {
                        // TODO: this should be fixed
                        return Err(FsError::AlreadyExists);
                    }

                    let kind = Kind::Dir {
                        parent: cur_inode.downgrade(),
                        path: PathBuf::from(""),
                        entries: HashMap::new(),
                    };

                    drop(guard);
                    let inode = self.create_inode_with_default_stat(
                        inodes,
                        kind,
                        false,
                        segment_name.clone().into(),
                    );

                    // reborrow to insert
                    {
                        let mut guard = cur_inode.write();
                        match guard.deref_mut() {
                            Kind::Dir { entries, .. } | Kind::Root { entries } => {
                                entries.insert(segment_name, inode.clone());
                            }
                            _ => unreachable!("Dir or Root became not Dir or Root"),
                        }
                    }
                    cur_inode = inode;
                }
                _ => return Err(FsError::BaseNotDirectory),
            }
        }

        // TODO: review open flags (read, write); they were added without consideration
        self.create_fd(
            rights,
            rights_inheriting,
            flags,
            fd_flags,
            Fd::READ | Fd::WRITE,
            cur_inode,
        )
        .map_err(fs_error_from_wasi_err)
    }

    /// Opens a user-supplied file in the directory specified with the
    /// name and flags given
    // dead code because this is an API for external use
    // TODO: is this used anywhere? Is it even sound?
    #[allow(dead_code, clippy::too_many_arguments)]
    pub fn open_file_at(
        &mut self,
        inodes: &WasiInodes,
        base: WasiFd,
        file: Box<dyn VirtualFile + Send + Sync + 'static>,
        open_flags: u16,
        name: String,
        rights: Rights,
        rights_inheriting: Rights,
        flags: Fdflags,
        fd_flags: Fdflagsext,
    ) -> Result<WasiFd, FsError> {
        // TODO: check permissions here? probably not, but this should be
        // an explicit choice, so justify it in a comment when we remove this one
        let base_inode = self.get_fd_inode(base).map_err(fs_error_from_wasi_err)?;

        let guard = base_inode.read();
        match guard.deref() {
            Kind::Dir { entries, .. } | Kind::Root { entries } => {
                if let Some(_entry) = entries.get(&name) {
                    // TODO: eventually change the logic here to allow overwrites
                    return Err(FsError::AlreadyExists);
                }

                let kind = Kind::File {
                    handle: Some(Arc::new(RwLock::new(file))),
                    path: PathBuf::from(""),
                    fd: None,
                };

                drop(guard);
                let inode = self
                    .create_inode(inodes, kind, false, name.clone())
                    .map_err(|_| FsError::IOError)?;

                {
                    let mut guard = base_inode.write();
                    match guard.deref_mut() {
                        Kind::Dir { entries, .. } | Kind::Root { entries } => {
                            entries.insert(name, inode.clone());
                        }
                        _ => unreachable!("Dir or Root became not Dir or Root"),
                    }
                }

                // Here, we clone the inode so we can use it to overwrite the fd field below.
                let real_fd = self
                    .create_fd(
                        rights,
                        rights_inheriting,
                        flags,
                        fd_flags,
                        open_flags,
                        inode.clone(),
                    )
                    .map_err(fs_error_from_wasi_err)?;

                {
                    let mut guard = inode.kind.write().unwrap();
                    match guard.deref_mut() {
                        Kind::File { fd, .. } => {
                            *fd = Some(real_fd);
                        }
                        _ => unreachable!("We just created a Kind::File"),
                    }
                }

                Ok(real_fd)
            }
            _ => Err(FsError::BaseNotDirectory),
        }
    }

    /// Change the backing of a given file descriptor
    /// Returns the old backing
    /// TODO: add examples
    #[allow(dead_code)]
    pub fn swap_file(
        &self,
        fd: WasiFd,
        mut file: Box<dyn VirtualFile + Send + Sync + 'static>,
    ) -> Result<Option<Box<dyn VirtualFile + Send + Sync + 'static>>, FsError> {
        match fd {
            __WASI_STDIN_FILENO => {
                let mut target = WasiInodes::stdin_mut(&self.fd_map)?;
                Ok(Some(target.swap(file)))
            }
            __WASI_STDOUT_FILENO => {
                let mut target = WasiInodes::stdout_mut(&self.fd_map)?;
                Ok(Some(target.swap(file)))
            }
            __WASI_STDERR_FILENO => {
                let mut target = WasiInodes::stderr_mut(&self.fd_map)?;
                Ok(Some(target.swap(file)))
            }
            _ => {
                let base_inode = self.get_fd_inode(fd).map_err(fs_error_from_wasi_err)?;
                {
                    // happy path
                    let guard = base_inode.read();
                    match guard.deref() {
                        Kind::File { handle, .. } => {
                            if let Some(handle) = handle {
                                let mut handle = handle.write().unwrap();
                                std::mem::swap(handle.deref_mut(), &mut file);
                                return Ok(Some(file));
                            }
                        }
                        _ => return Err(FsError::NotAFile),
                    }
                }
                // slow path
                let mut guard = base_inode.write();
                match guard.deref_mut() {
                    Kind::File { handle, .. } => {
                        if let Some(handle) = handle {
                            let mut handle = handle.write().unwrap();
                            std::mem::swap(handle.deref_mut(), &mut file);
                            Ok(Some(file))
                        } else {
                            handle.replace(Arc::new(RwLock::new(file)));
                            Ok(None)
                        }
                    }
                    _ => Err(FsError::NotAFile),
                }
            }
        }
    }

    /// refresh size from filesystem
    pub fn filestat_resync_size(&self, fd: WasiFd) -> Result<Filesize, Errno> {
        let inode = self.get_fd_inode(fd)?;
        let mut guard = inode.write();
        match guard.deref_mut() {
            Kind::File { handle, .. } => {
                if let Some(h) = handle {
                    let h = h.read().unwrap();
                    let new_size = h.size();
                    drop(h);
                    drop(guard);

                    inode.stat.write().unwrap().st_size = new_size;
                    Ok(new_size as Filesize)
                } else {
                    Err(Errno::Badf)
                }
            }
            Kind::Dir { .. } | Kind::Root { .. } => Err(Errno::Isdir),
            _ => Err(Errno::Inval),
        }
    }

    /// Changes the current directory
    pub fn set_current_dir(&self, path: &str) {
        let mut guard = self.current_dir.lock().unwrap();
        *guard = path.to_string();
    }

    /// Gets the current directory
    pub fn get_current_dir(
        &self,
        inodes: &WasiInodes,
        base: WasiFd,
    ) -> Result<(InodeGuard, String), Errno> {
        self.get_current_dir_inner(inodes, base, 0)
    }

    pub(crate) fn get_current_dir_inner(
        &self,
        inodes: &WasiInodes,
        base: WasiFd,
        symlink_count: u32,
    ) -> Result<(InodeGuard, String), Errno> {
        let mut symlink_count = symlink_count;
        let current_dir = {
            let guard = self.current_dir.lock().unwrap();
            guard.clone()
        };
        let cur_inode = self.get_fd_inode(base)?;
        let inode = self.get_inode_at_path_inner(
            inodes,
            cur_inode,
            current_dir.as_str(),
            &mut symlink_count,
            true,
        )?;
        Ok((inode, current_dir))
    }

    /// Resolve a path in the POSIX namespace visible to the WASIX guest.
    ///
    /// This function intentionally resolves guest paths, not host-native paths.
    /// A Windows host path may contain `\`, drive prefixes, or UNC prefixes, but
    /// those belong to mount setup and backing filesystem access. Once a host
    /// directory is mounted into WASIX, the guest observes a POSIX path tree
    /// where `/` is the only separator. Raw syscall paths must therefore be
    /// parsed with POSIX rules even when the runtime itself is running on
    /// Windows.
    ///
    /// POSIX path resolution is stricter than Rust's `Path::components()`:
    /// explicit `.`, explicit `..`, an empty pathname, and a trailing slash are
    /// all observable. In particular, `file/` and `file/.` must fail with
    /// `Errno::Notdir`, `lstat("symlink_to_dir/")` must follow the symlink to
    /// prove the result is a directory, and `lstat("symlink_to_file/")` must
    /// fail with `Errno::Notdir`. For that reason this function uses a small
    /// POSIX component parser instead of `Path::components()`.
    ///
    /// Symlink following follows the POSIX rule used by `openat`-style APIs:
    /// intermediate symlinks are always followed, while the final component is
    /// followed only when `follow_symlinks` is true. Recursive symlink
    /// resolution increments `symlink_count`, and symlink depth exhaustion maps
    /// to `Errno::Loop`.
    ///
    /// There are two loops here with different jobs. The outer loop walks the
    /// parsed path components. The inner `component_lookup` loop normally runs
    /// once, but has one virtual-root overlay case: when the current inode is
    /// `Kind::Root` and a component is not found directly, it can jump through
    /// the mounted `entries["/"]` inode and retry the same component. That is
    /// WASIX virtual-root behavior, not plain POSIX filesystem traversal.
    ///
    /// Keep these edge cases intact when editing this function:
    ///
    /// - Empty pathnames are `Errno::Noent`; they do not resolve to the base
    ///   inode unless a separate `AT_EMPTY_PATH`-style extension is introduced.
    /// - Absolute paths resolve from `VIRTUAL_ROOT_FD`, independent of the
    ///   caller-provided starting inode.
    /// - A literal root pathname (`/`, `//`, and so on) preserves historical
    ///   WASIX behavior: if the virtual root contains a mounted `entries["/"]`
    ///   directory, the literal root path resolves to that mounted directory.
    ///   This special case is intentionally limited to an all-slashes pathname.
    /// - Parent traversal is semantic, not a string rewrite. The virtual root's
    ///   parent is itself, but a mounted directory whose guest name is `/` still
    ///   has the virtual root as its parent. Therefore `/..` may resolve to
    ///   `Kind::Root` after walking from the mounted `/` directory upward, and
    ///   traversal that genuinely reaches `Kind::Root` must not be remapped
    ///   back to `entries["/"]` at the end. That distinction lets WASI guests
    ///   see the virtual root with all preopens via `..` without changing the
    ///   behavior of opening literal `/`.
    /// - `.` and `..` are semantic components: they require the current inode
    ///   to be a directory or virtual root, otherwise they fail with
    ///   `Errno::Notdir`.
    /// - Special files may be returned only as the final component. As path
    ///   prefixes, they fail with `Errno::Notdir`.
    ///
    /// The returned `InodeGuard` is the inode for the resolved final object in
    /// the WASIX inode graph. It is not necessarily an already-open host file:
    /// file inodes discovered here are normally created with `handle: None`,
    /// and `path_open` or a similar caller opens the backing file later. If the
    /// final object is a symlink and `follow_symlinks` is false, the returned
    /// inode is the symlink itself; otherwise symlink targets are resolved
    /// recursively and the returned inode is the target.
    ///
    /// Directory `entries` are a lazy cache over the backing filesystem. When a
    /// child name is already present in the current `Kind::Dir` or `Kind::Root`,
    /// that cached inode wins. When a child is missing from a `Kind::Dir`, this
    /// resolver builds the backing path for that one component, checks the
    /// ephemeral symlink table, then calls `root_fs.symlink_metadata()` without
    /// following symlinks. Based on that metadata it materializes a `Kind::Dir`,
    /// `Kind::File`, `Kind::Symlink`, or supported special-file inode. Persistent
    /// backing entries are inserted into the parent directory cache; ephemeral
    /// symlink inodes are transient and are not cached as directory entries.
    ///
    /// Cached directory entries are part of the guest-visible directory model,
    /// not merely an implementation detail. A later `fd_readdir` over a backing
    /// directory must merge these cached children with host children instead of
    /// hiding non-preopen cache entries; otherwise cleanup and tree-walking code
    /// can miss inodes that this resolver can still reach.
    ///
    /// This function is therefore not a full synchronization pass. It observes
    /// the backing filesystem on cache misses, but cached entries are reused
    /// without re-statting. Syscalls that mutate the filesystem are responsible
    /// for keeping the inode cache and ephemeral symlink map coherent with their
    /// changes.
    fn get_inode_at_path_inner(
        &self,
        inodes: &WasiInodes,
        mut cur_inode: InodeGuard,
        path_str: &str,
        symlink_count: &mut u32,
        follow_symlinks: bool,
    ) -> Result<InodeGuard, Errno> {
        if *symlink_count > MAX_SYMLINKS {
            return Err(Errno::Loop);
        }

        if path_str.is_empty() {
            return Err(Errno::Noent);
        }

        if path_str.starts_with('/') {
            cur_inode = self.get_fd_inode(VIRTUAL_ROOT_FD)?;
        }

        let is_all_slashes = path_str.bytes().all(|b| b == b'/');

        // Absolute root paths should resolve to the mounted "/" inode when present.
        // This keeps "/" behavior aligned with historical path traversal semantics.
        if is_all_slashes {
            if let Kind::Root { entries } = cur_inode.read().deref()
                && let Some(root_entry) = entries.get("/")
            {
                return Ok(root_entry.clone());
            }
            return Ok(cur_inode);
        }

        // POSIX path resolution is stricter than `Path::components()`: explicit
        // `.`/`..` and a trailing slash are observable because they require the
        // current result to be a directory after symlink resolution.
        let path = PosixPath::new(path_str);
        let components = path.components(true, true);

        let n_components = components.len();

        // TODO: rights checks
        // for each component traverse file structure loading inodes as
        // necessary.
        'path_iter: for (i, component) in components.into_iter().enumerate() {
            // Since we're resolving the path against the given inode, we want to
            // assume '/a/b' to be the same as `a/b` relative to the inode, so
            // we skip over the RootDir component.
            if matches!(component, PosixPathComponent::RootDir) {
                continue 'path_iter;
            }

            // Note: when current component is last and follow is off, then we
            // return inode of the symlink itself, however if current component
            // is inner we will follow symlinks even with follow off.
            // Following symlinks uses recursive resolution, thus if current
            // component is not last, we recurse with follow always on. Only
            // last component with follow off will result in symlink not being
            // followed.
            let last_component = i == n_components - 1;

            let component_str = match component {
                PosixPathComponent::CurDir => {
                    let is_dir = {
                        let guard = cur_inode.read();
                        matches!(guard.deref(), Kind::Dir { .. } | Kind::Root { .. })
                    };
                    if is_dir {
                        continue 'path_iter;
                    }
                    return Err(Errno::Notdir);
                }
                PosixPathComponent::ParentDir => {
                    let parent_inode = {
                        let guard = cur_inode.read();
                        match guard.deref() {
                            Kind::Root { .. } => None,
                            Kind::Dir { parent, .. } => {
                                Some(parent.upgrade().ok_or(Errno::Access)?)
                            }
                            _ => return Err(Errno::Notdir),
                        }
                    };
                    if let Some(parent_inode) = parent_inode {
                        cur_inode = parent_inode;
                    }
                    continue 'path_iter;
                }
                PosixPathComponent::Normal(component) => component,
                PosixPathComponent::RootDir => unreachable!("RootDir is handled above"),
            };

            'component_lookup: loop {
                // 1. Read-Only Lookup Phase
                // --
                // Match current inode against known entry types, and if it happens
                // to be a directory, then resolve current component as an entry in
                // that directory.
                // Note: this loop practically never does more than one iteration.
                // There is only one exotic case when this loop would do another
                // iteration, and it is when current inode happens to be Root
                // containing '/' entry.
                let component_resolution = {
                    match cur_inode.clone().read().deref() {
                        Kind::Buffer { .. } => {
                            unimplemented!("state::get_inode_at_path for buffers")
                        }
                        Kind::File { .. }
                        | Kind::Socket { .. }
                        | Kind::PipeRx { .. }
                        | Kind::PipeTx { .. }
                        | Kind::DuplexPipe { .. }
                        | Kind::EventNotifications { .. }
                        | Kind::Epoll { .. } => {
                            return Err(Errno::Notdir);
                        }
                        Kind::Symlink { .. } => break 'component_lookup,
                        Kind::Root { entries } => {
                            if let Some(entry) = entries.get(component_str) {
                                cur_inode = entry.clone();
                                break 'component_lookup;
                            } else if let Some(root) = entries.get("/") {
                                // This is quite exotic case where Root itself
                                // has '/' entry in it, and we want to follow
                                // from there.
                                // Note: this is one and only case where
                                // 'component_lookup loop would do another
                                // iteration - the only actual reason for it to
                                // be a loop.
                                cur_inode = root.clone();
                                continue 'component_lookup;
                            } else {
                                // Root is not capable of having something other
                                // then preopenned folders
                                return Err(Errno::Notcapable);
                            }
                        }
                        Kind::Dir {
                            entries,
                            path: cur_dir,
                            ..
                        } => {
                            // When component resolves to directory entry, then
                            // next component needs to resolve to a child node
                            // within that directory.
                            // Here we are handling all variants of directory
                            // children.

                            if let Some(entry) = entries.get(component_str) {
                                // We found component in cached entries, so we
                                // can continue. If it is a symlink it will be
                                // resolved in next the step.
                                cur_inode = entry.clone();
                                break 'component_lookup;
                            }

                            // We did not find the component in cached entries,
                            // so we will create new inode for it.
                            let entry_path_buf = PosixPath::from_path(cur_dir)
                                .join(&PosixPath::new(component_str))
                                .into_path_buf();

                            // Current component of the path we're resolving, as
                            // a string...
                            let entry_name = component_str.to_string();

                            // ...and its relevant path within current inode
                            // being the directory.
                            // Note: the entry_path does not need to match the
                            // path we're resolving, e.g. if this is a recursive
                            // call from symlink resolution branch.
                            let entry_path = entry_path_buf.to_string_lossy().to_string();

                            if let Some((path_to_symlink, relative_path)) =
                                self.ephemeral_symlink_at(&entry_path_buf)
                            {
                                // Ephemeral symlink are transient records; they
                                // are virtual, and they are not persisted in
                                // directory like symbolic links, so we will
                                // create a temporary inode for them.
                                // We resolve them but don't cache them as dir
                                // entries.
                                ComponentResolution::Create {
                                    kind: Kind::Symlink {
                                        symlink_kind: SymlinkKind::Virtual,
                                        path_to_symlink,
                                        relative_path,
                                    },
                                    name: entry_path,
                                    entry_name,
                                    is_ephemeral: true,
                                }
                            } else {
                                // Otherwise it is persistent, and we create new
                                // inode for it that we will cache in directory
                                // entries.
                                // Note: this gets metadata of the file entry
                                // without following symbolic links.
                                let metadata = self
                                    .root_fs
                                    .symlink_metadata(&entry_path_buf)
                                    .ok()
                                    .ok_or(Errno::Noent)?;
                                let file_type = metadata.file_type();
                                if file_type.is_dir() {
                                    // load DIR
                                    ComponentResolution::Create {
                                        kind: Kind::Dir {
                                            parent: cur_inode.downgrade(),
                                            path: entry_path_buf,
                                            entries: Default::default(),
                                        },
                                        name: entry_path,
                                        entry_name,
                                        is_ephemeral: false,
                                    }
                                } else if file_type.is_file() {
                                    // load file
                                    ComponentResolution::Create {
                                        kind: Kind::File {
                                            handle: None,
                                            path: entry_path_buf,
                                            fd: None,
                                        },
                                        name: entry_path,
                                        entry_name,
                                        is_ephemeral: false,
                                    }
                                } else if file_type.is_symlink() {
                                    // load symbolic link
                                    // Note: as opposed to ephemeral symlinks,
                                    // which are transient, these are
                                    // persistent, i.e. they have actual entry
                                    // in the directory
                                    // structure.
                                    let link_value = self
                                        .root_fs
                                        .readlink(&entry_path_buf)
                                        .ok()
                                        .ok_or(Errno::Noent)?;
                                    debug!("attempting to decompose path {:?}", link_value);
                                    ComponentResolution::BackingSymlink {
                                        file: entry_path_buf,
                                        link_value,
                                        entry_name,
                                    }
                                } else {
                                    #[cfg(unix)]
                                    {
                                        //use std::os::unix::fs::FileTypeExt;
                                        let file_type: Filetype = if file_type.is_char_device() {
                                            Filetype::CharacterDevice
                                        } else if file_type.is_block_device() {
                                            Filetype::BlockDevice
                                        } else if file_type.is_fifo() {
                                            // FIFO doesn't seem to fit any other type, so unknown
                                            Filetype::Unknown
                                        } else if file_type.is_socket() {
                                            // TODO: how do we know if it's a `SocketStream` or
                                            // a `SocketDgram`?
                                            Filetype::SocketStream
                                        } else {
                                            unimplemented!(
                                                "state::get_inode_at_path unknown file type: not file, directory, symlink, char device, block device, fifo, or socket"
                                            );
                                        };

                                        ComponentResolution::Special {
                                            kind: Kind::File {
                                                handle: None,
                                                path: entry_path_buf,
                                                fd: None,
                                            },
                                            name: entry_path.into(),
                                            entry_name,
                                            stat: Filestat {
                                                st_filetype: file_type,
                                                st_ino: Inode::from_path(path_str).as_u64(),
                                                st_size: metadata.len(),
                                                st_ctim: metadata.created(),
                                                st_mtim: metadata.modified(),
                                                st_atim: metadata.accessed(),
                                                ..Filestat::default()
                                            },
                                        }
                                    }
                                    #[cfg(not(unix))]
                                    unimplemented!(
                                        "state::get_inode_at_path unknown file type: not file, directory, or symlink"
                                    );
                                }
                            } // end of non-ephemeral entry case
                        } // end of Kind::Dir match case
                    } // end of match
                }; // end of component_resolution block

                // 2. Create an INode and update directory entries
                // --
                // The cur_inode is definitely a directory (Kind::Dir) at this
                // stage, and we need to create an inode (new_inode) and cache
                // as an entry in current directory (entry_name => cur_inode).
                let (entry_name, new_inode, should_insert, should_return) =
                    match component_resolution {
                        ComponentResolution::Create {
                            kind,
                            name,
                            entry_name,
                            is_ephemeral,
                        } => {
                            let new_inode = self.create_inode(inodes, kind, false, name)?;
                            (entry_name, new_inode, !is_ephemeral, false)
                        }
                        ComponentResolution::BackingSymlink {
                            file,
                            link_value,
                            entry_name,
                        } => {
                            let new_inode = self.create_inode(
                                inodes,
                                Kind::Symlink {
                                    symlink_kind: SymlinkKind::Backing,
                                    path_to_symlink: PosixPath::from_path(&file)
                                        .strip_root_prefix()
                                        .into_path_buf(),
                                    relative_path: link_value,
                                },
                                false,
                                file.to_string_lossy().to_string(),
                            )?;
                            (entry_name, new_inode, false, false)
                        }
                        ComponentResolution::Special {
                            kind,
                            name,
                            entry_name,
                            stat,
                        } => {
                            let new_inode =
                                self.create_inode_with_stat(inodes, kind, false, name, stat);
                            (entry_name, new_inode, true, true)
                        }
                    };

                {
                    let mut guard = cur_inode.write();
                    let Kind::Dir { entries, .. } = guard.deref_mut() else {
                        unreachable!("Attempted to insert special device into non-directory");
                    };

                    if should_insert {
                        entries.insert(entry_name, new_inode.clone());
                    }

                    if should_return {
                        // Special files cannot be traversed further, so return the inode directly.
                        if last_component {
                            return Ok(new_inode);
                        }
                        return Err(Errno::Notdir);
                    }
                }

                // Assign current inode and leave 'component_loop
                // Note: this is a shortcut for doing next iteration matching
                // Kind::Dir for same cur_inode and finding there matching entry
                // that we just inserted, and exiting 'component_lookup.
                cur_inode = new_inode;
                break 'component_lookup;
            } // end of 'component_lookup loop

            // 3. Follow Symbolic Links
            // --
            // We continue with Symlink resolution unless...
            if last_component && !follow_symlinks {
                // ...this symlink is the very last component of the path to
                // resolve, and symlink following is off,
                // ...or this is not a symlink at all
                continue 'path_iter;
            }

            // The cur_inode can be a symlink (Kind::Symlink) or something else.
            let (symlink_kind, path_to_symlink, relative_path) = {
                let guard = cur_inode.read();
                let Kind::Symlink {
                    symlink_kind,
                    path_to_symlink,
                    relative_path,
                } = guard.deref()
                else {
                    // not a symlink, so we continue with next path component
                    continue 'path_iter;
                };
                (
                    *symlink_kind,
                    path_to_symlink.clone(),
                    relative_path.clone(),
                )
            };

            let (new_base_fd, new_path) =
                self.resolve_symlink_target_path(symlink_kind, &path_to_symlink, &relative_path)?;
            let new_base_inode = self.get_fd_inode(new_base_fd)?;
            let new_path = PosixPath::from_path(&new_path).as_str().to_owned();

            // We want to always follow symlinks unless we're resolving very
            // last path component, then and only then we want to stop symlink
            // following if it was originally off.
            let follow_symlinks_inner = !last_component || follow_symlinks;

            debug!("Following symlink recursively");
            *symlink_count += 1;
            if *symlink_count > MAX_SYMLINKS {
                return Err(Errno::Loop);
            }
            let symlink_inode = self.get_inode_at_path_inner(
                inodes,
                new_base_inode,
                &new_path,
                symlink_count,
                follow_symlinks_inner,
            )?;

            // The rest of the path resolution will be relative to resolved
            // symlink target.
            cur_inode = symlink_inode;
        }

        Ok(cur_inode)
    }

    pub(crate) fn resolve_symlink_target_path(
        &self,
        symlink_kind: SymlinkKind,
        path_to_symlink: &Path,
        relative_path: &Path,
    ) -> Result<(WasiFd, PathBuf), Errno> {
        let relative_posix = PosixPath::from_path(relative_path);
        if matches!(symlink_kind, SymlinkKind::Virtual) && relative_posix.is_absolute() {
            return Ok((VIRTUAL_ROOT_FD, relative_path.to_owned()));
        }

        let symlink_parent = match symlink_kind {
            SymlinkKind::Virtual => PosixPath::from_path(path_to_symlink)
                .parent()
                .into_path_buf(),
            SymlinkKind::Backing => {
                let symlink_path_buf =
                    PosixPath::new("/").join(&PosixPath::from_path(path_to_symlink));
                let symlink_path = symlink_path_buf.as_posix_path();
                let mount_entry = self
                    .root_fs
                    .root()
                    .mount_entries()
                    .into_iter()
                    .filter(|entry| {
                        symlink_path
                            .strip_prefix(&PosixPath::from_path(&entry.path))
                            .is_some()
                    })
                    .max_by_key(|entry| PosixPath::from_path(&entry.path).as_str().len())
                    .ok_or(Errno::Perm)?;
                let mount_path = mount_entry.path;

                let symlink_relative = symlink_path
                    .strip_prefix(&PosixPath::from_path(&mount_path))
                    .ok_or(Errno::Perm)?;
                let symlink_parent = symlink_relative.parent().into_path_buf();
                let contained_target = if relative_posix.is_absolute() {
                    let stripped = relative_posix
                        .strip_prefix(&PosixPath::from_path(&mount_entry.source_path))
                        .ok_or(Errno::Perm)?;
                    PosixPathBuf::from(stripped.as_str().to_owned())
                } else {
                    PosixPathBuf::resolve_relative(
                        &PosixPath::from_path(&symlink_parent),
                        &relative_posix,
                        false,
                    )?
                };

                return Ok((
                    VIRTUAL_ROOT_FD,
                    PosixPath::from_path(&mount_path)
                        .join(&contained_target.as_posix_path())
                        .into_path_buf(),
                ));
            }
        };

        Ok((
            VIRTUAL_ROOT_FD,
            PosixPathBuf::resolve_relative(
                &PosixPath::from_path(&symlink_parent),
                &relative_posix,
                true,
            )?
            .into_path_buf(),
        ))
    }

    pub(crate) fn rebase_symlink_location(&self, new_symlink_path: &Path) -> PathBuf {
        PosixPath::from_path(new_symlink_path)
            .strip_root_prefix()
            .into_path_buf()
    }

    /// gets a host file from a base directory and a path
    /// this function ensures the fs remains sandboxed
    // NOTE: follow symlinks is super weird right now
    // even if it's false, it still follows symlinks, just not the last
    // symlink so
    // This will be resolved when we have tests asserting the correct behavior
    pub(crate) fn get_inode_at_path(
        &self,
        inodes: &WasiInodes,
        base: WasiFd,
        path: &str,
        follow_symlinks: bool,
    ) -> Result<InodeGuard, Errno> {
        let base_inode = self.get_fd_inode(base)?;
        let mut symlink_count = 0;
        self.get_inode_at_path_inner(
            inodes,
            base_inode,
            path,
            &mut symlink_count,
            follow_symlinks,
        )
    }

    pub(crate) fn get_inode_at_path_from_inode(
        &self,
        inodes: &WasiInodes,
        base_inode: InodeGuard,
        path: &str,
        follow_symlinks: bool,
    ) -> Result<InodeGuard, Errno> {
        let mut symlink_count = 0;
        self.get_inode_at_path_inner(
            inodes,
            base_inode,
            path,
            &mut symlink_count,
            follow_symlinks,
        )
    }

    /// Returns the parent Dir or Root that the file at a given path is in and the file name
    /// stripped off
    pub(crate) fn get_parent_inode_at_path(
        &self,
        inodes: &WasiInodes,
        base: WasiFd,
        path: &Path,
        follow_symlinks: bool,
    ) -> Result<(InodeGuard, String), Errno> {
        let (parent_dir, new_entity_name) = PosixPath::from_path(path).parent_path_and_name()?;
        if parent_dir.as_str().is_empty() {
            return self.get_fd_inode(base).map(|v| (v, new_entity_name));
        }
        self.get_inode_at_path(inodes, base, parent_dir.as_str(), follow_symlinks)
            .map(|v| (v, new_entity_name))
    }

    pub fn get_fd(&self, fd: WasiFd) -> Result<Fd, Errno> {
        let ret = self
            .fd_map
            .read()
            .unwrap()
            .get(fd)
            .ok_or(Errno::Badf)
            .cloned();

        if ret.is_err() && fd == VIRTUAL_ROOT_FD {
            Ok(Self::virtual_root_fd(self.root_inode.clone()))
        } else {
            ret
        }
    }

    pub fn get_fd_inode(&self, fd: WasiFd) -> Result<InodeGuard, Errno> {
        // see `VIRTUAL_ROOT_FD` for details as to why this exists
        if fd == VIRTUAL_ROOT_FD {
            return Ok(self.root_inode.clone());
        }
        self.fd_map
            .read()
            .unwrap()
            .get(fd)
            .ok_or(Errno::Badf)
            .map(|a| a.inode.clone())
    }

    pub fn filestat_fd(&self, fd: WasiFd) -> Result<Filestat, Errno> {
        let inode = self.get_fd_inode(fd)?;
        let guard = inode.stat.read().unwrap();
        Ok(*guard.deref())
    }

    pub fn fdstat(&self, fd: WasiFd) -> Result<Fdstat, Errno> {
        match fd {
            __WASI_STDIN_FILENO => {
                return Ok(Fdstat {
                    fs_filetype: Filetype::CharacterDevice,
                    fs_flags: Fdflags::empty(),
                    fs_rights_base: STDIN_DEFAULT_RIGHTS,
                    fs_rights_inheriting: Rights::empty(),
                });
            }
            __WASI_STDOUT_FILENO => {
                return Ok(Fdstat {
                    fs_filetype: Filetype::CharacterDevice,
                    fs_flags: Fdflags::APPEND,
                    fs_rights_base: STDOUT_DEFAULT_RIGHTS,
                    fs_rights_inheriting: Rights::empty(),
                });
            }
            __WASI_STDERR_FILENO => {
                return Ok(Fdstat {
                    fs_filetype: Filetype::CharacterDevice,
                    fs_flags: Fdflags::APPEND,
                    fs_rights_base: STDERR_DEFAULT_RIGHTS,
                    fs_rights_inheriting: Rights::empty(),
                });
            }
            VIRTUAL_ROOT_FD => {
                return Ok(Fdstat {
                    fs_filetype: Filetype::Directory,
                    fs_flags: Fdflags::empty(),
                    // TODO: fix this
                    fs_rights_base: ALL_RIGHTS,
                    fs_rights_inheriting: ALL_RIGHTS,
                });
            }
            _ => (),
        }
        let fd = self.get_fd(fd)?;

        let guard = fd.inode.read();
        let deref = guard.deref();
        Ok(Fdstat {
            fs_filetype: match deref {
                Kind::File { .. } => Filetype::RegularFile,
                Kind::Dir { .. } => Filetype::Directory,
                Kind::Symlink { .. } => Filetype::SymbolicLink,
                Kind::Socket { socket } => match &socket.inner.protected.read().unwrap().kind {
                    InodeSocketKind::TcpStream { .. } => Filetype::SocketStream,
                    InodeSocketKind::Raw { .. } => Filetype::SocketRaw,
                    InodeSocketKind::PreSocket { props, .. } => match props.ty {
                        Socktype::Stream => Filetype::SocketStream,
                        Socktype::Dgram => Filetype::SocketDgram,
                        Socktype::Raw => Filetype::SocketRaw,
                        Socktype::Seqpacket => Filetype::SocketSeqpacket,
                        _ => Filetype::Unknown,
                    },
                    _ => Filetype::Unknown,
                },
                _ => Filetype::Unknown,
            },
            fs_flags: fd.inner.flags,
            fs_rights_base: fd.inner.rights,
            fs_rights_inheriting: fd.inner.rights_inheriting, // TODO(lachlan): Is this right?
        })
    }

    pub fn prestat_fd(&self, fd: WasiFd) -> Result<Prestat, Errno> {
        let inode = self.get_fd_inode(fd)?;
        //trace!("in prestat_fd {:?}", self.get_fd(fd)?);

        if inode.is_preopened {
            Ok(self.prestat_fd_inner(inode.deref()))
        } else {
            Err(Errno::Badf)
        }
    }

    pub(crate) fn prestat_fd_inner(&self, inode_val: &InodeVal) -> Prestat {
        Prestat {
            pr_type: Preopentype::Dir,
            u: PrestatEnum::Dir {
                // WASI spec: pr_name_len is the length of the path string, NOT including null terminator
                pr_name_len: inode_val.name.read().unwrap().len() as u32,
            }
            .untagged(),
        }
    }

    /// Creates an inode and inserts it given a Kind and some extra data
    pub(crate) fn create_inode(
        &self,
        inodes: &WasiInodes,
        kind: Kind,
        is_preopened: bool,
        name: String,
    ) -> Result<InodeGuard, Errno> {
        let stat = self.get_stat_for_kind(&kind)?;
        Ok(self.create_inode_with_stat(inodes, kind, is_preopened, name.into(), stat))
    }

    /// Creates an inode and inserts it given a Kind, does not assume the file exists.
    pub(crate) fn create_inode_with_default_stat(
        &self,
        inodes: &WasiInodes,
        kind: Kind,
        is_preopened: bool,
        name: Cow<'static, str>,
    ) -> InodeGuard {
        let stat = Filestat::default();
        self.create_inode_with_stat(inodes, kind, is_preopened, name, stat)
    }

    /// Creates an inode with the given filestat and inserts it.
    pub(crate) fn create_inode_with_stat(
        &self,
        inodes: &WasiInodes,
        kind: Kind,
        is_preopened: bool,
        name: Cow<'static, str>,
        mut stat: Filestat,
    ) -> InodeGuard {
        match &kind {
            Kind::File {
                handle: Some(handle),
                ..
            } => {
                let guard = handle.read().unwrap();
                stat.st_size = guard.size();
            }
            Kind::Buffer { buffer } => {
                stat.st_size = buffer.len() as u64;
            }
            _ => {}
        }

        let inode_key: Cow<'_, str> = match &kind {
            Kind::File { path, .. } | Kind::Dir { path, .. } => {
                let path_str = path.to_string_lossy();
                if path_str.is_empty() {
                    Cow::Borrowed(name.as_ref())
                } else {
                    path_str
                }
            }
            Kind::Symlink {
                path_to_symlink, ..
            } => {
                let path_str = path_to_symlink.to_string_lossy();
                if path_str.is_empty() {
                    Cow::Borrowed(name.as_ref())
                } else {
                    path_str
                }
            }
            _ => Cow::Borrowed(name.as_ref()),
        };

        let st_ino = Inode::from_path(&inode_key);
        stat.st_ino = st_ino.as_u64();

        inodes.add_inode_val(InodeVal {
            stat: RwLock::new(stat),
            is_preopened,
            name: RwLock::new(name),
            kind: RwLock::new(kind),
        })
    }

    fn make_fd(
        rights: Rights,
        rights_inheriting: Rights,
        fs_flags: Fdflags,
        fd_flags: Fdflagsext,
        open_flags: u16,
        inode: InodeGuard,
        idx: Option<WasiFd>,
    ) -> Fd {
        let is_stdio = matches!(
            idx,
            Some(__WASI_STDIN_FILENO) | Some(__WASI_STDOUT_FILENO) | Some(__WASI_STDERR_FILENO)
        );
        Fd {
            inner: FdInner {
                rights,
                rights_inheriting,
                flags: fs_flags,
                offset: Arc::new(AtomicU64::new(0)),
                fd_flags,
            },
            open_flags,
            inode,
            is_stdio,
        }
    }

    /// Insert a new fd into an already write-locked fd map.
    ///
    /// Lock order: callers must hold `fd_map.write()` and must not hold any inode
    /// lock while acquiring the fd map lock.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn insert_fd_locked(
        fd_map: &mut FdList,
        rights: Rights,
        rights_inheriting: Rights,
        fs_flags: Fdflags,
        fd_flags: Fdflagsext,
        open_flags: u16,
        inode: InodeGuard,
        idx: Option<WasiFd>,
        exclusive: bool,
    ) -> Result<WasiFd, Errno> {
        let fd = Self::make_fd(
            rights,
            rights_inheriting,
            fs_flags,
            fd_flags,
            open_flags,
            inode,
            idx,
        );

        match idx {
            Some(idx) => {
                if idx > MAX_FD {
                    return Err(Errno::Badf);
                }
                if fd_map.insert(exclusive, idx, fd) {
                    Ok(idx)
                } else {
                    Err(Errno::Exist)
                }
            }
            None => Ok(fd_map.insert_first_free(fd)),
        }
    }

    /// Duplicate an fd into an already write-locked fd map.
    pub(crate) fn clone_fd_locked(
        fs: &WasiFs,
        fd_map: &mut FdList,
        fd: WasiFd,
        min_result_fd: WasiFd,
        cloexec: Option<bool>,
    ) -> Result<WasiFd, Errno> {
        let fd = Self::get_fd_from_locked_map(fs, fd_map, fd)?;
        Self::ensure_file_handle_present(&fd)?;
        if min_result_fd > MAX_FD {
            return Err(Errno::Inval);
        }
        Ok(fd_map.insert_first_free_after(
            Fd {
                inner: FdInner {
                    rights: fd.inner.rights,
                    rights_inheriting: fd.inner.rights_inheriting,
                    flags: fd.inner.flags,
                    offset: fd.inner.offset.clone(),
                    fd_flags: match cloexec {
                        None => fd.inner.fd_flags,
                        Some(cloexec) => {
                            let mut f = fd.inner.fd_flags;
                            f.set(Fdflagsext::CLOEXEC, cloexec);
                            f
                        }
                    },
                },
                open_flags: fd.open_flags,
                inode: fd.inode,
                is_stdio: fd.is_stdio,
            },
            min_result_fd,
        ))
    }

    /// Resolve an fd from a write-locked map (includes [`VIRTUAL_ROOT_FD`] fallback).
    pub(crate) fn get_fd_from_locked_map(
        fs: &WasiFs,
        fd_map: &FdList,
        fd: WasiFd,
    ) -> Result<Fd, Errno> {
        match fd_map.get(fd) {
            Some(fd) => Ok(fd.clone()),
            None if fd == VIRTUAL_ROOT_FD => Ok(Self::virtual_root_fd(fs.root_inode.clone())),
            None => Err(Errno::Badf),
        }
    }

    fn virtual_root_fd(root_inode: InodeGuard) -> Fd {
        Fd {
            inner: FdInner {
                rights: ALL_RIGHTS,
                rights_inheriting: ALL_RIGHTS,
                flags: Fdflags::empty(),
                offset: Arc::new(AtomicU64::new(0)),
                fd_flags: Fdflagsext::empty(),
            },
            open_flags: 0,
            inode: root_inode,
            is_stdio: false,
        }
    }

    fn ensure_file_handle_present(fd: &Fd) -> Result<(), Errno> {
        let guard = fd.inode.read();
        match guard.deref() {
            Kind::File { handle: None, .. } => Err(Errno::Badf),
            _ => Ok(()),
        }
    }

    /// POSIX dup2: copy `src` onto exact slot `dst`, replacing any existing entry.
    ///
    /// Holds `fd_map.write()` for the full remove+insert. Returns a flush target for
    /// the replaced `dst` entry (if any), captured while the lock is held and before
    /// `remove` calls `drop_one_handle`, which may clear the inode's file handle.
    pub(crate) fn dup2_at(
        &self,
        src: WasiFd,
        dst: WasiFd,
    ) -> Result<Option<VirtualFileLock>, Errno> {
        if dst > MAX_FD {
            return Err(Errno::Badf);
        }

        let flush_target = {
            let mut fd_map = self.fd_map.write().unwrap();

            let fd_entry = fd_map.get(src).ok_or(Errno::Badf)?;
            Self::ensure_file_handle_present(fd_entry)?;

            if src == dst {
                return Ok(None);
            }

            if let Some(target_fd) = fd_map.get(dst)
                && !target_fd.is_stdio
                && target_fd.inode.is_preopened
            {
                warn!("Refusing dup2({src}, {dst}) because FD {dst} is pre-opened");
                return Err(Errno::Notsup);
            }

            let new_fd_entry = Fd {
                inner: FdInner {
                    offset: fd_entry.inner.offset.clone(),
                    rights: fd_entry.inner.rights_inheriting,
                    fd_flags: {
                        let mut f = fd_entry.inner.fd_flags;
                        f.set(Fdflagsext::CLOEXEC, false);
                        f
                    },
                    ..fd_entry.inner
                },
                inode: fd_entry.inode.clone(),
                ..*fd_entry
            };

            let flush_target = fd_map
                .get(dst)
                .and_then(|fd| Self::file_flush_target(&fd.inode));

            fd_map.remove(dst);

            if !fd_map.insert(true, dst, new_fd_entry) {
                panic!("Internal error: expected FD {dst} to be free after remove in dup2_at");
            }

            flush_target
        };

        Ok(flush_target)
    }

    pub fn create_fd(
        &self,
        rights: Rights,
        rights_inheriting: Rights,
        fs_flags: Fdflags,
        fd_flags: Fdflagsext,
        open_flags: u16,
        inode: InodeGuard,
    ) -> Result<WasiFd, Errno> {
        self.create_fd_ext(
            rights,
            rights_inheriting,
            fs_flags,
            fd_flags,
            open_flags,
            inode,
            None,
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_fd(
        &self,
        rights: Rights,
        rights_inheriting: Rights,
        fs_flags: Fdflags,
        fd_flags: Fdflagsext,
        open_flags: u16,
        inode: InodeGuard,
        idx: WasiFd,
    ) -> Result<(), Errno> {
        self.create_fd_ext(
            rights,
            rights_inheriting,
            fs_flags,
            fd_flags,
            open_flags,
            inode,
            Some(idx),
            true,
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_fd_ext(
        &self,
        rights: Rights,
        rights_inheriting: Rights,
        fs_flags: Fdflags,
        fd_flags: Fdflagsext,
        open_flags: u16,
        inode: InodeGuard,
        idx: Option<WasiFd>,
        exclusive: bool,
    ) -> Result<WasiFd, Errno> {
        let mut fd_map = self.fd_map.write().unwrap();
        Self::insert_fd_locked(
            &mut fd_map,
            rights,
            rights_inheriting,
            fs_flags,
            fd_flags,
            open_flags,
            inode,
            idx,
            exclusive,
        )
    }

    pub fn clone_fd(&self, fd: WasiFd) -> Result<WasiFd, Errno> {
        self.clone_fd_ext(fd, 0, None)
    }

    pub fn clone_fd_ext(
        &self,
        fd: WasiFd,
        min_result_fd: WasiFd,
        cloexec: Option<bool>,
    ) -> Result<WasiFd, Errno> {
        let mut fd_map = self.fd_map.write().unwrap();
        Self::clone_fd_locked(self, &mut fd_map, fd, min_result_fd, cloexec)
    }

    /// Low level function to remove an inode, that is it deletes the WASI FS's
    /// knowledge of a file.
    ///
    /// This function returns the inode if it existed and was removed.
    ///
    /// # Safety
    /// - The caller must ensure that all references to the specified inode have
    ///   been removed from the filesystem.
    pub unsafe fn remove_inode(&self, inodes: &WasiInodes, ino: Inode) -> Option<Arc<InodeVal>> {
        let mut guard = inodes.protected.write().unwrap();
        guard.lookup.remove(&ino).and_then(|a| Weak::upgrade(&a))
    }

    pub(crate) fn create_stdout(&self, inodes: &WasiInodes) {
        self.create_std_dev_inner(
            inodes,
            Box::<Stdout>::default(),
            "stdout",
            __WASI_STDOUT_FILENO,
            STDOUT_DEFAULT_RIGHTS,
            Fdflags::APPEND,
            FS_STDOUT_INO,
        );
    }

    pub(crate) fn create_stdin(&self, inodes: &WasiInodes) {
        self.create_std_dev_inner(
            inodes,
            Box::<Stdin>::default(),
            "stdin",
            __WASI_STDIN_FILENO,
            STDIN_DEFAULT_RIGHTS,
            Fdflags::empty(),
            FS_STDIN_INO,
        );
    }

    pub(crate) fn create_stderr(&self, inodes: &WasiInodes) {
        self.create_std_dev_inner(
            inodes,
            Box::<Stderr>::default(),
            "stderr",
            __WASI_STDERR_FILENO,
            STDERR_DEFAULT_RIGHTS,
            Fdflags::APPEND,
            FS_STDERR_INO,
        );
    }

    pub(crate) fn create_rootfd(&self) -> Result<(), String> {
        // create virtual root
        let all_rights = ALL_RIGHTS;
        // TODO: make this a list of positive rights instead of negative ones
        // root gets all right for now
        let root_rights = all_rights
            /*
            & (!Rights::FD_WRITE)
            & (!Rights::FD_ALLOCATE)
            & (!Rights::PATH_CREATE_DIRECTORY)
            & (!Rights::PATH_CREATE_FILE)
            & (!Rights::PATH_LINK_SOURCE)
            & (!Rights::PATH_RENAME_SOURCE)
            & (!Rights::PATH_RENAME_TARGET)
            & (!Rights::PATH_FILESTAT_SET_SIZE)
            & (!Rights::PATH_FILESTAT_SET_TIMES)
            & (!Rights::FD_FILESTAT_SET_SIZE)
            & (!Rights::FD_FILESTAT_SET_TIMES)
            & (!Rights::PATH_SYMLINK)
            & (!Rights::PATH_UNLINK_FILE)
            & (!Rights::PATH_REMOVE_DIRECTORY)
            */;
        let fd = self
            .create_fd(
                root_rights,
                root_rights,
                Fdflags::empty(),
                Fdflagsext::empty(),
                Fd::READ,
                self.root_inode.clone(),
            )
            .map_err(|e| format!("Could not create root fd: {e}"))?;
        self.preopen_fds.write().unwrap().push(fd);
        Ok(())
    }

    pub(crate) fn create_preopens(
        &self,
        inodes: &WasiInodes,
        ignore_duplicates: bool,
    ) -> Result<(), String> {
        for preopen_name in self.init_vfs_preopens.iter() {
            let kind = Kind::Dir {
                parent: self.root_inode.downgrade(),
                path: PathBuf::from(preopen_name),
                entries: Default::default(),
            };
            let rights = Rights::FD_ADVISE
                | Rights::FD_TELL
                | Rights::FD_SEEK
                | Rights::FD_READ
                | Rights::PATH_OPEN
                | Rights::FD_READDIR
                | Rights::PATH_READLINK
                | Rights::PATH_FILESTAT_GET
                | Rights::FD_FILESTAT_GET
                | Rights::PATH_LINK_SOURCE
                | Rights::PATH_RENAME_SOURCE
                | Rights::POLL_FD_READWRITE
                | Rights::SOCK_SHUTDOWN;
            let inode = self
                .create_inode(inodes, kind, true, preopen_name.clone())
                .map_err(|e| {
                    format!(
                        "Failed to create inode for preopened dir (name `{preopen_name}`): WASI error code: {e}",
                    )
                })?;
            let fd_flags = Fd::READ;
            let fd = self
                .create_fd(
                    rights,
                    rights,
                    Fdflags::empty(),
                    Fdflagsext::empty(),
                    fd_flags,
                    inode.clone(),
                )
                .map_err(|e| format!("Could not open fd for file {preopen_name:?}: {e}"))?;
            {
                let mut guard = self.root_inode.write();
                if let Kind::Root { entries } = guard.deref_mut() {
                    let existing_entry = entries.insert(preopen_name.clone(), inode);
                    if existing_entry.is_some() && !ignore_duplicates {
                        return Err(format!("Found duplicate entry for alias `{preopen_name}`"));
                    }
                }
            }
            self.preopen_fds.write().unwrap().push(fd);
        }

        for PreopenedDir {
            path,
            alias,
            read,
            write,
            create,
        } in self.init_preopens.iter()
        {
            debug!(
                "Attempting to preopen {} with alias {:?}",
                &path.to_string_lossy(),
                &alias
            );
            let cur_dir_metadata = self
                .root_fs
                .metadata(path)
                .map_err(|e| format!("Could not get metadata for file {path:?}: {e}"))?;

            let kind = if cur_dir_metadata.is_dir() {
                Kind::Dir {
                    parent: self.root_inode.downgrade(),
                    path: path.clone(),
                    entries: Default::default(),
                }
            } else {
                return Err(format!(
                    "WASI only supports pre-opened directories right now; found \"{}\"",
                    path.to_string_lossy()
                ));
            };

            let rights = {
                // TODO: review tell' and fd_readwrite
                let mut rights = Rights::FD_ADVISE | Rights::FD_TELL | Rights::FD_SEEK;
                if *read {
                    rights |= Rights::FD_READ
                        | Rights::PATH_OPEN
                        | Rights::FD_READDIR
                        | Rights::PATH_READLINK
                        | Rights::PATH_FILESTAT_GET
                        | Rights::FD_FILESTAT_GET
                        | Rights::PATH_LINK_SOURCE
                        | Rights::PATH_RENAME_SOURCE
                        | Rights::POLL_FD_READWRITE
                        | Rights::SOCK_SHUTDOWN;
                }
                if *write {
                    rights |= Rights::FD_DATASYNC
                        | Rights::FD_FDSTAT_SET_FLAGS
                        | Rights::FD_WRITE
                        | Rights::FD_SYNC
                        | Rights::FD_ALLOCATE
                        | Rights::PATH_OPEN
                        | Rights::PATH_RENAME_TARGET
                        | Rights::PATH_FILESTAT_SET_SIZE
                        | Rights::PATH_FILESTAT_SET_TIMES
                        | Rights::FD_FILESTAT_SET_SIZE
                        | Rights::FD_FILESTAT_SET_TIMES
                        | Rights::PATH_REMOVE_DIRECTORY
                        | Rights::PATH_UNLINK_FILE
                        | Rights::POLL_FD_READWRITE
                        | Rights::SOCK_SHUTDOWN;
                }
                if *create {
                    rights |= Rights::PATH_CREATE_DIRECTORY
                        | Rights::PATH_CREATE_FILE
                        | Rights::PATH_LINK_TARGET
                        | Rights::PATH_OPEN
                        | Rights::PATH_RENAME_TARGET
                        | Rights::PATH_SYMLINK;
                }

                rights
            };
            let inode = if let Some(alias) = &alias {
                self.create_inode(inodes, kind, true, alias.clone())
            } else {
                self.create_inode(inodes, kind, true, path.to_string_lossy().into_owned())
            }
            .map_err(|e| {
                format!("Failed to create inode for preopened dir: WASI error code: {e}")
            })?;
            let fd_flags = {
                let mut fd_flags = 0;
                if *read {
                    fd_flags |= Fd::READ;
                }
                if *write {
                    // TODO: introduce API for finer grained control
                    fd_flags |= Fd::WRITE | Fd::APPEND | Fd::TRUNCATE;
                }
                if *create {
                    fd_flags |= Fd::CREATE;
                }
                fd_flags
            };
            let fd = self
                .create_fd(
                    rights,
                    rights,
                    Fdflags::empty(),
                    Fdflagsext::empty(),
                    fd_flags,
                    inode.clone(),
                )
                .map_err(|e| format!("Could not open fd for file {path:?}: {e}"))?;
            {
                let mut guard = self.root_inode.write();
                if let Kind::Root { entries } = guard.deref_mut() {
                    let key = if let Some(alias) = &alias {
                        alias.clone()
                    } else {
                        path.to_string_lossy().into_owned()
                    };
                    let existing_entry = entries.insert(key.clone(), inode);
                    if existing_entry.is_some() && !ignore_duplicates {
                        return Err(format!("Found duplicate entry for alias `{key}`"));
                    }
                }
            }
            self.preopen_fds.write().unwrap().push(fd);
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn create_std_dev_inner(
        &self,
        inodes: &WasiInodes,
        handle: Box<dyn VirtualFile + Send + Sync + 'static>,
        name: &'static str,
        raw_fd: WasiFd,
        rights: Rights,
        fd_flags: Fdflags,
        st_ino: Inode,
    ) {
        let inode = {
            let stat = Filestat {
                st_filetype: Filetype::CharacterDevice,
                st_ino: st_ino.as_u64(),
                ..Filestat::default()
            };
            let kind = Kind::File {
                fd: Some(raw_fd),
                handle: Some(Arc::new(RwLock::new(handle))),
                path: "".into(),
            };
            inodes.add_inode_val(InodeVal {
                stat: RwLock::new(stat),
                is_preopened: true,
                name: RwLock::new(name.to_string().into()),
                kind: RwLock::new(kind),
            })
        };
        self.fd_map.write().unwrap().insert(
            false,
            raw_fd,
            Fd {
                inner: FdInner {
                    rights,
                    rights_inheriting: Rights::empty(),
                    flags: fd_flags,
                    offset: Arc::new(AtomicU64::new(0)),
                    fd_flags: Fdflagsext::empty(),
                },
                // since we're not calling open on this, we don't need open flags
                open_flags: 0,
                inode,
                is_stdio: true,
            },
        );
    }

    pub fn get_stat_for_kind(&self, kind: &Kind) -> Result<Filestat, Errno> {
        let md = match kind {
            Kind::File { handle, path, .. } => match handle {
                Some(wf) => {
                    let wf = wf.read().unwrap();
                    return Ok(Filestat {
                        st_filetype: Filetype::RegularFile,
                        st_ino: Inode::from_path(path.to_string_lossy().as_ref()).as_u64(),
                        st_size: wf.size(),
                        st_atim: wf.last_accessed(),
                        st_mtim: wf.last_modified(),
                        st_ctim: wf.created_time(),

                        ..Filestat::default()
                    });
                }
                None => self
                    .root_fs
                    .metadata(path)
                    .map_err(fs_error_into_wasi_err)?,
            },
            Kind::Dir { path, .. } => self
                .root_fs
                .metadata(path)
                .map_err(fs_error_into_wasi_err)?,
            Kind::Symlink {
                path_to_symlink,
                relative_path,
                ..
            } => {
                let symlink_path = PosixPath::new("/")
                    .join(&PosixPath::from_path(path_to_symlink))
                    .into_path_buf();

                match self.root_fs.symlink_metadata(&symlink_path) {
                    Ok(md) => md,
                    Err(FsError::EntryNotFound)
                        if self.ephemeral_symlink_at(&symlink_path).is_some() =>
                    {
                        return Ok(Filestat {
                            st_filetype: Filetype::SymbolicLink,
                            st_size: relative_path.as_os_str().len() as u64,
                            ..Filestat::default()
                        });
                    }
                    Err(err) => return Err(fs_error_into_wasi_err(err)),
                }
            }
            _ => return Err(Errno::Io),
        };
        Ok(Filestat {
            st_filetype: virtual_file_type_to_wasi_file_type(md.file_type()),
            st_size: md.len(),
            st_atim: md.accessed(),
            st_mtim: md.modified(),
            st_ctim: md.created(),
            ..Filestat::default()
        })
    }

    /// Closes an open FD under `fd_map.write()`, capturing a file handle for
    /// post-close flush while the map lock is held.
    ///
    /// Lock order: `fd_map` write, then inode read (never the reverse).
    pub(crate) fn close_fd_and_capture_flush(&self, fd: WasiFd) -> CloseFdOutcome {
        let mut fd_map = self.fd_map.write().unwrap();
        Self::close_fd_locked(&mut fd_map, fd)
    }

    /// Closes an open FD in an already write-locked fd map.
    fn close_fd_locked(fd_map: &mut FdList, fd: WasiFd) -> CloseFdOutcome {
        let Some(fd_ref) = fd_map.get(fd) else {
            trace!(%fd, "closing file descriptor failed - {}", Errno::Badf);
            return CloseFdOutcome::not_found();
        };

        if !fd_ref.is_stdio && fd_ref.inode.is_preopened {
            return CloseFdOutcome {
                skipped_preopen: true,
                removed: false,
                flush_target: None,
            };
        }

        let flush_target = Self::file_flush_target(&fd_ref.inode);

        match fd_map.remove(fd) {
            Some(fd_ref) => {
                let inode = fd_ref.inode.ino().as_u64();
                let ref_cnt = fd_ref.inode.ref_cnt();
                if ref_cnt == 1 {
                    trace!(%fd, %inode, %ref_cnt, "closing file descriptor");
                } else {
                    trace!(%fd, %inode, %ref_cnt, "weakening file descriptor");
                }
            }
            None => {
                trace!(%fd, "closing file descriptor failed - {}", Errno::Badf);
                return CloseFdOutcome::not_found();
            }
        }

        CloseFdOutcome {
            skipped_preopen: false,
            removed: true,
            flush_target,
        }
    }

    pub(crate) async fn flush_file_best_effort(file: VirtualFileLock) {
        let result = FlushPoller { file }.await;
        match result {
            Ok(())
            | Err(Errno::Isdir)
            | Err(Errno::Io)
            | Err(Errno::Access)
            // EINVAL is returned by e.g. pipe-backed stdio and is safe to ignore.
            | Err(Errno::Inval) => {}
            Err(err) => trace!("flush during bulk close failed - {}", err),
        }
    }

    fn file_flush_target(inode: &InodeGuard) -> Option<VirtualFileLock> {
        let guard = inode.read();
        match guard.deref() {
            Kind::File {
                handle: Some(file), ..
            } => Some(file.clone()),
            _ => None,
        }
    }

    /// Closes an open FD, handling all details such as FD being preopen
    pub(crate) fn close_fd(&self, fd: WasiFd) -> Result<(), Errno> {
        let _ = self.close_fd_and_capture_flush(fd);
        Ok(())
    }
}

impl std::fmt::Debug for WasiFs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Ok(guard) = self.current_dir.try_lock() {
            write!(f, "current_dir={} ", guard.as_str())?;
        } else {
            write!(f, "current_dir=(locked) ")?;
        }
        if let Ok(guard) = self.fd_map.read() {
            write!(
                f,
                "next_fd={} max_fd={:?} ",
                guard.next_free_fd(),
                guard.last_fd()
            )?;
        } else {
            write!(f, "next_fd=(locked) max_fd=(locked) ")?;
        }
        write!(f, "{:?}", self.root_fs)
    }
}

/// Returns the default filesystem backing
pub fn default_fs_backing() -> Arc<dyn virtual_fs::FileSystem + Send + Sync> {
    cfg_if::cfg_if! {
        if #[cfg(feature = "host-fs")] {
            Arc::new(virtual_fs::host_fs::FileSystem::new(tokio::runtime::Handle::current(), "/").unwrap())
        } else if #[cfg(not(feature = "host-fs"))] {
            Arc::<virtual_fs::mem_fs::FileSystem>::default()
        } else {
            Arc::<FallbackFileSystem>::default()
        }
    }
}

#[derive(Debug, Default)]
pub struct FallbackFileSystem;

impl FallbackFileSystem {
    fn fail() -> ! {
        panic!(
            "No filesystem set for wasmer-wasi, please enable either the `host-fs` or `mem-fs` feature or set your custom filesystem with `WasiEnvBuilder::set_fs`"
        );
    }
}

impl FileSystem for FallbackFileSystem {
    fn readlink(&self, _path: &Path) -> virtual_fs::Result<PathBuf> {
        Self::fail()
    }
    fn read_dir(&self, _path: &Path) -> Result<virtual_fs::ReadDir, FsError> {
        Self::fail();
    }
    fn create_dir(&self, _path: &Path) -> Result<(), FsError> {
        Self::fail();
    }
    fn remove_dir(&self, _path: &Path) -> Result<(), FsError> {
        Self::fail();
    }
    fn rename<'a>(&'a self, _from: &Path, _to: &Path) -> BoxFuture<'a, Result<(), FsError>> {
        Self::fail();
    }
    fn metadata(&self, _path: &Path) -> Result<virtual_fs::Metadata, FsError> {
        Self::fail();
    }
    fn symlink_metadata(&self, _path: &Path) -> Result<virtual_fs::Metadata, FsError> {
        Self::fail();
    }
    fn remove_file(&self, _path: &Path) -> Result<(), FsError> {
        Self::fail();
    }
    fn new_open_options(&self) -> virtual_fs::OpenOptions<'_> {
        Self::fail();
    }
}

pub fn virtual_file_type_to_wasi_file_type(file_type: virtual_fs::FileType) -> Filetype {
    // TODO: handle other file types
    if file_type.is_dir() {
        Filetype::Directory
    } else if file_type.is_file() {
        Filetype::RegularFile
    } else if file_type.is_symlink() {
        Filetype::SymbolicLink
    } else {
        Filetype::Unknown
    }
}

pub fn fs_error_from_wasi_err(err: Errno) -> FsError {
    match err {
        Errno::Badf => FsError::InvalidFd,
        Errno::Exist => FsError::AlreadyExists,
        Errno::Io => FsError::IOError,
        Errno::Addrinuse => FsError::AddressInUse,
        Errno::Addrnotavail => FsError::AddressNotAvailable,
        Errno::Pipe => FsError::BrokenPipe,
        Errno::Connaborted => FsError::ConnectionAborted,
        Errno::Connrefused => FsError::ConnectionRefused,
        Errno::Connreset => FsError::ConnectionReset,
        Errno::Intr => FsError::Interrupted,
        Errno::Inval => FsError::InvalidInput,
        Errno::Notconn => FsError::NotConnected,
        Errno::Nodev => FsError::NoDevice,
        Errno::Noent => FsError::EntryNotFound,
        Errno::Perm => FsError::PermissionDenied,
        Errno::Timedout => FsError::TimedOut,
        Errno::Proto => FsError::UnexpectedEof,
        Errno::Again => FsError::WouldBlock,
        Errno::Nospc => FsError::WriteZero,
        Errno::Notempty => FsError::DirectoryNotEmpty,
        _ => FsError::UnknownError,
    }
}

pub fn fs_error_into_wasi_err(fs_error: FsError) -> Errno {
    match fs_error {
        FsError::AlreadyExists => Errno::Exist,
        FsError::AddressInUse => Errno::Addrinuse,
        FsError::AddressNotAvailable => Errno::Addrnotavail,
        FsError::BaseNotDirectory => Errno::Notdir,
        FsError::BrokenPipe => Errno::Pipe,
        FsError::ConnectionAborted => Errno::Connaborted,
        FsError::ConnectionRefused => Errno::Connrefused,
        FsError::ConnectionReset => Errno::Connreset,
        FsError::Interrupted => Errno::Intr,
        FsError::InvalidData => Errno::Io,
        FsError::InvalidFd => Errno::Badf,
        FsError::InvalidInput => Errno::Inval,
        FsError::IOError => Errno::Io,
        FsError::NoDevice => Errno::Nodev,
        FsError::NotAFile => Errno::Inval,
        FsError::NotConnected => Errno::Notconn,
        FsError::EntryNotFound => Errno::Noent,
        FsError::PermissionDenied => Errno::Perm,
        FsError::TimedOut => Errno::Timedout,
        FsError::UnexpectedEof => Errno::Proto,
        FsError::WouldBlock => Errno::Again,
        FsError::WriteZero => Errno::Nospc,
        FsError::DirectoryNotEmpty => Errno::Notempty,
        FsError::StorageFull => Errno::Overflow,
        FsError::Lock | FsError::UnknownError => Errno::Io,
        FsError::Unsupported => Errno::Notsup,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use once_cell::sync::OnceCell;
    use tempfile::tempdir;
    use virtual_fs::{RootFileSystemBuilder, TmpFileSystem};
    use wasmer::Engine;
    use wasmer_config::package::PackageId;

    use crate::WasiEnvBuilder;
    use crate::bin_factory::{BinaryPackage, BinaryPackageMount, BinaryPackageMounts};

    fn webc_symlink_fs() -> virtual_fs::WebcVolumeFileSystem {
        let timestamps = webc::v3::Timestamps::default();
        let dir = webc::v3::write::Directory::new(
            std::collections::BTreeMap::from_iter([
                (
                    webc::PathSegment::parse("target.txt").unwrap(),
                    webc::v3::write::DirEntry::File(webc::v3::write::FileEntry::borrowed(
                        b"target", timestamps,
                    )),
                ),
                (
                    webc::PathSegment::parse("link").unwrap(),
                    webc::v3::write::DirEntry::Symlink(webc::v3::write::SymlinkEntry::borrowed(
                        "target.txt",
                        timestamps,
                    )),
                ),
            ]),
            timestamps,
        );
        let manifest = webc::metadata::Manifest::default();
        let mut writer = webc::v3::write::Writer::new(webc::v3::ChecksumAlgorithm::Sha256)
            .write_manifest(&manifest)
            .unwrap()
            .write_atoms(std::collections::BTreeMap::new())
            .unwrap();
        writer.write_volume("atom", dir).unwrap();
        let webc = writer.finish(webc::v3::SignatureAlgorithm::None).unwrap();
        let container = wasmer_package::utils::from_bytes(webc).unwrap();
        let volume = container.volumes()["atom"].clone();

        virtual_fs::WebcVolumeFileSystem::new(volume)
    }

    #[tokio::test]
    async fn test_relative_path_to_absolute() {
        let inodes = WasiInodes::new();
        let fs_backing =
            WasiFsRoot::from_filesystem(Arc::new(RootFileSystemBuilder::default().build_tmp()));
        let wasi_fs = WasiFs::new_init(fs_backing, &inodes, FS_ROOT_INO).unwrap();

        // Test absolute path (returned as-is, no normalization)
        assert_eq!(
            wasi_fs.relative_path_to_absolute("/foo/bar".to_string()),
            "/foo/bar"
        );
        assert_eq!(wasi_fs.relative_path_to_absolute("/".to_string()), "/");

        // Absolute paths with special components are not normalized
        assert_eq!(
            wasi_fs.relative_path_to_absolute("//foo//bar//".to_string()),
            "//foo//bar//"
        );
        assert_eq!(
            wasi_fs.relative_path_to_absolute("/a/b/./c".to_string()),
            "/a/b/./c"
        );
        assert_eq!(
            wasi_fs.relative_path_to_absolute("/a/b/../c".to_string()),
            "/a/b/../c"
        );

        // Test relative path with root as current dir
        assert_eq!(
            wasi_fs.relative_path_to_absolute("foo/bar".to_string()),
            "/foo/bar"
        );
        assert_eq!(wasi_fs.relative_path_to_absolute("foo".to_string()), "/foo");

        // Test with different current directory
        wasi_fs.set_current_dir("/home/user");
        assert_eq!(
            wasi_fs.relative_path_to_absolute("file.txt".to_string()),
            "/home/user/file.txt"
        );
        assert_eq!(
            wasi_fs.relative_path_to_absolute("dir/file.txt".to_string()),
            "/home/user/dir/file.txt"
        );

        // Test relative paths with . and .. components
        wasi_fs.set_current_dir("/a/b/c");
        assert_eq!(
            wasi_fs.relative_path_to_absolute("./file.txt".to_string()),
            "/a/b/c/./file.txt"
        );
        assert_eq!(
            wasi_fs.relative_path_to_absolute("../file.txt".to_string()),
            "/a/b/c/../file.txt"
        );
        assert_eq!(
            wasi_fs.relative_path_to_absolute("../../file.txt".to_string()),
            "/a/b/c/../../file.txt"
        );

        // Test edge cases
        assert_eq!(
            wasi_fs.relative_path_to_absolute(".".to_string()),
            "/a/b/c/."
        );
        assert_eq!(
            wasi_fs.relative_path_to_absolute("..".to_string()),
            "/a/b/c/.."
        );
        assert_eq!(wasi_fs.relative_path_to_absolute("".to_string()), "/a/b/c/");

        // Test current directory with trailing slash
        wasi_fs.set_current_dir("/home/user/");
        assert_eq!(
            wasi_fs.relative_path_to_absolute("file.txt".to_string()),
            "/home/user/file.txt"
        );

        // Test current directory without trailing slash
        wasi_fs.set_current_dir("/home/user");
        assert_eq!(
            wasi_fs.relative_path_to_absolute("file.txt".to_string()),
            "/home/user/file.txt"
        );
    }

    #[cfg(feature = "host-fs")]
    #[tokio::test]
    async fn mapped_preopen_inode_paths_should_stay_in_guest_space() {
        let root_dir = tempdir().unwrap();
        let hamlet_dir = root_dir.path().join("hamlet");
        std::fs::create_dir_all(&hamlet_dir).unwrap();

        let host_fs = virtual_fs::host_fs::FileSystem::new(
            tokio::runtime::Handle::current(),
            root_dir.path(),
        )
        .unwrap();

        let init = WasiEnvBuilder::new("test_prog")
            .engine(Engine::default())
            .fs(Arc::new(host_fs) as Arc<dyn FileSystem + Send + Sync>)
            .map_dir("hamlet", "/hamlet")
            .unwrap()
            .build_init()
            .unwrap();

        let preopen_inode = {
            let guard = init.state.fs.root_inode.read();
            let Kind::Root { entries } = guard.deref() else {
                panic!("expected root inode");
            };
            entries.get("hamlet").unwrap().clone()
        };
        let guard = preopen_inode.read();

        let Kind::Dir { path, .. } = guard.deref() else {
            panic!("expected preopen inode to be a directory");
        };

        assert_eq!(path, std::path::Path::new("/hamlet"));
    }

    #[cfg(all(unix, feature = "host-fs", feature = "sys"))]
    #[tokio::test]
    async fn backing_absolute_host_symlink_targets_stay_within_guest_mount() {
        let root_dir = tempfile::Builder::new()
            .prefix("wasix-backing-symlink")
            .tempdir_in("/tmp")
            .unwrap();
        let dir1 = root_dir.path().join("dir1");
        let dir2 = root_dir.path().join("dir2");
        std::fs::create_dir_all(&dir1).unwrap();
        std::fs::write(dir1.join("file1"), b"hello").unwrap();
        std::os::unix::fs::symlink(&dir1, &dir2).unwrap();

        let host_fs = virtual_fs::host_fs::FileSystem::new(
            tokio::runtime::Handle::current(),
            root_dir.path(),
        )
        .unwrap();
        let mount_fs = virtual_fs::MountFileSystem::new();
        mount_fs
            .mount(
                Path::new("/"),
                Arc::new(RootFileSystemBuilder::default().build_tmp()),
            )
            .unwrap();
        mount_fs
            .mount(
                Path::new("/host"),
                Arc::new(host_fs) as Arc<dyn FileSystem + Send + Sync>,
            )
            .unwrap();

        let inodes = WasiInodes::new();
        let fs_backing = WasiFsRoot::from_mount_fs(mount_fs);
        let wasi_fs =
            WasiFs::new_with_preopen(&inodes, &[], &["/".to_string()], fs_backing).unwrap();

        let literal_link = wasi_fs
            .get_inode_at_path(&inodes, crate::VIRTUAL_ROOT_FD, "/host/dir2", false)
            .unwrap();
        assert!(matches!(
            literal_link.read().deref(),
            Kind::Symlink {
                symlink_kind: SymlinkKind::Backing,
                relative_path,
                ..
            } if relative_path == Path::new("/dir1")
        ));

        let followed_dir = wasi_fs
            .get_inode_at_path(&inodes, crate::VIRTUAL_ROOT_FD, "/host/dir2", true)
            .unwrap();
        let followed_dir_path = {
            let guard = followed_dir.read();
            let Kind::Dir { path, .. } = guard.deref() else {
                panic!("expected followed backing symlink to resolve to a directory");
            };
            assert_eq!(path, Path::new("/host/dir1"));
            path.clone()
        };
        let mut entries = wasi_fs.root_fs.read_dir(&followed_dir_path).unwrap();
        assert!(entries.any(|entry| entry.unwrap().path() == Path::new("/host/dir1/file1")));

        let child = wasi_fs
            .get_inode_at_path(&inodes, crate::VIRTUAL_ROOT_FD, "/host/dir2/file1", true)
            .unwrap();
        assert!(matches!(
            child.read().deref(),
            Kind::File { path, .. } if path == Path::new("/host/dir1/file1")
        ));
    }

    #[tokio::test]
    async fn dot_mapped_preopen_uses_guest_current_dir() {
        let init = WasiEnvBuilder::new("test_prog")
            .engine(Engine::default())
            .current_dir("/work")
            .map_dir(".", "/work")
            .unwrap()
            .build_init()
            .unwrap();

        let preopen_inode = {
            let guard = init.state.fs.root_inode.read();
            let Kind::Root { entries } = guard.deref() else {
                panic!("expected root inode");
            };
            entries.get(".").unwrap().clone()
        };
        let guard = preopen_inode.read();

        let Kind::Dir { path, .. } = guard.deref() else {
            panic!("expected preopen inode to be a directory");
        };

        assert_eq!(path, std::path::Path::new("/work"));
    }

    #[tokio::test]
    async fn symlinked_directory_components_resolve_to_target_entries() {
        let inodes = WasiInodes::new();
        let fs_backing =
            WasiFsRoot::from_filesystem(Arc::new(RootFileSystemBuilder::default().build_tmp()));
        let wasi_fs =
            WasiFs::new_with_preopen(&inodes, &[], &["/".to_string()], fs_backing).unwrap();
        let root = &wasi_fs.root_fs;

        root.create_dir(Path::new("/orig")).unwrap();
        root.new_open_options()
            .create(true)
            .write(true)
            .open(Path::new("/orig/child.txt"))
            .unwrap();
        root.create_symlink(Path::new("/orig"), Path::new("/linked"))
            .unwrap();

        let literal_link = wasi_fs
            .get_inode_at_path(&inodes, crate::VIRTUAL_ROOT_FD, "/linked", false)
            .unwrap();
        assert!(matches!(
            literal_link.read().deref(),
            Kind::Symlink {
                relative_path,
                ..
            } if relative_path == Path::new("/orig")
        ));

        let followed_dir = wasi_fs
            .get_inode_at_path(&inodes, crate::VIRTUAL_ROOT_FD, "/linked", true)
            .unwrap();
        assert!(matches!(
            followed_dir.read().deref(),
            Kind::Dir { path, .. } if path == Path::new("/orig")
        ));

        let child = wasi_fs
            .get_inode_at_path(&inodes, crate::VIRTUAL_ROOT_FD, "/linked/child.txt", true)
            .unwrap();
        assert!(matches!(
            child.read().deref(),
            Kind::File { path, .. } if path == Path::new("/orig/child.txt")
        ));

        let child_without_final_follow = wasi_fs
            .get_inode_at_path(&inodes, crate::VIRTUAL_ROOT_FD, "/linked/child.txt", false)
            .unwrap();
        assert!(matches!(
            child_without_final_follow.read().deref(),
            Kind::File { path, .. } if path == Path::new("/orig/child.txt")
        ));
    }

    #[tokio::test]
    async fn webc_backing_symlink_resolves_to_target_entry() {
        let inodes = WasiInodes::new();
        let fs_backing = WasiFsRoot::from_filesystem(Arc::new(webc_symlink_fs()));
        let wasi_fs =
            WasiFs::new_with_preopen(&inodes, &[], &["/".to_string()], fs_backing).unwrap();

        let literal_link = wasi_fs
            .get_inode_at_path(&inodes, crate::VIRTUAL_ROOT_FD, "/link", false)
            .unwrap();
        assert!(matches!(
            literal_link.read().deref(),
            Kind::Symlink {
                relative_path,
                ..
            } if relative_path == Path::new("target.txt")
        ));

        let followed_file = wasi_fs
            .get_inode_at_path(&inodes, crate::VIRTUAL_ROOT_FD, "/link", true)
            .unwrap();
        assert!(matches!(
            followed_file.read().deref(),
            Kind::File { path, .. } if path == Path::new("/target.txt")
        ));
    }

    #[tokio::test]
    async fn path_resolution_preserves_posix_directory_component_rules() {
        let inodes = WasiInodes::new();
        let fs_backing =
            WasiFsRoot::from_filesystem(Arc::new(RootFileSystemBuilder::default().build_tmp()));
        let wasi_fs =
            WasiFs::new_with_preopen(&inodes, &[], &["/".to_string()], fs_backing).unwrap();
        let root = &wasi_fs.root_fs;

        root.create_dir(Path::new("/dir")).unwrap();
        root.new_open_options()
            .create(true)
            .write(true)
            .open(Path::new("/file"))
            .unwrap();
        root.create_symlink(Path::new("/dir"), Path::new("/dir-link"))
            .unwrap();
        root.create_symlink(Path::new("/file"), Path::new("/file-link"))
            .unwrap();

        let empty_path = wasi_fs
            .get_inode_at_path(&inodes, crate::VIRTUAL_ROOT_FD, "", true)
            .unwrap_err();
        assert_eq!(empty_path, Errno::Noent);

        let (single_component_parent, single_component_name) = wasi_fs
            .get_parent_inode_at_path(&inodes, crate::VIRTUAL_ROOT_FD, Path::new("new-file"), true)
            .unwrap();
        assert_eq!(single_component_name, "new-file");
        assert!(matches!(
            single_component_parent.read().deref(),
            Kind::Root { .. }
        ));

        let root_parent = wasi_fs
            .get_inode_at_path(&inodes, crate::VIRTUAL_ROOT_FD, "/..", true)
            .unwrap();
        assert!(matches!(root_parent.read().deref(), Kind::Root { .. }));

        let escaped_symlink_target = wasi_fs
            .resolve_symlink_target_path(
                SymlinkKind::Virtual,
                Path::new("fs_sandbox_symlink.dir/link"),
                Path::new("../../README.md"),
            )
            .unwrap_err();
        assert_eq!(escaped_symlink_target, Errno::Perm);

        let (_, contained_symlink_target) = wasi_fs
            .resolve_symlink_target_path(
                SymlinkKind::Virtual,
                Path::new("fs_sandbox_symlink.dir/link"),
                Path::new("../README.md"),
            )
            .unwrap();
        assert_eq!(contained_symlink_target, Path::new("README.md"));

        let (_, sibling_preopen_symlink_target) = wasi_fs
            .resolve_symlink_target_path(
                SymlinkKind::Virtual,
                Path::new("temp/act3"),
                Path::new("../hamlet/act3"),
            )
            .unwrap();
        assert_eq!(sibling_preopen_symlink_target, Path::new("hamlet/act3"));

        let escaped_sibling_preopen_symlink_target = wasi_fs
            .resolve_symlink_target_path(
                SymlinkKind::Virtual,
                Path::new("temp/act3"),
                Path::new("../../outside"),
            )
            .unwrap_err();
        assert_eq!(escaped_sibling_preopen_symlink_target, Errno::Perm);

        root.create_dir(Path::new("/outerdir")).unwrap();
        root.create_dir(Path::new("/outerdir/dest")).unwrap();
        root.new_open_options()
            .create(true)
            .write(true)
            .open(Path::new("/outerdir/evil"))
            .unwrap();
        let dest_dir = wasi_fs
            .get_inode_at_path(&inodes, crate::VIRTUAL_ROOT_FD, "/outerdir/dest", true)
            .unwrap();
        let current_link = wasi_fs.create_inode_with_default_stat(
            &inodes,
            Kind::Symlink {
                symlink_kind: SymlinkKind::Virtual,
                path_to_symlink: PathBuf::from("outerdir/dest/current"),
                relative_path: PathBuf::from("."),
            },
            false,
            Cow::Borrowed("current"),
        );
        let parent_link = wasi_fs.create_inode_with_default_stat(
            &inodes,
            Kind::Symlink {
                symlink_kind: SymlinkKind::Virtual,
                path_to_symlink: PathBuf::from("outerdir/dest/parent"),
                relative_path: PathBuf::from("current/.."),
            },
            false,
            Cow::Borrowed("parent"),
        );
        {
            let mut guard = dest_dir.write();
            let Kind::Dir { entries, .. } = guard.deref_mut() else {
                panic!("expected destination to be a directory");
            };
            entries.insert("current".to_string(), current_link);
            entries.insert("parent".to_string(), parent_link);
        }

        let parent_symlink_target = wasi_fs
            .get_inode_at_path(
                &inodes,
                crate::VIRTUAL_ROOT_FD,
                "/outerdir/dest/parent/evil",
                true,
            )
            .unwrap();
        assert!(matches!(
            parent_symlink_target.read().deref(),
            Kind::File { path, .. } if path == Path::new("/outerdir/evil")
        ));

        let file_dot = wasi_fs
            .get_inode_at_path(&inodes, crate::VIRTUAL_ROOT_FD, "/file/.", true)
            .unwrap_err();
        assert_eq!(file_dot, Errno::Notdir);

        let file_slash = wasi_fs
            .get_inode_at_path(&inodes, crate::VIRTUAL_ROOT_FD, "/file/", true)
            .unwrap_err();
        assert_eq!(file_slash, Errno::Notdir);

        let symlinked_dir_slash = wasi_fs
            .get_inode_at_path(&inodes, crate::VIRTUAL_ROOT_FD, "/dir-link/", false)
            .unwrap();
        assert!(matches!(
            symlinked_dir_slash.read().deref(),
            Kind::Dir { path, .. } if path == Path::new("/dir")
        ));

        let symlinked_file_slash = wasi_fs
            .get_inode_at_path(&inodes, crate::VIRTUAL_ROOT_FD, "/file-link/", false)
            .unwrap_err();
        assert_eq!(symlinked_file_slash, Errno::Notdir);

        root.create_symlink(Path::new("/loop"), Path::new("/loop"))
            .unwrap();
        let symlink_loop = wasi_fs
            .get_inode_at_path(&inodes, crate::VIRTUAL_ROOT_FD, "/loop", true)
            .unwrap_err();
        assert_eq!(symlink_loop, Errno::Loop);
    }

    #[tokio::test]
    async fn writable_root_is_preserved_through_root_overlays() {
        let base_root = Arc::new(RootFileSystemBuilder::default().build_tmp());
        let root = WasiFsRoot::from_filesystem(base_root);
        assert!(root.writable_root().is_some());

        let lower = Arc::new(TmpFileSystem::new()) as Arc<dyn FileSystem + Send + Sync>;
        root.stack_root_filesystem(lower).unwrap();

        assert!(root.writable_root().is_some());
    }

    #[tokio::test]
    async fn conditional_union_merges_root_and_non_root_package_mounts_once() {
        let inodes = WasiInodes::new();
        let fs_backing =
            WasiFsRoot::from_filesystem(Arc::new(RootFileSystemBuilder::default().build_tmp()));
        let wasi_fs = WasiFs::new_init(fs_backing, &inodes, FS_ROOT_INO).unwrap();

        let root_layer = TmpFileSystem::new();
        root_layer
            .new_open_options()
            .create(true)
            .write(true)
            .open(Path::new("/root.txt"))
            .unwrap();

        let public_mount = TmpFileSystem::new();
        public_mount
            .new_open_options()
            .create(true)
            .write(true)
            .open(Path::new("/index.html"))
            .unwrap();

        let pkg = BinaryPackage {
            id: PackageId::new_named("ns/pkg", "0.1.0".parse().unwrap()),
            package_ids: vec![],
            when_cached: None,
            entrypoint_cmd: None,
            hash: OnceCell::new(),
            package_mounts: Some(Arc::new(BinaryPackageMounts {
                root_layer: Some(Arc::new(root_layer)),
                mounts: vec![BinaryPackageMount {
                    guest_path: PathBuf::from("/public"),
                    fs: Arc::new(public_mount),
                    source_path: PathBuf::from("/"),
                }],
            })),
            commands: vec![],
            uses: vec![],
            file_system_memory_footprint: 0,
            additional_host_mapped_directories: vec![],
        };

        wasi_fs.conditional_union(&pkg).await.unwrap();
        assert!(
            wasi_fs
                .root_fs
                .metadata(Path::new("/root.txt"))
                .unwrap()
                .is_file()
        );
        assert!(
            wasi_fs
                .root_fs
                .metadata(Path::new("/public/index.html"))
                .unwrap()
                .is_file()
        );

        wasi_fs.conditional_union(&pkg).await.unwrap();
        assert!(
            wasi_fs
                .root_fs
                .metadata(Path::new("/root.txt"))
                .unwrap()
                .is_file()
        );
        assert!(
            wasi_fs
                .root_fs
                .metadata(Path::new("/public/index.html"))
                .unwrap()
                .is_file()
        );
    }
}
