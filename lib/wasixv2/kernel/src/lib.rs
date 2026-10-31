//! The kernel: tasks, processes, credentials, signals, namespaces, IPC, memory, terminals, pseudo
//! filesystems and the socket layer. Implements `Syscalls`.
//!
//! Knows nothing about the wasm engine, the layers, or concrete filesystem drivers and network
//! backends.

// The kernel's state is one deep graph of types (task table, process tree, cgroups, namespaces,
// filesystems); proving it `Send` and `Sync` for the VFS's traits goes past the default depth.
#![recursion_limit = "256"]

mod acct;
mod aio;
mod binfmt_misc;
mod cgroup;
mod chrdev;
mod coredump;
mod cputime;
mod cred;
mod devinet;
mod entry;
mod epoll;
mod eventfd;
mod fasync;
mod file;
mod fs;
mod fuse;
mod futex;
mod ident;
mod io_uring;
mod ioctl;
mod ipc;
mod kcmp;
mod keys;
mod landlock;
mod loader;
mod lsm;
mod memfd;
mod mm;
mod net;
mod netlink;
mod ns;
mod nsfs;
mod ntp;
mod pid;
mod pipe;
mod poll;
mod posix_timer;
mod printk;
mod process;
mod procfs;
mod pseudo;
mod ptrace;
mod reboot;
mod resource;
mod restart;
mod rseq;
mod sched;
mod seccomp;
mod signal;
mod signalfd;
mod socket;
mod startup;
mod stdio;
mod sync;
mod sysctl;
mod sysfs;
mod taint;
mod thread;
mod time;
mod timekeeping;
mod timer;
mod timerfd;
mod tty;
mod uidgid;
mod unix;
mod userns;
mod watch_queue;
mod work;
mod yama;
pub use watch_queue::WatchList;

use std::{
    collections::HashMap,
    ops::Range,
    sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard},
    time::Duration,
};

pub use coredump::UsermodeHelper;
pub use cred::Credentials;
pub use entry::{Dispatch, SyscallEntry};
pub use file::{FileObject, Io, SeekBase};
pub use fs::{
    HostClock,
    mount::{FilesystemType, FsParam},
};
pub use ident::{Uname, UnameError};
pub use printk::Level as LogLevel;
pub use reboot::PowerRequest;
pub use timekeeping::Timekeeper;
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
    AtomicWait, AtomicWord, Binfmt, Caller, Control, CpuAccount, CpuTime, Errno, ExitMm, Fd,
    GroupCpu, GroupStop, GuestAction, Image, LocalFuture, MaybeFuture, PidView, SharedMapping,
    SharedMappings, SysResult, Syscalls, TaskId, UserSlice, UserSliceMut, VforkWait, WorkFlag,
};
use wasixv2_vfs::{Filesystem, Mount, Vfs};

use crate::{
    cred::{IdChange, IdKind, given},
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
    /// The machine's clocks: [`Kernel::time`] as a host, whose wall clock is the machine's.
    host: Arc<dyn Host>,
    time: Arc<Timekeeper>,
    /// Shared with the process filesystems, which read it.
    state: Arc<TaskTable>,
    // ---- ns ----
    /// The initial user namespace (`init_user_ns`), which owns the initial namespaces and
    /// which [`Kernel::spawn`]ed processes' credentials are in.
    init_user_ns: Arc<ns::UserNs>,
    /// The initial namespaces (`init_nsproxy`), which [`Kernel::spawn`]ed processes start in.
    init_ns: Arc<ns::NsProxy>,
    /// The inode numbers of namespaces made after boot.
    ns_inums: Arc<ns::Inums>,
    /// Makes the IP network of each network namespace made after boot; without it, they have
    /// none.
    network_factory: Option<ns::NetworkFactory>,
    /// The bind table of `AF_VSOCK` sockets, which no namespace in vsock's global mode
    /// separates.
    vsock: Arc<net::vsock::Table>,
    // ---- fs ----
    vfs: Arc<Vfs>,
    /// The inodes of pipes, sockets and anonymous files.
    pseudo: pseudo::Pseudo,
    /// `fs.nr_open`, which every descriptor table reads.
    nr_open: Arc<file::NrOpen>,
    /// `kernel.tainted`.
    taint: Arc<taint::Taint>,
    /// `fs.pipe-max-size`.
    pipe_max_size: Arc<pipe::PipeMaxSize>,
    /// `fs.aio-nr` and `fs.aio-max-nr`.
    aio: Arc<aio::AioSysctl>,
    watch_users: watch_queue::Users,
    /// `vm.overcommit_*` and `vm.max_map_count`.
    vm: Arc<mm::VmSysctls>,
    /// The internal shmem filesystem behind `memfd_create` (`shm_mnt`); without one,
    /// `memfd_create` fails with `ENOSYS`.
    shm: Option<Arc<Mount>>,
    /// inotify watches and dnotify registrations, fed by the VFS's change notifications.
    inotify: Arc<fs::inotify::Registry>,
    dnotify: Arc<fs::dnotify::Dnotify>,
    /// fanotify groups and their marks, fed the same way.
    fanotify: Arc<fs::fanotify::Registry>,
    /// The `fs.*` tunables the kernel acts on.
    sysctls: Arc<fs::FsSysctls>,
    /// The filesystem types `mount` makes instances of.
    filesystems: Arc<fs::mount::Registry>,
    /// The cgroup v2 hierarchy (`cgrp_dfl_root`).
    cgroups: Arc<cgroup::Hierarchy>,
    /// The kernel log.
    printk: Arc<printk::Printk>,
    /// BSD process accounting.
    acct: Arc<acct::Acct>,
    /// Where core dumps go.
    coredump: Arc<coredump::Coredump>,
    /// The integer sysctls of [`sysctl::Tunable`].
    tunables: Arc<sysctl::Tunables>,
    /// `kernel.random.boot_id`.
    boot_id: [u8; 16],
    // ---- thr ----
    futexes: Arc<futex::Futexes>,
    // ---- proc ----
    /// The process tree: parents, zombies and vfork relations.
    procs: process::Procs,
    /// The numbers of every task in the pid namespaces it is in.
    pids: Arc<pid::Pids>,
    /// The formats `execve` starts besides scripts; by default anything with wasm's magic.
    binfmt: Arc<dyn Binfmt>,
    /// The formats registered at run time, tried before scripts, per user namespace
    /// (`binfmt_misc`).
    binfmt_misc: Arc<binfmt_misc::Instances>,
    // ---- ipc ----
    /// Serialises epoll nesting changes with their loop checks (`epnested_mutex`).
    epoll_nesting: Mutex<()>,
    /// Every armed timer: timerfds and interval timers.
    timers: timer::Timers,
    // ---- sig ----
    /// The signals queued for each user, against `RLIMIT_SIGPENDING`.
    sigqueue_users: Arc<signal::SigqueueUsers>,
    /// io_uring pins charged to each registering user (user_struct::locked_vm).
    memlock_users: mm::MemlockUsers,
    /// Sends the I/O signals of files with `O_ASYNC` to their owners.
    sigio: Arc<fasync::Sigio>,
    /// A task entered seccomp once: from then on, every syscall entry looks at its task's
    /// mode (`TIF_SECCOMP` for everyone, until the first).
    seccomp_used: std::sync::atomic::AtomicBool,
    seccomp_logging: Arc<seccomp::Logging>,
    /// Every key, by serial, and each user's (`key_serial_tree`, `key_user_tree`).
    keys: Arc<keys::Keys>,
    /// Yama's `ptrace_scope` and the `PR_SET_PTRACER` exceptions.
    yama: Arc<yama::Yama>,
    /// The last audit session id handed out (`session_id`).
    audit_sessions: Arc<std::sync::atomic::AtomicU32>,
    /// Who traces whom (`ptrace`).
    tracees: Arc<ptrace::Tracees>,
    /// What hides the order of the resources `kcmp` compares, made at its first use.
    kcmp: std::sync::OnceLock<kcmp::Cookies>,
    // ---- base ----
    /// What [`Kernel::spawn`]ed processes run as.
    credentials: Credentials,
    /// The kernel's own init, pid 1, which [`Kernel::spawn`]ed processes are children of.
    init: Option<TaskId>,
    /// Ctrl-Alt-Del and what `reboot` asked the machine to do.
    reboot: reboot::Reboot,
    // ---- tty ----
    /// Process groups and sessions, as terminals and the kernel signal them.
    jobs: tty::Jobs,
    /// The host's console, if the embedder gave one ([`Kernel::with_console`]).
    console: Option<Arc<tty::Tty>>,
    /// The pseudo-terminals, and their filesystem's nodes.
    ptys: Arc<tty::Ptys>,
}

#[derive(Default)]
struct State {
    last_id: u32,
    tasks: HashMap<TaskId, Task>,
    /// Released leaders keep their credentials and scheduler metadata until reap.
    /// Linux kernel/exit.c:release_task unhashes their task identity there.
    retired: HashMap<TaskId, thread::RetiredTask>,
    /// The tasks of reaped processes whose instances still unwind: out of the table
    /// (`release_task`), until the execution layer lets them go (`put_task_struct`).
    dead: HashMap<TaskId, thread::DeadTask>,
    /// The kernel's [`Kernel::pids`], which a task leaving the table detaches from.
    pids: Arc<pid::Pids>,
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
    /// Retired threads' I/O counts (`signal->ioac`); procfs adds live tasks' counts.
    ioac: IoAccounting,
    /// The auxiliary vector the image started with, as `/proc/<pid>/auxv` shows it
    /// (`mm->saved_auxv`): pairs of words up to `AT_NULL`.
    auxv: Mutex<Vec<u8>>,
    exe_write_access: Mutex<Option<Arc<wasixv2_vfs::ExecAccess>>>,
    // ---- thr ----
    group: thread::Group,
    // ---- base ----
    /// Monotonic time the process started.
    start: std::time::Duration,
    /// The CPU time of the thread group, its dead threads included ([`cputime`]).
    cpu: Arc<GroupCpu>,
    /// What its reaped children used (`cutime`, `cstime`, `cmaxrss`).
    children: Mutex<cputime::Children>,
    /// The most memory one of its tasks left, in KiB (`signal_struct::maxrss`).
    maxrss: std::sync::atomic::AtomicU64,
    /// The timers on its CPU clocks and its threads'.
    cpu_timers: cputime::CpuTimers,
    ident: Mutex<ident::Ident>,
    resources: Mutex<resource::Resources>,
    // ---- ns ----
    /// The pid namespace the process is in (`task_active_pid_ns`), for its whole life.
    pid_ns: Arc<ns::PidNs>,
    /// The IPC namespace whose semaphores the process's adjustments are for (`sysvsem`), until
    /// its last task applied them on its way out (`exit_sem`), so a process that is gone holds
    /// no namespace, as Linux's `exit_task_namespaces` lets it go.
    sysvsem: Mutex<Option<Arc<ns::IpcNs>>>,
}

struct Task {
    /// Objective credentials (kernel/cred.c `commit_creds`), shared until replaced.
    cred: Arc<cred::Cred>,
    /// The active syscall for /proc/<pid>/syscall, cleared on return.
    syscall: entry::CallSlot,
    /// Per-thread I/O counts (`task->ioac`).
    ioac: Arc<IoAccounting>,
    process: Arc<Process>,
    /// The address space the task runs on (`task->mm`).
    mm: Arc<Mm>,
    files: Arc<Mutex<FdTable>>,
    /// Root, working directory and umask (`fs_struct`).
    fs: Arc<Mutex<FsContext>>,
    /// The namespaces the task is in (`nsproxy`), until it is done with its memory on its
    /// way out (`exit_task_namespaces`), so a task that is gone holds none.
    ns: Option<Arc<ns::NsProxy>>,
    /// What the execution layer does on the task's next return to the guest.
    work: work::Work,
    /// `keyctl_session_to_parent` task work, applied on return to userspace.
    session_keyring_work: Option<Arc<keys::Key>>,
    /// How an interrupted call goes on through `restart_syscall` (`restart_block`).
    restart: Option<restart::Restart>,
    /// The task's name (`comm`), which `PR_SET_NAME` and `execve` set.
    comm: ident::Comm,
    /// Policy, priorities, affinity and timer slack: all per thread, as on Linux.
    sched: sched::Sched,
    prctl: ident::TaskPrctl,
    /// Seccomp mode and filters, and `no_new_privs`.
    seccomp: seccomp::Seccomp,
    /// The I/O priority it set (`io_context`), shared with `CLONE_IO`.
    io: Option<Arc<sched::IoContext>>,
    // ---- thr ----
    thread: thread::ThreadState,
    // ---- sig ----
    /// The task's masks and pending signals, and its thread group's dispositions.
    signals: Arc<signal::TaskSignals>,
    // ---- base ----
    /// The CPU time the execution layer charges the task with.
    cpu: Arc<CpuAccount>,
    /// The credentials an io_uring request's personality lends the task while it is issued
    /// (`override_creds`), which its own calls use instead of the process's.
    cred_override: Option<Arc<cred::Cred>>,
    // ---- ipc ----
    /// The rings the task submitted to and its registered ring descriptors
    /// (`task->io_uring`).
    uring: Arc<io_uring::Tctx>,
    // ---- cgroup ----
    /// The task's place in the cgroup hierarchy (`task->cgroups`).
    css: cgroup::Css,
}

/// `struct task_io_accounting`: what the read and write calls moved and how many there were.
/// Every file's data is in memory, so nothing is read from or written to storage
/// (`read_bytes`, `write_bytes`).
#[derive(Default)]
struct IoAccounting {
    rchar: std::sync::atomic::AtomicU64,
    wchar: std::sync::atomic::AtomicU64,
    syscr: std::sync::atomic::AtomicU64,
    syscw: std::sync::atomic::AtomicU64,
}

impl IoAccounting {
    fn account(&self, write: bool, bytes: u32) {
        use std::sync::atomic::Ordering::Relaxed;
        let (chars, calls) = match write {
            true => (&self.wchar, &self.syscw),
            false => (&self.rchar, &self.syscr),
        };
        chars.fetch_add(u64::from(bytes), Relaxed);
        calls.fetch_add(1, Relaxed);
    }

    /// kernel/exit.c `__exit_signal`: retire a task's accounting into its signal group.
    fn add(&self, counts: [u64; 4]) {
        use std::sync::atomic::Ordering::Relaxed;
        for (counter, count) in [&self.rchar, &self.wchar, &self.syscr, &self.syscw]
            .into_iter()
            .zip(counts)
        {
            counter.fetch_add(count, Relaxed);
        }
    }

    /// `rchar`, `wchar`, `syscr`, `syscw`.
    fn counts(&self) -> [u64; 4] {
        use std::sync::atomic::Ordering::Relaxed;
        [&self.rchar, &self.wchar, &self.syscr, &self.syscw].map(|c| c.load(Relaxed))
    }
}

impl Task {
    /// The namespaces of a task that runs, as every syscall's caller does.
    fn live_ns(&self) -> &Arc<ns::NsProxy> {
        self.ns
            .as_ref()
            .expect("a task that runs has its namespaces")
    }
}

impl Kernel {
    /// A kernel whose root filesystem is `root`, with a wall clock of its own over `host`'s.
    /// Further filesystems are mounted through [`Kernel::vfs`].
    pub fn new(host: Arc<dyn Host>, root: Arc<dyn Filesystem>) -> Self {
        Self::with_timekeeper(Timekeeper::new(host), root)
    }

    /// A kernel whose wall clock is `time`'s, which the machine's filesystems also stamp
    /// their files with.
    pub fn with_timekeeper(time: Arc<Timekeeper>, root: Arc<dyn Filesystem>) -> Self {
        let host: Arc<dyn Host> = time.clone();
        let vfs = Vfs::new(root);
        let clock = Arc::new(fs::HostClock(host.clone()));
        let pids = Arc::new(pid::Pids::default());
        let state = Arc::new(TaskTable(RwLock::new(State {
            pids: pids.clone(),
            ..State::default()
        })));
        let procs = process::Procs::new(pids.clone());
        let jobs = tty::Jobs::new(&state, &procs.tree, &pids);
        let timers = timer::Timers::new(host.clone());
        time.on_clock_was_set_delayed(timers.clock_was_set_delayed_hook());
        let sigio = fasync::Sigio::new(jobs.clone(), &timers);
        let keys = keys::Keys::new(host.clone(), &timers);
        let inotify = Arc::new(fs::inotify::Registry::default());
        let dnotify = Arc::new(fs::dnotify::Dnotify::new(sigio.clone()));
        let fanotify = Arc::new(fs::fanotify::Registry::default());
        fanotify.bind(&state, &vfs);
        fs::dnotify::Notify::install(&vfs, inotify.clone(), dnotify.clone(), fanotify.clone());
        let init_user_ns = ns::UserNs::initial_user();
        let binfmt_misc = binfmt_misc::BinfmtMisc::new(
            clock.clone(),
            &Arc::downgrade(&state),
            &Arc::downgrade(&vfs),
        );
        let binfmt_misc = binfmt_misc::Instances::new(binfmt_misc, &init_user_ns);
        let filesystems = fs::mount::Registry::new();
        let fuse_connections = Arc::new(fuse::Registry::default());
        filesystems.add(
            "fusectl",
            Arc::new(fuse::ControlType(fuse_connections.clone())),
        );
        filesystems.add(
            "fuse",
            Arc::new(fuse::FuseType {
                state: Arc::downgrade(&state),
                host: host.clone(),
                registry: fuse_connections,
            }),
        );
        let misc_type = Arc::new(binfmt_misc::BinfmtMiscType(binfmt_misc.clone()));
        filesystems.add("binfmt_misc", misc_type);
        let printk = Arc::new(printk::Printk::new(host.clone()));
        // `start_kernel`'s `linux_banner`.
        let uname = Uname::default();
        let banner = format!("Linux version {} {}", uname.release(), uname.version());
        printk.log(printk::Level::Notice, &banner);
        let cgroups = cgroup::Hierarchy::new(clock.clone(), host.clone(), printk.clone());
        cgroups.start_notify(&timers, &vfs);
        Self {
            printk,
            acct: Arc::default(),
            coredump: Arc::default(),
            tunables: Arc::default(),
            boot_id: {
                let mut id = [0; 16];
                host.random(&mut id);
                procfs::uuid_v4(id)
            },
            ptys: tty::Ptys::new(clock.clone()),
            pseudo: pseudo::Pseudo::new(&vfs, clock),
            nr_open: Arc::default(),
            taint: Arc::default(),
            watch_users: watch_queue::Users::default(),
            pipe_max_size: Arc::default(),
            aio: Arc::default(),
            vm: Arc::default(),
            sigio,
            timers,
            host,
            time,
            state,
            init_ns: Arc::new(ns::NsProxy::initial(
                &init_user_ns,
                vfs.namespace().clone(),
                Uname::default().to_utsname(),
                None,
                cgroups.root().clone(),
            )),
            cgroups,
            init_user_ns,
            ns_inums: Arc::default(),
            network_factory: None,
            inotify,
            dnotify,
            fanotify,
            sysctls: Arc::default(),
            filesystems,
            vsock: Arc::default(),
            shm: None,
            vfs,
            futexes: Arc::default(),
            procs,
            pids,
            binfmt: Arc::new(process::WasmMagic),
            binfmt_misc,
            epoll_nesting: Mutex::default(),
            sigqueue_users: Arc::default(),
            memlock_users: Default::default(),
            seccomp_used: Default::default(),
            keys,
            seccomp_logging: Arc::default(),
            yama: Arc::default(),
            kcmp: std::sync::OnceLock::new(),
            tracees: Default::default(),
            audit_sessions: Arc::default(),
            credentials: Credentials::default(),
            init: None,
            reboot: Default::default(),
            jobs,
            console: None,
        }
    }

    /// Makes [`Kernel::spawn`]ed processes run as `credentials` instead of uid and gid 1.
    pub fn with_credentials(mut self, credentials: Credentials) -> Self {
        self.credentials = credentials;
        self
    }

    /// Makes `uname` report `uname` in the initial UTS namespace.
    pub fn with_uname(self, uname: &Uname) -> Self {
        *self.init_ns.uts.lock_or_recover() = uname.to_utsname();
        self
    }

    /// Gives the initial network namespace `network` for IP sockets, before the first process
    /// starts in it.
    pub fn with_network(mut self, network: Arc<dyn wasixv2_vnet::Network>) -> Self {
        assert!(
            self.state.read_or_recover().tasks.is_empty(),
            "the network comes before the first process"
        );
        let uname = *self.init_ns.uts.lock_or_recover();
        let mnt = self.vfs.namespace().clone();
        self.init_ns = Arc::new(ns::NsProxy::initial(
            &self.init_user_ns,
            mnt,
            uname,
            Some(network),
            self.cgroups.root().clone(),
        ));
        self
    }

    /// Gives each network namespace a process makes (`CLONE_NEWNET`) the network `factory`
    /// makes: Linux starts one with nothing but loopback (`copy_net_ns`). Without a factory, a
    /// new network namespace has no IP network, and IP sockets in it fail with `EAFNOSUPPORT`.
    pub fn with_network_factory<N: wasixv2_vnet::Network + 'static>(
        mut self,
        factory: impl Fn() -> Arc<N> + Send + Sync + 'static,
    ) -> Self {
        self.network_factory = Some(Arc::new(move || -> Arc<dyn wasixv2_vnet::Network> {
            factory()
        }));
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
    /// of root (`init/init_task.c INIT_CRED`). Spawned processes use the configured
    /// credentials. Without an init, spawned processes
    /// have no parent (`getppid` is 0) and orphans no reaper.
    pub fn with_init(mut self) -> Self {
        assert!(
            self.state.read_or_recover().last_id == 0,
            "init is the first process"
        );
        let init = self
            .new_process_as(
                &Credentials::new(0, 0),
                vec!["init".to_string()],
                Vec::new(),
                self.fd_table(),
                0,
            )
            .expect("the first process gets pid 1");
        self.task_signals(init).group().make_init();
        self.init = Some(init);
        self
    }

    /// An empty descriptor table, bounded by the machine's `fs.nr_open`.
    fn fd_table(&self) -> FdTable {
        FdTable::new(self.nr_open.clone())
    }

    /// Starts a process with a single task whose descriptors 0, 1 and 2 are `config.stdio`,
    /// running as the kernel's [`Credentials`], as a child of init when the kernel has one, in
    /// the initial namespaces. `EAGAIN` when the initial pid namespace has no pid left.
    pub fn spawn(&self, config: ProcessConfig) -> Result<TaskId, Errno> {
        let cred = cred::Cred::new(&self.credentials, self.init_user_ns.clone());
        let mut files = self.fd_table();
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
    ) -> Result<TaskId, Errno> {
        self.new_process_as(&self.credentials, args, env, files, parent)
    }

    /// [`Kernel::new_process`] running as `credentials`.
    fn new_process_as(
        &self,
        credentials: &Credentials,
        args: Vec<String>,
        env: Vec<String>,
        files: FdTable,
        parent: u32,
    ) -> Result<TaskId, Errno> {
        let cred = cred::Cred::new(credentials, self.init_user_ns.clone());
        let pid_ns = self.init_ns.pid_for_children.clone();
        let mut state = self.state.lock_or_recover();
        let id = TaskId(state.last_id + 1);
        state.pids.alloc(id.0, &pid_ns, &[], |_| false)?;
        state.last_id += 1;
        let name = args.first().map(String::as_bytes).unwrap_or_default();
        let comm = ident::Comm::from_path(name);
        let process = Arc::new(Process {
            pid: id,
            start: self.host.monotonic(),
            cpu: Arc::default(),
            children: Mutex::default(),
            maxrss: Default::default(),
            cpu_timers: Default::default(),
            ident: Mutex::new(ident::Ident::new(id.0)),
            resources: Mutex::default(),
            pid_ns,
            sysvsem: Mutex::new(Some(self.init_ns.ipc.clone())),
            args: Mutex::new(args.into_iter().map(String::into_bytes).collect()),
            env: Mutex::new(env.into_iter().map(String::into_bytes).collect()),
            exe: Mutex::new(None),
            auxv: Mutex::default(),
            ioac: IoAccounting::default(),
            exe_write_access: Mutex::new(None),
            group: thread::Group::new(),
        });
        let flag = Arc::new(WorkFlag::default());
        let account = signal::Account::new(self.sigqueue_users.clone(), &process);
        let cpu = Arc::new(CpuAccount::new(process.cpu.clone()));
        // The root cgroup has no `pids.max`.
        let css = self
            .cgroups
            .fork(self.cgroups.root().clone(), cpu.clone(), false)
            .expect("the root cgroup takes every task");
        let task = Task {
            cred: Arc::new(cred),
            css,
            syscall: Default::default(),
            ioac: Default::default(),
            comm,
            sched: sched::Sched::default(),
            prctl: ident::TaskPrctl::default(),
            seccomp: seccomp::Seccomp::default(),
            io: None,
            process,
            mm: Arc::default(),
            files: Arc::new(Mutex::new(files)),
            fs: Arc::new(Mutex::new(FsContext::new(self.vfs.root()))),
            ns: Some(self.init_ns.clone()),
            work: work::Work::new(flag.clone()),
            restart: None,
            session_keyring_work: None,
            thread: thread::ThreadState::default(),
            signals: signal::Signals::new(Some(account)).attach(
                id,
                signal::Inherited::none(),
                flag,
            ),
            cpu,
            cred_override: None,
            uring: Arc::default(),
        };
        let process = task.process.clone();
        task.signals.publish_cred(task.cred.clone());
        state.tasks.insert(id, task);
        drop(state);
        // ---- proc ----
        self.procs.add(process, parent, Signal::SIGCHLD.0, false);
        Ok(id)
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
    fn startup_size<'a>(&'a self, caller: &'a Caller<'_>) -> MaybeFuture<'a, SysResult<u32>> {
        MaybeFuture::Future(Box::pin(async move {
            let process = self.process(caller.task());
            // The task table precedes argument locks, as in copy_process.
            let identity = self.identity(caller.task());
            let aux = process.exec_aux();
            let (args, env) = (
                process.args.lock_or_recover(),
                process.env.lock_or_recover(),
            );
            Ok(Startup::new(&args, &env, identity, aux).size()?)
        }))
    }

    fn startup_get<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        buf: UserSliceMut,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            if !buf.addr.is_multiple_of(16) {
                return Err(Errno::EINVAL.into());
            }
            let process = self.process(caller.task());
            let (args, env) = (
                process.args.lock_or_recover().clone(),
                process.env.lock_or_recover().clone(),
            );
            let startup = Startup::new(
                &args,
                &env,
                self.identity(caller.task()),
                process.exec_aux(),
            );
            // The address space starts out as the image left it.
            self.mem_size(caller).await;
            if buf.len < startup.size()? {
                return Err(Errno::EINVAL.into());
            }
            let mut random = [0; 16];
            self.host.random(&mut random);
            let block = startup.build(buf.addr, random)?;
            buf.copy_out(caller.memory(), &block).await?;
            *process.auxv.lock_or_recover() = startup.auxv(&block);
            Ok(0)
        }))
    }

    // ---- fs ----
    fn openat<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        flags: OpenFlags,
        mode: Mode,
    ) -> MaybeFuture<'a, SysResult<Fd>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_openat(caller, dfd, filename, flags, mode).await
        }))
    }

    fn openat2<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        how: ConstOpenHowPtr,
        usize: Usize,
    ) -> MaybeFuture<'a, SysResult<Fd>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_openat2(caller, dfd, filename, how, usize).await
        }))
    }

    fn close<'a>(&'a self, caller: &'a Caller<'_>, fd: Fd) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move { self.sys_close(caller, fd).await }))
    }

    fn close_range<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: u32,
        max_fd: u32,
        flags: CloseRangeFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_close_range(caller, fd, max_fd, flags).await
        }))
    }

    fn dup<'a>(&'a self, caller: &'a Caller<'_>, fildes: Fd) -> MaybeFuture<'a, SysResult<Fd>> {
        MaybeFuture::Future(Box::pin(async move { self.sys_dup(caller, fildes) }))
    }

    fn dup3<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        oldfd: Fd,
        newfd: Fd,
        flags: OpenFlags,
    ) -> MaybeFuture<'a, SysResult<Fd>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_dup3(caller, oldfd, newfd, flags).await
        }))
    }

    fn fcntl64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        cmd: FcntlCmd,
        arg: Ulong,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_fcntl(caller, fd, cmd, arg).await
        }))
    }

    fn flock<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        cmd: FlockOp,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_flock(caller, fd, cmd).await },
        ))
    }

    fn readv<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        vec: ConstIovecArray,
        vlen: Ulong,
    ) -> MaybeFuture<'a, SysResult<u32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_readv(caller, fd, vec, vlen).await
        }))
    }

    fn writev<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        vec: ConstIovecArray,
        vlen: Ulong,
    ) -> MaybeFuture<'a, SysResult<u32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_writev(caller, fd, vec, vlen).await
        }))
    }

    fn pread64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        buf: UserSliceMut,
        pos: Loff,
    ) -> MaybeFuture<'a, SysResult<u32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_pread(caller, fd, buf, pos).await
        }))
    }

    fn pwrite64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        buf: UserSlice,
        pos: Loff,
    ) -> MaybeFuture<'a, SysResult<u32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_pwrite(caller, fd, buf, pos).await
        }))
    }

    fn preadv<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        vec: ConstIovecArray,
        vlen: Ulong,
        pos: i64,
    ) -> MaybeFuture<'a, SysResult<u32>> {
        MaybeFuture::Future(Box::pin(async move {
            // `do_preadv`: only preadv2 gives -1 a meaning.
            if pos < 0 {
                return Err(Errno::EINVAL.into());
            }
            self.sys_preadv2(caller, fd, vec, vlen, pos, RwfFlags(0))
                .await
        }))
    }

    fn pwritev<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        vec: ConstIovecArray,
        vlen: Ulong,
        pos: i64,
    ) -> MaybeFuture<'a, SysResult<u32>> {
        MaybeFuture::Future(Box::pin(async move {
            if pos < 0 {
                return Err(Errno::EINVAL.into());
            }
            self.sys_pwritev2(caller, fd, vec, vlen, pos, RwfFlags(0))
                .await
        }))
    }

    fn preadv2<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        vec: ConstIovecArray,
        vlen: Ulong,
        pos: i64,
        flags: RwfFlags,
    ) -> MaybeFuture<'a, SysResult<u32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_preadv2(caller, fd, vec, vlen, pos, flags).await
        }))
    }

    fn pwritev2<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        vec: ConstIovecArray,
        vlen: Ulong,
        pos: i64,
        flags: RwfFlags,
    ) -> MaybeFuture<'a, SysResult<u32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_pwritev2(caller, fd, vec, vlen, pos, flags).await
        }))
    }

    fn lseek<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        offset: Loff,
        whence: SeekWhence,
    ) -> MaybeFuture<'a, SysResult<i64>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_lseek(caller, fd, offset, whence).await
        }))
    }

    fn sendfile64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        out_fd: Fd,
        in_fd: Fd,
        offset: LoffPtr,
        count: Usize,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_sendfile(caller, out_fd, in_fd, offset, count)
                .await
        }))
    }

    fn splice<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd_in: Fd,
        off_in: LoffPtr,
        fd_out: Fd,
        off_out: LoffPtr,
        len: Usize,
        flags: SpliceFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_splice(caller, fd_in, off_in, fd_out, off_out, len, flags)
                .await
        }))
    }

    fn tee<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fdin: Fd,
        fdout: Fd,
        len: Usize,
        flags: SpliceFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_tee(caller, fdin, fdout, len, flags).await
        }))
    }

    fn vmsplice<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        uiov: ConstIovecArray,
        nr_segs: Ulong,
        flags: SpliceFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_vmsplice(caller, fd, uiov, nr_segs, flags).await
        }))
    }

    fn copy_file_range<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd_in: Fd,
        off_in: LoffPtr,
        fd_out: Fd,
        off_out: LoffPtr,
        len: Usize,
        flags: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_copy_file_range(caller, fd_in, off_in, fd_out, off_out, len, flags)
                .await
        }))
    }

    fn truncate64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        path: Cstr,
        length: Loff,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_truncate(caller, path, length).await
        }))
    }

    fn ftruncate64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        length: Loff,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_ftruncate(caller, fd, length).await
        }))
    }

    fn fallocate<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        mode: FallocFlags,
        offset: Loff,
        len: Loff,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_fallocate(caller, fd, mode, offset, len).await
        }))
    }

    fn fadvise64_64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        _offset: Loff,
        len: Loff,
        advice: FadviseAdvice,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_fadvise(caller, fd, len, advice)
        }))
    }

    fn readahead<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        _offset: Loff,
        _count: Usize,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move { self.sys_readahead(caller, fd) }))
    }

    fn sync<'a>(&'a self, _: &'a Caller<'_>) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move { self.sys_sync().await }))
    }

    fn fsync<'a>(&'a self, caller: &'a Caller<'_>, fd: Fd) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_fsync(caller, fd, false).await },
        ))
    }

    fn fdatasync<'a>(&'a self, caller: &'a Caller<'_>, fd: Fd) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_fsync(caller, fd, true).await },
        ))
    }

    fn syncfs<'a>(&'a self, caller: &'a Caller<'_>, fd: Fd) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move { self.sys_syncfs(caller, fd).await }))
    }

    fn sync_file_range<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        offset: Loff,
        nbytes: Loff,
        flags: SyncFileRangeFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_sync_file_range(caller, fd, offset, nbytes, flags)
                .await
        }))
    }

    fn getdents64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        dirent: LinuxDirent64Ptr,
        count: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_getdents64(caller, fd, dirent, count).await
        }))
    }

    fn mkdirat<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        dfd: Fd,
        pathname: Cstr,
        mode: Mode,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_mkdirat(caller, dfd, pathname, mode).await
        }))
    }

    fn mknodat<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        mode: Mode,
        dev: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_mknodat(caller, dfd, filename, mode, dev).await
        }))
    }

    fn unlinkat<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        dfd: Fd,
        pathname: Cstr,
        flag: AtFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_unlinkat(caller, dfd, pathname, flag).await
        }))
    }

    fn symlinkat<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        oldname: Cstr,
        newdfd: Fd,
        newname: Cstr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_symlinkat(caller, oldname, newdfd, newname).await
        }))
    }

    fn linkat<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        olddfd: Fd,
        oldname: Cstr,
        newdfd: Fd,
        newname: Cstr,
        flags: AtFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_linkat(caller, olddfd, oldname, newdfd, newname, flags)
                .await
        }))
    }

    fn renameat2<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        olddfd: Fd,
        oldname: Cstr,
        newdfd: Fd,
        newname: Cstr,
        flags: RenameFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_renameat2(caller, olddfd, oldname, newdfd, newname, flags)
                .await
        }))
    }

    fn readlinkat<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        dfd: Fd,
        pathname: Cstr,
        buf: Buf,
        bufsiz: i32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_readlinkat(caller, dfd, pathname, buf, bufsiz)
                .await
        }))
    }

    fn faccessat<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        mode: AccessMode,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_faccessat2(caller, dfd, filename, mode, AtFlags(0))
                .await
        }))
    }

    fn faccessat2<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        mode: AccessMode,
        flags: AtFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_faccessat2(caller, dfd, filename, mode, flags)
                .await
        }))
    }

    fn chdir<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        filename: Cstr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_chdir(caller, filename).await },
        ))
    }

    fn fchdir<'a>(&'a self, caller: &'a Caller<'_>, fd: Fd) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move { self.sys_fchdir(caller, fd).await }))
    }

    fn fchroot<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        flags: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_fchroot(caller, fd, flags).await
        }))
    }

    fn chroot<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        filename: Cstr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_chroot(caller, filename).await },
        ))
    }

    fn getcwd<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        buf: UserSliceMut,
    ) -> MaybeFuture<'a, SysResult<u32>> {
        MaybeFuture::Future(Box::pin(async move { self.sys_getcwd(caller, buf).await }))
    }

    fn umask<'a>(&'a self, caller: &'a Caller<'_>, mask: i32) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move { self.sys_umask(caller, mask) }))
    }

    fn fchmod<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        mode: Mode,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_fchmod(caller, fd, mode).await },
        ))
    }

    fn fchmodat<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        mode: Mode,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_fchmodat2(caller, dfd, filename, mode, AtFlags(0))
                .await
        }))
    }

    fn fchmodat2<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        mode: Mode,
        flags: AtFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_fchmodat2(caller, dfd, filename, mode, flags).await
        }))
    }

    fn fchown<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        user: Uid,
        group: Gid,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_fchown(caller, fd, user, group).await
        }))
    }

    fn fchownat<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        user: Uid,
        group: Gid,
        flag: AtFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_fchownat(caller, dfd, filename, user, group, flag)
                .await
        }))
    }

    fn statx<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        flags: AtFlags,
        mask: StatxMask,
        buffer: StatxPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_statx(caller, dfd, filename, flags, mask, buffer)
                .await
        }))
    }

    fn statfs64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pathname: Cstr,
        sz: Usize,
        buf: Statfs64Ptr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_statfs(caller, pathname, sz, buf).await
        }))
    }

    fn fstatfs64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        sz: Usize,
        buf: Statfs64Ptr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_fstatfs(caller, fd, sz, buf).await
        }))
    }

    fn utimensat_time64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        utimes: ConstKernelTimespecArray,
        flags: AtFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_utimensat(caller, dfd, filename, utimes, flags)
                .await
        }))
    }

    fn fanotify_init<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        flags: u32,
        event_f_flags: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_fanotify_init(caller, flags, event_f_flags)
        }))
    }

    fn fanotify_mark<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fanotify_fd: Fd,
        flags: u32,
        mask: u64,
        dfd: Fd,
        pathname: Cstr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_fanotify_mark(caller, fanotify_fd, flags, mask, dfd, pathname)
                .await
        }))
    }

    fn file_getattr<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        ufattr: wasixv2_abi::FileAttrPtr,
        usize: Usize,
        at_flags: AtFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_file_getattr(caller, dfd, filename, ufattr, usize, at_flags)
                .await
        }))
    }

    fn file_setattr<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        ufattr: wasixv2_abi::ConstFileAttrPtr,
        usize: Usize,
        at_flags: AtFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_file_setattr(caller, dfd, filename, ufattr, usize, at_flags)
                .await
        }))
    }

    fn name_to_handle_at<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        dfd: Fd,
        name: Cstr,
        handle: FileHandlePtr,
        mnt_id: S32Ptr,
        flag: i32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_name_to_handle_at(caller, dfd, name, handle, mnt_id, flag)
                .await
        }))
    }

    fn open_by_handle_at<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        mountdirfd: Fd,
        handle: ConstFileHandlePtr,
        flags: OpenFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_open_by_handle_at(caller, mountdirfd, handle, flags)
                .await
        }))
    }

    fn quotactl<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        cmd: u32,
        special: Cstr,
        id: wasixv2_abi::Qid,
        addr: VoidPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_quotactl(caller, cmd, special, id, addr).await
        }))
    }

    fn quotactl_fd<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        cmd: u32,
        id: wasixv2_abi::Qid,
        addr: VoidPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_quotactl_fd(caller, fd, cmd, id, addr).await
        }))
    }

    fn setxattr<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pathname: Cstr,
        name: Cstr,
        value: UserSlice,
        flags: XattrFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            let target = self.xattr_path(caller, pathname, true).await?;
            self.xattr_set(caller, target, name, value, flags).await
        }))
    }

    fn lsetxattr<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pathname: Cstr,
        name: Cstr,
        value: UserSlice,
        flags: XattrFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            let target = self.xattr_path(caller, pathname, false).await?;
            self.xattr_set(caller, target, name, value, flags).await
        }))
    }

    fn fsetxattr<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        name: Cstr,
        value: UserSlice,
        flags: XattrFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            let target = self.xattr_of_fd(caller, fd)?;
            self.xattr_set(caller, target, name, value, flags).await
        }))
    }

    fn getxattr<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pathname: Cstr,
        name: Cstr,
        value: UserSliceMut,
    ) -> MaybeFuture<'a, SysResult<u32>> {
        MaybeFuture::Future(Box::pin(async move {
            let target = self.xattr_path(caller, pathname, true).await?;
            self.xattr_get(caller, target, name, value).await
        }))
    }

    fn lgetxattr<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pathname: Cstr,
        name: Cstr,
        value: UserSliceMut,
    ) -> MaybeFuture<'a, SysResult<u32>> {
        MaybeFuture::Future(Box::pin(async move {
            let target = self.xattr_path(caller, pathname, false).await?;
            self.xattr_get(caller, target, name, value).await
        }))
    }

    fn fgetxattr<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        name: Cstr,
        value: UserSliceMut,
    ) -> MaybeFuture<'a, SysResult<u32>> {
        MaybeFuture::Future(Box::pin(async move {
            let target = self.xattr_of_fd(caller, fd)?;
            self.xattr_get(caller, target, name, value).await
        }))
    }

    fn listxattr<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pathname: Cstr,
        list: UserSliceMut,
    ) -> MaybeFuture<'a, SysResult<u32>> {
        MaybeFuture::Future(Box::pin(async move {
            let target = self.xattr_path(caller, pathname, true).await?;
            self.xattr_list(caller, target, list).await
        }))
    }

    fn llistxattr<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pathname: Cstr,
        list: UserSliceMut,
    ) -> MaybeFuture<'a, SysResult<u32>> {
        MaybeFuture::Future(Box::pin(async move {
            let target = self.xattr_path(caller, pathname, false).await?;
            self.xattr_list(caller, target, list).await
        }))
    }

    fn flistxattr<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        list: UserSliceMut,
    ) -> MaybeFuture<'a, SysResult<u32>> {
        MaybeFuture::Future(Box::pin(async move {
            let target = self.xattr_of_fd(caller, fd)?;
            self.xattr_list(caller, target, list).await
        }))
    }

    fn removexattr<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pathname: Cstr,
        name: Cstr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            let target = self.xattr_path(caller, pathname, true).await?;
            self.xattr_remove(caller, target, name).await
        }))
    }

    fn lremovexattr<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pathname: Cstr,
        name: Cstr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            let target = self.xattr_path(caller, pathname, false).await?;
            self.xattr_remove(caller, target, name).await
        }))
    }

    fn fremovexattr<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        name: Cstr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            let target = self.xattr_of_fd(caller, fd)?;
            self.xattr_remove(caller, target, name).await
        }))
    }

    fn setxattrat<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        dfd: Fd,
        pathname: Cstr,
        at_flags: AtFlags,
        name: Cstr,
        uargs: ConstXattrArgsPtr,
        usize: Usize,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_setxattrat(caller, dfd, pathname, at_flags, name, uargs, usize)
                .await
        }))
    }

    fn getxattrat<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        dfd: Fd,
        pathname: Cstr,
        at_flags: AtFlags,
        name: Cstr,
        uargs: XattrArgsPtr,
        usize: Usize,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_getxattrat(caller, dfd, pathname, at_flags, name, uargs, usize)
                .await
        }))
    }

    fn listxattrat<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        dfd: Fd,
        pathname: Cstr,
        at_flags: AtFlags,
        list: UserSliceMut,
    ) -> MaybeFuture<'a, SysResult<u32>> {
        MaybeFuture::Future(Box::pin(async move {
            let target = self.xattr_at(caller, dfd, pathname, at_flags).await?;
            self.xattr_list(caller, target, list).await
        }))
    }

    fn removexattrat<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        dfd: Fd,
        pathname: Cstr,
        at_flags: AtFlags,
        name: Cstr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            let target = self.xattr_at(caller, dfd, pathname, at_flags).await?;
            self.xattr_remove(caller, target, name).await
        }))
    }

    fn inotify_init1<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        flags: wasixv2_abi::InotifyInitFlags,
    ) -> MaybeFuture<'a, SysResult<Fd>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_inotify_init1(caller, flags) },
        ))
    }

    fn inotify_add_watch<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        pathname: Cstr,
        mask: wasixv2_abi::InotifyMask,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_inotify_add_watch(caller, fd, pathname, mask).await
        }))
    }

    fn inotify_rm_watch<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        wd: i32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_inotify_rm_watch(caller, fd, wd)
        }))
    }

    fn ioctl<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        request: IoctlRequest,
        arg: Ulong,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_ioctl(caller, fd, request, arg).await
        }))
    }

    fn read<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        buf: UserSliceMut,
    ) -> MaybeFuture<'a, SysResult<u32>> {
        MaybeFuture::Future(Box::pin(async move {
            let file = self.file(caller.task(), fd)?;
            file.check_readable()?;
            self.read_buf(caller, &file, buf, fs::At::Position).await
        }))
    }

    fn write<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        buf: UserSlice,
    ) -> MaybeFuture<'a, SysResult<u32>> {
        MaybeFuture::Future(Box::pin(async move {
            let file = self.file(caller.task(), fd)?;
            file.check_writable()?;
            self.write_buf(caller, &file, buf, fs::At::Position).await
        }))
    }

    // ---- thr ----
    fn thread_spawn<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        flags: Ulong,
        start_arg: Uptr,
        parent_tid: S32Ptr,
        tls: Uptr,
        child_tid: S32Ptr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_thread_spawn(caller, flags, start_arg, parent_tid, tls, child_tid)
                .await
        }))
    }

    fn exit<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        error_code: i32,
    ) -> MaybeFuture<'a, SysResult<()>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_exit(caller, error_code).await },
        ))
    }

    fn exit_group<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        error_code: i32,
    ) -> MaybeFuture<'a, SysResult<()>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_exit_group(caller, error_code).await
        }))
    }

    fn set_tid_address<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        tidptr: S32Ptr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            Ok(self.sys_set_tid_address(caller, tidptr))
        }))
    }

    fn set_robust_list<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        head: RobustListHeadPtr,
        len: Usize,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_set_robust_list(caller, head, len)
        }))
    }

    fn get_robust_list<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pid: i32,
        head_ptr: RobustListHeadPtrPtr,
        len_ptr: UsizePtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_get_robust_list(caller, pid, head_ptr, len_ptr)
                .await
        }))
    }

    #[allow(clippy::too_many_arguments)]
    fn futex_time64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        uaddr: U32Ptr,
        op: FutexOp,
        val: u32,
        utime: Ulong,
        uaddr2: U32Ptr,
        val3: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_futex(caller, uaddr, op, val, utime, uaddr2, val3)
                .await
        }))
    }

    fn futex_waitv<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        waiters: wasixv2_abi::ConstFutexWaitvArray,
        nr_futexes: u32,
        flags: u32,
        timeout: KernelTimespecPtr,
        clockid: wasixv2_abi::ClockId,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_futex_waitv(caller, waiters, nr_futexes, flags, timeout, clockid)
                .await
        }))
    }

    fn futex_wake<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        uaddr: U32Ptr,
        mask: Ulong,
        nr: i32,
        flags: wasixv2_abi::Futex2Flags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_futex_wake(caller, uaddr, mask, nr, flags).await
        }))
    }

    #[allow(clippy::too_many_arguments)]
    fn futex_wait<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        uaddr: U32Ptr,
        val: Ulong,
        mask: Ulong,
        flags: wasixv2_abi::Futex2Flags,
        timeout: KernelTimespecPtr,
        clockid: wasixv2_abi::ClockId,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_futex_wait(caller, uaddr, val, mask, flags, timeout, clockid)
                .await
        }))
    }

    fn futex_requeue<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        waiters: wasixv2_abi::ConstFutexWaitvArray,
        flags: u32,
        nr_wake: i32,
        nr_requeue: i32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_futex_requeue(caller, waiters, flags, nr_wake, nr_requeue)
                .await
        }))
    }
    // ---- end thr ----

    // ---- proc ----
    fn clone<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        clone_flags: Ulong,
        newsp: Uptr,
        parent_tidptr: S32Ptr,
        tls: Uptr,
        child_tidptr: S32Ptr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_clone(caller, clone_flags, newsp, parent_tidptr, tls, child_tidptr)
                .await
        }))
    }

    fn clone3<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        uargs: wasixv2_abi::ConstCloneArgsPtr,
        size: Usize,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_clone3(caller, uargs, size).await
        }))
    }

    fn unshare<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        unshare_flags: Ulong,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_unshare(caller, unshare_flags) },
        ))
    }

    fn acct<'a>(&'a self, caller: &'a Caller<'_>, name: Cstr) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move { self.sys_acct(caller, name).await }))
    }

    fn syslog<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        r#type: i32,
        buf: Buf,
        len: i32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_syslog(caller, r#type, buf, len).await
        }))
    }

    fn mount<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        dev_name: Cstr,
        dir_name: Cstr,
        r#type: Cstr,
        flags: wasixv2_abi::MountFlags,
        data: wasixv2_abi::ConstVoidPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_mount(caller, dev_name, dir_name, r#type, flags, data)
                .await
        }))
    }

    fn umount2<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        name: Cstr,
        flags: wasixv2_abi::UmountFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_umount2(caller, name, flags).await
        }))
    }

    fn pivot_root<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        new_root: Cstr,
        put_old: Cstr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_pivot_root(caller, new_root, put_old).await
        }))
    }

    fn mount_setattr<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        dfd: Fd,
        path: Cstr,
        flags: AtFlags,
        uattr: wasixv2_abi::ConstMountAttrPtr,
        usize: Usize,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_mount_setattr(caller, dfd, path, flags, uattr, usize)
                .await
        }))
    }

    fn open_tree_attr<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        flags: u32,
        uattr: wasixv2_abi::ConstMountAttrPtr,
        usize: Usize,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_open_tree_attr(caller, dfd, filename, flags, uattr, usize)
                .await
        }))
    }

    fn listns<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        req: wasixv2_abi::ConstNsIdReqPtr,
        ns_ids: wasixv2_abi::U64Array,
        nr_ns_ids: Usize,
        flags: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_listns(caller, req, ns_ids, nr_ns_ids, flags).await
        }))
    }

    fn open_tree<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        flags: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_open_tree(caller, dfd, filename, flags).await
        }))
    }

    fn move_mount<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        from_dfd: Fd,
        from_pathname: Cstr,
        to_dfd: Fd,
        to_pathname: Cstr,
        flags: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_move_mount(caller, from_dfd, from_pathname, to_dfd, to_pathname, flags)
                .await
        }))
    }

    fn fsopen<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fs_name: Cstr,
        flags: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_fsopen(caller, fs_name, flags).await
        }))
    }

    fn fsconfig<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        cmd: u32,
        key: Cstr,
        value: wasixv2_abi::ConstVoidPtr,
        aux: i32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_fsconfig(caller, fd, cmd, key, value, aux).await
        }))
    }

    fn fsmount<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fs_fd: Fd,
        flags: u32,
        attr_flags: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_fsmount(caller, fs_fd, flags, attr_flags).await
        }))
    }

    fn fspick<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        dfd: Fd,
        path: Cstr,
        flags: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_fspick(caller, dfd, path, flags).await
        }))
    }

    fn statmount<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        req: wasixv2_abi::ConstMntIdReqPtr,
        buf: wasixv2_abi::VoidPtr,
        bufsize: Usize,
        flags: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_statmount(caller, req, buf, bufsize, flags).await
        }))
    }

    fn listmount<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        req: wasixv2_abi::ConstMntIdReqPtr,
        mnt_ids: wasixv2_abi::U64Array,
        nr_mnt_ids: Usize,
        flags: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_listmount(caller, req, mnt_ids, nr_mnt_ids, flags)
                .await
        }))
    }

    fn setns<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        flags: i32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move { self.sys_setns(caller, fd, flags) }))
    }

    fn execve<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        filename: Cstr,
        argv: wasixv2_abi::CstrArray,
        envp: wasixv2_abi::CstrArray,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            let cwd = Fd(wasixv2_abi::AtFd::AT_FDCWD.0);
            self.sys_execveat(caller, cwd, filename, argv, envp, AtFlags(0))
                .await
        }))
    }

    fn execveat<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        filename: Cstr,
        argv: wasixv2_abi::CstrArray,
        envp: wasixv2_abi::CstrArray,
        flags: AtFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_execveat(caller, fd, filename, argv, envp, flags)
                .await
        }))
    }

    fn waitid<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        which: wasixv2_abi::Idtype,
        upid: Pid,
        infop: wasixv2_abi::SiginfoPtr,
        options: wasixv2_abi::WaitOptions,
        ru: RusagePtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_waitid(caller, which, upid, infop, options, ru)
                .await
        }))
    }

    fn ptrace<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        request: wasixv2_abi::PtraceRequest,
        pid: Pid,
        addr: Uptr,
        data: Ulong,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_ptrace(caller, request, pid, addr, data).await
        }))
    }

    fn kcmp<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pid1: Pid,
        pid2: Pid,
        r#type: i32,
        idx1: Ulong,
        idx2: Ulong,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_kcmp(caller, pid1, pid2, r#type, idx1, idx2).await
        }))
    }

    fn pidfd_getfd<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pidfd: Fd,
        fd: Fd,
        flags: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_pidfd_getfd(caller, pidfd, fd, flags)
        }))
    }

    fn pidfd_open<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pid: Pid,
        flags: u32,
    ) -> MaybeFuture<'a, SysResult<Fd>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_pidfd_open(caller, pid, flags) },
        ))
    }

    fn pidfd_send_signal<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pidfd: Fd,
        sig: Signal,
        info: wasixv2_abi::ConstSiginfoPtr,
        flags: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_pidfd_send_signal(caller, pidfd, sig, info, flags)
                .await
        }))
    }
    // ---- end proc ----

    /// `task_tgid_vnr(current)`: the leader is in the caller's own pid namespace.
    fn getpid<'a>(&'a self, caller: &'a Caller<'_>) -> MaybeFuture<'a, SysResult<Pid>> {
        MaybeFuture::Ready(Ok(self.pids.own_nr(self.process(caller.task()).pid.0)))
    }

    fn gettid<'a>(&'a self, caller: &'a Caller<'_>) -> MaybeFuture<'a, SysResult<Pid>> {
        MaybeFuture::Ready(Ok(self.own_tid(caller.task())))
    }

    // ---- sig ----

    fn signal_callback_register<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        callback: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_signal_callback_register(caller, callback).await
        }))
    }

    fn rt_sigaction<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        sig: Signal,
        act: wasixv2_abi::ConstSigactionPtr,
        oact: wasixv2_abi::SigactionPtr,
        sigsetsize: Usize,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_rt_sigaction(caller, sig, act, oact, sigsetsize)
                .await
        }))
    }

    fn rt_sigprocmask<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        how: SigprocmaskHow,
        nset: ConstSigsetPtr,
        oset: SigsetPtr,
        sigsetsize: Usize,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_rt_sigprocmask(caller, how, nset, oset, sigsetsize)
                .await
        }))
    }

    fn rt_sigpending<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        uset: SigsetPtr,
        sigsetsize: Usize,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_rt_sigpending(caller, uset, sigsetsize).await
        }))
    }

    fn rt_sigsuspend<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        unewset: ConstSigsetPtr,
        sigsetsize: Usize,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_rt_sigsuspend(caller, unewset, sigsetsize).await
        }))
    }

    fn rt_sigtimedwait_time64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        uthese: ConstSigsetPtr,
        uinfo: wasixv2_abi::SiginfoPtr,
        uts: ConstKernelTimespecPtr,
        sigsetsize: Usize,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_rt_sigtimedwait(caller, uthese, uinfo, uts, sigsetsize)
                .await
        }))
    }

    fn rt_sigqueueinfo<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pid: Pid,
        sig: Signal,
        uinfo: wasixv2_abi::ConstSiginfoPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_rt_sigqueueinfo(caller, pid, None, sig, uinfo)
                .await
        }))
    }

    fn rt_tgsigqueueinfo<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        tgid: Pid,
        pid: Pid,
        sig: Signal,
        uinfo: wasixv2_abi::ConstSiginfoPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_rt_sigqueueinfo(caller, tgid, Some(pid), sig, uinfo)
                .await
        }))
    }

    fn sigaltstack<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        uss: wasixv2_abi::ConstSigaltstackPtr,
        uoss: wasixv2_abi::SigaltstackPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_sigaltstack(caller, uss, uoss).await
        }))
    }

    fn kill<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pid: Pid,
        sig: Signal,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_kill(caller, pid, sig).await },
        ))
    }

    fn tkill<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pid: Pid,
        sig: Signal,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_tgkill(caller, None, pid, sig).await
        }))
    }

    fn tgkill<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        tgid: Pid,
        pid: Pid,
        sig: Signal,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_tgkill(caller, Some(tgid), pid, sig).await
        }))
    }

    fn setitimer<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        which: wasixv2_abi::ItimerWhich,
        value: wasixv2_abi::ConstKernelOldItimervalPtr,
        ovalue: wasixv2_abi::KernelOldItimervalPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_setitimer(caller, which, value, ovalue).await
        }))
    }

    fn getitimer<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        which: wasixv2_abi::ItimerWhich,
        value: wasixv2_abi::KernelOldItimervalPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_getitimer(caller, which, value).await
        }))
    }

    fn timer_create<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        which_clock: ClockId,
        timer_event_spec: wasixv2_abi::ConstSigeventPtr,
        created_timer_id: wasixv2_abi::TimerIdPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_timer_create(caller, which_clock, timer_event_spec, created_timer_id)
                .await
        }))
    }

    fn timer_settime64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        timer_id: wasixv2_abi::TimerId,
        flags: TimerFlags,
        new_setting: ConstKernelItimerspecPtr,
        old_setting: KernelItimerspecPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_timer_settime(caller, timer_id, flags, new_setting, old_setting)
                .await
        }))
    }

    fn timer_gettime64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        timer_id: wasixv2_abi::TimerId,
        setting: KernelItimerspecPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_timer_gettime(caller, timer_id, setting).await
        }))
    }

    fn timer_getoverrun<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        timer_id: wasixv2_abi::TimerId,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_timer_getoverrun(caller, timer_id)
        }))
    }

    fn timer_delete<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        timer_id: wasixv2_abi::TimerId,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_timer_delete(caller, timer_id) },
        ))
    }

    // ---- base ----
    fn getppid<'a>(&'a self, caller: &'a Caller<'_>) -> MaybeFuture<'a, SysResult<Pid>> {
        MaybeFuture::Future(Box::pin(async move { Ok(self.sys_getppid(caller)) }))
    }
    fn getuid<'a>(&'a self, caller: &'a Caller<'_>) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            Ok(self.sys_getid(caller, IdKind::User, false))
        }))
    }
    fn geteuid<'a>(&'a self, caller: &'a Caller<'_>) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            Ok(self.sys_getid(caller, IdKind::User, true))
        }))
    }
    fn getgid<'a>(&'a self, caller: &'a Caller<'_>) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            Ok(self.sys_getid(caller, IdKind::Group, false))
        }))
    }
    fn getegid<'a>(&'a self, caller: &'a Caller<'_>) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            Ok(self.sys_getid(caller, IdKind::Group, true))
        }))
    }
    fn setuid<'a>(&'a self, caller: &'a Caller<'_>, uid: Uid) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_set_ids(caller, IdKind::User, IdChange::Id(uid))
        }))
    }
    fn setgid<'a>(&'a self, caller: &'a Caller<'_>, gid: Gid) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_set_ids(caller, IdKind::Group, IdChange::Id(gid))
        }))
    }
    fn setreuid<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        ruid: Uid,
        euid: Uid,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_set_ids(caller, IdKind::User, IdChange::Re(given(ruid), given(euid)))
        }))
    }
    fn setregid<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        rgid: Gid,
        egid: Gid,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_set_ids(
                caller,
                IdKind::Group,
                IdChange::Re(given(rgid), given(egid)),
            )
        }))
    }
    fn setresuid<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        r: Uid,
        e: Uid,
        s: Uid,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_set_ids(
                caller,
                IdKind::User,
                IdChange::Res(given(r), given(e), given(s)),
            )
        }))
    }
    fn setresgid<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        r: Gid,
        e: Gid,
        s: Gid,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_set_ids(
                caller,
                IdKind::Group,
                IdChange::Res(given(r), given(e), given(s)),
            )
        }))
    }
    fn getresuid<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        r: UidPtr,
        e: UidPtr,
        s: UidPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_getresid(caller, IdKind::User, [r, e, s]).await
        }))
    }
    fn getresgid<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        r: GidPtr,
        e: GidPtr,
        s: GidPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_getresid(caller, IdKind::Group, [r, e, s]).await
        }))
    }
    fn setfsuid<'a>(&'a self, caller: &'a Caller<'_>, uid: Uid) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_setfsid(caller, IdKind::User, uid)
        }))
    }
    fn setfsgid<'a>(&'a self, caller: &'a Caller<'_>, gid: Gid) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_setfsid(caller, IdKind::Group, gid)
        }))
    }
    fn getgroups<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        size: i32,
        list: GidArray,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_getgroups(caller, size, list).await
        }))
    }
    fn setgroups<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        size: i32,
        list: ConstGidArray,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_setgroups(caller, size, list).await
        }))
    }
    fn capget<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        header: wasixv2_abi::UserCapHeaderPtr,
        data: wasixv2_abi::UserCapDataArray,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_capget(caller, header, data).await
        }))
    }
    fn capset<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        header: wasixv2_abi::UserCapHeaderPtr,
        data: wasixv2_abi::ConstUserCapDataArray,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_capset(caller, header, data).await
        }))
    }
    fn getpgid<'a>(&'a self, caller: &'a Caller<'_>, pid: Pid) -> MaybeFuture<'a, SysResult<Pid>> {
        MaybeFuture::Future(Box::pin(async move { self.sys_getpgid(caller, pid) }))
    }
    fn setpgid<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pid: Pid,
        pgid: Pid,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move { self.sys_setpgid(caller, pid, pgid) }))
    }
    fn getsid<'a>(&'a self, caller: &'a Caller<'_>, pid: Pid) -> MaybeFuture<'a, SysResult<Pid>> {
        MaybeFuture::Future(Box::pin(async move { self.sys_getsid(caller, pid) }))
    }
    fn setsid<'a>(&'a self, caller: &'a Caller<'_>) -> MaybeFuture<'a, SysResult<Pid>> {
        MaybeFuture::Future(Box::pin(async move { self.sys_setsid(caller) }))
    }
    fn uname<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        name: NewUtsnamePtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move { self.sys_uname(caller, name).await }))
    }
    fn reboot<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        magic1: i32,
        magic2: i32,
        cmd: wasixv2_abi::RebootCmd,
        arg: wasixv2_abi::VoidPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_reboot(caller, magic1, magic2, cmd, arg)
        }))
    }
    fn sethostname<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        name: ConstBuf,
        len: i32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_sethostname(caller, name, len, false).await
        }))
    }
    fn setdomainname<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        name: ConstBuf,
        len: i32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_sethostname(caller, name, len, true).await
        }))
    }
    fn personality<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        personality: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_personality(caller, personality)
        }))
    }
    fn prctl<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        option: PrctlOption,
        a2: Ulong,
        a3: Ulong,
        a4: Ulong,
        a5: Ulong,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_prctl(caller, option, a2, a3, a4, a5).await
        }))
    }

    fn seccomp<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        op: u32,
        flags: u32,
        uargs: VoidPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_seccomp(caller, op, flags, uargs).await
        }))
    }

    fn landlock_create_ruleset<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        attr: wasixv2_abi::ConstLandlockRulesetAttrPtr,
        size: Usize,
        flags: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_landlock_create_ruleset(caller, attr, size, flags)
                .await
        }))
    }

    fn landlock_add_rule<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        ruleset_fd: Fd,
        rule_type: i32,
        rule_attr: wasixv2_abi::ConstVoidPtr,
        flags: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_landlock_add_rule(caller, ruleset_fd, rule_type, rule_attr.addr(), flags)
                .await
        }))
    }

    fn landlock_restrict_self<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        ruleset_fd: Fd,
        flags: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_landlock_restrict_self(caller, ruleset_fd, flags)
                .await
        }))
    }

    fn add_key<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        r#type: Cstr,
        description: Cstr,
        payload: wasixv2_abi::ConstVoidPtr,
        plen: Usize,
        ringid: wasixv2_abi::KeySerial,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            let (kind, desc, data) = (r#type.addr(), description.addr(), payload.addr());
            self.sys_add_key(caller, kind, desc, data, plen, ringid)
                .await
        }))
    }

    fn request_key<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        r#type: Cstr,
        description: Cstr,
        callout_info: Cstr,
        destringid: wasixv2_abi::KeySerial,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            let (kind, desc, callout) = (r#type.addr(), description.addr(), callout_info.addr());
            self.sys_request_key(caller, kind, desc, callout, destringid)
                .await
        }))
    }

    fn keyctl<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        option: i32,
        arg2: Ulong,
        arg3: Ulong,
        arg4: Ulong,
        arg5: Ulong,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_keyctl(caller, option, arg2, arg3, arg4, arg5)
                .await
        }))
    }

    fn lsm_get_self_attr<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        attr: u32,
        ctx: wasixv2_abi::LsmCtxPtr,
        size: U32Ptr,
        flags: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_lsm_get_self_attr(caller, attr, ctx.addr(), size, flags)
                .await
        }))
    }

    fn lsm_set_self_attr<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        attr: u32,
        ctx: wasixv2_abi::ConstLsmCtxPtr,
        size: u32,
        flags: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_lsm_set_self_attr(caller, attr, ctx.addr(), size, flags)
                .await
        }))
    }

    fn lsm_list_modules<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        ids: wasixv2_abi::U64Array,
        size: U32Ptr,
        flags: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_lsm_list_modules(caller, ids.addr(), size, flags)
                .await
        }))
    }

    fn prlimit64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pid: Pid,
        resource: RlimitResource,
        new: ConstRlimit64Ptr,
        old: Rlimit64Ptr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_prlimit64(caller, pid, resource, new, old).await
        }))
    }
    fn getrusage<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        who: RusageWho,
        ru: RusagePtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_getrusage(caller, who, ru).await
        }))
    }
    fn times<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        tbuf: TmsPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move { self.sys_times(caller, tbuf).await }))
    }
    fn sysinfo<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        info: SysinfoPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_sysinfo(caller, info).await },
        ))
    }
    fn getrandom<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        buf: UserSliceMut,
        flags: GrndFlags,
    ) -> MaybeFuture<'a, SysResult<u32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_getrandom(caller, buf, flags).await
        }))
    }
    fn sched_yield<'a>(&'a self, _: &'a Caller<'_>) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move { self.sys_sched_yield().await }))
    }
    fn sched_getscheduler<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pid: Pid,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_sched_getscheduler(caller, pid) },
        ))
    }
    fn sched_setscheduler<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pid: Pid,
        policy: SchedPolicy,
        param: ConstSchedParamPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_sched_setscheduler(caller, pid, Some(policy), param)
                .await
        }))
    }
    fn sched_setparam<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pid: Pid,
        param: ConstSchedParamPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_sched_setscheduler(caller, pid, None, param).await
        }))
    }
    fn sched_getparam<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pid: Pid,
        param: SchedParamPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_sched_getparam(caller, pid, param).await
        }))
    }
    fn sched_setattr<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pid: Pid,
        uattr: SchedAttrPtr,
        flags: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_sched_setattr(caller, pid, uattr, flags).await
        }))
    }
    fn sched_getattr<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pid: Pid,
        uattr: SchedAttrPtr,
        usize: u32,
        flags: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_sched_getattr(caller, pid, uattr, usize, flags)
                .await
        }))
    }
    fn sched_get_priority_max<'a>(
        &'a self,
        _: &'a Caller<'_>,
        policy: SchedPolicy,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_sched_priority(policy, true) },
        ))
    }
    fn sched_get_priority_min<'a>(
        &'a self,
        _: &'a Caller<'_>,
        policy: SchedPolicy,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_sched_priority(policy, false) },
        ))
    }
    fn sched_rr_get_interval_time64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pid: Pid,
        interval: KernelTimespecPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_sched_rr_get_interval(caller, pid, interval).await
        }))
    }
    fn sched_getaffinity<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pid: Pid,
        len: u32,
        mask: UlongArray,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_sched_getaffinity(caller, pid, len, mask).await
        }))
    }
    fn sched_setaffinity<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pid: Pid,
        len: u32,
        mask: UlongArray,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_sched_setaffinity(caller, pid, len, mask).await
        }))
    }
    fn getcpu<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        cpu: U32Ptr,
        node: U32Ptr,
        _: VoidPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_getcpu(caller, cpu, node).await },
        ))
    }
    fn ioprio_set<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        which: i32,
        who: i32,
        ioprio: i32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_ioprio_set(caller, which, who, ioprio)
        }))
    }
    fn ioprio_get<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        which: i32,
        who: i32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_ioprio_get(caller, which, who) },
        ))
    }
    fn getpriority<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        which: PrioWhich,
        who: i32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_getpriority(caller, which, who) },
        ))
    }
    fn setpriority<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        which: PrioWhich,
        who: i32,
        nice: i32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_setpriority(caller, which, who, nice)
        }))
    }
    fn membarrier<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        cmd: MembarrierCmd,
        flags: MembarrierCmdFlags,
        _: i32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_membarrier(caller, cmd, flags) },
        ))
    }
    fn brk<'a>(&'a self, caller: &'a Caller<'_>, brk: Uptr) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move { self.sys_brk(caller, brk).await }))
    }
    fn mmap<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        addr: Uptr,
        len: Ulong,
        prot: ProtFlags,
        flags: MapFlags,
        fd: Fd,
        offset: Loff,
    ) -> MaybeFuture<'a, SysResult<Uptr>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_mmap(caller, addr, len, prot, flags, fd, offset)
                .await
        }))
    }
    fn munmap<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        addr: Uptr,
        len: Usize,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_munmap(caller, addr, len).await },
        ))
    }
    fn mremap<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        addr: Uptr,
        old_len: Ulong,
        new_len: Ulong,
        flags: MremapFlags,
        new_addr: Uptr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_mremap(caller, addr, old_len, new_len, flags, new_addr)
                .await
        }))
    }
    fn mprotect<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        start: Uptr,
        len: Usize,
        prot: ProtFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_mprotect(caller, start, len, prot).await
        }))
    }
    fn madvise<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        start: Uptr,
        len: Usize,
        advice: MadviseAdvice,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_madvise(caller, start, len, advice).await
        }))
    }
    fn vhangup<'a>(&'a self, caller: &'a Caller<'_>) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move { self.sys_vhangup(caller.task()) }))
    }
    fn process_vm_readv<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pid: Pid,
        lvec: wasixv2_abi::ConstIovecArray,
        liovcnt: Ulong,
        rvec: wasixv2_abi::ConstIovecArray,
        riovcnt: Ulong,
        flags: Ulong,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_process_vm_rw(caller, pid, lvec, liovcnt, rvec, riovcnt, flags, false)
                .await
        }))
    }
    fn process_vm_writev<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pid: Pid,
        lvec: wasixv2_abi::ConstIovecArray,
        liovcnt: Ulong,
        rvec: wasixv2_abi::ConstIovecArray,
        riovcnt: Ulong,
        flags: Ulong,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_process_vm_rw(caller, pid, lvec, liovcnt, rvec, riovcnt, flags, true)
                .await
        }))
    }
    fn remap_file_pages<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        start: Uptr,
        size: Ulong,
        prot: Ulong,
        pgoff: Ulong,
        _flags: Ulong,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            // `flags` are ignored but for `MAP_NONBLOCK`, which changes nothing here.
            self.sys_remap_file_pages(caller, start, size, prot, pgoff)
                .await
        }))
    }
    fn mseal<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        start: Uptr,
        len: Usize,
        flags: Ulong,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_mseal(caller, start, len, flags).await
        }))
    }
    fn process_mrelease<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pidfd: Fd,
        flags: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_process_mrelease(caller, pidfd, flags).await
        }))
    }

    fn process_madvise<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        pidfd: Fd,
        vec: wasixv2_abi::ConstIovecArray,
        vlen: Usize,
        behavior: MadviseAdvice,
        flags: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_process_madvise(caller, pidfd, vec, vlen, behavior, flags)
                .await
        }))
    }
    fn cachestat<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        cstat_range: wasixv2_abi::ConstCachestatRangePtr,
        cstat: wasixv2_abi::CachestatPtr,
        flags: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_cachestat(caller, fd, cstat_range, cstat, flags)
                .await
        }))
    }
    fn mincore<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        start: Uptr,
        len: Usize,
        vec: Buf,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_mincore(caller, start, len, vec).await
        }))
    }
    fn msync<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        start: Uptr,
        len: Usize,
        flags: MsyncFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_msync(caller, start, len, flags).await
        }))
    }
    fn mlock<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        start: Uptr,
        len: Usize,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_mlock(caller, start, len, MlockFlags::empty())
                .await
        }))
    }
    fn mlock2<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        start: Uptr,
        len: Usize,
        flags: MlockFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_mlock(caller, start, len, flags).await
        }))
    }
    fn munlock<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        start: Uptr,
        len: Usize,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_munlock(caller, start, len).await
        }))
    }
    fn mlockall<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        flags: MclFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_mlockall(caller, flags).await },
        ))
    }
    fn munlockall<'a>(&'a self, caller: &'a Caller<'_>) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move { self.sys_munlockall(caller).await }))
    }
    fn memfd_create<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        uname: Cstr,
        flags: MemfdFlags,
    ) -> MaybeFuture<'a, SysResult<Fd>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_memfd_create(caller, uname, flags).await
        }))
    }
    fn clock_gettime64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        clock: ClockId,
        tp: KernelTimespecPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_clock_gettime(caller, clock, tp).await
        }))
    }
    fn clock_getres_time64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        clock: ClockId,
        tp: KernelTimespecPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_clock_getres(caller, clock, tp).await
        }))
    }
    fn clock_settime64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        clock: ClockId,
        tp: ConstKernelTimespecPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_clock_settime(caller, clock, tp).await
        }))
    }
    fn clock_adjtime64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        clock: ClockId,
        utx: wasixv2_abi::KernelTimexPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_clock_adjtime(caller, clock, utx).await
        }))
    }
    fn clock_nanosleep_time64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        clock: ClockId,
        flags: TimerFlags,
        rqtp: ConstKernelTimespecPtr,
        rmtp: KernelTimespecPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_clock_nanosleep(caller, clock, flags, rqtp, rmtp)
                .await
        }))
    }
    fn restart_syscall<'a>(&'a self, caller: &'a Caller<'_>) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_restart_syscall(caller).await },
        ))
    }
    fn signalfd4<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        ufd: Fd,
        user_mask: ConstSigsetPtr,
        sizemask: Usize,
        flags: wasixv2_abi::SignalfdFlags,
    ) -> MaybeFuture<'a, SysResult<Fd>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_signalfd4(caller, ufd, user_mask, sizemask, flags)
                .await
        }))
    }

    fn timerfd_create<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        clock: ClockId,
        flags: TimerfdFlags,
    ) -> MaybeFuture<'a, SysResult<Fd>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_timerfd_create(caller, clock, flags).await
        }))
    }
    fn timerfd_settime64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        flags: TimerfdSettimeFlags,
        new: ConstKernelItimerspecPtr,
        old: KernelItimerspecPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_timerfd_settime(caller, fd, flags, new, old).await
        }))
    }
    fn timerfd_gettime64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        out: KernelItimerspecPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_timerfd_gettime(caller, fd, out).await
        }))
    }

    // ---- ipc ----

    fn msgget<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        key: i32,
        msgflg: i32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_msgget(caller, key, msgflg) },
        ))
    }

    fn msgctl<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        msqid: i32,
        cmd: i32,
        buf: Msqid64DsPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_msgctl(caller, msqid, cmd, buf.addr()).await
        }))
    }

    fn msgrcv<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        msqid: i32,
        msgp: MsgbufPtr,
        msgsz: Usize,
        msgtyp: i32,
        msgflg: i32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            let n = self
                .sys_msgrcv(caller, msqid, msgp, msgsz, msgtyp, msgflg)
                .await?;
            self.sync_segments(caller).await;
            Ok(n)
        }))
    }

    fn msgsnd<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        msqid: i32,
        msgp: ConstMsgbufPtr,
        msgsz: Usize,
        msgflg: i32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sync_segments(caller).await;
            self.sys_msgsnd(caller, msqid, msgp, msgsz, msgflg).await
        }))
    }

    fn semget<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        key: i32,
        nsems: i32,
        semflg: i32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_semget(caller, key, nsems, semflg)
        }))
    }

    fn semctl<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        semid: i32,
        semnum: i32,
        cmd: i32,
        arg: Ulong,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_semctl(caller, semid, semnum, cmd, arg).await
        }))
    }

    fn semop<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        semid: i32,
        tsops: ConstSembufArray,
        nsops: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            let none = ConstKernelTimespecPtr::NULL;
            self.semtimedop_time64(caller, semid, tsops, nsops, none)
                .await
        }))
    }

    fn semtimedop_time64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        semid: i32,
        tsops: ConstSembufArray,
        nsops: u32,
        timeout: ConstKernelTimespecPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sync_segments(caller).await;
            let result = self
                .sys_semtimedop(caller, semid, tsops, nsops, timeout)
                .await;
            self.sync_segments(caller).await;
            result
        }))
    }

    fn shmget<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        key: i32,
        size: Usize,
        shmflg: i32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_shmget(caller, key, size, shmflg)
        }))
    }

    fn shmctl<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        shmid: i32,
        cmd: i32,
        buf: Shmid64DsPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_shmctl(caller, shmid, cmd, buf.addr()).await
        }))
    }

    fn shmat<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        shmid: i32,
        shmaddr: Uptr,
        shmflg: i32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_shmat(caller, shmid, shmaddr, shmflg).await
        }))
    }

    fn shmdt<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        shmaddr: Uptr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_shmdt(caller, shmaddr).await },
        ))
    }

    fn mq_open<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        u_name: Cstr,
        oflag: OpenFlags,
        mode: Mode,
        u_attr: ConstMqAttrPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_mq_open(caller, u_name, oflag, mode, u_attr).await
        }))
    }

    fn mq_unlink<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        u_name: Cstr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_mq_unlink(caller, u_name).await },
        ))
    }

    fn mq_timedsend_time64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        mqdes: i32,
        u_msg_ptr: ConstBuf,
        msg_len: Usize,
        msg_prio: u32,
        u_abs_timeout: ConstKernelTimespecPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
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
        }))
    }

    fn mq_timedreceive_time64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        mqdes: i32,
        u_msg_ptr: Buf,
        msg_len: Usize,
        u_msg_prio: U32Ptr,
        u_abs_timeout: ConstKernelTimespecPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
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
        }))
    }

    fn mq_notify<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        mqdes: i32,
        u_notification: ConstSigeventPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_mq_notify(caller, Fd(mqdes), u_notification).await
        }))
    }

    fn mq_getsetattr<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        mqdes: i32,
        u_mqstat: ConstMqAttrPtr,
        u_omqstat: MqAttrPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_mq_getsetattr(caller, Fd(mqdes), u_mqstat, u_omqstat)
                .await
        }))
    }

    // ---- poll ----

    fn pipe2<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fildes: wasixv2_abi::FdPair,
        flags: OpenFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_pipe2(caller, fildes, flags).await
        }))
    }

    fn eventfd2<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        count: u32,
        flags: wasixv2_abi::EventfdFlags,
    ) -> MaybeFuture<'a, SysResult<Fd>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_eventfd2(caller, count, flags).await
        }))
    }

    fn rseq_slice_yield<'a>(&'a self, _caller: &'a Caller<'_>) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            // Linux `kernel/rseq.c:sys_rseq_slice_yield`: no extension was granted,
            // so there is no CPU slice to relinquish and no scheduling side effect.
            Ok(0)
        }))
    }

    fn rseq<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        rseq: wasixv2_abi::VoidPtr,
        rseq_len: u32,
        flags: i32,
        sig: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_rseq(caller, rseq, rseq_len, flags, sig).await
        }))
    }

    // ---- aio ----
    fn io_setup<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        nr_events: u32,
        ctxp: wasixv2_abi::AioContextPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_io_setup(caller, nr_events, ctxp).await
        }))
    }

    fn io_destroy<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        ctx: wasixv2_abi::AioContext,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_io_destroy(caller, ctx).await },
        ))
    }

    fn io_submit<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        ctx_id: wasixv2_abi::AioContext,
        nr: wasixv2_abi::Slong,
        iocbpp: wasixv2_abi::ConstIocbPtrArray,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_io_submit(caller, ctx_id, nr, iocbpp).await
        }))
    }

    fn io_cancel<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        ctx_id: wasixv2_abi::AioContext,
        iocb: wasixv2_abi::IocbPtr,
        result: wasixv2_abi::IoEventPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_io_cancel(caller, ctx_id, iocb, result).await
        }))
    }

    fn io_pgetevents_time64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        ctx_id: wasixv2_abi::AioContext,
        min_nr: wasixv2_abi::Slong,
        nr: wasixv2_abi::Slong,
        events: wasixv2_abi::IoEventArray,
        timeout: KernelTimespecPtr,
        usig: wasixv2_abi::ConstAioSigsetPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_io_pgetevents(caller, ctx_id, min_nr, nr, events, timeout, usig)
                .await
        }))
    }

    // ---- io_uring ----
    fn io_uring_setup<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        entries: u32,
        params: wasixv2_abi::IoUringParamsPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_io_uring_setup(caller, entries, params).await
        }))
    }

    #[allow(clippy::too_many_arguments)]
    fn io_uring_enter<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        to_submit: u32,
        min_complete: u32,
        flags: u32,
        argp: wasixv2_abi::ConstVoidPtr,
        argsz: Usize,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_io_uring_enter(caller, fd, to_submit, min_complete, flags, argp, argsz)
                .await
        }))
    }

    fn io_uring_register<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        opcode: u32,
        arg: wasixv2_abi::VoidPtr,
        nr_args: u32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_io_uring_register(caller, fd, opcode, arg, nr_args)
                .await
        }))
    }

    fn ppoll_time64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        ufds: wasixv2_abi::PollfdArray,
        nfds: u32,
        tsp: KernelTimespecPtr,
        sigmask: ConstSigsetPtr,
        sigsetsize: Usize,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_ppoll(caller, ufds, nfds, tsp, sigmask, sigsetsize)
                .await
        }))
    }

    fn pselect6_time64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        n: i32,
        inp: wasixv2_abi::KernelFdSetPtr,
        outp: wasixv2_abi::KernelFdSetPtr,
        exp: wasixv2_abi::KernelFdSetPtr,
        tsp: KernelTimespecPtr,
        sig: wasixv2_abi::ConstSigsetArgpackPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_pselect6(caller, n, inp, outp, exp, tsp, sig).await
        }))
    }

    fn epoll_create1<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        flags: wasixv2_abi::EpollCreateFlags,
    ) -> MaybeFuture<'a, SysResult<Fd>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_epoll_create1(caller, flags).await
        }))
    }

    fn epoll_ctl<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        epfd: Fd,
        op: wasixv2_abi::EpollCtlOp,
        fd: Fd,
        event: wasixv2_abi::ConstEpollEventPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_epoll_ctl(caller, epfd, op, fd, event).await
        }))
    }

    fn epoll_pwait<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        epfd: Fd,
        events: wasixv2_abi::EpollEventArray,
        maxevents: i32,
        timeout: i32,
        sigmask: ConstSigsetPtr,
        sigsetsize: Usize,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_epoll_pwait(
                caller, epfd, events, maxevents, timeout, sigmask, sigsetsize,
            )
            .await
        }))
    }

    fn epoll_pwait2<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        epfd: Fd,
        events: wasixv2_abi::EpollEventArray,
        maxevents: i32,
        timeout: ConstKernelTimespecPtr,
        sigmask: ConstSigsetPtr,
        sigsetsize: Usize,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_epoll_pwait2(
                caller, epfd, events, maxevents, timeout, sigmask, sigsetsize,
            )
            .await
        }))
    }

    // ---- net ----

    fn socket<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        family: wasixv2_abi::AddressFamily,
        r#type: i32,
        protocol: i32,
    ) -> MaybeFuture<'a, SysResult<Fd>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_socket(caller, family, r#type, protocol).await
        }))
    }

    fn socketpair<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        family: wasixv2_abi::AddressFamily,
        r#type: i32,
        protocol: i32,
        usockvec: wasixv2_abi::FdPair,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_socketpair(caller, family, r#type, protocol, usockvec)
                .await
        }))
    }

    fn bind<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        umyaddr: wasixv2_abi::ConstSockaddrPtr,
        addrlen: wasixv2_abi::Socklen,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_bind(caller, fd, umyaddr, addrlen).await
        }))
    }

    fn listen<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        backlog: i32,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_listen(caller, fd, backlog).await
        }))
    }

    fn accept<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        upeer_sockaddr: wasixv2_abi::SockaddrPtr,
        upeer_addrlen: wasixv2_abi::SocklenPtr,
    ) -> MaybeFuture<'a, SysResult<Fd>> {
        MaybeFuture::Future(Box::pin(async move {
            let flags = wasixv2_abi::SocketFlags(0);
            self.sys_accept4(caller, fd, upeer_sockaddr, upeer_addrlen, flags)
                .await
        }))
    }

    fn accept4<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        upeer_sockaddr: wasixv2_abi::SockaddrPtr,
        upeer_addrlen: wasixv2_abi::SocklenPtr,
        flags: wasixv2_abi::SocketFlags,
    ) -> MaybeFuture<'a, SysResult<Fd>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_accept4(caller, fd, upeer_sockaddr, upeer_addrlen, flags)
                .await
        }))
    }

    fn connect<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        uservaddr: wasixv2_abi::ConstSockaddrPtr,
        addrlen: wasixv2_abi::Socklen,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_connect(caller, fd, uservaddr, addrlen).await
        }))
    }

    fn getsockname<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        usockaddr: wasixv2_abi::SockaddrPtr,
        usockaddr_len: wasixv2_abi::SocklenPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_getsockname(caller, fd, usockaddr, usockaddr_len)
                .await
        }))
    }

    fn getpeername<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        usockaddr: wasixv2_abi::SockaddrPtr,
        usockaddr_len: wasixv2_abi::SocklenPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_getpeername(caller, fd, usockaddr, usockaddr_len)
                .await
        }))
    }

    fn sendto<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        buff: UserSlice,
        flags: wasixv2_abi::MsgFlags,
        addr: wasixv2_abi::ConstSockaddrPtr,
        addr_len: wasixv2_abi::Socklen,
    ) -> MaybeFuture<'a, SysResult<u32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_sendto(caller, fd, buff, flags, addr, addr_len)
                .await
        }))
    }

    fn recvfrom<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        ubuf: UserSliceMut,
        flags: wasixv2_abi::MsgFlags,
        addr: wasixv2_abi::SockaddrPtr,
        addr_len: wasixv2_abi::SocklenPtr,
    ) -> MaybeFuture<'a, SysResult<u32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_recvfrom(caller, fd, ubuf, flags, addr, addr_len)
                .await
        }))
    }

    fn setsockopt<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        level: wasixv2_abi::SocketLevel,
        optname: i32,
        optval: UserSlice,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_setsockopt(caller, fd, level, optname, optval)
                .await
        }))
    }

    fn getsockopt<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        level: wasixv2_abi::SocketLevel,
        optname: i32,
        optval: wasixv2_abi::Buf,
        optlen: wasixv2_abi::SocklenPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_getsockopt(caller, fd, level, optname, optval, optlen)
                .await
        }))
    }

    fn shutdown<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        how: wasixv2_abi::ShutdownHow,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(
            async move { self.sys_shutdown(caller, fd, how).await },
        ))
    }

    fn sendmsg<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        msg: wasixv2_abi::ConstUserMsghdrPtr,
        flags: wasixv2_abi::MsgFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_sendmsg(caller, fd, msg, flags).await
        }))
    }

    fn recvmsg<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        msg: wasixv2_abi::UserMsghdrPtr,
        flags: wasixv2_abi::MsgFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_recvmsg(caller, fd, msg, flags).await
        }))
    }

    fn sendmmsg<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        mmsg: wasixv2_abi::MmsghdrArray,
        vlen: u32,
        flags: wasixv2_abi::MsgFlags,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_sendmmsg(caller, fd, mmsg, vlen, flags).await
        }))
    }

    fn recvmmsg_time64<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        fd: Fd,
        mmsg: wasixv2_abi::MmsghdrArray,
        vlen: u32,
        flags: wasixv2_abi::MsgFlags,
        timeout: KernelTimespecPtr,
    ) -> MaybeFuture<'a, SysResult<i32>> {
        MaybeFuture::Future(Box::pin(async move {
            self.sys_recvmmsg(caller, fd, mmsg, vlen, flags, timeout)
                .await
        }))
    }
}

impl Control for Kernel {
    fn run_task_work<'a>(&'a self, caller: &'a Caller<'_>) -> LocalFuture<'a, ()> {
        self.io_uring_run_task_work(caller)
    }

    fn take_pending(&self, task: TaskId) -> Option<GuestAction> {
        self.key_session_task_work(task);
        // ---- proc ----
        self.kill_vfork_parent(task);
        if let Some(parent) = self.dead_vfork_parent(task) {
            self.task_signals(task).group().clear_notify(task);
            return Some(GuestAction::ExitMm { task: parent });
        }
        let exit =
            self.state
                .read_or_recover()
                .tasks
                .get(&task)
                .and_then(|t| match t.work.peek() {
                    Some(GuestAction::Exit(code)) => Some(*code),
                    // A shared-instance vfork child hands back the instance instead of
                    // unwinding it; its fatal exit still owes the tracer an EXIT stop.
                    Some(GuestAction::VforkReturn { child, .. }) if *child == task => {
                        t.process.group.status()
                    }
                    _ => None,
                });
        if let Some(code) = exit
            && self.ptrace_pending_exit(task, code)
        {
            return Some(GuestAction::Stopped);
        }
        let (action, signals, dead, task_work) = {
            let mut guard = self.state.lock_or_recover();
            let state = &mut *guard;
            let task_work = state
                .tasks
                .get(&task)
                .map(|t| (t.uring.clone(), t.work.flag().clone()));
            // ---- proc ----
            // A reaped process's task, still unwinding, takes what was left for it: its exit,
            // or a vfork child's hand-back of the instance.
            let (work, signals, dead) = match state.tasks.get_mut(&task) {
                Some(entry) => (&mut entry.work, entry.signals.clone(), false),
                None => {
                    let dead = state.dead.get_mut(&task)?;
                    (&mut dead.work, dead.signals.clone(), true)
                }
            };
            work.flag().clear();
            let action = work.take();
            let action = action.map(|action| process::vfork_parent_resumes(state, action));
            (action, signals, dead, task_work)
        };
        // A producer can queue task work between exec's hook and this flag clear.
        // Keep that work visible so exec repeats its memory-bearing hook before returning.
        if let Some((tctx, flag)) = task_work
            && tctx.pending()
        {
            flag.raise();
        }
        match action {
            // What made the task leave its wait is here.
            Some(action) => {
                signals.group().clear_notify(task);
                if let GuestAction::Exit(code) = action
                    && !dead
                {
                    self.exit_signals(task, &signals);
                    self.ptrace_exit_task(task, code);
                }
                Some(action)
            }
            None if dead => None,
            None => self.take_signal(task, &signals),
        }
    }

    fn work_flag(&self, task: TaskId) -> Arc<WorkFlag> {
        let state = self.state.lock_or_recover();
        match (state.tasks.get(&task), state.dead.get(&task)) {
            (Some(task), _) => task.work.flag().clone(),
            (None, Some(dead)) => dead.work.flag().clone(),
            (None, None) => Arc::new(WorkFlag::always()),
        }
    }

    fn signal_return(&self, task: TaskId, mask: Option<u64>) {
        Kernel::signal_return(self, task, mask)
    }

    fn fault(&self, task: TaskId, signal: i32) -> bool {
        self.force_fatal_signal(task, signal)
    }

    /// `put_task_struct`, or `release_task` for a task its reaper has not unhashed.
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

impl CpuTime for Kernel {
    fn cpu_account(&self, task: TaskId) -> Arc<CpuAccount> {
        let state = self.state.read_or_recover();
        let account = match (state.tasks.get(&task), state.dead.get(&task)) {
            (Some(t), _) => Some(t.cpu.clone()),
            (None, dead) => dead.map(|dead| dead.cpu.clone()),
        };
        account.unwrap_or_else(|| Arc::new(CpuAccount::detached()))
    }

    fn cpu_timers(&self, task: TaskId) {
        self.run_cpu_timers(task);
    }
}

impl PidView for Kernel {
    /// The process's id: its leader's (`task->group_leader`), which outlives the task.
    fn tgid(&self, task: TaskId) -> TaskId {
        let state = self.state.read_or_recover();
        state.tasks.get(&task).map_or(task, |t| t.process.pid)
    }

    fn pid_in(&self, viewer: TaskId, task: TaskId) -> i32 {
        let state = self.state.read_or_recover();
        let Some(viewer) = state.tasks.get(&viewer) else {
            return 0;
        };
        self.pids.nr(&viewer.process.pid_ns, task.0)
    }
}

impl GroupStop for Kernel {
    fn stop_wait(&self, task: TaskId) -> LocalFuture<'_, ()> {
        Box::pin(Kernel::stop_wait(self, task))
    }
}

impl VforkWait for Kernel {
    fn vfork_wait(&self, task: TaskId) -> LocalFuture<'_, ()> {
        Box::pin(Kernel::vfork_wait(self, task))
    }
}

impl ExitMm for Kernel {
    fn exit_mm<'a>(&'a self, caller: &'a Caller<'a>) -> LocalFuture<'a, ()> {
        Box::pin(Kernel::exit_mm(self, caller))
    }
}

impl SharedMappings for Kernel {
    fn shared_mappings(&self, task: TaskId) -> Vec<SharedMapping> {
        Kernel::shared_mappings(self, task)
    }

    fn wipe_on_fork(&self, task: TaskId) -> Vec<Range<u64>> {
        Kernel::wipe_on_fork(self, task)
    }

    fn page_access(&self, task: TaskId) -> Option<Arc<wasixv2_syscalls::PageAccess>> {
        let state = self.state.read_or_recover();
        state.tasks.get(&task).map(|t| t.mm.access().clone())
    }

    fn attach_memory(
        &self,
        task: TaskId,
        memory: Arc<dyn wasixv2_syscalls::UserMemory + Send + Sync>,
    ) {
        // Like mmget_not_zero (mm/memory.c:access_process_vm), an exited
        // image cannot acquire a new owner. Keep state held through attachment
        // so exit_mm cannot mark its final user left_mm between check and attach.
        let state = self.state.read_or_recover();
        if let Some(entry) = state.tasks.get(&task)
            && state
                .tasks
                .values()
                .any(|t| Arc::ptr_eq(&t.mm, &entry.mm) && !t.thread.left_mm())
        {
            entry.mm.attach_foreign(memory);
        }
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
