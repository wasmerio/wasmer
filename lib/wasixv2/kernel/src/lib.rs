//! The kernel: tasks, processes, credentials, signals, namespaces, IPC, memory, terminals, pseudo
//! filesystems and the socket layer. Implements `Syscalls`.
//!
//! Knows nothing about the wasm engine, the layers, or concrete filesystem drivers and network
//! backends.
//!
//! Every process has exactly one task so far, and the working directory is always `/`.

mod file;
mod fs;
mod ioctl;
mod mm;
mod startup;
mod stdio;
mod time;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub use file::{FileObject, Io};
use wasixv2_abi::{
    AtFlags, ClockId, ConstIovecArray, ConstKernelTimespecPtr, ConstSigsetPtr, Cstr, FcntlCmd,
    FutexOp, GuestPtr, IoctlRequest, KernelTimespecPtr, Loff, MapFlags, Mode, NewUtsname,
    NewUtsnamePtr, OpenFlags, Pid, ProtFlags, S32Ptr, SeekWhence, Signal, SigprocmaskHow, Sigset,
    SigsetPtr, TimerFlags, U32Ptr, Ulong, Uptr, Usize,
};
use wasixv2_host::Host;
pub use wasixv2_host::Stdio;
use wasixv2_syscalls::{
    Caller, Control, Errno, Fd, GuestAction, SysResult, Syscalls, TaskId, UserSlice, UserSliceMut,
    read_value, write_value,
};
use wasixv2_vfs::Inode;

use crate::{
    file::{FdTable, OpenFile},
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
    /// The root directory, which is also every process's working directory.
    root: Arc<dyn Inode>,
    state: Mutex<State>,
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
}

struct Task {
    process: Arc<Process>,
    files: Arc<Mutex<FdTable>>,
    pending: Option<GuestAction>,
    /// Blocked signals; bit `n - 1` is signal `n`.
    sigmask: u64,
}

impl Kernel {
    pub fn new(host: Arc<dyn Host>, root: Arc<dyn Inode>) -> Self {
        Self {
            host,
            root,
            state: Mutex::default(),
        }
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
        };
        let task = Task {
            process: Arc::new(process),
            files: Arc::new(Mutex::new(files)),
            pending: None,
            sigmask: 0,
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

    fn exit_process(&self, task: TaskId, status: i32) {
        // Linux keeps only the low byte of the status.
        let action = GuestAction::Exit(status & 0xff);
        self.with_task(task, |task| task.pending = Some(action));
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

    async fn openat(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        flags: OpenFlags,
        _mode: Mode,
    ) -> SysResult<Fd> {
        self.open(caller, dfd, filename, flags).await
    }

    async fn close(&self, caller: &Caller<'_>, fd: Fd) -> SysResult<i32> {
        self.close_fd(caller, fd)
    }

    async fn unlinkat(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        pathname: Cstr,
        flag: AtFlags,
    ) -> SysResult<i32> {
        self.unlink(caller, dfd, pathname, flag).await
    }

    async fn fcntl64(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        cmd: FcntlCmd,
        arg: Ulong,
    ) -> SysResult<i32> {
        self.fcntl(caller, fd, cmd, arg)
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

    async fn readv(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        vec: ConstIovecArray,
        vlen: Ulong,
    ) -> SysResult<u32> {
        self.read_vectored(caller, fd, vec, vlen).await
    }

    async fn writev(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        vec: ConstIovecArray,
        vlen: Ulong,
    ) -> SysResult<u32> {
        self.write_vectored(caller, fd, vec, vlen).await
    }

    async fn pread64(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        buf: UserSliceMut,
        pos: i64,
    ) -> SysResult<u32> {
        self.pread(caller, fd, buf, pos).await
    }

    async fn pwrite64(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        buf: UserSlice,
        pos: i64,
    ) -> SysResult<u32> {
        self.pwrite(caller, fd, buf, pos).await
    }

    async fn lseek(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        offset: Loff,
        whence: SeekWhence,
    ) -> SysResult<i64> {
        Ok(self.seek(caller, fd, offset, whence)? as i64)
    }

    async fn exit(&self, caller: &Caller<'_>, error_code: i32) -> SysResult<()> {
        // The task is the last of its thread group, so the process exits with its status.
        self.exit_process(caller.task(), error_code);
        Ok(())
    }

    async fn exit_group(&self, caller: &Caller<'_>, error_code: i32) -> SysResult<()> {
        self.exit_process(caller.task(), error_code);
        Ok(())
    }

    // Nothing reads `clear_child_tid` until there are threads to join.
    async fn set_tid_address(&self, caller: &Caller<'_>, _: S32Ptr) -> SysResult<i32> {
        Ok(caller.task().0 as i32)
    }

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

    /// `FUTEX_WAIT` and `FUTEX_WAKE` for a process with one task: nothing else can wake a waiter,
    /// so a wait without a timeout never returns, as on Linux.
    async fn futex_time64(
        &self,
        caller: &Caller<'_>,
        uaddr: U32Ptr,
        op: FutexOp,
        val: u32,
        utime: Ulong,
        _uaddr2: U32Ptr,
        _val3: u32,
    ) -> SysResult<i32> {
        match FutexOp(op.0 & FutexOp::FUTEX_CMD_MASK.0) {
            FutexOp::FUTEX_WAKE => Ok(0),
            FutexOp::FUTEX_WAIT => {
                let current: u32 = read_value(caller.memory(), GuestPtr::new(uaddr.addr())).await?;
                if current != val {
                    return Err(Errno::EAGAIN.into());
                }
                if utime == 0 {
                    std::future::pending::<()>().await;
                }
                let timeout =
                    read_value(caller.memory(), ConstKernelTimespecPtr::new(utime)).await?;
                self.host.sleep(time::from_timespec(timeout)?).await;
                Err(Errno::ETIMEDOUT.into())
            }
            _ => Err(Errno::ENOSYS.into()),
        }
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
        let duration = match flags.contains(TimerFlags::TIMER_ABSTIME) {
            true => request.saturating_sub(now),
            false => request,
        };
        self.host.sleep(duration).await;
        Ok(0)
    }
}

impl Control for Kernel {
    fn take_pending(&self, task: TaskId) -> Option<GuestAction> {
        self.with_task(task, |task| task.pending.take())
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
