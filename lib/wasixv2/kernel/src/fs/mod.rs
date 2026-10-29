//! Filesystem syscalls: argument decoding and per-process state around `wasixv2_vfs`, which
//! decides every path and permission question.

mod attr;
pub(crate) mod dnotify;
pub(crate) mod fanotify;
mod fhandle;
mod fifo;
mod fileattr;
mod fsopen;
pub(crate) mod inotify;
pub(crate) mod io;
mod lock;
pub(crate) mod mount;
mod path;
mod quota;
mod splice;
mod statmount;

pub(crate) use fifo::FifoFile;
pub(crate) use io::{At, read_iovecs};

use std::{
    cell::Cell,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicI32, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;

use wasixv2_abi::{
    AtFd, AtFlags, CloseRangeFlags, ConstOpenHowPtr, Cstr, FcntlCmd, FdFlags, OpenFlags, OpenHow,
    ResolveFlags,
};
use wasixv2_host::Host;
use wasixv2_syscalls::{
    Caller, CstrError, Errno, Fd, SysErr, SysResult, TaskId, UserMemory, read_cstr, read_value,
};
use wasixv2_vfs::{
    Clock, Cred, Ctx, FsResult, LeaseWait, LookupFlags, MODE_PERM, PATH_MAX, Path, Timestamp,
};
use wasixv2_wait::{Interrupted, interruptible};

use crate::{
    Kernel,
    file::{Backing, FdTable, OpenFile},
    signal::TaskSignals,
    sync::MutexExt,
};

/// The `fs.*` tunables of `/proc/sys/fs` that the kernel acts on, `proc_dointvec` integers.
#[derive(Debug)]
pub(crate) struct FsSysctls {
    /// `fs.dir-notify-enable`: whether `F_NOTIFY` works.
    dir_notify_enable: AtomicI32,
    /// `fs.leases-enable`: whether `F_SETLEASE` gives new leases.
    leases_enable: AtomicI32,
    /// `fs.lease-break-time`: the seconds a lease holder has to give way; none at most 0.
    lease_break_time: AtomicI32,
}

impl Default for FsSysctls {
    /// Linux's defaults.
    fn default() -> Self {
        Self {
            dir_notify_enable: AtomicI32::new(1),
            leases_enable: AtomicI32::new(1),
            lease_break_time: AtomicI32::new(45),
        }
    }
}

impl FsSysctls {
    pub(crate) fn dir_notify_enable(&self) -> i32 {
        self.dir_notify_enable.load(Ordering::Relaxed)
    }

    pub(crate) fn set_dir_notify_enable(&self, value: i32) {
        self.dir_notify_enable.store(value, Ordering::Relaxed);
    }

    pub(crate) fn leases_enable(&self) -> i32 {
        self.leases_enable.load(Ordering::Relaxed)
    }

    pub(crate) fn set_leases_enable(&self, value: i32) {
        self.leases_enable.store(value, Ordering::Relaxed);
    }

    pub(crate) fn lease_break_time(&self) -> i32 {
        self.lease_break_time.load(Ordering::Relaxed)
    }

    pub(crate) fn set_lease_break_time(&self, value: i32) {
        self.lease_break_time.store(value, Ordering::Relaxed);
    }
}

/// How a task's opens and truncations wait for the leases they break: on the host's
/// monotonic clock, at most `fs.lease-break-time` per holder, until a signal for the task.
pub(crate) struct LeaseWaiter {
    host: Arc<dyn Host>,
    sysctls: Arc<FsSysctls>,
    signals: Arc<TaskSignals>,
    /// A signal ended a wait: the call restarts (`-ERESTARTSYS` from `__break_lease`).
    interrupted: Cell<bool>,
}

#[async_trait(?Send)]
impl LeaseWait for LeaseWaiter {
    fn now(&self) -> Duration {
        self.host.monotonic()
    }

    fn break_time(&self) -> Option<Duration> {
        let secs = self.sysctls.lease_break_time();
        (secs > 0).then(|| Duration::from_secs(secs as u64))
    }

    async fn wait<'a>(
        &'a self,
        changed: Pin<Box<dyn Future<Output = ()> + 'a>>,
        deadline: Option<Duration>,
    ) -> FsResult<()> {
        let wait = async {
            match deadline {
                Some(deadline) => {
                    let timer = self.host.sleep_until(deadline);
                    futures_util::future::select(changed, timer).await;
                }
                None => changed.await,
            }
        };
        match interruptible(wait, &*self.signals).await {
            Ok(()) => Ok(()),
            Err(Interrupted) => {
                self.interrupted.set(true);
                Err(Errno::EINTR)
            }
        }
    }

    async fn wait_killable<'a>(
        &'a self,
        answer: Pin<Box<dyn Future<Output = FsResult<()>> + 'a>>,
    ) -> FsResult<()> {
        match interruptible(answer, &self.signals.killable()).await {
            Ok(result) => result,
            Err(Interrupted) => {
                self.interrupted.set(true);
                Err(Errno::EINTR)
            }
        }
    }
}

/// The soft `RLIMIT_NOFILE` every process has until resource limits exist.
pub(crate) const NOFILE: usize = 1024;

/// `FASYNC` (`asm-generic/fcntl.h`).
pub(crate) const FASYNC: u32 = 0o20000;

/// `VALID_OPEN_FLAGS`: what `open` keeps; everything else is ignored by `open` and rejected by
/// `openat2`.
const VALID_OPEN_FLAGS: u32 = OpenFlags::O_ACCMODE.0
    | OpenFlags::O_CREAT.0
    | OpenFlags::O_EXCL.0
    | OpenFlags::O_NOCTTY.0
    | OpenFlags::O_TRUNC.0
    | OpenFlags::O_APPEND.0
    | OpenFlags::O_NONBLOCK.0
    | OpenFlags::O_DSYNC.0
    | FASYNC
    | OpenFlags::O_DIRECT.0
    | OpenFlags::O_LARGEFILE.0
    | OpenFlags::O_DIRECTORY.0
    | OpenFlags::O_NOFOLLOW.0
    | OpenFlags::O_NOATIME.0
    | OpenFlags::O_CLOEXEC.0
    | OpenFlags::__O_SYNC.0
    | OpenFlags::O_PATH.0
    | OpenFlags::__O_TMPFILE.0
    | OpenFlags::O_EMPTYPATH.0;

/// `O_PATH_FLAGS`.
const O_PATH_FLAGS: u32 = OpenFlags::O_DIRECTORY.0
    | OpenFlags::O_NOFOLLOW.0
    | OpenFlags::O_PATH.0
    | OpenFlags::O_CLOEXEC.0
    | OpenFlags::O_EMPTYPATH.0;

const VALID_RESOLVE_FLAGS: u64 = ResolveFlags::RESOLVE_NO_XDEV.0
    | ResolveFlags::RESOLVE_NO_MAGICLINKS.0
    | ResolveFlags::RESOLVE_NO_SYMLINKS.0
    | ResolveFlags::RESOLVE_BENEATH.0
    | ResolveFlags::RESOLVE_IN_ROOT.0
    | ResolveFlags::RESOLVE_CACHED.0;

/// The host's realtime clock as the VFS's clock, for filesystem timestamps.
pub struct HostClock(pub Arc<dyn Host>);

impl Clock for HostClock {
    fn now(&self) -> Timestamp {
        let now = self.0.realtime();
        Timestamp::new(now.as_secs() as i64, now.subsec_nanos())
    }
}

/// A process's root, working directory and umask (Linux's `fs_struct`).
#[derive(Clone)]
pub(crate) struct FsContext {
    pub(crate) root: Path,
    pub(crate) cwd: Path,
    pub(crate) umask: u32,
}

impl FsContext {
    pub(crate) fn new(root: Path) -> Self {
        Self {
            cwd: root.clone(),
            root,
            umask: 0o022,
        }
    }

    /// `current_chrooted`: the root is not the root of the mount namespace `ns` (what is
    /// mounted on top of it, as entering the namespace gives).
    pub(crate) fn chrooted(&self, ns: &crate::ns::NsProxy) -> bool {
        self.root != ns.mnt.tree.entry_root()
    }
}

/// What a filesystem syscall runs with: a snapshot of the caller's context and credentials.
pub(crate) struct Env {
    pub(crate) fs: FsContext,
    pub(crate) cred: Cred,
    pub(crate) task: u32,
    leases: LeaseWaiter,
}

impl Env {
    pub(crate) fn cx(&self) -> Ctx<'_> {
        Ctx {
            cred: &self.cred,
            root: &self.fs.root,
            umask: self.fs.umask,
            task: self.task,
            leases: Some(&self.leases),
        }
    }

    /// The error of a VFS call made with [`Env::cx`]: a restart when a signal ended its wait
    /// for leases.
    pub(crate) fn error(&self, errno: Errno) -> SysErr {
        match self.leases.interrupted.get() {
            true => SysErr::Restart,
            false => errno.into(),
        }
    }
}

impl Kernel {
    pub(crate) fn env(&self, caller: &Caller<'_>) -> Env {
        self.task_env(caller.task())
    }

    pub(crate) fn task_env(&self, task: TaskId) -> Env {
        let (fs, signals) = self.task_ref(task, |task| {
            let fs = task.fs.lock_or_recover().clone();
            (fs, task.signals.clone())
        });
        Env {
            fs,
            cred: self.current_cred(task).fs(),
            task: task.0,
            leases: LeaseWaiter {
                host: self.host.clone(),
                sysctls: self.sysctls.clone(),
                signals,
                interrupted: Cell::new(false),
            },
        }
    }

    /// The directory a relative path starts from: the working directory or `dfd`'s location
    /// (`path_init`). Absolute paths never look at `dfd`.
    pub(crate) fn start(
        &self,
        caller: &Caller<'_>,
        env: &Env,
        dfd: Fd,
        path: &[u8],
    ) -> Result<Path, Errno> {
        if path.starts_with(b"/") || dfd.0 == AtFd::AT_FDCWD.0 {
            return Ok(env.fs.cwd.clone());
        }
        Ok(self.file(caller.task(), dfd)?.path().clone())
    }

    /// The path an fd-based operation acts on: the file's, or its object's inode.
    pub(crate) fn fd_path(&self, caller: &Caller<'_>, fd: Fd) -> Result<Path, Errno> {
        Ok(self.file(caller.task(), fd)?.path().clone())
    }

    /// An open file usable for I/O-like operations: `O_PATH` descriptors are `EBADF`.
    pub(crate) fn io_file(&self, caller: &Caller<'_>, fd: Fd) -> Result<Arc<OpenFile>, Errno> {
        let file = self.file(caller.task(), fd)?;
        if file.flags().contains(OpenFlags::O_PATH) {
            return Err(Errno::EBADF);
        }
        Ok(file)
    }

    /// Resolves `dfd` + `path` with `AT_*` flags (`AT_SYMLINK_NOFOLLOW`, `AT_EMPTY_PATH`).
    pub(crate) async fn resolve_at(
        &self,
        caller: &Caller<'_>,
        env: &Env,
        dfd: Fd,
        path: &[u8],
        flags: AtFlags,
        follow_default: bool,
    ) -> Result<Path, Errno> {
        let empty = flags.contains(AtFlags::AT_EMPTY_PATH);
        if path.is_empty() && empty {
            return self.start_or_fd(caller, env, dfd);
        }
        let start = self.start(caller, env, dfd, path)?;
        let lookup = LookupFlags {
            follow: follow_default && !flags.contains(AtFlags::AT_SYMLINK_NOFOLLOW),
            empty,
            ..LookupFlags::default()
        };
        self.vfs.lookup(&env.cx(), &start, path, lookup).await
    }

    /// `dfd` itself, for an empty path or a walk rooted there: the working directory for
    /// `AT_FDCWD`.
    fn start_or_fd(&self, caller: &Caller<'_>, env: &Env, dfd: Fd) -> Result<Path, Errno> {
        if dfd.0 == AtFd::AT_FDCWD.0 {
            return Ok(env.fs.cwd.clone());
        }
        self.fd_path(caller, dfd)
    }

    /// Moves `task`'s working directory to `path`, as `chdir` would, for the embedder.
    pub async fn set_working_dir(&self, task: TaskId, path: &[u8]) -> Result<(), Errno> {
        let env = self.task_env(task);
        let lookup = LookupFlags {
            follow: true,
            directory: true,
            ..LookupFlags::default()
        };
        let dir = self
            .vfs
            .lookup(&env.cx(), &env.fs.cwd, path, lookup)
            .await?;
        self.vfs
            .permission(&env.cx(), &dir, wasixv2_vfs::MAY_EXEC | 0x40)
            .await?;
        self.with_task(task, |task| task.fs.lock_or_recover().cwd = dir);
        Ok(())
    }

    pub(crate) async fn sys_openat(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        flags: OpenFlags,
        mode: u32,
    ) -> SysResult<Fd> {
        self.openat_to(caller, dfd, filename, flags, mode, FileDest::Table)
            .await
    }

    pub(crate) async fn openat_to(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        flags: OpenFlags,
        mode: u32,
        dest: FileDest<'_>,
    ) -> SysResult<Fd> {
        self.do_open(caller, dfd, filename, build_open_how(flags, mode), dest)
            .await
    }

    pub(crate) async fn sys_openat2(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        how: ConstOpenHowPtr,
        size: u32,
    ) -> SysResult<Fd> {
        self.openat2_to(caller, dfd, filename, how, size, FileDest::Table)
            .await
    }

    pub(crate) async fn openat2_to(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        how: ConstOpenHowPtr,
        size: u32,
        dest: FileDest<'_>,
    ) -> SysResult<Fd> {
        const VER0: u32 = 24;
        if size < VER0 {
            return Err(Errno::EINVAL.into());
        }
        if size > 4096 {
            return Err(Errno::E2BIG.into());
        }
        let value: OpenHow = read_value(caller.memory(), how).await?;
        // `copy_struct_from_user`: a larger struct must be zero past what we know.
        if size > VER0 {
            let mut rest = vec![0; (size - VER0) as usize];
            caller
                .memory()
                .read(how.addr() + VER0, &mut rest)
                .await
                .map_err(|_| Errno::EFAULT)?;
            if rest.iter().any(|&b| b != 0) {
                return Err(Errno::E2BIG.into());
            }
        }
        self.do_open(caller, dfd, filename, value, dest).await
    }

    /// `build_open_flags` + `do_file_open`.
    async fn do_open(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        how: OpenHow,
        dest: FileDest<'_>,
    ) -> SysResult<Fd> {
        let (flags, lookup) = build_open_flags(&how)?;
        let path = read_path(caller.memory(), filename).await?;
        let env = self.env(caller);
        // `path_init`: `LOOKUP_IN_ROOT` starts even an absolute path at `dfd`, its root.
        let start = if (path.is_empty() && lookup.empty) || lookup.in_root {
            self.start_or_fd(caller, &env, dfd)?
        } else {
            self.start(caller, &env, dfd, &path)?
        };
        self.open_from(
            caller,
            &env,
            &start,
            &path,
            flags,
            how.mode as u32,
            lookup,
            dest,
        )
        .await
    }

    /// `do_filp_open` of `path` from `start` with flags `build_open_flags` checked, and the
    /// new descriptor.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn open_from(
        &self,
        caller: &Caller<'_>,
        env: &Env,
        start: &Path,
        path: &[u8],
        flags: OpenFlags,
        mode: u32,
        lookup: LookupFlags,
        dest: FileDest<'_>,
    ) -> SysResult<Fd> {
        let vfs_how = wasixv2_vfs::OpenHow {
            flags,
            mode,
            lookup,
        };
        // `get_unused_fd_flags` before `do_filp_open`: at the limit, nothing is created,
        // truncated or waited for.
        let cloexec = flags.contains(OpenFlags::O_CLOEXEC);
        if dest.is_direct() && cloexec {
            return Err(Errno::EINVAL.into());
        }
        let fd = dest.reserve(self, caller, cloexec)?;
        let file = self
            .vfs
            .open(&env.cx(), start, path, &vfs_how)
            .await
            .map_err(|errno| env.error(errno))?;
        // `hook_file_open`'s `allowed_access`, kept for ftruncate and ioctl.
        let rights = match file.path_only {
            true => None,
            false => self.landlock(caller).map(|d| d.file_rights(&file.path)),
        };
        let backing = self.attach(caller, file, flags).await?;
        let file = OpenFile::new(backing, flags).with_landlock(rights);
        fd.install(Arc::new(file))
    }

    /// `do_dentry_open`'s `f_op->open`: a FIFO attaches to its pipe and a device node to its
    /// driver; what the inode holds directly is read through the VFS.
    pub(crate) async fn attach(
        &self,
        caller: &Caller<'_>,
        file: wasixv2_vfs::File,
        flags: OpenFlags,
    ) -> SysResult<Backing> {
        if file.path_only {
            return Ok(Backing::Inode(file));
        }
        let path = file.path.clone();
        // A namespace's inode opens as the namespace (`ns_file_operations`).
        if let Some(ns) = crate::nsfs::namespace(&path) {
            return Ok(Backing::Stream(
                Arc::new(crate::nsfs::NsFile::new(ns)),
                path,
            ));
        }
        // `/proc/kmsg` opens as the kernel log (`proc_kmsg_operations`).
        if crate::procfs::is_kmsg(&path) {
            return Ok(Backing::Stream(self.open_proc_kmsg(caller)?, path));
        }
        // A message queue's file opens as the queue (`mqueue_file_operations`).
        if let Some(queue) = crate::ipc::mqueue::queue_of(&path) {
            return Ok(Backing::Stream(Arc::new(queue), path));
        }
        let object: Arc<dyn crate::FileObject> = match file.file_type() {
            wasixv2_vfs::FileType::Fifo => {
                if let Some(object) = crate::stdio::reopen(&file) {
                    return Ok(Backing::Stream(object, path));
                }
                let interrupt = self.interrupt(caller);
                let capacity = self.new_pipe_size(caller);
                Arc::new(fifo::open(file, flags, capacity, &*interrupt).await?)
            }
            wasixv2_vfs::FileType::CharDevice | wasixv2_vfs::FileType::BlockDevice => {
                self.open_device(caller, file, flags).await?
            }
            _ => return Ok(Backing::Inode(file)),
        };
        Ok(Backing::Stream(object, path))
    }

    pub(crate) fn install(
        &self,
        caller: &Caller<'_>,
        file: Arc<OpenFile>,
        cloexec: bool,
    ) -> SysResult<Fd> {
        let files = self.files(caller.task());
        let mut files = files.lock_or_recover();
        Ok(files.install(file, cloexec).ok_or(Errno::EMFILE)?)
    }

    /// Reserves the `N` lowest free descriptors, all or none (`EMFILE`), for files that are
    /// installed once nothing can fail any more.
    pub(crate) fn reserve_fds<const N: usize>(
        &self,
        caller: &Caller<'_>,
        cloexec: bool,
    ) -> Result<[FdReservation; N], Errno> {
        let table = self.files(caller.task());
        let mut files = table.lock_or_recover();
        if !files.has_room(N) {
            return Err(Errno::EMFILE);
        }
        let fds = [(); N].map(|()| files.reserve().expect("room checked above"));
        drop(files);
        Ok(fds.map(|fd| FdReservation {
            table: Some(table.clone()),
            fd,
            cloexec,
        }))
    }

    /// What closing a descriptor does besides dropping the reference: release the process's
    /// POSIX locks on the file (`filp_flush` → `locks_remove_posix`).
    pub(crate) fn closed(&self, caller: &Caller<'_>, file: &OpenFile) {
        self.closed_by(self.process(caller.task()).pid, file);
    }

    /// [`Kernel::closed`] for the thread group `pid`, which needs none of its tasks: a process
    /// whose last task is gone still closes its descriptors.
    pub(crate) fn closed_by(&self, pid: TaskId, file: &OpenFile) {
        // ---- ipc ----
        self.mq_flush(pid, file);
        if let Backing::Inode(inner) = &file.backing
            && !inner.path_only
        {
            let key = inner.lock_key();
            let owner = wasixv2_vfs::LockOwner::Process(u64::from(pid.0));
            self.vfs.locks.release_owner(key, owner);
            let object = wasixv2_vfs::FsObject {
                sb: key.0,
                ino: key.1,
            };
            self.dnotify.flush(object, file, pid);
        }
    }

    pub(crate) async fn sys_close(&self, caller: &Caller<'_>, fd: Fd) -> SysResult<i32> {
        let files = self.files(caller.task());
        let file = files.lock_or_recover().remove(fd).ok_or(Errno::EBADF)?;
        let result = if let Backing::Inode(inode) = &file.backing {
            inode
                .flush(u64::from(self.process(caller.task()).pid.0))
                .await
        } else {
            Ok(())
        };
        self.closed(caller, &file);
        result?;
        Ok(0)
    }

    pub(crate) async fn sys_close_range(
        &self,
        caller: &Caller<'_>,
        first: u32,
        last: u32,
        flags: CloseRangeFlags,
    ) -> SysResult<i32> {
        let known = CloseRangeFlags::CLOSE_RANGE_UNSHARE | CloseRangeFlags::CLOSE_RANGE_CLOEXEC;
        if !(flags & !known).is_empty() || first > last {
            return Err(Errno::EINVAL.into());
        }
        let mut files = self.files(caller.task());
        // `unshare_fd`: a table shared with other tasks is copied first, so only the
        // caller's descriptors close. The task and this function hold one reference each.
        if flags.contains(CloseRangeFlags::CLOSE_RANGE_UNSHARE) && Arc::strong_count(&files) > 2 {
            let copy = Arc::new(Mutex::new(files.lock_or_recover().dup()?));
            self.with_task(caller.task(), |task| task.files = copy.clone());
            files = copy;
        }
        let closed: Vec<Arc<OpenFile>> = {
            let mut table = files.lock_or_recover();
            let end = table.end().min(last as usize + 1);
            let range = (first as usize..end).map(|index| Fd(index as i32));
            match flags.contains(CloseRangeFlags::CLOSE_RANGE_CLOEXEC) {
                true => {
                    for fd in range {
                        table.set_cloexec(fd, true);
                    }
                    Vec::new()
                }
                false => range.filter_map(|fd| table.remove(fd)).collect(),
            }
        };
        for file in &closed {
            if let Backing::Inode(inode) = &file.backing {
                let _ = inode
                    .flush(u64::from(self.process(caller.task()).pid.0))
                    .await;
            }
            self.closed(caller, file);
        }
        Ok(0)
    }

    pub(crate) fn sys_dup(&self, caller: &Caller<'_>, fd: Fd) -> SysResult<Fd> {
        let file = self.file(caller.task(), fd)?;
        self.install(caller, file, false)
    }

    pub(crate) async fn sys_dup3(
        &self,
        caller: &Caller<'_>,
        old: Fd,
        new: Fd,
        flags: OpenFlags,
    ) -> SysResult<Fd> {
        if !(flags & !OpenFlags::O_CLOEXEC).is_empty() {
            return Err(Errno::EINVAL.into());
        }
        if old == new {
            return Err(Errno::EINVAL.into());
        }
        // `do_dup2` under `file_lock`: the lookup and the install are one step, so a
        // concurrent close of `old` cannot be undone.
        let files = self.files(caller.task());
        let replaced = {
            let mut table = files.lock_or_recover();
            if new.0 < 0 || new.0 as usize >= table.limit() {
                return Err(Errno::EBADF.into());
            }
            let file = table.get(old).ok_or(Errno::EBADF)?;
            // `expand_files`: a table that cannot grow to `new` is `EBADF` here, not `EMFILE`.
            if table.expand(new.0 as usize).is_err() {
                return Err(Errno::EBADF.into());
            }
            // A descriptor reserved by an open still in progress.
            if table.is_reserved(new) {
                return Err(Errno::EBUSY.into());
            }
            table.install_at(new, file, flags.contains(OpenFlags::O_CLOEXEC))
        };
        if let Some(replaced) = replaced {
            if let Backing::Inode(inode) = &replaced.backing {
                let _ = inode
                    .flush(u64::from(self.process(caller.task()).pid.0))
                    .await;
            }
            self.closed(caller, &replaced);
        }
        Ok(new)
    }

    /// `fcntl`: descriptor flags, status flags, duplication, record locks, owners and signals
    /// of signal-driven I/O, pipe sizes and seals.
    pub(crate) async fn sys_fcntl(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        cmd: FcntlCmd,
        arg: u32,
    ) -> SysResult<i32> {
        let file = self.file(caller.task(), fd)?;
        let path_only = file.flags().contains(OpenFlags::O_PATH);
        let files = self.files(caller.task());
        match cmd {
            FcntlCmd::F_DUPFD | FcntlCmd::F_DUPFD_CLOEXEC => {
                let mut table = files.lock_or_recover();
                if arg as usize >= table.limit() {
                    return Err(Errno::EINVAL.into());
                }
                let cloexec = cmd == FcntlCmd::F_DUPFD_CLOEXEC;
                let fd = table
                    .install_from(arg as usize, file, cloexec)
                    .ok_or(Errno::EMFILE)?;
                Ok(fd.0)
            }
            FcntlCmd::F_GETFD => {
                let cloexec = files.lock_or_recover().cloexec(fd).unwrap_or_default();
                Ok(if cloexec {
                    FdFlags::FD_CLOEXEC.0 as i32
                } else {
                    0
                })
            }
            FcntlCmd::F_SETFD => {
                let cloexec = arg & FdFlags::FD_CLOEXEC.0 != 0;
                files.lock_or_recover().set_cloexec(fd, cloexec);
                Ok(0)
            }
            FcntlCmd::F_GETFL => Ok(file.flags().0 as i32),
            // `f_created_query`: whether this open made the file (`FMODE_CREATED`).
            FcntlCmd::F_CREATED_QUERY => Ok(match &file.backing {
                Backing::Inode(inner) => i32::from(inner.created),
                Backing::Stream(..) => 0,
            }),
            _ if path_only => Err(Errno::EBADF.into()),
            FcntlCmd::F_SETFL => {
                let flags = OpenFlags(arg);
                // `setfl`: an append-only file stays `O_APPEND`.
                if ((flags ^ file.flags()) & OpenFlags::O_APPEND).0 != 0
                    && let Backing::Inode(inner) = &file.backing
                    && inner.node().inode_flags() & wasixv2_vfs::FS_APPEND_FL != 0
                {
                    return Err(Errno::EPERM.into());
                }
                // `setfl`: O_NOATIME needs ownership, as at open.
                if flags.contains(OpenFlags::O_NOATIME)
                    && !file.flags().contains(OpenFlags::O_NOATIME)
                    && let Backing::Inode(inner) = &file.backing
                {
                    let attr = inner.node().getattr().await?;
                    if !self.env(caller).cred.owns(&attr) {
                        return Err(Errno::EPERM.into());
                    }
                }
                if flags.contains(OpenFlags::O_DIRECT) && !file.can_direct() {
                    return Err(Errno::EINVAL.into());
                }
                // The object's `fasync` changes `O_ASYNC`; without one it stays as it is.
                if (flags.0 ^ file.flags().0) & FASYNC != 0
                    && let Some(result) = self.fasync(caller, fd, &file, flags.0 & FASYNC != 0)
                {
                    result?;
                }
                file.set_status_flags(flags);
                Ok(0)
            }
            FcntlCmd::F_GETOWN => Ok(self.f_getown(caller, &file)),
            FcntlCmd::F_SETOWN => self.f_setown(caller, &file, arg as i32),
            FcntlCmd::F_GETOWN_EX => self.f_getown_ex(caller, &file, arg).await,
            FcntlCmd::F_SETOWN_EX => self.f_setown_ex(caller, &file, arg).await,
            FcntlCmd::F_GETOWNER_UIDS => self.f_getowner_uids(caller, &file, arg).await,
            FcntlCmd::F_GETSIG => Ok(self.f_getsig(&file)),
            FcntlCmd::F_SETSIG => self.f_setsig(&file, arg),
            FcntlCmd::F_GETPIPE_SZ | FcntlCmd::F_SETPIPE_SZ => {
                // `pipe_set_size`: `capable(CAP_SYS_RESOURCE)`.
                let privileged = self
                    .cred(caller)
                    .capable_init(wasixv2_vfs::Cap::SysResource);
                let max = self.pipe_max_size.get();
                crate::pipe::fcntl(&file, cmd, arg, max, privileged)
            }
            FcntlCmd::F_ADD_SEALS | FcntlCmd::F_GET_SEALS => {
                self.memfd_fcntl(&file, cmd, arg).await
            }
            FcntlCmd::F_NOTIFY => self.fcntl_dirnotify(caller, fd, &file, arg),
            FcntlCmd::F_SETLEASE => self.fcntl_setlease(caller, fd, &file, arg).await,
            FcntlCmd::F_GETLEASE => Ok(self.fcntl_getlease(&file)),
            _ => self.fcntl_lock(caller, fd, &file, cmd, arg).await,
        }
    }
}

/// A new open file goes either into the process table or into an io_uring table.
/// Linux io_uring/openclose.c:io_openat2 and net.c:io_accept skip ordinary fd
/// reservation for a direct descriptor, installing it after the operation succeeds.
#[derive(Clone, Copy)]
pub(crate) enum FileDest<'a> {
    Table,
    Direct(&'a crate::io_uring::Ctx, u32),
}

impl<'a> FileDest<'a> {
    pub(crate) fn is_direct(self) -> bool {
        matches!(self, Self::Direct(..))
    }

    pub(crate) fn reserve(
        self,
        kernel: &Kernel,
        caller: &Caller<'_>,
        cloexec: bool,
    ) -> Result<FileSlot<'a>, Errno> {
        match self {
            Self::Table => {
                let [slot] = kernel.reserve_fds(caller, cloexec)?;
                Ok(FileSlot::Table(slot))
            }
            Self::Direct(ctx, index) => Ok(FileSlot::Direct(ctx, index)),
        }
    }
}

pub(crate) enum FileSlot<'a> {
    Table(FdReservation),
    Direct(&'a crate::io_uring::Ctx, u32),
}

impl FileSlot<'_> {
    pub(crate) fn install(self, file: Arc<OpenFile>) -> SysResult<Fd> {
        match self {
            Self::Table(slot) => Ok(slot.install(file)),
            Self::Direct(ctx, index) => Ok(Fd(ctx.install_direct(file, index)?)),
        }
    }
}

/// A descriptor reserved by `Kernel::reserve_fds`. Dropping it unused gives the number back.
pub(crate) struct FdReservation {
    /// `None` once installed.
    table: Option<Arc<Mutex<FdTable>>>,
    fd: Fd,
    cloexec: bool,
}

impl FdReservation {
    pub(crate) fn fd(&self) -> Fd {
        self.fd
    }

    /// Installs `file` at the reserved descriptor (`fd_install`).
    pub(crate) fn install(mut self, file: Arc<OpenFile>) -> Fd {
        let table = self.table.take().expect("installed once");
        table.lock_or_recover().fill(self.fd, file, self.cloexec);
        self.fd
    }
}

impl Drop for FdReservation {
    fn drop(&mut self) {
        if let Some(table) = &self.table {
            table.lock_or_recover().release(self.fd);
        }
    }
}

/// `__kernel_fsid_t` of a filesystem on `dev`, as `statfs` and fanotify report it.
pub(crate) fn fsid(dev: wasixv2_vfs::Device) -> [i32; 2] {
    [(dev.major << 20 | dev.minor) as i32, 0]
}

/// `build_open_how`: unknown bits are ignored, `O_PATH` drops everything else, and the mode
/// counts only when creating.
pub(crate) fn build_open_how(flags: OpenFlags, mode: u32) -> OpenHow {
    let mut flags = flags.0 & VALID_OPEN_FLAGS;
    if flags & OpenFlags::O_PATH.0 != 0 {
        flags &= O_PATH_FLAGS;
    }
    let creating = flags & (OpenFlags::O_CREAT.0 | OpenFlags::__O_TMPFILE.0) != 0;
    let mode = if creating { mode & MODE_PERM } else { 0 };
    OpenHow {
        flags: u64::from(flags),
        mode: u64::from(mode),
        resolve: ResolveFlags(0),
    }
}

/// `build_open_flags`: the open flags and lookup flags of a valid `open_how`.
pub(crate) fn build_open_flags(how: &OpenHow) -> Result<(OpenFlags, LookupFlags), Errno> {
    let raw = how.flags;
    let strip = u64::from(OpenFlags::O_CLOEXEC.0);
    if (raw & !strip) & !u64::from(VALID_OPEN_FLAGS) != 0 {
        return Err(Errno::EINVAL);
    }
    if how.resolve.0 & !VALID_RESOLVE_FLAGS != 0 {
        return Err(Errno::EINVAL);
    }
    let resolve = how.resolve;
    if resolve.contains(ResolveFlags::RESOLVE_BENEATH)
        && resolve.contains(ResolveFlags::RESOLVE_IN_ROOT)
    {
        return Err(Errno::EINVAL);
    }
    let mut flags = OpenFlags(raw as u32);
    let creating = flags.intersects(OpenFlags::O_CREAT | OpenFlags::__O_TMPFILE);
    if creating {
        if how.mode & !u64::from(MODE_PERM) != 0 {
            return Err(Errno::EINVAL);
        }
    } else if how.mode != 0 {
        return Err(Errno::EINVAL);
    }
    if flags.contains(OpenFlags::O_DIRECTORY | OpenFlags::O_CREAT) {
        return Err(Errno::EINVAL);
    }
    let access = flags & OpenFlags::O_ACCMODE;
    if flags.contains(OpenFlags::__O_TMPFILE)
        && (!flags.contains(OpenFlags::O_DIRECTORY) || access == OpenFlags::O_RDONLY)
    {
        return Err(Errno::EINVAL);
    }
    if flags.contains(OpenFlags::O_PATH) && flags.0 & !O_PATH_FLAGS != 0 {
        return Err(Errno::EINVAL);
    }
    if flags.contains(OpenFlags::__O_SYNC) {
        flags |= OpenFlags::O_DSYNC;
    }
    if resolve.contains(ResolveFlags::RESOLVE_CACHED)
        && flags.intersects(OpenFlags::O_TRUNC | OpenFlags::O_CREAT | OpenFlags::__O_TMPFILE)
    {
        return Err(Errno::EAGAIN);
    }
    let excl_create = flags.contains(OpenFlags::O_CREAT | OpenFlags::O_EXCL);
    let lookup = LookupFlags {
        follow: !flags.contains(OpenFlags::O_NOFOLLOW) && !excl_create,
        directory: flags.contains(OpenFlags::O_DIRECTORY),
        empty: flags.contains(OpenFlags::O_EMPTYPATH),
        no_xdev: resolve.contains(ResolveFlags::RESOLVE_NO_XDEV),
        no_symlinks: resolve.contains(ResolveFlags::RESOLVE_NO_SYMLINKS),
        no_magiclinks: resolve.contains(ResolveFlags::RESOLVE_NO_MAGICLINKS),
        beneath: resolve.contains(ResolveFlags::RESOLVE_BENEATH),
        in_root: resolve.contains(ResolveFlags::RESOLVE_IN_ROOT),
    };

    Ok((flags, lookup))
}

pub(crate) async fn read_path(memory: &dyn UserMemory, path: Cstr) -> Result<Vec<u8>, Errno> {
    // Linux fs/namei.c:getname_flags/strncpy_from_user faults on unmapped
    // pathname bytes. Host loads in this guard are allowed only for LLVM's
    // null va_list workaround; a pathname is not a va_list.
    if path.addr() < memory.low_guard().await {
        return Err(Errno::EFAULT);
    }
    read_cstr(memory, path, PATH_MAX)
        .await
        .map_err(|err| match err {
            CstrError::Fault => Errno::EFAULT,
            CstrError::TooLong => Errno::ENAMETOOLONG,
        })
}

/// Linux include/linux/fs.h:getname_maybe_null: only these callers allow a NULL
/// pathname with AT_EMPTY_PATH; non-NULL pointers still require a readable byte.
pub(crate) async fn read_path_maybe_null(
    memory: &dyn UserMemory,
    path: Cstr,
    flags: AtFlags,
) -> Result<Vec<u8>, Errno> {
    if path.is_null() && flags.contains(AtFlags::AT_EMPTY_PATH) {
        Ok(Vec::new())
    } else {
        read_path(memory, path).await
    }
}
