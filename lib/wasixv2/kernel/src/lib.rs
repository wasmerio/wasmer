//! The kernel: tasks, processes, credentials, signals, namespaces, IPC, memory, terminals, pseudo
//! filesystems and the socket layer. Implements `Syscalls`.
//!
//! Knows nothing about the wasm engine, the layers, or concrete filesystem drivers and network
//! backends.

mod epoll;
mod eventfd;
mod file;
mod fs;
mod futex;
mod ioctl;
mod mm;
mod net;
mod netlink;
mod pipe;
mod poll;
mod socket;
mod startup;
mod stdio;
mod thread;
mod time;
mod unix;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub use file::{FileObject, Io};
pub use fs::HostClock;
use wasixv2_abi::{
    AccessMode, AtFlags, Buf, ClockId, CloseRangeFlags, ConstFileHandlePtr, ConstIovecArray,
    ConstKernelTimespecArray, ConstKernelTimespecPtr, ConstOpenHowPtr, ConstSigsetPtr,
    ConstXattrArgsPtr, Cstr, FadviseAdvice, FallocFlags, FcntlCmd, FileHandlePtr, FlockOp, FutexOp,
    Gid, IoctlRequest, KernelTimespecPtr, LinuxDirent64Ptr, Loff, LoffPtr, MapFlags, Mode,
    NewUtsname, NewUtsnamePtr, OpenFlags, Pid, ProtFlags, RenameFlags, RobustListHeadPtr,
    RobustListHeadPtrPtr, RwfFlags, S32Ptr, SeekWhence, Signal, SigprocmaskHow, Sigset, SigsetPtr,
    Statfs64Ptr, StatxMask, StatxPtr, SyncFileRangeFlags, TimerFlags, U32Ptr, Uid, Ulong, Uptr,
    Usize, UsizePtr, XattrArgsPtr, XattrFlags,
};
use wasixv2_host::Host;
pub use wasixv2_host::Stdio;
use wasixv2_syscalls::{
    Caller, Control, Errno, Fd, GuestAction, SysResult, Syscalls, TaskId, UserSlice, UserSliceMut,
    read_value, write_value,
};
use wasixv2_vfs::{Filesystem, Vfs};

use crate::{
    file::{FdTable, OpenFile},
    fs::FsContext,
    mm::{AddressSpace, PAGE_SIZE, Placement},
    startup::Startup,
};

/// Most bytes one `read` or `write` moves. Partial transfers are valid for both, and this bounds
/// the host allocation a guest-chosen length can cause.
const IO_CHUNK: usize = 64 * 1024;

/// The uid and gid of every process until credentials exist. Not 0: root bypasses permission
/// checks, so programs take paths that only make sense for a privileged user.
const ID: i32 = 1;

/// `uname` fields. The release is the wasixv2 ABI version, not a Linux version: the target is not
/// Linux, and programs that parse the release to guess Linux features should not match it.
const SYSNAME: &str = "wasixv2";
const NODENAME: &str = "localhost";
const RELEASE: &str = "2.0.0";
const VERSION: &str = "#1";
const MACHINE: &str = "wasm32";
const DOMAINNAME: &str = "(none)";

/// What a new process starts with.
pub struct ProcessConfig {
    /// `argv`; `args[0]` is the program name.
    pub args: Vec<String>,
    /// The environment, as `KEY=VALUE` strings.
    pub env: Vec<String>,
    /// Descriptors 0, 1 and 2.
    pub stdio: Stdio,
}

pub struct Kernel {
    host: Arc<dyn Host>,
    state: Mutex<State>,
    // ---- net ----
    /// The network namespace IP sockets live in; without one, `socket` fails with
    /// `EAFNOSUPPORT`.
    // TODO: one per network namespace once namespaces exist.
    network: Option<Arc<dyn wasixv2_vnet::Network>>,
    /// Bound Unix socket names (the abstract namespace and socket inodes).
    unix_names: Arc<unix::Names>,
    // ---- fs ----
    vfs: Arc<Vfs>,
    // ---- thr ----
    futexes: futex::Futexes,
}

#[derive(Default)]
struct State {
    last_id: u32,
    tasks: HashMap<TaskId, Task>,
}

struct Process {
    pid: TaskId,
    args: Vec<String>,
    env: Vec<String>,
    mm: Mutex<AddressSpace>,
    // ---- thr ----
    group: thread::Group,
}

struct Task {
    process: Arc<Process>,
    files: Arc<Mutex<FdTable>>,
    /// Root, working directory and umask (`fs_struct`).
    fs: Arc<Mutex<FsContext>>,
    pending: Option<GuestAction>,
    /// Blocked signals; bit `n - 1` is signal `n`.
    sigmask: u64,
    // ---- thr ----
    thread: thread::ThreadState,
}

impl Kernel {
    /// A kernel whose root filesystem is `root`. Further filesystems are mounted through
    /// [`Kernel::vfs`].
    pub fn new(host: Arc<dyn Host>, root: Arc<dyn Filesystem>) -> Self {
        Self {
            host,
            state: Mutex::default(),
            network: None,
            unix_names: Arc::default(),
            vfs: Vfs::new(root),
            futexes: futex::Futexes::default(),
        }
    }

    /// Gives the kernel a network for IP sockets.
    pub fn with_network(mut self, network: Arc<dyn wasixv2_vnet::Network>) -> Self {
        self.network = Some(network);
        self
    }

    /// The machine's VFS, for the embedder to mount filesystems.
    pub fn vfs(&self) -> &Arc<Vfs> {
        &self.vfs
    }

    /// Starts a process with a single task whose descriptors 0, 1 and 2 are `config.stdio`.
    pub fn spawn(&self, config: ProcessConfig) -> TaskId {
        let mut files = FdTable::default();
        for file in stdio::open_files(config.stdio) {
            files.install(Arc::new(file), false);
        }
        let mut state = self.state.lock().unwrap();
        state.last_id += 1;
        let id = TaskId(state.last_id);
        let process = Process {
            pid: id,
            args: config.args,
            env: config.env,
            mm: Mutex::default(),
            group: thread::Group::new(),
        };
        let task = Task {
            process: Arc::new(process),
            files: Arc::new(Mutex::new(files)),
            fs: Arc::new(Mutex::new(FsContext::new(self.vfs.root()))),
            pending: None,
            sigmask: 0,
            thread: thread::ThreadState::default(),
        };
        state.tasks.insert(id, task);
        id
    }

    fn file(&self, task: TaskId, fd: Fd) -> Result<Arc<OpenFile>, Errno> {
        let file = self.files(task).lock().unwrap().get(fd);
        file.ok_or(Errno::EBADF)
    }

    fn files(&self, task: TaskId) -> Arc<Mutex<FdTable>> {
        self.with_task(task, |task| task.files.clone())
    }

    fn process(&self, task: TaskId) -> Arc<Process> {
        self.with_task(task, |task| task.process.clone())
    }

    fn with_task<R>(&self, task: TaskId, f: impl FnOnce(&mut Task) -> R) -> R {
        let mut state = self.state.lock().unwrap();
        let task = state
            .tasks
            .get_mut(&task)
            .expect("syscall from an unknown task");
        f(task)
    }
}

impl Syscalls for Kernel {
    async fn startup_size(&self, caller: &Caller<'_>) -> SysResult<u32> {
        let process = self.process(caller.task());
        Ok(Startup::new(&process.args, &process.env).size()?)
    }

    async fn startup_get(&self, caller: &Caller<'_>, buf: UserSliceMut) -> SysResult<i32> {
        if !buf.addr.is_multiple_of(16) {
            return Err(Errno::EINVAL.into());
        }
        let process = self.process(caller.task());
        let startup = Startup::new(&process.args, &process.env);
        if buf.len < startup.size()? {
            return Err(Errno::EINVAL.into());
        }
        let mut random = [0; 16];
        self.host.random(&mut random);
        let block = startup.build(buf.addr, random)?;
        buf.copy_out(caller.memory(), &block).await?;
        Ok(0)
    }

    // ---- fs ----
    async fn openat(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        flags: OpenFlags,
        mode: Mode,
    ) -> SysResult<Fd> {
        self.sys_openat(caller, dfd, filename, flags, mode).await
    }

    async fn openat2(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        how: ConstOpenHowPtr,
        usize: Usize,
    ) -> SysResult<Fd> {
        self.sys_openat2(caller, dfd, filename, how, usize).await
    }

    async fn close(&self, caller: &Caller<'_>, fd: Fd) -> SysResult<i32> {
        self.sys_close(caller, fd)
    }

    async fn close_range(
        &self,
        caller: &Caller<'_>,
        fd: u32,
        max_fd: u32,
        flags: CloseRangeFlags,
    ) -> SysResult<i32> {
        self.sys_close_range(caller, fd, max_fd, flags)
    }

    async fn dup(&self, caller: &Caller<'_>, fildes: Fd) -> SysResult<Fd> {
        self.sys_dup(caller, fildes)
    }

    async fn dup3(
        &self,
        caller: &Caller<'_>,
        oldfd: Fd,
        newfd: Fd,
        flags: OpenFlags,
    ) -> SysResult<Fd> {
        self.sys_dup3(caller, oldfd, newfd, flags)
    }

    async fn fcntl64(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        cmd: FcntlCmd,
        arg: Ulong,
    ) -> SysResult<i32> {
        self.sys_fcntl(caller, fd, cmd, arg).await
    }

    async fn flock(&self, caller: &Caller<'_>, fd: Fd, cmd: FlockOp) -> SysResult<i32> {
        self.sys_flock(caller, fd, cmd).await
    }

    async fn readv(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        vec: ConstIovecArray,
        vlen: Ulong,
    ) -> SysResult<u32> {
        self.sys_readv(caller, fd, vec, vlen).await
    }

    async fn writev(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        vec: ConstIovecArray,
        vlen: Ulong,
    ) -> SysResult<u32> {
        self.sys_writev(caller, fd, vec, vlen).await
    }

    async fn pread64(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        buf: UserSliceMut,
        pos: Loff,
    ) -> SysResult<u32> {
        self.sys_pread(caller, fd, buf, pos).await
    }

    async fn pwrite64(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        buf: UserSlice,
        pos: Loff,
    ) -> SysResult<u32> {
        self.sys_pwrite(caller, fd, buf, pos).await
    }

    async fn preadv(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        vec: ConstIovecArray,
        vlen: Ulong,
        pos: i64,
    ) -> SysResult<u32> {
        self.sys_preadv2(caller, fd, vec, vlen, pos.max(0), RwfFlags(0))
            .await
    }

    async fn pwritev(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        vec: ConstIovecArray,
        vlen: Ulong,
        pos: i64,
    ) -> SysResult<u32> {
        self.sys_pwritev2(caller, fd, vec, vlen, pos.max(0), RwfFlags(0))
            .await
    }

    async fn preadv2(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        vec: ConstIovecArray,
        vlen: Ulong,
        pos: i64,
        flags: RwfFlags,
    ) -> SysResult<u32> {
        self.sys_preadv2(caller, fd, vec, vlen, pos, flags).await
    }

    async fn pwritev2(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        vec: ConstIovecArray,
        vlen: Ulong,
        pos: i64,
        flags: RwfFlags,
    ) -> SysResult<u32> {
        self.sys_pwritev2(caller, fd, vec, vlen, pos, flags).await
    }

    async fn lseek(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        offset: Loff,
        whence: SeekWhence,
    ) -> SysResult<i64> {
        self.sys_lseek(caller, fd, offset, whence).await
    }

    async fn sendfile64(
        &self,
        caller: &Caller<'_>,
        out_fd: Fd,
        in_fd: Fd,
        offset: LoffPtr,
        count: Usize,
    ) -> SysResult<i32> {
        self.sys_sendfile(caller, out_fd, in_fd, offset, count)
            .await
    }

    async fn copy_file_range(
        &self,
        caller: &Caller<'_>,
        fd_in: Fd,
        off_in: LoffPtr,
        fd_out: Fd,
        off_out: LoffPtr,
        len: Usize,
        flags: u32,
    ) -> SysResult<i32> {
        self.sys_copy_file_range(caller, fd_in, off_in, fd_out, off_out, len, flags)
            .await
    }

    async fn truncate64(&self, caller: &Caller<'_>, path: Cstr, length: Loff) -> SysResult<i32> {
        self.sys_truncate(caller, path, length).await
    }

    async fn ftruncate64(&self, caller: &Caller<'_>, fd: Fd, length: Loff) -> SysResult<i32> {
        self.sys_ftruncate(caller, fd, length).await
    }

    async fn fallocate(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        mode: FallocFlags,
        offset: Loff,
        len: Loff,
    ) -> SysResult<i32> {
        self.sys_fallocate(caller, fd, mode, offset, len).await
    }

    async fn fadvise64_64(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        _offset: Loff,
        len: Loff,
        advice: FadviseAdvice,
    ) -> SysResult<i32> {
        self.sys_fadvise(caller, fd, len, advice)
    }

    async fn readahead(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        _offset: Loff,
        _count: Usize,
    ) -> SysResult<i32> {
        self.sys_readahead(caller, fd)
    }

    async fn sync(&self, _: &Caller<'_>) -> SysResult<i32> {
        self.sys_sync().await
    }

    async fn fsync(&self, caller: &Caller<'_>, fd: Fd) -> SysResult<i32> {
        self.sys_fsync(caller, fd, false).await
    }

    async fn fdatasync(&self, caller: &Caller<'_>, fd: Fd) -> SysResult<i32> {
        self.sys_fsync(caller, fd, true).await
    }

    async fn syncfs(&self, caller: &Caller<'_>, fd: Fd) -> SysResult<i32> {
        self.sys_syncfs(caller, fd).await
    }

    async fn sync_file_range(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        offset: Loff,
        nbytes: Loff,
        flags: SyncFileRangeFlags,
    ) -> SysResult<i32> {
        self.sys_sync_file_range(caller, fd, offset, nbytes, flags)
            .await
    }

    async fn getdents64(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        dirent: LinuxDirent64Ptr,
        count: u32,
    ) -> SysResult<i32> {
        self.sys_getdents64(caller, fd, dirent, count).await
    }

    async fn mkdirat(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        pathname: Cstr,
        mode: Mode,
    ) -> SysResult<i32> {
        self.sys_mkdirat(caller, dfd, pathname, mode).await
    }

    async fn mknodat(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        mode: Mode,
        dev: u32,
    ) -> SysResult<i32> {
        self.sys_mknodat(caller, dfd, filename, mode, dev).await
    }

    async fn unlinkat(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        pathname: Cstr,
        flag: AtFlags,
    ) -> SysResult<i32> {
        self.sys_unlinkat(caller, dfd, pathname, flag).await
    }

    async fn symlinkat(
        &self,
        caller: &Caller<'_>,
        oldname: Cstr,
        newdfd: Fd,
        newname: Cstr,
    ) -> SysResult<i32> {
        self.sys_symlinkat(caller, oldname, newdfd, newname).await
    }

    async fn linkat(
        &self,
        caller: &Caller<'_>,
        olddfd: Fd,
        oldname: Cstr,
        newdfd: Fd,
        newname: Cstr,
        flags: AtFlags,
    ) -> SysResult<i32> {
        self.sys_linkat(caller, olddfd, oldname, newdfd, newname, flags)
            .await
    }

    async fn renameat2(
        &self,
        caller: &Caller<'_>,
        olddfd: Fd,
        oldname: Cstr,
        newdfd: Fd,
        newname: Cstr,
        flags: RenameFlags,
    ) -> SysResult<i32> {
        self.sys_renameat2(caller, olddfd, oldname, newdfd, newname, flags)
            .await
    }

    async fn readlinkat(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        pathname: Cstr,
        buf: Buf,
        bufsiz: i32,
    ) -> SysResult<i32> {
        self.sys_readlinkat(caller, dfd, pathname, buf, bufsiz)
            .await
    }

    async fn faccessat(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        mode: AccessMode,
    ) -> SysResult<i32> {
        self.sys_faccessat2(caller, dfd, filename, mode, AtFlags(0))
            .await
    }

    async fn faccessat2(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        mode: AccessMode,
        flags: AtFlags,
    ) -> SysResult<i32> {
        self.sys_faccessat2(caller, dfd, filename, mode, flags)
            .await
    }

    async fn chdir(&self, caller: &Caller<'_>, filename: Cstr) -> SysResult<i32> {
        self.sys_chdir(caller, filename).await
    }

    async fn fchdir(&self, caller: &Caller<'_>, fd: Fd) -> SysResult<i32> {
        self.sys_fchdir(caller, fd).await
    }

    async fn chroot(&self, caller: &Caller<'_>, filename: Cstr) -> SysResult<i32> {
        self.sys_chroot(caller, filename).await
    }

    async fn getcwd(&self, caller: &Caller<'_>, buf: UserSliceMut) -> SysResult<u32> {
        self.sys_getcwd(caller, buf).await
    }

    async fn umask(&self, caller: &Caller<'_>, mask: i32) -> SysResult<i32> {
        self.sys_umask(caller, mask)
    }

    async fn fchmod(&self, caller: &Caller<'_>, fd: Fd, mode: Mode) -> SysResult<i32> {
        self.sys_fchmod(caller, fd, mode).await
    }

    async fn fchmodat(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        mode: Mode,
    ) -> SysResult<i32> {
        self.sys_fchmodat2(caller, dfd, filename, mode, AtFlags(0))
            .await
    }

    async fn fchmodat2(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        mode: Mode,
        flags: AtFlags,
    ) -> SysResult<i32> {
        self.sys_fchmodat2(caller, dfd, filename, mode, flags).await
    }

    async fn fchown(&self, caller: &Caller<'_>, fd: Fd, user: Uid, group: Gid) -> SysResult<i32> {
        self.sys_fchown(caller, fd, user, group).await
    }

    async fn fchownat(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        user: Uid,
        group: Gid,
        flag: AtFlags,
    ) -> SysResult<i32> {
        self.sys_fchownat(caller, dfd, filename, user, group, flag)
            .await
    }

    async fn statx(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        flags: AtFlags,
        mask: StatxMask,
        buffer: StatxPtr,
    ) -> SysResult<i32> {
        self.sys_statx(caller, dfd, filename, flags, mask, buffer)
            .await
    }

    async fn statfs64(
        &self,
        caller: &Caller<'_>,
        pathname: Cstr,
        sz: Usize,
        buf: Statfs64Ptr,
    ) -> SysResult<i32> {
        self.sys_statfs(caller, pathname, sz, buf).await
    }

    async fn fstatfs64(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        sz: Usize,
        buf: Statfs64Ptr,
    ) -> SysResult<i32> {
        self.sys_fstatfs(caller, fd, sz, buf).await
    }

    async fn utimensat_time64(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        utimes: ConstKernelTimespecArray,
        flags: AtFlags,
    ) -> SysResult<i32> {
        self.sys_utimensat(caller, dfd, filename, utimes, flags)
            .await
    }

    async fn name_to_handle_at(
        &self,
        _: &Caller<'_>,
        _: Fd,
        _: Cstr,
        _: FileHandlePtr,
        _: S32Ptr,
        _: i32,
    ) -> SysResult<i32> {
        Err(Errno::EOPNOTSUPP.into())
    }

    async fn open_by_handle_at(
        &self,
        _: &Caller<'_>,
        _: Fd,
        _: ConstFileHandlePtr,
        _: OpenFlags,
    ) -> SysResult<i32> {
        Err(Errno::EOPNOTSUPP.into())
    }

    async fn setxattr(
        &self,
        caller: &Caller<'_>,
        pathname: Cstr,
        name: Cstr,
        value: UserSlice,
        flags: XattrFlags,
    ) -> SysResult<i32> {
        let target = self.xattr_path(caller, pathname, true).await?;
        self.xattr_set(caller, target, name, value, flags).await
    }

    async fn lsetxattr(
        &self,
        caller: &Caller<'_>,
        pathname: Cstr,
        name: Cstr,
        value: UserSlice,
        flags: XattrFlags,
    ) -> SysResult<i32> {
        let target = self.xattr_path(caller, pathname, false).await?;
        self.xattr_set(caller, target, name, value, flags).await
    }

    async fn fsetxattr(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        name: Cstr,
        value: UserSlice,
        flags: XattrFlags,
    ) -> SysResult<i32> {
        let target = self.xattr_of_fd(caller, fd)?;
        self.xattr_set(caller, target, name, value, flags).await
    }

    async fn getxattr(
        &self,
        caller: &Caller<'_>,
        pathname: Cstr,
        name: Cstr,
        value: UserSliceMut,
    ) -> SysResult<u32> {
        let target = self.xattr_path(caller, pathname, true).await?;
        self.xattr_get(caller, target, name, value).await
    }

    async fn lgetxattr(
        &self,
        caller: &Caller<'_>,
        pathname: Cstr,
        name: Cstr,
        value: UserSliceMut,
    ) -> SysResult<u32> {
        let target = self.xattr_path(caller, pathname, false).await?;
        self.xattr_get(caller, target, name, value).await
    }

    async fn fgetxattr(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        name: Cstr,
        value: UserSliceMut,
    ) -> SysResult<u32> {
        let target = self.xattr_of_fd(caller, fd)?;
        self.xattr_get(caller, target, name, value).await
    }

    async fn listxattr(
        &self,
        caller: &Caller<'_>,
        pathname: Cstr,
        list: UserSliceMut,
    ) -> SysResult<u32> {
        let target = self.xattr_path(caller, pathname, true).await?;
        self.xattr_list(caller, target, list).await
    }

    async fn llistxattr(
        &self,
        caller: &Caller<'_>,
        pathname: Cstr,
        list: UserSliceMut,
    ) -> SysResult<u32> {
        let target = self.xattr_path(caller, pathname, false).await?;
        self.xattr_list(caller, target, list).await
    }

    async fn flistxattr(&self, caller: &Caller<'_>, fd: Fd, list: UserSliceMut) -> SysResult<u32> {
        let target = self.xattr_of_fd(caller, fd)?;
        self.xattr_list(caller, target, list).await
    }

    async fn removexattr(&self, caller: &Caller<'_>, pathname: Cstr, name: Cstr) -> SysResult<i32> {
        let target = self.xattr_path(caller, pathname, true).await?;
        self.xattr_remove(caller, target, name).await
    }

    async fn lremovexattr(
        &self,
        caller: &Caller<'_>,
        pathname: Cstr,
        name: Cstr,
    ) -> SysResult<i32> {
        let target = self.xattr_path(caller, pathname, false).await?;
        self.xattr_remove(caller, target, name).await
    }

    async fn fremovexattr(&self, caller: &Caller<'_>, fd: Fd, name: Cstr) -> SysResult<i32> {
        let target = self.xattr_of_fd(caller, fd)?;
        self.xattr_remove(caller, target, name).await
    }

    async fn setxattrat(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        pathname: Cstr,
        at_flags: AtFlags,
        name: Cstr,
        uargs: ConstXattrArgsPtr,
        usize: Usize,
    ) -> SysResult<i32> {
        self.sys_setxattrat(caller, dfd, pathname, at_flags, name, uargs, usize)
            .await
    }

    async fn getxattrat(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        pathname: Cstr,
        at_flags: AtFlags,
        name: Cstr,
        uargs: XattrArgsPtr,
        usize: Usize,
    ) -> SysResult<i32> {
        self.sys_getxattrat(caller, dfd, pathname, at_flags, name, uargs, usize)
            .await
    }

    async fn listxattrat(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        pathname: Cstr,
        at_flags: AtFlags,
        list: UserSliceMut,
    ) -> SysResult<u32> {
        let target = self.xattr_at(caller, dfd, pathname, at_flags).await?;
        self.xattr_list(caller, target, list).await
    }

    async fn removexattrat(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        pathname: Cstr,
        at_flags: AtFlags,
        name: Cstr,
    ) -> SysResult<i32> {
        let target = self.xattr_at(caller, dfd, pathname, at_flags).await?;
        self.xattr_remove(caller, target, name).await
    }

    async fn ioctl(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        request: IoctlRequest,
        arg: Ulong,
    ) -> SysResult<i32> {
        self.sys_ioctl(caller, fd, request, arg).await
    }

    async fn read(&self, caller: &Caller<'_>, fd: Fd, buf: UserSliceMut) -> SysResult<u32> {
        let file = self.file(caller.task(), fd)?;
        file.check_readable()?;
        let mut data = vec![0; (buf.len as usize).min(IO_CHUNK)];
        let n = file.read(&mut data).await?;
        buf.copy_out(caller.memory(), &data[..n]).await?;
        Ok(n as u32)
    }

    async fn write(&self, caller: &Caller<'_>, fd: Fd, buf: UserSlice) -> SysResult<u32> {
        let file = self.file(caller.task(), fd)?;
        file.check_writable()?;
        let data = buf.copy_in(caller.memory(), IO_CHUNK).await?;
        let n = file.write(&data).await?;
        Ok(n as u32)
    }

    // ---- thr ----
    async fn thread_spawn(
        &self,
        caller: &Caller<'_>,
        flags: Ulong,
        start_arg: Uptr,
        parent_tid: S32Ptr,
        tls: Uptr,
        child_tid: S32Ptr,
    ) -> SysResult<i32> {
        self.sys_thread_spawn(caller, flags, start_arg, parent_tid, tls, child_tid)
            .await
    }

    async fn exit(&self, caller: &Caller<'_>, error_code: i32) -> SysResult<()> {
        self.sys_exit(caller, error_code).await
    }

    async fn exit_group(&self, caller: &Caller<'_>, error_code: i32) -> SysResult<()> {
        self.sys_exit_group(caller, error_code).await
    }

    async fn set_tid_address(&self, caller: &Caller<'_>, tidptr: S32Ptr) -> SysResult<i32> {
        Ok(self.sys_set_tid_address(caller, tidptr))
    }

    async fn set_robust_list(
        &self,
        caller: &Caller<'_>,
        head: RobustListHeadPtr,
        len: Usize,
    ) -> SysResult<i32> {
        self.sys_set_robust_list(caller, head, len)
    }

    async fn get_robust_list(
        &self,
        caller: &Caller<'_>,
        pid: i32,
        head_ptr: RobustListHeadPtrPtr,
        len_ptr: UsizePtr,
    ) -> SysResult<i32> {
        self.sys_get_robust_list(caller, pid, head_ptr, len_ptr)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn futex_time64(
        &self,
        caller: &Caller<'_>,
        uaddr: U32Ptr,
        op: FutexOp,
        val: u32,
        utime: Ulong,
        uaddr2: U32Ptr,
        val3: u32,
    ) -> SysResult<i32> {
        self.sys_futex(caller, uaddr, op, val, utime, uaddr2, val3)
            .await
    }
    // ---- end thr ----

    async fn getpid(&self, caller: &Caller<'_>) -> SysResult<Pid> {
        Ok(self.process(caller.task()).pid.0 as Pid)
    }

    async fn getuid(&self, _: &Caller<'_>) -> SysResult<i32> {
        Ok(ID)
    }

    async fn geteuid(&self, _: &Caller<'_>) -> SysResult<i32> {
        Ok(ID)
    }

    async fn getgid(&self, _: &Caller<'_>) -> SysResult<i32> {
        Ok(ID)
    }

    async fn getegid(&self, _: &Caller<'_>) -> SysResult<i32> {
        Ok(ID)
    }

    // TODO: the nodename and domainname come from the UTS namespace once `sethostname` exists.
    async fn uname(&self, caller: &Caller<'_>, name: NewUtsnamePtr) -> SysResult<i32> {
        fn field(s: &str) -> [u8; 65] {
            let mut out = [0; 65];
            out[..s.len()].copy_from_slice(s.as_bytes());
            out
        }
        let uts = NewUtsname {
            sysname: field(SYSNAME),
            nodename: field(NODENAME),
            release: field(RELEASE),
            version: field(VERSION),
            machine: field(MACHINE),
            domainname: field(DOMAINNAME),
        };
        write_value(caller.memory(), name, &uts).await?;
        Ok(0)
    }

    async fn gettid(&self, caller: &Caller<'_>) -> SysResult<Pid> {
        Ok(caller.task().0 as Pid)
    }

    /// Tracks the mask; nothing delivers signals yet.
    async fn rt_sigprocmask(
        &self,
        caller: &Caller<'_>,
        how: SigprocmaskHow,
        nset: ConstSigsetPtr,
        oset: SigsetPtr,
        sigsetsize: Usize,
    ) -> SysResult<i32> {
        if sigsetsize as usize != size_of::<Sigset>() {
            return Err(Errno::EINVAL.into());
        }
        let new = match nset.is_null() {
            true => None,
            false => Some(sigset_bits(read_value(caller.memory(), nset).await?)),
        };
        let old = self.with_task(caller.task(), |task| task.sigmask);
        if let Some(set) = new {
            let mask = match how {
                SigprocmaskHow::SIG_BLOCK => old | set,
                SigprocmaskHow::SIG_UNBLOCK => old & !set,
                SigprocmaskHow::SIG_SETMASK => set,
                _ => return Err(Errno::EINVAL.into()),
            };
            let unblockable = signal_bit(Signal::SIGKILL) | signal_bit(Signal::SIGSTOP);
            self.with_task(caller.task(), |task| task.sigmask = mask & !unblockable);
        }
        if !oset.is_null() {
            let old = Sigset {
                sig: [old as u32, (old >> 32) as u32],
            };
            write_value(caller.memory(), oset, &old).await?;
        }
        Ok(0)
    }

    /// The break never moves, so musl's malloc falls back to `mmap`.
    async fn brk(&self, _: &Caller<'_>, _: Uptr) -> SysResult<i32> {
        Ok(0)
    }

    /// Anonymous mappings only, at an address the kernel picks; see `mm`.
    async fn mmap(
        &self,
        caller: &Caller<'_>,
        _addr: Uptr,
        len: Ulong,
        _prot: ProtFlags,
        flags: MapFlags,
        _fd: Fd,
        offset: Loff,
    ) -> SysResult<Uptr> {
        if offset % PAGE_SIZE as i64 != 0 {
            return Err(Errno::EINVAL.into());
        }
        // Shared anonymous memory is ordinary memory within one address space. Wasm modules
        // cannot share part of their memory, so no other process will ever see it.
        match flags & MapFlags::MAP_TYPE {
            MapFlags::MAP_SHARED | MapFlags::MAP_PRIVATE | MapFlags::MAP_SHARED_VALIDATE => {}
            _ => return Err(Errno::EINVAL.into()),
        }
        if !flags.contains(MapFlags::MAP_ANONYMOUS) {
            return Err(Errno::ENODEV.into());
        }
        // TODO: MAP_FIXED over pages `mmap` already handed out, if wasm allows it.
        if flags.intersects(MapFlags::MAP_FIXED | MapFlags::MAP_FIXED_NOREPLACE) {
            return Err(Errno::EINVAL.into());
        }
        let pages = mm::pages(len)?;
        let process = self.process(caller.task());
        let placement = process.mm.lock().unwrap().place(pages)?;
        let addr = match placement {
            Placement::Reuse(addr) => {
                mm::zero(caller.memory(), addr, pages * PAGE_SIZE).await?;
                addr
            }
            Placement::Grow(pages) => {
                let previous = caller.memory().grow(pages).await.ok_or(Errno::ENOMEM)?;
                process.mm.lock().unwrap().grown(previous, pages);
                previous
            }
        };
        Ok(addr as Uptr)
    }

    async fn munmap(&self, caller: &Caller<'_>, addr: Uptr, len: Usize) -> SysResult<i32> {
        if !u64::from(addr).is_multiple_of(PAGE_SIZE) || len == 0 {
            return Err(Errno::EINVAL.into());
        }
        let process = self.process(caller.task());
        process.mm.lock().unwrap().unmap(addr.into(), len.into());
        Ok(0)
    }

    async fn mprotect(
        &self,
        _: &Caller<'_>,
        start: Uptr,
        _: Usize,
        _: ProtFlags,
    ) -> SysResult<i32> {
        if !u64::from(start).is_multiple_of(PAGE_SIZE) {
            return Err(Errno::EINVAL.into());
        }
        Ok(0)
    }

    async fn clock_gettime64(
        &self,
        caller: &Caller<'_>,
        which_clock: ClockId,
        tp: KernelTimespecPtr,
    ) -> SysResult<i32> {
        let now = time::now(&*self.host, which_clock)?;
        write_value(caller.memory(), tp, &time::to_timespec(now)).await?;
        Ok(0)
    }

    // Nothing interrupts a sleep yet, so `rmtp` is never written.
    async fn clock_nanosleep_time64(
        &self,
        caller: &Caller<'_>,
        which_clock: ClockId,
        flags: TimerFlags,
        rqtp: ConstKernelTimespecPtr,
        _rmtp: KernelTimespecPtr,
    ) -> SysResult<i32> {
        let now = time::now(&*self.host, which_clock)?;
        let request = time::from_timespec(read_value(caller.memory(), rqtp).await?)?;
        let absolute = flags.contains(TimerFlags::TIMER_ABSTIME);
        let realtime = matches!(
            which_clock,
            ClockId::CLOCK_REALTIME | ClockId::CLOCK_REALTIME_COARSE
        );
        if absolute && !realtime {
            // Every other clock time::now accepts is the host's monotonic clock.
            self.host.sleep_until(request).await;
            return Ok(0);
        }
        let duration = match absolute {
            true => request.saturating_sub(now),
            false => request,
        };
        self.host.sleep(duration).await;
        Ok(0)
    }

    // ---- poll ----

    async fn pipe2(
        &self,
        caller: &Caller<'_>,
        fildes: wasixv2_abi::FdPair,
        flags: OpenFlags,
    ) -> SysResult<i32> {
        self.sys_pipe2(caller, fildes, flags).await
    }

    async fn eventfd2(
        &self,
        caller: &Caller<'_>,
        count: u32,
        flags: wasixv2_abi::EventfdFlags,
    ) -> SysResult<Fd> {
        self.sys_eventfd2(caller, count, flags).await
    }

    async fn ppoll_time64(
        &self,
        caller: &Caller<'_>,
        ufds: wasixv2_abi::PollfdArray,
        nfds: u32,
        tsp: KernelTimespecPtr,
        sigmask: ConstSigsetPtr,
        sigsetsize: Usize,
    ) -> SysResult<i32> {
        self.sys_ppoll(caller, ufds, nfds, tsp, sigmask, sigsetsize)
            .await
    }

    async fn pselect6_time64(
        &self,
        caller: &Caller<'_>,
        n: i32,
        inp: wasixv2_abi::KernelFdSetPtr,
        outp: wasixv2_abi::KernelFdSetPtr,
        exp: wasixv2_abi::KernelFdSetPtr,
        tsp: KernelTimespecPtr,
        sig: wasixv2_abi::ConstSigsetArgpackPtr,
    ) -> SysResult<i32> {
        self.sys_pselect6(caller, n, inp, outp, exp, tsp, sig).await
    }

    async fn epoll_create1(
        &self,
        caller: &Caller<'_>,
        flags: wasixv2_abi::EpollCreateFlags,
    ) -> SysResult<Fd> {
        self.sys_epoll_create1(caller, flags).await
    }

    async fn epoll_ctl(
        &self,
        caller: &Caller<'_>,
        epfd: Fd,
        op: wasixv2_abi::EpollCtlOp,
        fd: Fd,
        event: wasixv2_abi::ConstEpollEventPtr,
    ) -> SysResult<i32> {
        self.sys_epoll_ctl(caller, epfd, op, fd, event).await
    }

    async fn epoll_pwait(
        &self,
        caller: &Caller<'_>,
        epfd: Fd,
        events: wasixv2_abi::EpollEventArray,
        maxevents: i32,
        timeout: i32,
        sigmask: ConstSigsetPtr,
        sigsetsize: Usize,
    ) -> SysResult<i32> {
        self.sys_epoll_pwait(
            caller, epfd, events, maxevents, timeout, sigmask, sigsetsize,
        )
        .await
    }

    async fn epoll_pwait2(
        &self,
        caller: &Caller<'_>,
        epfd: Fd,
        events: wasixv2_abi::EpollEventArray,
        maxevents: i32,
        timeout: ConstKernelTimespecPtr,
        sigmask: ConstSigsetPtr,
        sigsetsize: Usize,
    ) -> SysResult<i32> {
        self.sys_epoll_pwait2(
            caller, epfd, events, maxevents, timeout, sigmask, sigsetsize,
        )
        .await
    }

    // ---- net ----

    async fn socket(
        &self,
        caller: &Caller<'_>,
        family: wasixv2_abi::AddressFamily,
        r#type: i32,
        protocol: i32,
    ) -> SysResult<Fd> {
        self.sys_socket(caller, family, r#type, protocol).await
    }

    async fn socketpair(
        &self,
        caller: &Caller<'_>,
        family: wasixv2_abi::AddressFamily,
        r#type: i32,
        protocol: i32,
        usockvec: wasixv2_abi::FdPair,
    ) -> SysResult<i32> {
        self.sys_socketpair(caller, family, r#type, protocol, usockvec)
            .await
    }

    async fn bind(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        umyaddr: wasixv2_abi::ConstSockaddrPtr,
        addrlen: wasixv2_abi::Socklen,
    ) -> SysResult<i32> {
        self.sys_bind(caller, fd, umyaddr, addrlen).await
    }

    async fn listen(&self, caller: &Caller<'_>, fd: Fd, backlog: i32) -> SysResult<i32> {
        self.sys_listen(caller, fd, backlog).await
    }

    async fn accept(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        upeer_sockaddr: wasixv2_abi::SockaddrPtr,
        upeer_addrlen: wasixv2_abi::SocklenPtr,
    ) -> SysResult<Fd> {
        let flags = wasixv2_abi::SocketFlags(0);
        self.sys_accept4(caller, fd, upeer_sockaddr, upeer_addrlen, flags)
            .await
    }

    async fn accept4(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        upeer_sockaddr: wasixv2_abi::SockaddrPtr,
        upeer_addrlen: wasixv2_abi::SocklenPtr,
        flags: wasixv2_abi::SocketFlags,
    ) -> SysResult<Fd> {
        self.sys_accept4(caller, fd, upeer_sockaddr, upeer_addrlen, flags)
            .await
    }

    async fn connect(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        uservaddr: wasixv2_abi::ConstSockaddrPtr,
        addrlen: wasixv2_abi::Socklen,
    ) -> SysResult<i32> {
        self.sys_connect(caller, fd, uservaddr, addrlen).await
    }

    async fn getsockname(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        usockaddr: wasixv2_abi::SockaddrPtr,
        usockaddr_len: wasixv2_abi::SocklenPtr,
    ) -> SysResult<i32> {
        self.sys_getsockname(caller, fd, usockaddr, usockaddr_len)
            .await
    }

    async fn getpeername(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        usockaddr: wasixv2_abi::SockaddrPtr,
        usockaddr_len: wasixv2_abi::SocklenPtr,
    ) -> SysResult<i32> {
        self.sys_getpeername(caller, fd, usockaddr, usockaddr_len)
            .await
    }

    async fn sendto(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        buff: UserSlice,
        flags: wasixv2_abi::MsgFlags,
        addr: wasixv2_abi::ConstSockaddrPtr,
        addr_len: wasixv2_abi::Socklen,
    ) -> SysResult<u32> {
        self.sys_sendto(caller, fd, buff, flags, addr, addr_len)
            .await
    }

    async fn recvfrom(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        ubuf: UserSliceMut,
        flags: wasixv2_abi::MsgFlags,
        addr: wasixv2_abi::SockaddrPtr,
        addr_len: wasixv2_abi::SocklenPtr,
    ) -> SysResult<u32> {
        self.sys_recvfrom(caller, fd, ubuf, flags, addr, addr_len)
            .await
    }

    async fn setsockopt(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        level: wasixv2_abi::SocketLevel,
        optname: i32,
        optval: UserSlice,
    ) -> SysResult<i32> {
        self.sys_setsockopt(caller, fd, level, optname, optval)
            .await
    }

    async fn getsockopt(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        level: wasixv2_abi::SocketLevel,
        optname: i32,
        optval: wasixv2_abi::Buf,
        optlen: wasixv2_abi::SocklenPtr,
    ) -> SysResult<i32> {
        self.sys_getsockopt(caller, fd, level, optname, optval, optlen)
            .await
    }

    async fn shutdown(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        how: wasixv2_abi::ShutdownHow,
    ) -> SysResult<i32> {
        self.sys_shutdown(caller, fd, how).await
    }

    async fn sendmsg(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        msg: wasixv2_abi::ConstUserMsghdrPtr,
        flags: wasixv2_abi::MsgFlags,
    ) -> SysResult<i32> {
        self.sys_sendmsg(caller, fd, msg, flags).await
    }

    async fn recvmsg(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        msg: wasixv2_abi::UserMsghdrPtr,
        flags: wasixv2_abi::MsgFlags,
    ) -> SysResult<i32> {
        self.sys_recvmsg(caller, fd, msg, flags).await
    }

    async fn sendmmsg(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        mmsg: wasixv2_abi::MmsghdrArray,
        vlen: u32,
        flags: wasixv2_abi::MsgFlags,
    ) -> SysResult<i32> {
        self.sys_sendmmsg(caller, fd, mmsg, vlen, flags).await
    }

    async fn recvmmsg_time64(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        mmsg: wasixv2_abi::MmsghdrArray,
        vlen: u32,
        flags: wasixv2_abi::MsgFlags,
        _timeout: KernelTimespecPtr,
    ) -> SysResult<i32> {
        self.sys_recvmmsg(caller, fd, mmsg, vlen, flags).await
    }
}

impl Control for Kernel {
    fn take_pending(&self, task: TaskId) -> Option<GuestAction> {
        let mut state = self.state.lock().unwrap();
        let entry = state
            .tasks
            .get_mut(&task)
            .expect("syscall from an unknown task");
        let pending = entry.pending.take();
        // ---- thr ----
        if entry.thread.reapable(pending) {
            state.tasks.remove(&task);
        }
        pending
    }
}

fn sigset_bits(set: Sigset) -> u64 {
    u64::from(set.sig[0]) | (u64::from(set.sig[1]) << 32)
}

fn signal_bit(signal: Signal) -> u64 {
    1 << (signal.0 - 1)
}

#[cfg(test)]
mod tests;
