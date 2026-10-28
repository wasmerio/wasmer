//! The kernel: tasks, processes, credentials, signals, namespaces, IPC, memory, terminals, pseudo
//! filesystems and the socket layer. Implements `Syscalls`.
//!
//! Knows nothing about the wasm engine, the layers, or concrete filesystem drivers and network
//! backends.

mod chrdev;
mod cred;
mod epoll;
mod eventfd;
mod file;
mod fs;
mod futex;
mod ident;
mod ioctl;
mod ipc;
mod memfd;
mod mm;
mod net;
mod netlink;
mod pipe;
mod poll;
mod posix_timer;
mod process;
mod procfs;
mod pseudo;
mod resource;
mod restart;
mod sched;
mod signal;
mod signalfd;
mod socket;
mod startup;
mod stdio;
mod sync;
mod thread;
mod time;
mod timer;
mod timerfd;
mod tty;
mod unix;
mod work;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard},
    time::Duration,
};

pub use cred::Credentials;
pub use file::{FileObject, Io, SeekBase};
pub use fs::HostClock;
pub use ident::{Uname, UnameError};
use wasixv2_abi::{
    AccessMode, AtFlags, Buf, ClockId, CloseRangeFlags, ConstBuf, ConstFileHandlePtr,
    ConstGidArray, ConstIovecArray, ConstKernelItimerspecPtr, ConstKernelTimespecArray,
    ConstKernelTimespecPtr, ConstMqAttrPtr, ConstMsgbufPtr, ConstOpenHowPtr, ConstRlimit64Ptr,
    ConstSchedParamPtr, ConstSembufArray, ConstSigeventPtr, ConstSigsetPtr, ConstXattrArgsPtr,
    Cstr, FadviseAdvice, FallocFlags, FcntlCmd, FileHandlePtr, FlockOp, FutexOp, Gid, GidArray,
    GidPtr, GrndFlags, IoctlRequest, KernelItimerspecPtr, KernelTimespecPtr, LinuxDirent64Ptr,
    Loff, LoffPtr, MadviseAdvice, MapFlags, MclFlags, MembarrierCmd, MembarrierCmdFlags,
    MemfdFlags, MlockFlags, Mode, MqAttrPtr, MremapFlags, MsgbufPtr, Msqid64DsPtr, MsyncFlags,
    NewUtsnamePtr, OpenFlags, Pid, PrctlOption, PrioWhich, ProtFlags, RenameFlags, Rlimit64Ptr,
    RlimitResource, RobustListHeadPtr, RobustListHeadPtrPtr, RusagePtr, RusageWho, RwfFlags,
    S32Ptr, SchedAttrPtr, SchedParamPtr, SchedPolicy, SeekWhence, Shmid64DsPtr, Signal,
    SigprocmaskHow, Sigset, SigsetPtr, SpliceFlags, Statfs64Ptr, StatxMask, StatxPtr,
    SyncFileRangeFlags, SysinfoPtr, TimerFlags, TimerfdFlags, TimerfdSettimeFlags, TmsPtr, U32Ptr,
    Uid, UidPtr, Ulong, UlongArray, Uptr, Usize, UsizePtr, VoidPtr, XattrArgsPtr, XattrFlags,
};
use wasixv2_host::Host;
pub use wasixv2_host::{Console, Stdio, WindowSize};
use wasixv2_syscalls::{
    AtomicWait, AtomicWord, Binfmt, Caller, Control, Errno, ExitMm, Fd, GroupStop, GuestAction,
    Image, LocalFuture, SysResult, Syscalls, TaskId, UserSlice, UserSliceMut, WorkFlag,
};
use wasixv2_vfs::{Filesystem, Mount, Vfs};

use crate::{
    cred::{IdChange, IdKind},
    file::{FdTable, OpenFile},
    fs::FsContext,
    mm::Mm,
    startup::Startup,
    sync::{MutexExt, RwLockExt},
};

/// Most bytes one step of a `read` or `write` moves through the kernel; larger transfers go
/// in several steps. Bounds the host allocation a guest-chosen length can cause.
const IO_CHUNK: usize = 64 * 1024;

/// Most bytes any read or write family call transfers (`MAX_RW_COUNT`: `INT_MAX` rounded down
/// to the page size), so a count always fits the `ssize_t` result.
const MAX_RW_COUNT: usize = i32::MAX as usize & !(wasixv2_syscalls::WASM_PAGE_SIZE as usize - 1);

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
    /// Shared with the process filesystems, which read it.
    state: Arc<TaskTable>,
    // ---- net ----
    /// The network namespace IP sockets live in; without one, `socket` fails with
    /// `EAFNOSUPPORT`.
    // TODO: one per network namespace once namespaces exist.
    network: Option<Arc<dyn wasixv2_vnet::Network>>,
    /// Bound Unix socket names (the abstract namespace and socket inodes).
    unix_names: Arc<unix::Names>,
    // ---- fs ----
    vfs: Arc<Vfs>,
    /// The inodes of pipes, sockets and anonymous files.
    pseudo: pseudo::Pseudo,
    /// The internal shmem filesystem behind `memfd_create` (`shm_mnt`); without one,
    /// `memfd_create` fails with `ENOSYS`.
    shm: Option<Arc<Mount>>,
    /// inotify watches, fed by the VFS's change notifications.
    inotify: Arc<fs::inotify::Registry>,
    // ---- thr ----
    futexes: futex::Futexes,
    // ---- ipc ----
    /// SysV IPC objects and POSIX message queues.
    ipc: ipc::Ipc,
    // ---- proc ----
    /// The process tree: parents, zombies and vfork relations.
    procs: process::Procs,
    /// The formats `execve` starts besides scripts; by default anything with wasm's magic.
    binfmt: Arc<dyn Binfmt>,
    // ---- ipc ----
    /// Serialises epoll nesting changes with their loop checks (`epnested_mutex`).
    epoll_nesting: Mutex<()>,
    /// Every armed timer: timerfds and interval timers.
    timers: timer::Timers,
    // ---- sig ----
    /// The signals queued for each user, against `RLIMIT_SIGPENDING`.
    sigqueue_users: Arc<signal::SigqueueUsers>,
    // ---- base ----
    /// What [`Kernel::spawn`]ed processes run as.
    credentials: Credentials,
    /// The kernel's own init, pid 1, which [`Kernel::spawn`]ed processes are children of.
    init: Option<TaskId>,
    /// The UTS names (one UTS namespace per kernel), shared with the process filesystems.
    uts: Arc<Mutex<wasixv2_abi::NewUtsname>>,
    // ---- tty ----
    /// Process groups and sessions, as terminals and the kernel signal them.
    jobs: tty::Jobs,
    /// The host's console, if the embedder gave one ([`Kernel::with_console`]).
    console: Option<Arc<tty::Tty>>,
}

#[derive(Default)]
struct State {
    last_id: u32,
    tasks: HashMap<TaskId, Task>,
}

/// The task table. Syscalls only read it, to find the caller's descriptor table, signals,
/// process and filesystem context, so readers share it and never wait for each other; only
/// task creation, exit and changes to a task's resource groups take it exclusively.
#[derive(Default)]
struct TaskTable(RwLock<State>);

impl TaskTable {
    /// Exclusive access, taken over from a panicking holder as [`sync`]'s helpers do.
    fn lock_or_recover(&self) -> RwLockWriteGuard<'_, State> {
        self.0.write_or_recover()
    }

    /// Shared access, for lookups.
    fn read_or_recover(&self) -> RwLockReadGuard<'_, State> {
        self.0.read_or_recover()
    }
}

struct Process {
    pid: TaskId,
    // ---- proc ----
    /// `argv` and the environment of the current image, as the bytes `execve` was given; it
    /// replaces them.
    args: Mutex<Vec<Vec<u8>>>,
    env: Mutex<Vec<Vec<u8>>>,
    /// The file the image was loaded from (`mm->exe_file`); `None` for a process the embedder
    /// started from bytes.
    exe: Mutex<Option<wasixv2_vfs::Path>>,
    // ---- thr ----
    group: thread::Group,
    // ---- base ----
    /// Monotonic time the process started, the origin of its CPU clocks.
    start: std::time::Duration,
    ident: Mutex<ident::Ident>,
    cred: Mutex<cred::Cred>,
    resources: Mutex<resource::Resources>,
}

struct Task {
    process: Arc<Process>,
    /// The address space the task runs on (`task->mm`).
    mm: Arc<Mm>,
    files: Arc<Mutex<FdTable>>,
    /// Root, working directory and umask (`fs_struct`).
    fs: Arc<Mutex<FsContext>>,
    /// What the execution layer does on the task's next return to the guest.
    work: work::Work,
    /// How an interrupted call goes on through `restart_syscall` (`restart_block`).
    restart: Option<restart::Restart>,
    /// The task's name (`comm`), which `PR_SET_NAME` and `execve` set.
    comm: ident::Comm,
    /// Policy, priorities, affinity and timer slack: all per thread, as on Linux.
    sched: sched::Sched,
    // ---- thr ----
    thread: thread::ThreadState,
    // ---- sig ----
    /// The task's masks and pending signals, and its thread group's dispositions.
    signals: Arc<signal::TaskSignals>,
}

impl Kernel {
    /// A kernel whose root filesystem is `root`. Further filesystems are mounted through
    /// [`Kernel::vfs`].
    pub fn new(host: Arc<dyn Host>, root: Arc<dyn Filesystem>) -> Self {
        let vfs = Vfs::new(root);
        let clock = Arc::new(fs::HostClock(host.clone()));
        let state = Arc::default();
        let procs = process::Procs::default();
        let jobs = tty::Jobs::new(&state, &procs.tree);
        Self {
            pseudo: pseudo::Pseudo::new(&vfs, clock),
            timers: timer::Timers::new(host.clone()),
            host,
            state,
            network: None,
            unix_names: Arc::default(),
            inotify: fs::inotify::Registry::install(&vfs),
            shm: None,
            vfs,
            futexes: futex::Futexes::default(),
            ipc: ipc::Ipc::default(),
            procs,
            binfmt: Arc::new(process::WasmMagic),
            epoll_nesting: Mutex::default(),
            sigqueue_users: Arc::default(),
            credentials: Credentials::default(),
            init: None,
            uts: Arc::new(Mutex::new(Uname::default().to_utsname())),
            jobs,
            console: None,
        }
    }

    /// Makes [`Kernel::spawn`]ed processes run as `credentials` instead of uid and gid 1.
    pub fn with_credentials(mut self, credentials: Credentials) -> Self {
        self.credentials = credentials;
        self
    }

    /// Makes `uname` report `uname`.
    pub fn with_uname(self, uname: &Uname) -> Self {
        *self.uts.lock_or_recover() = uname.to_utsname();
        self
    }

    /// Gives the kernel a network for IP sockets.
    pub fn with_network(mut self, network: Arc<dyn wasixv2_vnet::Network>) -> Self {
        self.network = Some(network);
        self
    }

    /// Gives the kernel its internal shmem filesystem, a tmpfs mounted outside the tree, for
    /// `memfd_create`'s files. Seals work only on a tmpfs.
    pub fn with_shm(mut self, fs: Arc<dyn Filesystem>) -> Self {
        self.shm = Some(self.vfs.kern_mount(fs));
        self
    }

    /// Makes `execve` start only the images `binfmt` recognises (and `#!` scripts), instead of
    /// any file that starts with wasm's magic; anything else fails with the errno its probe
    /// returns before the caller's image is replaced.
    pub fn with_binfmt(mut self, binfmt: Arc<dyn Binfmt>) -> Self {
        self.binfmt = binfmt;
        self
    }

    /// [`Kernel::with_binfmt`] on a kernel already built.
    pub fn set_binfmt(&mut self, binfmt: Arc<dyn Binfmt>) {
        self.binfmt = binfmt;
    }

    /// The machine's VFS, for the embedder to mount filesystems.
    pub fn vfs(&self) -> &Arc<Vfs> {
        &self.vfs
    }

    /// Boots the kernel with its own init as pid 1, before any [`Kernel::spawn`], as Linux
    /// starts its first task: a process that runs no guest code, every spawned process is a
    /// child of and orphans are handed to. It ignores SIGCHLD, so whatever it inherits is reaped
    /// as it exits, and no signal kills it (`SIGNAL_UNKILLABLE`). It runs as the credentials
    /// given so far, so [`Kernel::with_credentials`] comes first. Without one, spawned processes
    /// have no parent (`getppid` is 0) and orphans no reaper.
    pub fn with_init(mut self) -> Self {
        assert!(
            self.state.read_or_recover().last_id == 0,
            "init is the first process"
        );
        let init = self.new_process(vec!["init".to_string()], Vec::new(), FdTable::default(), 0);
        self.task_signals(init).group().make_init();
        self.init = Some(init);
        self
    }

    /// Starts a process with a single task whose descriptors 0, 1 and 2 are `config.stdio`,
    /// running as the kernel's [`Credentials`], as a child of init when the kernel has one.
    pub fn spawn(&self, config: ProcessConfig) -> TaskId {
        let cred = cred::Cred::new(&self.credentials);
        let mut files = FdTable::default();
        for file in stdio::open_files(config.stdio, &self.pseudo, cred.fs_owner()) {
            files
                .install(Arc::new(file), false)
                .expect("a new table has room for stdio");
        }
        let parent = self.init.map_or(0, |init| init.0);
        self.new_process(config.args, config.env, files, parent)
    }

    /// A process with a single task, `args`, `env` and the descriptors `files`, running as the
    /// kernel's [`Credentials`], entered into the tree under `parent` (0 for none).
    fn new_process(
        &self,
        args: Vec<String>,
        env: Vec<String>,
        files: FdTable,
        parent: u32,
    ) -> TaskId {
        let cred = cred::Cred::new(&self.credentials);
        let mut state = self.state.lock_or_recover();
        state.last_id += 1;
        let id = TaskId(state.last_id);
        let name = args.first().map(String::as_bytes).unwrap_or_default();
        let comm = ident::Comm::from_path(name);
        let process = Arc::new(Process {
            pid: id,
            start: self.host.monotonic(),
            ident: Mutex::new(ident::Ident::new(id.0)),
            cred: Mutex::new(cred),
            resources: Mutex::default(),
            args: Mutex::new(args.into_iter().map(String::into_bytes).collect()),
            env: Mutex::new(env.into_iter().map(String::into_bytes).collect()),
            exe: Mutex::new(None),
            group: thread::Group::new(),
        });
        let flag = Arc::new(WorkFlag::default());
        let account = signal::Account::new(self.sigqueue_users.clone(), &process);
        let task = Task {
            comm,
            sched: sched::Sched::default(),
            process,
            mm: Arc::default(),
            files: Arc::new(Mutex::new(files)),
            fs: Arc::new(Mutex::new(FsContext::new(self.vfs.root()))),
            work: work::Work::new(flag.clone()),
            restart: None,
            thread: thread::ThreadState::default(),
            signals: signal::Signals::new(Some(account)).attach(
                id,
                signal::Inherited::none(),
                flag,
            ),
        };
        let process = task.process.clone();
        state.tasks.insert(id, task);
        drop(state);
        // ---- proc ----
        self.procs.add(process, parent, Signal::SIGCHLD.0);
        id
    }

    /// Whether `process` is the kernel's init, which outlives everything the kernel runs.
    fn is_init(&self, process: &Process) -> bool {
        self.init == Some(process.pid)
    }

    fn file(&self, task: TaskId, fd: Fd) -> Result<Arc<OpenFile>, Errno> {
        let file = self.files(task).lock_or_recover().get(fd);
        file.ok_or(Errno::EBADF)
    }

    fn files(&self, task: TaskId) -> Arc<Mutex<FdTable>> {
        self.task_ref(task, |task| task.files.clone())
    }

    fn process(&self, task: TaskId) -> Arc<Process> {
        self.task_ref(task, |task| task.process.clone())
    }

    /// Reads `task`'s entry under the shared lock.
    fn task_ref<R>(&self, task: TaskId, f: impl FnOnce(&Task) -> R) -> R {
        let state = self.state.read_or_recover();
        f(state
            .tasks
            .get(&task)
            .expect("syscall from an unknown task"))
    }

    fn mm(&self, task: TaskId) -> Arc<Mm> {
        self.task_ref(task, |task| task.mm.clone())
    }

    fn with_task<R>(&self, task: TaskId, f: impl FnOnce(&mut Task) -> R) -> R {
        let mut state = self.state.lock_or_recover();
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
        let (args, env) = (
            process.args.lock_or_recover(),
            process.env.lock_or_recover(),
        );
        Ok(Startup::new(&args, &env, process.identity()).size()?)
    }

    async fn startup_get(&self, caller: &Caller<'_>, buf: UserSliceMut) -> SysResult<i32> {
        if !buf.addr.is_multiple_of(16) {
            return Err(Errno::EINVAL.into());
        }
        let process = self.process(caller.task());
        let (args, env) = (
            process.args.lock_or_recover().clone(),
            process.env.lock_or_recover().clone(),
        );
        let startup = Startup::new(&args, &env, process.identity());
        // The address space starts out as the image left it.
        self.mem_size(caller).await;
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
        // `do_preadv`: only preadv2 gives -1 a meaning.
        if pos < 0 {
            return Err(Errno::EINVAL.into());
        }
        self.sys_preadv2(caller, fd, vec, vlen, pos, RwfFlags(0))
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
        if pos < 0 {
            return Err(Errno::EINVAL.into());
        }
        self.sys_pwritev2(caller, fd, vec, vlen, pos, RwfFlags(0))
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

    async fn splice(
        &self,
        caller: &Caller<'_>,
        fd_in: Fd,
        off_in: LoffPtr,
        fd_out: Fd,
        off_out: LoffPtr,
        len: Usize,
        flags: SpliceFlags,
    ) -> SysResult<i32> {
        self.sys_splice(caller, fd_in, off_in, fd_out, off_out, len, flags)
            .await
    }

    async fn tee(
        &self,
        caller: &Caller<'_>,
        fdin: Fd,
        fdout: Fd,
        len: Usize,
        flags: SpliceFlags,
    ) -> SysResult<i32> {
        self.sys_tee(caller, fdin, fdout, len, flags).await
    }

    async fn vmsplice(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        uiov: ConstIovecArray,
        nr_segs: Ulong,
        flags: SpliceFlags,
    ) -> SysResult<i32> {
        self.sys_vmsplice(caller, fd, uiov, nr_segs, flags).await
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

    async fn inotify_init1(
        &self,
        caller: &Caller<'_>,
        flags: wasixv2_abi::InotifyInitFlags,
    ) -> SysResult<Fd> {
        self.sys_inotify_init1(caller, flags)
    }

    async fn inotify_add_watch(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        pathname: Cstr,
        mask: wasixv2_abi::InotifyMask,
    ) -> SysResult<i32> {
        self.sys_inotify_add_watch(caller, fd, pathname, mask).await
    }

    async fn inotify_rm_watch(&self, caller: &Caller<'_>, fd: Fd, wd: i32) -> SysResult<i32> {
        self.sys_inotify_rm_watch(caller, fd, wd)
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
        self.read_buf(caller, &file, buf, fs::At::Position).await
    }

    async fn write(&self, caller: &Caller<'_>, fd: Fd, buf: UserSlice) -> SysResult<u32> {
        let file = self.file(caller.task(), fd)?;
        file.check_writable()?;
        self.write_buf(caller, &file, buf, fs::At::Position).await
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

    // ---- proc ----
    async fn clone(
        &self,
        caller: &Caller<'_>,
        clone_flags: Ulong,
        newsp: Uptr,
        parent_tidptr: S32Ptr,
        tls: Uptr,
        child_tidptr: S32Ptr,
    ) -> SysResult<i32> {
        self.sys_clone(caller, clone_flags, newsp, parent_tidptr, tls, child_tidptr)
            .await
    }

    async fn execve(
        &self,
        caller: &Caller<'_>,
        filename: Cstr,
        argv: wasixv2_abi::CstrArray,
        envp: wasixv2_abi::CstrArray,
    ) -> SysResult<i32> {
        let cwd = Fd(wasixv2_abi::AtFd::AT_FDCWD.0);
        self.sys_execveat(caller, cwd, filename, argv, envp, AtFlags(0))
            .await
    }

    async fn execveat(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        filename: Cstr,
        argv: wasixv2_abi::CstrArray,
        envp: wasixv2_abi::CstrArray,
        flags: AtFlags,
    ) -> SysResult<i32> {
        self.sys_execveat(caller, fd, filename, argv, envp, flags)
            .await
    }

    async fn waitid(
        &self,
        caller: &Caller<'_>,
        which: wasixv2_abi::Idtype,
        upid: Pid,
        infop: wasixv2_abi::SiginfoPtr,
        options: wasixv2_abi::WaitOptions,
        ru: RusagePtr,
    ) -> SysResult<i32> {
        self.sys_waitid(caller, which, upid, infop, options, ru)
            .await
    }

    async fn pidfd_open(&self, caller: &Caller<'_>, pid: Pid, flags: u32) -> SysResult<Fd> {
        self.sys_pidfd_open(caller, pid, flags)
    }

    async fn pidfd_send_signal(
        &self,
        caller: &Caller<'_>,
        pidfd: Fd,
        sig: Signal,
        info: wasixv2_abi::ConstSiginfoPtr,
        flags: u32,
    ) -> SysResult<i32> {
        self.sys_pidfd_send_signal(caller, pidfd, sig, info, flags)
            .await
    }
    // ---- end proc ----

    async fn getpid(&self, caller: &Caller<'_>) -> SysResult<Pid> {
        Ok(self.process(caller.task()).pid.0 as Pid)
    }

    async fn gettid(&self, caller: &Caller<'_>) -> SysResult<Pid> {
        Ok(caller.task().0 as Pid)
    }

    // ---- sig ----

    async fn signal_callback_register(&self, caller: &Caller<'_>, callback: u32) -> SysResult<i32> {
        self.sys_signal_callback_register(caller, callback).await
    }

    async fn rt_sigaction(
        &self,
        caller: &Caller<'_>,
        sig: Signal,
        act: wasixv2_abi::ConstSigactionPtr,
        oact: wasixv2_abi::SigactionPtr,
        sigsetsize: Usize,
    ) -> SysResult<i32> {
        self.sys_rt_sigaction(caller, sig, act, oact, sigsetsize)
            .await
    }

    async fn rt_sigprocmask(
        &self,
        caller: &Caller<'_>,
        how: SigprocmaskHow,
        nset: ConstSigsetPtr,
        oset: SigsetPtr,
        sigsetsize: Usize,
    ) -> SysResult<i32> {
        self.sys_rt_sigprocmask(caller, how, nset, oset, sigsetsize)
            .await
    }

    async fn rt_sigpending(
        &self,
        caller: &Caller<'_>,
        uset: SigsetPtr,
        sigsetsize: Usize,
    ) -> SysResult<i32> {
        self.sys_rt_sigpending(caller, uset, sigsetsize).await
    }

    async fn rt_sigsuspend(
        &self,
        caller: &Caller<'_>,
        unewset: ConstSigsetPtr,
        sigsetsize: Usize,
    ) -> SysResult<i32> {
        self.sys_rt_sigsuspend(caller, unewset, sigsetsize).await
    }

    async fn rt_sigtimedwait_time64(
        &self,
        caller: &Caller<'_>,
        uthese: ConstSigsetPtr,
        uinfo: wasixv2_abi::SiginfoPtr,
        uts: ConstKernelTimespecPtr,
        sigsetsize: Usize,
    ) -> SysResult<i32> {
        self.sys_rt_sigtimedwait(caller, uthese, uinfo, uts, sigsetsize)
            .await
    }

    async fn rt_sigqueueinfo(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        sig: Signal,
        uinfo: wasixv2_abi::ConstSiginfoPtr,
    ) -> SysResult<i32> {
        self.sys_rt_sigqueueinfo(caller, pid, None, sig, uinfo)
            .await
    }

    async fn rt_tgsigqueueinfo(
        &self,
        caller: &Caller<'_>,
        tgid: Pid,
        pid: Pid,
        sig: Signal,
        uinfo: wasixv2_abi::ConstSiginfoPtr,
    ) -> SysResult<i32> {
        self.sys_rt_sigqueueinfo(caller, tgid, Some(pid), sig, uinfo)
            .await
    }

    async fn sigaltstack(
        &self,
        caller: &Caller<'_>,
        uss: wasixv2_abi::ConstSigaltstackPtr,
        uoss: wasixv2_abi::SigaltstackPtr,
    ) -> SysResult<i32> {
        self.sys_sigaltstack(caller, uss, uoss).await
    }

    async fn kill(&self, caller: &Caller<'_>, pid: Pid, sig: Signal) -> SysResult<i32> {
        self.sys_kill(caller, pid, sig).await
    }

    async fn tkill(&self, caller: &Caller<'_>, pid: Pid, sig: Signal) -> SysResult<i32> {
        self.sys_tgkill(caller, None, pid, sig).await
    }

    async fn tgkill(
        &self,
        caller: &Caller<'_>,
        tgid: Pid,
        pid: Pid,
        sig: Signal,
    ) -> SysResult<i32> {
        self.sys_tgkill(caller, Some(tgid), pid, sig).await
    }

    async fn setitimer(
        &self,
        caller: &Caller<'_>,
        which: wasixv2_abi::ItimerWhich,
        value: wasixv2_abi::ConstKernelOldItimervalPtr,
        ovalue: wasixv2_abi::KernelOldItimervalPtr,
    ) -> SysResult<i32> {
        self.sys_setitimer(caller, which, value, ovalue).await
    }

    async fn getitimer(
        &self,
        caller: &Caller<'_>,
        which: wasixv2_abi::ItimerWhich,
        value: wasixv2_abi::KernelOldItimervalPtr,
    ) -> SysResult<i32> {
        self.sys_getitimer(caller, which, value).await
    }

    async fn timer_create(
        &self,
        caller: &Caller<'_>,
        which_clock: ClockId,
        timer_event_spec: wasixv2_abi::ConstSigeventPtr,
        created_timer_id: wasixv2_abi::TimerIdPtr,
    ) -> SysResult<i32> {
        self.sys_timer_create(caller, which_clock, timer_event_spec, created_timer_id)
            .await
    }

    async fn timer_settime64(
        &self,
        caller: &Caller<'_>,
        timer_id: wasixv2_abi::TimerId,
        flags: TimerFlags,
        new_setting: ConstKernelItimerspecPtr,
        old_setting: KernelItimerspecPtr,
    ) -> SysResult<i32> {
        self.sys_timer_settime(caller, timer_id, flags, new_setting, old_setting)
            .await
    }

    async fn timer_gettime64(
        &self,
        caller: &Caller<'_>,
        timer_id: wasixv2_abi::TimerId,
        setting: KernelItimerspecPtr,
    ) -> SysResult<i32> {
        self.sys_timer_gettime(caller, timer_id, setting).await
    }

    async fn timer_getoverrun(
        &self,
        caller: &Caller<'_>,
        timer_id: wasixv2_abi::TimerId,
    ) -> SysResult<i32> {
        self.sys_timer_getoverrun(caller, timer_id)
    }

    async fn timer_delete(
        &self,
        caller: &Caller<'_>,
        timer_id: wasixv2_abi::TimerId,
    ) -> SysResult<i32> {
        self.sys_timer_delete(caller, timer_id)
    }

    // ---- base ----
    async fn getppid(&self, caller: &Caller<'_>) -> SysResult<Pid> {
        Ok(self.sys_getppid(caller))
    }
    async fn getuid(&self, caller: &Caller<'_>) -> SysResult<i32> {
        Ok(self.sys_getid(caller, IdKind::User, false))
    }
    async fn geteuid(&self, caller: &Caller<'_>) -> SysResult<i32> {
        Ok(self.sys_getid(caller, IdKind::User, true))
    }
    async fn getgid(&self, caller: &Caller<'_>) -> SysResult<i32> {
        Ok(self.sys_getid(caller, IdKind::Group, false))
    }
    async fn getegid(&self, caller: &Caller<'_>) -> SysResult<i32> {
        Ok(self.sys_getid(caller, IdKind::Group, true))
    }
    async fn setuid(&self, caller: &Caller<'_>, uid: Uid) -> SysResult<i32> {
        self.sys_set_ids(caller, IdKind::User, IdChange::Id(uid))
    }
    async fn setgid(&self, caller: &Caller<'_>, gid: Gid) -> SysResult<i32> {
        self.sys_set_ids(caller, IdKind::Group, IdChange::Id(gid))
    }
    async fn setreuid(&self, caller: &Caller<'_>, ruid: Uid, euid: Uid) -> SysResult<i32> {
        self.sys_set_ids(caller, IdKind::User, IdChange::Re(ruid, euid))
    }
    async fn setregid(&self, caller: &Caller<'_>, rgid: Gid, egid: Gid) -> SysResult<i32> {
        self.sys_set_ids(caller, IdKind::Group, IdChange::Re(rgid, egid))
    }
    async fn setresuid(&self, caller: &Caller<'_>, r: Uid, e: Uid, s: Uid) -> SysResult<i32> {
        self.sys_set_ids(caller, IdKind::User, IdChange::Res(r, e, s))
    }
    async fn setresgid(&self, caller: &Caller<'_>, r: Gid, e: Gid, s: Gid) -> SysResult<i32> {
        self.sys_set_ids(caller, IdKind::Group, IdChange::Res(r, e, s))
    }
    async fn getresuid(
        &self,
        caller: &Caller<'_>,
        r: UidPtr,
        e: UidPtr,
        s: UidPtr,
    ) -> SysResult<i32> {
        self.sys_getresid(caller, IdKind::User, [r, e, s]).await
    }
    async fn getresgid(
        &self,
        caller: &Caller<'_>,
        r: GidPtr,
        e: GidPtr,
        s: GidPtr,
    ) -> SysResult<i32> {
        self.sys_getresid(caller, IdKind::Group, [r, e, s]).await
    }
    async fn setfsuid(&self, caller: &Caller<'_>, uid: Uid) -> SysResult<i32> {
        self.sys_setfsid(caller, IdKind::User, uid)
    }
    async fn setfsgid(&self, caller: &Caller<'_>, gid: Gid) -> SysResult<i32> {
        self.sys_setfsid(caller, IdKind::Group, gid)
    }
    async fn getgroups(&self, caller: &Caller<'_>, size: i32, list: GidArray) -> SysResult<i32> {
        self.sys_getgroups(caller, size, list).await
    }
    async fn setgroups(
        &self,
        caller: &Caller<'_>,
        size: i32,
        list: ConstGidArray,
    ) -> SysResult<i32> {
        self.sys_setgroups(caller, size, list).await
    }
    async fn capget(
        &self,
        caller: &Caller<'_>,
        header: wasixv2_abi::UserCapHeaderPtr,
        data: wasixv2_abi::UserCapDataArray,
    ) -> SysResult<i32> {
        self.sys_capget(caller, header, data).await
    }
    async fn capset(
        &self,
        caller: &Caller<'_>,
        header: wasixv2_abi::UserCapHeaderPtr,
        data: wasixv2_abi::ConstUserCapDataArray,
    ) -> SysResult<i32> {
        self.sys_capset(caller, header, data).await
    }
    async fn getpgid(&self, caller: &Caller<'_>, pid: Pid) -> SysResult<Pid> {
        self.sys_getpgid(caller, pid)
    }
    async fn setpgid(&self, caller: &Caller<'_>, pid: Pid, pgid: Pid) -> SysResult<i32> {
        self.sys_setpgid(caller, pid, pgid)
    }
    async fn getsid(&self, caller: &Caller<'_>, pid: Pid) -> SysResult<Pid> {
        self.sys_getsid(caller, pid)
    }
    async fn setsid(&self, caller: &Caller<'_>) -> SysResult<Pid> {
        self.sys_setsid(caller)
    }
    async fn uname(&self, caller: &Caller<'_>, name: NewUtsnamePtr) -> SysResult<i32> {
        self.sys_uname(caller, name).await
    }
    async fn sethostname(&self, caller: &Caller<'_>, name: ConstBuf, len: i32) -> SysResult<i32> {
        self.sys_sethostname(caller, name, len, false).await
    }
    async fn setdomainname(&self, caller: &Caller<'_>, name: ConstBuf, len: i32) -> SysResult<i32> {
        self.sys_sethostname(caller, name, len, true).await
    }
    async fn personality(&self, caller: &Caller<'_>, personality: u32) -> SysResult<i32> {
        self.sys_personality(caller, personality)
    }
    async fn prctl(
        &self,
        caller: &Caller<'_>,
        option: PrctlOption,
        a2: Ulong,
        a3: Ulong,
        a4: Ulong,
        a5: Ulong,
    ) -> SysResult<i32> {
        self.sys_prctl(caller, option, a2, a3, a4, a5).await
    }
    async fn prlimit64(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        resource: RlimitResource,
        new: ConstRlimit64Ptr,
        old: Rlimit64Ptr,
    ) -> SysResult<i32> {
        self.sys_prlimit64(caller, pid, resource, new, old).await
    }
    async fn getrusage(
        &self,
        caller: &Caller<'_>,
        who: RusageWho,
        ru: RusagePtr,
    ) -> SysResult<i32> {
        self.sys_getrusage(caller, who, ru).await
    }
    async fn times(&self, caller: &Caller<'_>, tbuf: TmsPtr) -> SysResult<i32> {
        self.sys_times(caller, tbuf).await
    }
    async fn sysinfo(&self, caller: &Caller<'_>, info: SysinfoPtr) -> SysResult<i32> {
        self.sys_sysinfo(caller, info).await
    }
    async fn getrandom(
        &self,
        caller: &Caller<'_>,
        buf: UserSliceMut,
        flags: GrndFlags,
    ) -> SysResult<u32> {
        self.sys_getrandom(caller, buf, flags).await
    }
    async fn sched_yield(&self, _: &Caller<'_>) -> SysResult<i32> {
        self.sys_sched_yield().await
    }
    async fn sched_getscheduler(&self, caller: &Caller<'_>, pid: Pid) -> SysResult<i32> {
        self.sys_sched_getscheduler(caller, pid)
    }
    async fn sched_setscheduler(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        policy: SchedPolicy,
        param: ConstSchedParamPtr,
    ) -> SysResult<i32> {
        self.sys_sched_setscheduler(caller, pid, Some(policy), param)
            .await
    }
    async fn sched_setparam(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        param: ConstSchedParamPtr,
    ) -> SysResult<i32> {
        self.sys_sched_setscheduler(caller, pid, None, param).await
    }
    async fn sched_getparam(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        param: SchedParamPtr,
    ) -> SysResult<i32> {
        self.sys_sched_getparam(caller, pid, param).await
    }
    async fn sched_setattr(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        uattr: SchedAttrPtr,
        flags: u32,
    ) -> SysResult<i32> {
        self.sys_sched_setattr(caller, pid, uattr, flags).await
    }
    async fn sched_getattr(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        uattr: SchedAttrPtr,
        usize: u32,
        flags: u32,
    ) -> SysResult<i32> {
        self.sys_sched_getattr(caller, pid, uattr, usize, flags)
            .await
    }
    async fn sched_get_priority_max(&self, _: &Caller<'_>, policy: SchedPolicy) -> SysResult<i32> {
        self.sys_sched_priority(policy, true)
    }
    async fn sched_get_priority_min(&self, _: &Caller<'_>, policy: SchedPolicy) -> SysResult<i32> {
        self.sys_sched_priority(policy, false)
    }
    async fn sched_rr_get_interval_time64(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        interval: KernelTimespecPtr,
    ) -> SysResult<i32> {
        self.sys_sched_rr_get_interval(caller, pid, interval).await
    }
    async fn sched_getaffinity(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        len: u32,
        mask: UlongArray,
    ) -> SysResult<i32> {
        self.sys_sched_getaffinity(caller, pid, len, mask).await
    }
    async fn sched_setaffinity(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        len: u32,
        mask: UlongArray,
    ) -> SysResult<i32> {
        self.sys_sched_setaffinity(caller, pid, len, mask).await
    }
    async fn getcpu(
        &self,
        caller: &Caller<'_>,
        cpu: U32Ptr,
        node: U32Ptr,
        _: VoidPtr,
    ) -> SysResult<i32> {
        self.sys_getcpu(caller, cpu, node).await
    }
    async fn getpriority(&self, caller: &Caller<'_>, which: PrioWhich, who: i32) -> SysResult<i32> {
        self.sys_getpriority(caller, which, who)
    }
    async fn setpriority(
        &self,
        caller: &Caller<'_>,
        which: PrioWhich,
        who: i32,
        nice: i32,
    ) -> SysResult<i32> {
        self.sys_setpriority(caller, which, who, nice)
    }
    async fn membarrier(
        &self,
        caller: &Caller<'_>,
        cmd: MembarrierCmd,
        flags: MembarrierCmdFlags,
        _: i32,
    ) -> SysResult<i32> {
        self.sys_membarrier(caller, cmd, flags)
    }
    async fn brk(&self, caller: &Caller<'_>, brk: Uptr) -> SysResult<i32> {
        self.sys_brk(caller, brk).await
    }
    async fn mmap(
        &self,
        caller: &Caller<'_>,
        addr: Uptr,
        len: Ulong,
        prot: ProtFlags,
        flags: MapFlags,
        fd: Fd,
        offset: Loff,
    ) -> SysResult<Uptr> {
        self.sys_mmap(caller, addr, len, prot, flags, fd, offset)
            .await
    }
    async fn munmap(&self, caller: &Caller<'_>, addr: Uptr, len: Usize) -> SysResult<i32> {
        self.sys_munmap(caller, addr, len).await
    }
    async fn mremap(
        &self,
        caller: &Caller<'_>,
        addr: Uptr,
        old_len: Ulong,
        new_len: Ulong,
        flags: MremapFlags,
        new_addr: Uptr,
    ) -> SysResult<i32> {
        self.sys_mremap(caller, addr, old_len, new_len, flags, new_addr)
            .await
    }
    async fn mprotect(
        &self,
        caller: &Caller<'_>,
        start: Uptr,
        len: Usize,
        prot: ProtFlags,
    ) -> SysResult<i32> {
        self.sys_mprotect(caller, start, len, prot).await
    }
    async fn madvise(
        &self,
        caller: &Caller<'_>,
        start: Uptr,
        len: Usize,
        advice: MadviseAdvice,
    ) -> SysResult<i32> {
        self.sys_madvise(caller, start, len, advice).await
    }
    async fn mincore(
        &self,
        caller: &Caller<'_>,
        start: Uptr,
        len: Usize,
        vec: Buf,
    ) -> SysResult<i32> {
        self.sys_mincore(caller, start, len, vec).await
    }
    async fn msync(
        &self,
        caller: &Caller<'_>,
        start: Uptr,
        len: Usize,
        flags: MsyncFlags,
    ) -> SysResult<i32> {
        self.sys_msync(caller, start, len, flags).await
    }
    async fn mlock(&self, caller: &Caller<'_>, start: Uptr, len: Usize) -> SysResult<i32> {
        self.sys_mlock(caller, start, len, MlockFlags::empty())
            .await
    }
    async fn mlock2(
        &self,
        caller: &Caller<'_>,
        start: Uptr,
        len: Usize,
        flags: MlockFlags,
    ) -> SysResult<i32> {
        self.sys_mlock(caller, start, len, flags).await
    }
    async fn munlock(&self, caller: &Caller<'_>, start: Uptr, len: Usize) -> SysResult<i32> {
        self.sys_mlock(caller, start, len, MlockFlags::empty())
            .await
    }
    async fn mlockall(&self, _: &Caller<'_>, flags: MclFlags) -> SysResult<i32> {
        self.sys_mlockall(flags)
    }
    async fn munlockall(&self, _: &Caller<'_>) -> SysResult<i32> {
        Ok(0)
    }
    async fn memfd_create(
        &self,
        caller: &Caller<'_>,
        uname: Cstr,
        flags: MemfdFlags,
    ) -> SysResult<Fd> {
        self.sys_memfd_create(caller, uname, flags).await
    }
    async fn clock_gettime64(
        &self,
        caller: &Caller<'_>,
        clock: ClockId,
        tp: KernelTimespecPtr,
    ) -> SysResult<i32> {
        self.sys_clock_gettime(caller, clock, tp).await
    }
    async fn clock_getres_time64(
        &self,
        caller: &Caller<'_>,
        clock: ClockId,
        tp: KernelTimespecPtr,
    ) -> SysResult<i32> {
        self.sys_clock_getres(caller, clock, tp).await
    }
    async fn clock_settime64(
        &self,
        caller: &Caller<'_>,
        clock: ClockId,
        tp: ConstKernelTimespecPtr,
    ) -> SysResult<i32> {
        self.sys_clock_settime(caller, clock, tp).await
    }
    async fn clock_nanosleep_time64(
        &self,
        caller: &Caller<'_>,
        clock: ClockId,
        flags: TimerFlags,
        rqtp: ConstKernelTimespecPtr,
        rmtp: KernelTimespecPtr,
    ) -> SysResult<i32> {
        self.sys_clock_nanosleep(caller, clock, flags, rqtp, rmtp)
            .await
    }
    async fn restart_syscall(&self, caller: &Caller<'_>) -> SysResult<i32> {
        self.sys_restart_syscall(caller).await
    }
    async fn signalfd4(
        &self,
        caller: &Caller<'_>,
        ufd: Fd,
        user_mask: ConstSigsetPtr,
        sizemask: Usize,
        flags: wasixv2_abi::SignalfdFlags,
    ) -> SysResult<Fd> {
        self.sys_signalfd4(caller, ufd, user_mask, sizemask, flags)
            .await
    }

    async fn timerfd_create(
        &self,
        caller: &Caller<'_>,
        clock: ClockId,
        flags: TimerfdFlags,
    ) -> SysResult<Fd> {
        self.sys_timerfd_create(caller, clock, flags).await
    }
    async fn timerfd_settime64(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        flags: TimerfdSettimeFlags,
        new: ConstKernelItimerspecPtr,
        old: KernelItimerspecPtr,
    ) -> SysResult<i32> {
        self.sys_timerfd_settime(caller, fd, flags, new, old).await
    }
    async fn timerfd_gettime64(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        out: KernelItimerspecPtr,
    ) -> SysResult<i32> {
        self.sys_timerfd_gettime(caller, fd, out).await
    }

    // ---- ipc ----

    async fn msgget(&self, caller: &Caller<'_>, key: i32, msgflg: i32) -> SysResult<i32> {
        self.sys_msgget(caller, key, msgflg)
    }

    async fn msgctl(
        &self,
        caller: &Caller<'_>,
        msqid: i32,
        cmd: i32,
        buf: Msqid64DsPtr,
    ) -> SysResult<i32> {
        self.sys_msgctl(caller, msqid, cmd, buf.addr()).await
    }

    async fn msgrcv(
        &self,
        caller: &Caller<'_>,
        msqid: i32,
        msgp: MsgbufPtr,
        msgsz: Usize,
        msgtyp: i32,
        msgflg: i32,
    ) -> SysResult<i32> {
        let n = self
            .sys_msgrcv(caller, msqid, msgp, msgsz, msgtyp, msgflg)
            .await?;
        self.sync_segments(caller).await;
        Ok(n)
    }

    async fn msgsnd(
        &self,
        caller: &Caller<'_>,
        msqid: i32,
        msgp: ConstMsgbufPtr,
        msgsz: Usize,
        msgflg: i32,
    ) -> SysResult<i32> {
        self.sync_segments(caller).await;
        self.sys_msgsnd(caller, msqid, msgp, msgsz, msgflg).await
    }

    async fn semget(
        &self,
        caller: &Caller<'_>,
        key: i32,
        nsems: i32,
        semflg: i32,
    ) -> SysResult<i32> {
        self.sys_semget(caller, key, nsems, semflg)
    }

    async fn semctl(
        &self,
        caller: &Caller<'_>,
        semid: i32,
        semnum: i32,
        cmd: i32,
        arg: Ulong,
    ) -> SysResult<i32> {
        self.sys_semctl(caller, semid, semnum, cmd, arg).await
    }

    async fn semop(
        &self,
        caller: &Caller<'_>,
        semid: i32,
        tsops: ConstSembufArray,
        nsops: u32,
    ) -> SysResult<i32> {
        let none = ConstKernelTimespecPtr::NULL;
        self.semtimedop_time64(caller, semid, tsops, nsops, none)
            .await
    }

    async fn semtimedop_time64(
        &self,
        caller: &Caller<'_>,
        semid: i32,
        tsops: ConstSembufArray,
        nsops: u32,
        timeout: ConstKernelTimespecPtr,
    ) -> SysResult<i32> {
        self.sync_segments(caller).await;
        let result = self
            .sys_semtimedop(caller, semid, tsops, nsops, timeout)
            .await;
        self.sync_segments(caller).await;
        result
    }

    async fn shmget(
        &self,
        caller: &Caller<'_>,
        key: i32,
        size: Usize,
        shmflg: i32,
    ) -> SysResult<i32> {
        self.sys_shmget(caller, key, size, shmflg)
    }

    async fn shmctl(
        &self,
        caller: &Caller<'_>,
        shmid: i32,
        cmd: i32,
        buf: Shmid64DsPtr,
    ) -> SysResult<i32> {
        self.sys_shmctl(caller, shmid, cmd, buf.addr()).await
    }

    async fn shmat(
        &self,
        caller: &Caller<'_>,
        shmid: i32,
        shmaddr: Uptr,
        shmflg: i32,
    ) -> SysResult<i32> {
        self.sys_shmat(caller, shmid, shmaddr, shmflg).await
    }

    async fn shmdt(&self, caller: &Caller<'_>, shmaddr: Uptr) -> SysResult<i32> {
        self.sys_shmdt(caller, shmaddr).await
    }

    async fn mq_open(
        &self,
        caller: &Caller<'_>,
        u_name: Cstr,
        oflag: OpenFlags,
        mode: Mode,
        u_attr: ConstMqAttrPtr,
    ) -> SysResult<i32> {
        self.sys_mq_open(caller, u_name, oflag, mode, u_attr).await
    }

    async fn mq_unlink(&self, caller: &Caller<'_>, u_name: Cstr) -> SysResult<i32> {
        self.sys_mq_unlink(caller, u_name).await
    }

    async fn mq_timedsend_time64(
        &self,
        caller: &Caller<'_>,
        mqdes: i32,
        u_msg_ptr: ConstBuf,
        msg_len: Usize,
        msg_prio: u32,
        u_abs_timeout: ConstKernelTimespecPtr,
    ) -> SysResult<i32> {
        self.sync_segments(caller).await;
        self.sys_mq_timedsend(
            caller,
            Fd(mqdes),
            u_msg_ptr,
            msg_len,
            msg_prio,
            u_abs_timeout,
        )
        .await
    }

    async fn mq_timedreceive_time64(
        &self,
        caller: &Caller<'_>,
        mqdes: i32,
        u_msg_ptr: Buf,
        msg_len: Usize,
        u_msg_prio: U32Ptr,
        u_abs_timeout: ConstKernelTimespecPtr,
    ) -> SysResult<i32> {
        let n = self
            .sys_mq_timedreceive(
                caller,
                Fd(mqdes),
                u_msg_ptr,
                msg_len,
                u_msg_prio,
                u_abs_timeout,
            )
            .await?;
        self.sync_segments(caller).await;
        Ok(n)
    }

    async fn mq_notify(
        &self,
        caller: &Caller<'_>,
        mqdes: i32,
        u_notification: ConstSigeventPtr,
    ) -> SysResult<i32> {
        self.sys_mq_notify(caller, Fd(mqdes), u_notification).await
    }

    async fn mq_getsetattr(
        &self,
        caller: &Caller<'_>,
        mqdes: i32,
        u_mqstat: ConstMqAttrPtr,
        u_omqstat: MqAttrPtr,
    ) -> SysResult<i32> {
        self.sys_mq_getsetattr(caller, Fd(mqdes), u_mqstat, u_omqstat)
            .await
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
        // ---- proc ----
        self.kill_vfork_parent(task);
        let (action, signals) = {
            let mut state = self.state.lock_or_recover();
            // ---- proc ----
            // A reaped process's task, still unwinding.
            let entry = state.tasks.get_mut(&task)?;
            entry.work.flag().clear();
            let (action, signals) = (entry.work.take(), entry.signals.clone());
            let action = action.map(|action| process::vfork_parent_resumes(&state, action));
            (action, signals)
        };
        match action {
            // What made the task leave its wait is here.
            Some(action) => {
                signals.group().clear_notify(task);
                if let GuestAction::Exit(_) = action {
                    self.exit_signals(task, &signals);
                }
                Some(action)
            }
            None => self.take_signal(task, &signals),
        }
    }

    fn work_flag(&self, task: TaskId) -> Arc<WorkFlag> {
        let state = self.state.lock_or_recover();
        match state.tasks.get(&task) {
            Some(task) => task.work.flag().clone(),
            None => Arc::new(WorkFlag::always()),
        }
    }

    fn signal_return(&self, task: TaskId, mask: Option<u64>) {
        Kernel::signal_return(self, task, mask)
    }

    fn fault(&self, task: TaskId, signal: i32) -> bool {
        self.force_fatal_signal(task, signal)
    }

    fn release(&self, task: TaskId) {
        self.release_task(task);
    }

    fn abort_thread(&self, task: TaskId) {
        Kernel::abort_thread(self, task);
    }

    fn group_wait(&self, task: TaskId) -> LocalFuture<'_, ()> {
        Box::pin(Kernel::group_wait(self, task))
    }

    // ---- proc ----
    fn take_image(&self, task: TaskId) -> Option<Image> {
        let mut state = self.state.lock_or_recover();
        state.tasks.get_mut(&task)?.work.take_image()
    }

    fn exec_wait(&self, task: TaskId) -> LocalFuture<'_, Result<(), i32>> {
        Box::pin(Kernel::exec_wait(self, task))
    }
}

impl GroupStop for Kernel {
    fn stop_wait(&self, task: TaskId) -> LocalFuture<'_, ()> {
        Box::pin(Kernel::stop_wait(self, task))
    }
}

impl ExitMm for Kernel {
    fn exit_mm<'a>(&'a self, caller: &'a Caller<'a>) -> LocalFuture<'a, ()> {
        Box::pin(Kernel::exit_mm(self, caller))
    }
}

impl AtomicWait for Kernel {
    fn atomic_wait<'a>(
        &'a self,
        caller: &'a Caller<'a>,
        addr: u32,
        expected: AtomicWord,
        timeout: Option<Duration>,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(Kernel::atomic_wait(self, caller, addr, expected, timeout))
    }

    fn atomic_notify(&self, task: TaskId, addr: u32, count: u32) -> u32 {
        Kernel::atomic_notify(self, task, addr, count)
    }
}

fn sigset_bits(set: Sigset) -> u64 {
    u64::from(set.sig[0]) | (u64::from(set.sig[1]) << 32)
}

#[cfg(test)]
mod tests;
