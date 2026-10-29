//! Secure computing (`kernel/seccomp.c`): strict mode and classic BPF filters over every
//! syscall, checked at syscall entry ([`crate::SyscallEntry`]).
//!
//! A task's mode and filters are its own (`task->seccomp`): a child or thread starts with its
//! creator's, `execve` keeps them, and `SECCOMP_FILTER_FLAG_TSYNC` gives the caller's to every
//! thread of its group. A filter sees the call as riscv32 makes it (`seccomp_data`): the
//! table's number and its six registers, as [`Kernel::syscall_entry`] makes them.

use std::sync::{Arc, atomic::Ordering};

use wasixv2_abi::{
    AuditArch, Errno, GuestLayout, GuestPtr, GuestPtrMut, SeccompData, SeccompFilterFlags,
    SeccompMode, SeccompNotif, SeccompNotifResp, SeccompNotifSizes, SeccompOp, SeccompRet, SiCode,
    Signal, SockFprog, VoidPtr,
};
use wasixv2_syscalls::{Caller, SysErr, SysResult, Syscall, SyscallRet, read_value, write_value};
use wasixv2_vfs::Cap;
use wasixv2_vnet::bpf;

use crate::{
    Kernel, Task,
    file::{Backing, OpenFile},
    signal::kernel_info,
};

mod notify;

pub(crate) use notify::Listener;

/// `MAX_INSNS_PER_PATH`: the most instructions the filters of a task may hold, counting four
/// more for each filter.
const MAX_INSNS_PER_PATH: usize = (1 << 18) / 8;

/// `MAX_ERRNO`.
const MAX_ERRNO: u32 = 4095;

/// The riscv32 numbers strict mode allows (`mode1_syscalls`): `read`, `write`, `exit` and
/// `rt_sigreturn`.
const MODE1_SYSCALLS: [u32; 4] = [63, 64, 93, 139];

/// `SECCOMP_MODE_DEAD`: a task a filter killed, which runs no syscall any more.
const MODE_DEAD: u32 = 3;

/// The filter flags `seccomp` takes (`SECCOMP_FILTER_FLAG_MASK`).
const FILTER_FLAGS: u32 = SeccompFilterFlags::SECCOMP_FILTER_FLAG_TSYNC.0
    | SeccompFilterFlags::SECCOMP_FILTER_FLAG_LOG.0
    | SeccompFilterFlags::SECCOMP_FILTER_FLAG_SPEC_ALLOW.0
    | SeccompFilterFlags::SECCOMP_FILTER_FLAG_NEW_LISTENER.0
    | SeccompFilterFlags::SECCOMP_FILTER_FLAG_TSYNC_ESRCH.0
    | SeccompFilterFlags::SECCOMP_FILTER_FLAG_WAIT_KILLABLE_RECV.0;

/// One attached filter (`struct seccomp_filter`), with the filters attached before it.
pub(crate) struct Filter {
    prog: bpf::Program,
    prev: Option<Arc<Filter>>,
    /// Its user notifications, if installed with `SECCOMP_FILTER_FLAG_NEW_LISTENER`.
    notif: Option<Arc<notify::Notif>>,
}

impl Filter {
    /// `is_ancestor`: whether `self` is `child` or one of the filters before it.
    fn is_ancestor_of(self: &Arc<Self>, child: Option<&Arc<Filter>>) -> bool {
        let mut walker = child;
        while let Some(f) = walker {
            if Arc::ptr_eq(self, f) {
                return true;
            }
            walker = f.prev.as_ref();
        }
        false
    }
}

/// A task's seccomp state (`task->seccomp`) and `no_new_privs`, which stay in step.
#[derive(Clone, Default)]
pub(crate) struct Seccomp {
    /// `PR_SET_NO_NEW_PRIVS` (`PFA_NO_NEW_PRIVS`): per task, inherited, kept by `execve`.
    pub(crate) no_new_privs: bool,
    /// `SECCOMP_MODE_*`, or [`MODE_DEAD`].
    mode: u32,
    filter: Option<Arc<Filter>>,
    /// `filter_count`.
    filters: u32,
}

impl Seccomp {
    /// What `/proc/[pid]/status` shows: `NoNewPrivs`, `Seccomp` and `Seccomp_filters`.
    pub(crate) fn status(&self) -> (bool, u32, u32) {
        (self.no_new_privs, self.mode, self.filters)
    }

    /// `seccomp_may_assign_mode`: a task changes its mode only from disabled.
    fn may_assign_mode(&self, mode: u32) -> bool {
        self.mode == SeccompMode::SECCOMP_MODE_DISABLED.0 || self.mode == mode
    }
}

/// What filters see: a `seccomp_data`, its words in the guest's byte order.
struct Data([u8; SeccompData::SIZE]);

impl bpf::Input for Data {
    fn data_len(&self) -> usize {
        self.0.len()
    }

    fn load(&self, k: i32, size: usize) -> Option<u32> {
        let at = usize::try_from(k).ok()?;
        let bytes = self.0.get(at..at.checked_add(size)?)?;
        Some(bytes.iter().rev().fold(0, |v, b| (v << 8) | u32::from(*b)))
    }

    fn ancillary(&self, _: i32, a: u32, _: u32) -> Option<u32> {
        // `Program::seccomp` refuses ancillary loads.
        Some(a)
    }
}

/// What the filters decided (`__seccomp_filter`'s actions).
enum Verdict {
    Allow,
    /// The call is skipped and returns `raw`.
    Skip(i64),
    /// The task or its group is killed with `sig`.
    KillThread(i32),
    KillProcess(i32),
    /// `SECCOMP_RET_USER_NOTIF` from `filter`, for the call `data` describes.
    Notify(Arc<Filter>, SeccompData),
}

impl Kernel {
    /// `secure_computing` for `call`, which riscv32 would make as the syscall `nr` with
    /// `args`: `None` runs it; otherwise the call is skipped and the result is what the caller
    /// sees, or the caller was killed.
    pub(crate) async fn secure_computing(
        &self,
        caller: &Caller<'_>,
        call: &Syscall,
        nr: u32,
        args: [u64; 6],
    ) -> Option<SysResult<SyscallRet>> {
        if !self.seccomp_used.load(Ordering::Acquire) {
            return None;
        }
        let (mode, filter) = self.task_ref(caller.task(), |t| {
            (t.seccomp.mode, t.seccomp.filter.clone())
        });
        let verdict = match mode {
            0 => return None,
            1 if MODE1_SYSCALLS.contains(&nr) => Verdict::Allow,
            // `__secure_computing_strict`: `do_exit(SIGKILL)`.
            1 => Verdict::KillThread(Signal::SIGKILL.0),
            2 => self.run_filters(caller, filter?, nr, args),
            _ => Verdict::KillThread(Signal::SIGKILL.0),
        };
        Some(match verdict {
            Verdict::Allow => return None,
            Verdict::Skip(raw) => skipped(call, raw),
            Verdict::KillThread(sig) => self.seccomp_kill(caller, sig, false).await,
            Verdict::KillProcess(sig) => self.seccomp_kill(caller, sig, true).await,
            Verdict::Notify(filter, data) => match &filter.notif {
                None => skipped(call, -i64::from(Errno::ENOSYS.0)),
                Some(notif) => match self.seccomp_notify(caller, notif, data).await {
                    Ok(notify::Outcome::Run) => return None,
                    Ok(notify::Outcome::Return(raw)) => skipped(call, raw),
                    Err(err) => Err(err),
                },
            },
        })
    }

    /// `__seccomp_filter` past strict mode: runs every filter and acts on the result that
    /// takes precedence (`seccomp_run_filters`).
    fn run_filters(
        &self,
        caller: &Caller<'_>,
        filter: Arc<Filter>,
        nr: u32,
        args: [u64; 6],
    ) -> Verdict {
        let mut bytes = Data([0; SeccompData::SIZE]);
        data_of(nr, args).write_to(&mut bytes.0);
        let mut ret = SeccompRet::SECCOMP_RET_ALLOW.0;
        let mut matched = &filter;
        let mut walker = Some(&filter);
        while let Some(f) = walker {
            let cur = f.prog.run(&bytes);
            if precedes(ret, cur) != ret {
                ret = cur;
                matched = f;
            }
            walker = f.prev.as_ref();
        }
        let action = ret & SeccompRet::SECCOMP_RET_ACTION_FULL.0;
        let data = ret & SeccompRet::SECCOMP_RET_DATA.0;
        // `seccomp_log` and `SECCOMP_FILTER_FLAG_LOG` record actions with the audit
        // subsystem, which there is none of: they go nowhere, as with audit disabled.
        match SeccompRet(action) {
            SeccompRet::SECCOMP_RET_ERRNO => Verdict::Skip(-i64::from(data.min(MAX_ERRNO))),
            SeccompRet::SECCOMP_RET_TRAP => {
                // `force_sig_seccomp`; the handler sees the registers rolled back, so the
                // call returns its first argument.
                let mut info = kernel_info(Signal::SIGSYS.0);
                info.si_code = SiCode::SYS_SECCOMP.0;
                info.si_errno = data as i32;
                info.sifields[4..8].copy_from_slice(&nr.to_le_bytes());
                info.sifields[8..12]
                    .copy_from_slice(&AuditArch::AUDIT_ARCH_RISCV32.0.to_le_bytes());
                self.task_signals(caller.task()).force(info);
                Verdict::Skip(i64::from(args[0] as u32 as i32))
            }
            // Without a tracer, as there is none: `ENOSYS`.
            SeccompRet::SECCOMP_RET_TRACE => Verdict::Skip(-i64::from(Errno::ENOSYS.0)),
            SeccompRet::SECCOMP_RET_USER_NOTIF => {
                Verdict::Notify(matched.clone(), data_of(nr, args))
            }
            SeccompRet::SECCOMP_RET_LOG | SeccompRet::SECCOMP_RET_ALLOW => Verdict::Allow,
            SeccompRet::SECCOMP_RET_KILL_THREAD => Verdict::KillThread(Signal::SIGSYS.0),
            // `SECCOMP_RET_KILL_PROCESS`, and every action Linux does not know.
            _ => Verdict::KillProcess(Signal::SIGSYS.0),
        }
    }

    /// The kill actions: the task dies of `sig`, or its whole group when `process` is set or
    /// it is the last task (Linux dumps core for the group then); it runs no syscall
    /// meanwhile (`SECCOMP_MODE_DEAD`).
    async fn seccomp_kill(
        &self,
        caller: &Caller<'_>,
        sig: i32,
        process: bool,
    ) -> SysResult<SyscallRet> {
        let task = caller.task();
        self.with_task(task, |t| t.seccomp.mode = MODE_DEAD);
        let group = process || self.process(task).group.single_threaded();
        match group {
            true => {
                self.force_fatal_signal(task, sig);
            }
            false => self.sys_exit(caller, sig).await?,
        }
        Err(SysErr::Errno(Errno::ENOSYS))
    }

    /// `seccomp(2)`.
    pub(crate) async fn sys_seccomp(
        &self,
        caller: &Caller<'_>,
        op: u32,
        flags: u32,
        uargs: VoidPtr,
    ) -> SysResult<i32> {
        match SeccompOp(op) {
            SeccompOp::SECCOMP_SET_MODE_STRICT => {
                if flags != 0 || !uargs.is_null() {
                    return Err(Errno::EINVAL.into());
                }
                self.set_mode_strict(caller)
            }
            SeccompOp::SECCOMP_SET_MODE_FILTER => self.set_mode_filter(caller, flags, uargs).await,
            SeccompOp::SECCOMP_GET_ACTION_AVAIL => {
                if flags != 0 {
                    return Err(Errno::EINVAL.into());
                }
                let action: u32 =
                    read_value(caller.memory(), GuestPtr::<u32>::new(uargs.addr())).await?;
                match SeccompRet(action) {
                    SeccompRet::SECCOMP_RET_KILL_PROCESS
                    | SeccompRet::SECCOMP_RET_KILL_THREAD
                    | SeccompRet::SECCOMP_RET_TRAP
                    | SeccompRet::SECCOMP_RET_ERRNO
                    | SeccompRet::SECCOMP_RET_USER_NOTIF
                    | SeccompRet::SECCOMP_RET_TRACE
                    | SeccompRet::SECCOMP_RET_LOG
                    | SeccompRet::SECCOMP_RET_ALLOW => Ok(0),
                    _ => Err(Errno::EOPNOTSUPP.into()),
                }
            }
            SeccompOp::SECCOMP_GET_NOTIF_SIZES => {
                if flags != 0 {
                    return Err(Errno::EINVAL.into());
                }
                let sizes = SeccompNotifSizes {
                    seccomp_notif: SeccompNotif::SIZE as u16,
                    seccomp_notif_resp: SeccompNotifResp::SIZE as u16,
                    seccomp_data: SeccompData::SIZE as u16,
                };
                let at = GuestPtrMut::<SeccompNotifSizes>::new(uargs.addr());
                write_value(caller.memory(), at, &sizes).await?;
                Ok(0)
            }
            _ => Err(Errno::EINVAL.into()),
        }
    }

    /// `prctl(PR_SET_SECCOMP)` (`prctl_set_seccomp`): strict mode ignores `filter`, and there
    /// are no flags.
    pub(crate) async fn prctl_set_seccomp(
        &self,
        caller: &Caller<'_>,
        mode: u32,
        filter: u32,
    ) -> SysResult<i32> {
        match SeccompMode(mode) {
            SeccompMode::SECCOMP_MODE_STRICT => self.set_mode_strict(caller),
            SeccompMode::SECCOMP_MODE_FILTER => {
                self.set_mode_filter(caller, 0, VoidPtr::new(filter)).await
            }
            _ => Err(Errno::EINVAL.into()),
        }
    }

    /// `prctl(PR_GET_SECCOMP)`: the caller's mode.
    pub(crate) fn prctl_get_seccomp(&self, caller: &Caller<'_>) -> i32 {
        self.task_ref(caller.task(), |t| t.seccomp.mode) as i32
    }

    /// `PR_SET_NO_NEW_PRIVS`, `PR_GET_NO_NEW_PRIVS`: the caller's own flag.
    pub(crate) fn set_no_new_privs(&self, caller: &Caller<'_>) {
        self.with_task(caller.task(), |t| t.seccomp.no_new_privs = true);
    }

    pub(crate) fn no_new_privs(&self, caller: &Caller<'_>) -> bool {
        self.task_ref(caller.task(), |t| t.seccomp.no_new_privs)
    }

    /// `seccomp_set_mode_strict`.
    fn set_mode_strict(&self, caller: &Caller<'_>) -> SysResult<i32> {
        let mode = SeccompMode::SECCOMP_MODE_STRICT.0;
        self.with_task(caller.task(), |t| {
            if !t.seccomp.may_assign_mode(mode) {
                return Err(Errno::EINVAL);
            }
            t.seccomp.mode = mode;
            Ok(())
        })?;
        self.seccomp_used.store(true, Ordering::Release);
        Ok(0)
    }

    /// `seccomp_set_mode_filter`: checks and attaches the program `fprog` points to, for the
    /// caller, or with `SECCOMP_FILTER_FLAG_TSYNC` for its whole group. With TSYNC a thread
    /// that cannot take the caller's filters (in strict mode, or with filters of its own) makes
    /// the call return its tid, or fail with `ESRCH` with `SECCOMP_FILTER_FLAG_TSYNC_ESRCH`.
    async fn set_mode_filter(
        &self,
        caller: &Caller<'_>,
        flags: u32,
        fprog: VoidPtr,
    ) -> SysResult<i32> {
        if flags & !FILTER_FLAGS != 0 {
            return Err(Errno::EINVAL.into());
        }
        let flags = SeccompFilterFlags(flags);
        let listen = flags.contains(SeccompFilterFlags::SECCOMP_FILTER_FLAG_NEW_LISTENER);
        // A tid and a listener would be one result; TSYNC_ESRCH makes failure an errno.
        if flags.contains(SeccompFilterFlags::SECCOMP_FILTER_FLAG_TSYNC)
            && listen
            && !flags.contains(SeccompFilterFlags::SECCOMP_FILTER_FLAG_TSYNC_ESRCH)
        {
            return Err(Errno::EINVAL.into());
        }
        let killable = flags.contains(SeccompFilterFlags::SECCOMP_FILTER_FLAG_WAIT_KILLABLE_RECV);
        if killable && !listen {
            return Err(Errno::EINVAL.into());
        }
        let prog = self.prepare_filter(caller, fprog).await?;
        let listener = match listen {
            true => {
                let [fd] = self.reserve_fds(caller, true)?;
                let mut first = [0; 8];
                self.host.random(&mut first);
                Some((fd, notify::Notif::new(u64::from_le_bytes(first), killable)))
            }
            false => None,
        };
        let task = caller.task();
        let mode = SeccompMode::SECCOMP_MODE_FILTER.0;
        let mut state = self.state.lock_or_recover();
        let me = state
            .tasks
            .get(&task)
            .expect("syscall from an unknown task");
        if !me.seccomp.may_assign_mode(mode) {
            return Err(Errno::EINVAL.into());
        }
        // `seccomp_attach_filter`.
        let mut total = prog.len();
        let mut walker = me.seccomp.filter.as_ref();
        while let Some(f) = walker {
            total += f.prog.len() + 4;
            walker = f.prev.as_ref();
        }
        if total > MAX_INSNS_PER_PATH {
            return Err(Errno::ENOMEM.into());
        }
        let tsync = flags.contains(SeccompFilterFlags::SECCOMP_FILTER_FLAG_TSYNC);
        let process = me.process.clone();
        if tsync {
            let mine = me.seccomp.filter.clone();
            let blocker = state.tasks.iter().find(|(id, t)| {
                **id != task
                    && Arc::ptr_eq(&t.process, &process)
                    && !t.thread.exited()
                    && !may_sync(&t.seccomp, mine.as_ref())
            });
            if let Some((&id, _)) = blocker {
                drop(state);
                if flags.contains(SeccompFilterFlags::SECCOMP_FILTER_FLAG_TSYNC_ESRCH) {
                    return Err(Errno::ESRCH.into());
                }
                return match self.own_tid(id) {
                    0 => Err(Errno::ESRCH.into()),
                    tid => Ok(tid),
                };
            }
        }
        let me = state
            .tasks
            .get_mut(&task)
            .expect("syscall from an unknown task");
        // `has_duplicate_listener`: one listener per chain of filters.
        if listener.is_some() {
            let mut walker = me.seccomp.filter.as_ref();
            while let Some(f) = walker {
                if f.notif.is_some() {
                    return Err(Errno::EBUSY.into());
                }
                walker = f.prev.as_ref();
            }
        }
        let filter = Arc::new(Filter {
            prog,
            prev: me.seccomp.filter.take(),
            notif: listener.as_ref().map(|(_, n)| n.clone()),
        });
        if let Some((_, notif)) = &listener {
            notif.set_filter(&filter);
        }
        me.seccomp.filter = Some(filter);
        me.seccomp.filters += 1;
        me.seccomp.mode = mode;
        let mine = me.seccomp.clone();
        // `seccomp_sync_threads`: not for a group on its way out.
        if tsync && !process.group.exiting() {
            for (_, t) in state.tasks.iter_mut().filter(|(id, t)| {
                **id != task && Arc::ptr_eq(&t.process, &process) && !t.thread.exited()
            }) {
                sync_thread(t, &mine);
            }
        }
        drop(state);
        self.seccomp_used.store(true, Ordering::Release);
        Ok(match listener {
            None => 0,
            Some((fd, notif)) => {
                let object = Arc::new(Listener(notif));
                let file = OpenFile::new(
                    Backing::Stream(object, self.pseudo.anon("seccomp notify")),
                    wasixv2_abi::OpenFlags::O_RDWR,
                );
                fd.install(Arc::new(file)).0
            }
        })
    }

    /// `seccomp_prepare_user_filter`: reads the `sock_fprog` and its program, which takes
    /// `no_new_privs` or `CAP_SYS_ADMIN` over the caller's user namespace (`EACCES`).
    async fn prepare_filter(&self, caller: &Caller<'_>, fprog: VoidPtr) -> SysResult<bpf::Program> {
        let fprog: SockFprog = read_value(caller.memory(), GuestPtr::new(fprog.addr())).await?;
        let len = usize::from(fprog.len);
        if len == 0 || len > bpf::MAX_PROGRAM_LEN / bpf::INSN_LEN {
            return Err(Errno::EINVAL.into());
        }
        let cred = self.cred(caller);
        if !self.no_new_privs(caller) && !cred.ns_capable(&cred.user_ns, Cap::SysAdmin) {
            return Err(Errno::EACCES.into());
        }
        let mut raw = vec![0; len * bpf::INSN_LEN];
        caller.memory().read(fprog.filter.addr(), &mut raw).await?;
        Ok(bpf::Program::seccomp(&raw, SeccompData::SIZE as u32)?)
    }
}

/// `populate_seccomp_data`: wasm has no program counter.
fn data_of(nr: u32, args: [u64; 6]) -> SeccompData {
    SeccompData {
        nr: nr as i32,
        arch: AuditArch::AUDIT_ARCH_RISCV32.0,
        instruction_pointer: 0,
        args,
    }
}

/// What a skipped `call` returns when its result register holds `raw`: an error for the values
/// that are `-errno`.
fn skipped(call: &Syscall, raw: i64) -> SysResult<SyscallRet> {
    match raw {
        -4095..=-1 => Err(SysErr::Errno(Errno(-raw as i32))),
        _ => Ok(call.ret_from_raw(raw)),
    }
}

/// `seccomp_run_filters`' choice between two results: the one whose action comes first
/// (`ACTION_ONLY` compares as signed, so `SECCOMP_RET_KILL_PROCESS` beats everything), the
/// earlier of two equal actions.
fn precedes(ret: u32, other: u32) -> u32 {
    let action = |ret: u32| (ret & SeccompRet::SECCOMP_RET_ACTION_FULL.0) as i32;
    match action(other) < action(ret) {
        true => other,
        false => ret,
    }
}

/// Whether a thread with `thread`'s state may take filters whose leaf is `caller`'s: it has
/// none, or only some of the caller's.
fn may_sync(thread: &Seccomp, caller: Option<&Arc<Filter>>) -> bool {
    match thread.mode {
        0 => true,
        2 => thread
            .filter
            .as_ref()
            .is_none_or(|f| f.is_ancestor_of(caller)),
        _ => false,
    }
}

/// `seccomp_sync_threads` for one thread: the caller's filters, and its `no_new_privs`, so a
/// thread cannot shed the restriction its filters were installed under.
fn sync_thread(thread: &mut Task, caller: &Seccomp) {
    thread.seccomp.filter = caller.filter.clone();
    thread.seccomp.filters = caller.filters;
    if caller.no_new_privs {
        thread.seccomp.no_new_privs = true;
    }
    if thread.seccomp.mode == SeccompMode::SECCOMP_MODE_DISABLED.0 {
        thread.seccomp.mode = SeccompMode::SECCOMP_MODE_FILTER.0;
    }
}
