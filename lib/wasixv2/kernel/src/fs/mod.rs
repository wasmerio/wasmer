//! Filesystem syscalls: argument decoding and per-process state around `wasixv2_vfs`, which
//! decides every path and permission question.

mod attr;
mod fifo;
pub(crate) mod inotify;
mod io;
mod lock;
mod path;

pub(crate) use io::{At, read_iovecs};

use std::sync::{Arc, Mutex};

use wasixv2_abi::{
    AtFd, AtFlags, CloseRangeFlags, ConstOpenHowPtr, Cstr, FcntlCmd, FdFlags, OpenFlags, OpenHow,
    ResolveFlags,
};
use wasixv2_host::Host;
use wasixv2_syscalls::{
    Caller, CstrError, Errno, Fd, SysResult, TaskId, UserMemory, read_cstr, read_value,
};
use wasixv2_vfs::{Clock, Cred, Ctx, LookupFlags, MODE_PERM, PATH_MAX, Path, Timestamp};

use crate::{
    Kernel,
    file::{Backing, FdTable, OpenFile},
};

/// The soft `RLIMIT_NOFILE` every process has until resource limits exist.
pub(crate) const NOFILE: usize = 1024;

/// `FASYNC` (`asm-generic/fcntl.h`).
const FASYNC: u32 = 0o20000;

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
}

/// What a filesystem syscall runs with: a snapshot of the caller's context and credentials.
pub(crate) struct Env {
    pub(crate) fs: FsContext,
    pub(crate) cred: Cred,
    pub(crate) task: u32,
}

impl Env {
    pub(crate) fn cx(&self) -> Ctx<'_> {
        Ctx {
            cred: &self.cred,
            root: &self.fs.root,
            umask: self.fs.umask,
            task: self.task,
        }
    }
}

impl Kernel {
    pub(crate) fn env(&self, caller: &Caller<'_>) -> Env {
        self.task_env(caller.task())
    }

    fn task_env(&self, task: TaskId) -> Env {
        let (fs, process) = self.task_ref(task, |task| {
            (task.fs.lock().unwrap().clone(), task.process.clone())
        });
        Env {
            fs,
            cred: process.cred().fs(),
            task: task.0,
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

    /// `dfd` itself, for an empty path: the working directory for `AT_FDCWD`.
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
            .permission(&env.cred, &dir, wasixv2_vfs::MAY_EXEC)
            .await?;
        self.with_task(task, |task| task.fs.lock().unwrap().cwd = dir);
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
        // `build_open_how`: unknown bits are ignored, O_PATH drops everything else.
        let mut flags = flags.0 & VALID_OPEN_FLAGS;
        if flags & OpenFlags::O_PATH.0 != 0 {
            flags &= O_PATH_FLAGS;
        }
        let creating = flags & (OpenFlags::O_CREAT.0 | OpenFlags::__O_TMPFILE.0) != 0;
        let mode = if creating { mode & MODE_PERM } else { 0 };
        let how = OpenHow {
            flags: u64::from(flags),
            mode: u64::from(mode),
            resolve: ResolveFlags(0),
        };
        self.do_open(caller, dfd, filename, how).await
    }

    pub(crate) async fn sys_openat2(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        how: ConstOpenHowPtr,
        size: u32,
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
        self.do_open(caller, dfd, filename, value).await
    }

    /// `build_open_flags` + `do_file_open`.
    async fn do_open(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        how: OpenHow,
    ) -> SysResult<Fd> {
        let raw = how.flags;
        let strip = u64::from(OpenFlags::O_CLOEXEC.0);
        if (raw & !strip) & !u64::from(VALID_OPEN_FLAGS) != 0 {
            return Err(Errno::EINVAL.into());
        }
        if how.resolve.0 & !VALID_RESOLVE_FLAGS != 0 {
            return Err(Errno::EINVAL.into());
        }
        let resolve = how.resolve;
        if resolve.contains(ResolveFlags::RESOLVE_BENEATH)
            && resolve.contains(ResolveFlags::RESOLVE_IN_ROOT)
        {
            return Err(Errno::EINVAL.into());
        }
        let mut flags = OpenFlags(raw as u32);
        let creating = flags.intersects(OpenFlags::O_CREAT | OpenFlags::__O_TMPFILE);
        if creating {
            if how.mode & !u64::from(MODE_PERM) != 0 {
                return Err(Errno::EINVAL.into());
            }
        } else if how.mode != 0 {
            return Err(Errno::EINVAL.into());
        }
        if flags.contains(OpenFlags::O_DIRECTORY | OpenFlags::O_CREAT) {
            return Err(Errno::EINVAL.into());
        }
        let access = flags & OpenFlags::O_ACCMODE;
        if flags.contains(OpenFlags::__O_TMPFILE)
            && (!flags.contains(OpenFlags::O_DIRECTORY) || access == OpenFlags::O_RDONLY)
        {
            return Err(Errno::EINVAL.into());
        }
        if flags.contains(OpenFlags::O_PATH) && flags.0 & !O_PATH_FLAGS != 0 {
            return Err(Errno::EINVAL.into());
        }
        if flags.contains(OpenFlags::__O_SYNC) {
            flags |= OpenFlags::O_DSYNC;
        }
        if resolve.contains(ResolveFlags::RESOLVE_CACHED)
            && flags.intersects(OpenFlags::O_TRUNC | OpenFlags::O_CREAT | OpenFlags::__O_TMPFILE)
        {
            return Err(Errno::EAGAIN.into());
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

        let path = read_path(caller.memory(), filename).await?;
        let env = self.env(caller);
        let start = if path.is_empty() && lookup.empty {
            self.start_or_fd(caller, &env, dfd)?
        } else {
            self.start(caller, &env, dfd, &path)?
        };
        let vfs_how = wasixv2_vfs::OpenHow {
            flags,
            mode: how.mode as u32,
            lookup,
        };
        // `get_unused_fd_flags` before `do_filp_open`: at the limit, nothing is created,
        // truncated or waited for.
        let cloexec = flags.contains(OpenFlags::O_CLOEXEC);
        let [fd] = self.reserve_fds(caller, cloexec)?;
        let file = self.vfs.open(&env.cx(), &start, &path, &vfs_how).await?;
        let backing = self.attach(caller, file, flags).await?;
        Ok(fd.install(Arc::new(OpenFile::new(backing, flags))))
    }

    /// `do_dentry_open`'s `f_op->open`: a FIFO attaches to its pipe and a device node to its
    /// driver; what the inode holds directly is read through the VFS.
    async fn attach(
        &self,
        caller: &Caller<'_>,
        file: wasixv2_vfs::File,
        flags: OpenFlags,
    ) -> SysResult<Backing> {
        if file.path_only {
            return Ok(Backing::Inode(file));
        }
        let path = file.path.clone();
        let object: Arc<dyn crate::FileObject> = match file.file_type() {
            wasixv2_vfs::FileType::Fifo => {
                if let Some(object) = crate::stdio::reopen(&file) {
                    return Ok(Backing::Stream(object, path));
                }
                let interrupt = self.interrupt(caller);
                Arc::new(fifo::open(file, flags, &*interrupt).await?)
            }
            wasixv2_vfs::FileType::CharDevice | wasixv2_vfs::FileType::BlockDevice => {
                self.open_device(file).await?
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
        let mut files = files.lock().unwrap();
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
        let mut files = table.lock().unwrap();
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
        if let Backing::Inode(file) = &file.backing
            && !file.path_only
        {
            let owner = wasixv2_vfs::LockOwner::Process(u64::from(pid.0));
            self.vfs.locks.release_owner(file.lock_key(), owner);
        }
    }

    pub(crate) fn sys_close(&self, caller: &Caller<'_>, fd: Fd) -> SysResult<i32> {
        let files = self.files(caller.task());
        let file = files.lock().unwrap().remove(fd).ok_or(Errno::EBADF)?;
        self.closed(caller, &file);
        Ok(0)
    }

    pub(crate) fn sys_close_range(
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
            let copy = Arc::new(Mutex::new(crate::process::copy_files(
                &files.lock().unwrap(),
            )));
            self.with_task(caller.task(), |task| task.files = copy.clone());
            files = copy;
        }
        let closed: Vec<Arc<OpenFile>> = {
            let mut table = files.lock().unwrap();
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
            self.closed(caller, file);
        }
        Ok(0)
    }

    pub(crate) fn sys_dup(&self, caller: &Caller<'_>, fd: Fd) -> SysResult<Fd> {
        let file = self.file(caller.task(), fd)?;
        self.install(caller, file, false)
    }

    pub(crate) fn sys_dup3(
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
        let mut table = files.lock().unwrap();
        if new.0 < 0 || new.0 as usize >= table.limit() {
            return Err(Errno::EBADF.into());
        }
        let file = table.get(old).ok_or(Errno::EBADF)?;
        // A descriptor reserved by an open still in progress.
        if table.is_reserved(new) {
            return Err(Errno::EBUSY.into());
        }
        let replaced = table.install_at(new, file, flags.contains(OpenFlags::O_CLOEXEC));
        drop(table);
        if let Some(replaced) = replaced {
            self.closed(caller, &replaced);
        }
        Ok(new)
    }

    /// `fcntl`: descriptor flags, status flags, duplication, record locks, pipe sizes and
    /// seals.
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
                let mut table = files.lock().unwrap();
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
                let cloexec = files.lock().unwrap().cloexec(fd).unwrap_or_default();
                Ok(if cloexec {
                    FdFlags::FD_CLOEXEC.0 as i32
                } else {
                    0
                })
            }
            FcntlCmd::F_SETFD => {
                let cloexec = arg & FdFlags::FD_CLOEXEC.0 != 0;
                files.lock().unwrap().set_cloexec(fd, cloexec);
                Ok(0)
            }
            FcntlCmd::F_GETFL => Ok(file.flags().0 as i32),
            _ if path_only => Err(Errno::EBADF.into()),
            FcntlCmd::F_SETFL => {
                let flags = OpenFlags(arg);
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
                file.set_status_flags(flags);
                Ok(0)
            }
            FcntlCmd::F_GETPIPE_SZ | FcntlCmd::F_SETPIPE_SZ => {
                let privileged = self.cred(caller).capable(wasixv2_vfs::Cap::SysResource);
                crate::pipe::fcntl(&file, cmd, arg, privileged)
            }
            FcntlCmd::F_ADD_SEALS | FcntlCmd::F_GET_SEALS => {
                self.memfd_fcntl(&file, cmd, arg).await
            }
            _ => self.fcntl_lock(caller, &file, cmd, arg).await,
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
        table.lock().unwrap().fill(self.fd, file, self.cloexec);
        self.fd
    }
}

impl Drop for FdReservation {
    fn drop(&mut self) {
        if let Some(table) = &self.table {
            table.lock().unwrap().release(self.fd);
        }
    }
}

pub(crate) async fn read_path(memory: &dyn UserMemory, path: Cstr) -> Result<Vec<u8>, Errno> {
    read_cstr(memory, path, PATH_MAX)
        .await
        .map_err(|err| match err {
            CstrError::Fault => Errno::EFAULT,
            CstrError::TooLong => Errno::ENAMETOOLONG,
        })
}
