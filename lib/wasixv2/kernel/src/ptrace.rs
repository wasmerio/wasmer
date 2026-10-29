//! Process tracing (kernel/ptrace.c): who may look into or attach to another process
//! (`ptrace_may_access`, with the capability, Yama and Landlock rules of the LSMs this kernel
//! has).

use std::sync::Arc;

use wasixv2_abi::GuestLayout as _;
use wasixv2_syscalls::TaskId;

use wasixv2_vfs::Cap;

use crate::{Kernel, Process, landlock::Domain, sync::MutexExt as _, yama};

/// `PTRACE_MODE_READ`: to read the target's state.
pub(crate) const MODE_READ: u32 = 0x01;
/// `PTRACE_MODE_ATTACH`: to attach to the target, or change it.
pub(crate) const MODE_ATTACH: u32 = 0x02;
/// `PTRACE_MODE_FSCREDS`: the caller's filesystem ids and effective capabilities count.
pub(crate) const MODE_FSCREDS: u32 = 0x08;
/// `PTRACE_MODE_REALCREDS`: the caller's real ids and permitted capabilities count.
pub(crate) const MODE_REALCREDS: u32 = 0x10;

pub(crate) const MODE_READ_FSCREDS: u32 = MODE_READ | MODE_FSCREDS;
pub(crate) const MODE_READ_REALCREDS: u32 = MODE_READ | MODE_REALCREDS;
pub(crate) const MODE_ATTACH_FSCREDS: u32 = MODE_ATTACH | MODE_FSCREDS;
pub(crate) const MODE_ATTACH_REALCREDS: u32 = MODE_ATTACH | MODE_REALCREDS;

impl Kernel {
    /// `ptrace_may_access`: whether the task `tracer` may access `target` in `mode`. Tasks of
    /// one thread group always may; otherwise the ids and dumpability (`__ptrace_may_access`),
    /// then the LSMs: capabilities (`cap_ptrace_access_check`), Yama for attaching
    /// (`yama_ptrace_access_check`), Landlock (`hook_ptrace_access_check`). A task that is gone
    /// may not be accessed.
    pub(crate) fn ptrace_may_access(&self, tracer: TaskId, target: TaskId, mode: u32) -> bool {
        let (me, them) = {
            let state = self.state.read_or_recover();
            match (state.tasks.get(&tracer), state.tasks.get(&target)) {
                (Some(me), Some(them)) => (me.process.clone(), them.process.clone()),
                _ => return false,
            }
        };
        self.process_may_access(&me, &them, mode)
    }

    /// [`Kernel::ptrace_may_access`] between processes.
    pub(crate) fn process_may_access(
        &self,
        me: &Arc<Process>,
        them: &Arc<Process>,
        mode: u32,
    ) -> bool {
        if Arc::ptr_eq(me, them) {
            return true;
        }
        let (cred, tcred) = (me.cred(), them.cred());
        let dumpable = them.ident.lock_or_recover().dumpable;
        if !cred.may_read_task(&tcred, dumpable, mode & MODE_FSCREDS != 0) {
            return false;
        }
        // `cap_ptrace_access_check`: the target's permitted capabilities are the caller's in
        // the same user namespace, or the caller has `CAP_SYS_PTRACE` over the target's.
        let caller_caps = match mode & MODE_FSCREDS != 0 {
            true => cred.effective,
            false => cred.permitted,
        };
        let subset =
            Arc::ptr_eq(&cred.user_ns, &tcred.user_ns) && tcred.permitted & !caller_caps == 0;
        let ptrace_cap = cred.ns_capable(&tcred.user_ns, Cap::SysPtrace);
        if !subset && !ptrace_cap {
            return false;
        }
        if mode & MODE_ATTACH != 0 && !self.yama_may_attach(me, them, ptrace_cap) {
            return false;
        }
        Domain::may_ptrace(cred.landlock.as_ref(), tcred.landlock.as_ref())
    }

    /// `yama_ptrace_access_check` for an attach of `me` to `them`; `ptrace_cap` is whether `me`
    /// has `CAP_SYS_PTRACE` over `them`'s user namespace.
    fn yama_may_attach(&self, me: &Process, them: &Process, ptrace_cap: bool) -> bool {
        match self.yama.scope() {
            yama::SCOPE_DISABLED => true,
            yama::SCOPE_RELATIONAL => {
                ptrace_cap
                    || self.is_descendant(me.pid.0, them.pid.0)
                    || self.ptracer_exception_found(me.pid.0, them.pid.0)
            }
            yama::SCOPE_CAPABILITY => ptrace_cap,
            _ => false,
        }
    }

    /// `yama_ptrace_traceme`: whether `parent` may become the tracer of `me` through
    /// `PTRACE_TRACEME`, and Landlock's `hook_ptrace_traceme`.
    pub(crate) fn may_traceme(&self, me: &Process, parent: &Process) -> bool {
        let (cred, pcred) = (me.cred(), parent.cred());
        let yama = match self.yama.scope() {
            yama::SCOPE_CAPABILITY => pcred.ns_capable(&cred.user_ns, Cap::SysPtrace),
            yama::SCOPE_NO_ATTACH => false,
            _ => true,
        };
        yama && Domain::may_ptrace(pcred.landlock.as_ref(), cred.landlock.as_ref())
    }

    /// `task_is_descendant`: whether the process `child` is `ancestor` or below it, by its
    /// parents.
    pub(crate) fn is_descendant(&self, ancestor: u32, child: u32) -> bool {
        let tree = self.procs.tree.lock_or_recover();
        let mut walker = child;
        while walker != 0 {
            if walker == ancestor {
                return true;
            }
            walker = tree.nodes.get(&walker).map_or(0, |node| node.parent);
        }
        false
    }

    /// `ptracer_exception_found`: whether `tracee` named `tracer`, or one of its ancestors, or
    /// anyone, with `PR_SET_PTRACER`.
    fn ptracer_exception_found(&self, tracer: u32, tracee: u32) -> bool {
        match self.yama.exception(tracee) {
            Some(None) => true,
            Some(Some(allowed)) => self.is_descendant(allowed, tracer),
            None => false,
        }
    }
}

impl Kernel {
    /// `yama_task_prctl`'s `PR_SET_PTRACER` for the caller's thread group: 0 forgets the
    /// exception, `PR_SET_PTRACER_ANY` allows anyone, a pid allows that task's process and its
    /// descendants (`EINVAL` for a pid the caller cannot see).
    pub(crate) fn yama_set_ptracer(
        &self,
        caller: &wasixv2_syscalls::Caller<'_>,
        arg2: u32,
    ) -> wasixv2_syscalls::SysResult<i32> {
        let me = self.process(caller.task()).pid.0;
        match arg2 {
            0 => self.yama.del(me),
            u32::MAX => self.yama.add(None, me),
            pid => {
                // `find_get_task_by_vpid`: any task the caller sees.
                let tracer = (pid as i32 > 0)
                    .then(|| self.find_vpid(caller, pid as i32))
                    .flatten()
                    .and_then(|tid| self.task_process(TaskId(tid)))
                    .ok_or(wasixv2_abi::Errno::EINVAL)?;
                self.yama.add(Some(tracer.pid.0), me);
            }
        }
        Ok(0)
    }
}

impl Kernel {
    /// The process of `task`, if the task is in the table.
    pub(crate) fn task_process(&self, task: TaskId) -> Option<Arc<Process>> {
        let state = self.state.read_or_recover();
        state.tasks.get(&task).map(|t| t.process.clone())
    }
}

/// `PIDFD_SELF_THREAD`: a pidfd argument that names the calling thread.
const PIDFD_SELF_THREAD: i32 = -10000;
/// `PIDFD_SELF_THREAD_GROUP`: a pidfd argument that names the caller's thread group leader.
const PIDFD_SELF_THREAD_GROUP: i32 = -10001;

impl Kernel {
    /// `pidfd_get_task`: the task a pidfd argument names: the caller or its leader for
    /// `PIDFD_SELF_*`, else the thread group leader of the pidfd's process (`ESRCH` once it
    /// is gone).
    pub(crate) fn pidfd_get_task(
        &self,
        caller: &wasixv2_syscalls::Caller<'_>,
        pidfd: wasixv2_abi::Fd,
    ) -> Result<TaskId, wasixv2_abi::Errno> {
        let task = match pidfd.0 {
            PIDFD_SELF_THREAD => caller.task(),
            PIDFD_SELF_THREAD_GROUP => self.process(caller.task()).pid,
            _ => TaskId(self.pidfd_pid(caller, pidfd)?),
        };
        match self.task_process(task) {
            Some(_) => Ok(task),
            None => Err(wasixv2_abi::Errno::ESRCH),
        }
    }

    /// `mm_access`: `task`'s address space, for a caller in it or one that may access the
    /// task in `mode` (or, to read, has `CAP_PERFMON`: `perfmon_capable`); `EACCES` otherwise,
    /// `ESRCH` for a task that left its memory.
    pub(crate) fn mm_access(
        &self,
        caller: TaskId,
        task: TaskId,
        mode: u32,
    ) -> Result<Arc<crate::mm::Mm>, wasixv2_abi::Errno> {
        let (mm, mine) = {
            let state = self.state.read_or_recover();
            let t = state.tasks.get(&task).ok_or(wasixv2_abi::Errno::ESRCH)?;
            if t.thread.left_mm() {
                return Err(wasixv2_abi::Errno::ESRCH);
            }
            let mine = state.tasks.get(&caller).map(|c| c.mm.clone());
            (t.mm.clone(), mine)
        };
        if mine.is_some_and(|mine| Arc::ptr_eq(&mine, &mm))
            || self.ptrace_may_access(caller, task, mode)
        {
            return Ok(mm);
        }
        let perfmon = || {
            let cred = self.process(caller).cred();
            cred.capable_init(Cap::Perfmon) || cred.capable_init(Cap::SysAdmin)
        };
        match mode & MODE_READ != 0 && perfmon() {
            true => Ok(mm),
            false => Err(wasixv2_abi::Errno::EACCES),
        }
    }
}

/// What [`access_remote_vm`] copies: out of the address space into the buffer, or into it.
pub(crate) enum Access<'a> {
    Read(&'a mut [u8]),
    Write(&'a [u8]),
}

/// `access_remote_vm`: copies between `buf` and the address space `mm` from `addr`, a page at a
/// time, up to the first page that faults, and returns how many bytes it copied: 0 for an
/// address space nobody else can reach (its memory is not shared, or its last user left it).
/// With `force` (`FOLL_FORCE`, as ptrace and `/proc/<pid>/mem` copy) the pages' protection does
/// not count, but what the host may not write (the low guard, read-only shared views) still
/// faults.
pub(crate) async fn access_remote_vm(
    mm: &crate::mm::Mm,
    addr: u32,
    buf: Access<'_>,
    force: bool,
) -> usize {
    let Some(memory) = mm.foreign() else {
        return 0;
    };
    let page = crate::mm::PAGE_SIZE;
    let len = match &buf {
        Access::Read(buf) => buf.len(),
        Access::Write(buf) => buf.len(),
    };
    let mut done = 0;
    let mut buf = buf;
    while done < len {
        let at = u64::from(addr) + done as u64;
        let Ok(at32) = u32::try_from(at) else {
            break;
        };
        let n = ((page - at % page) as usize).min(len - done);
        let copied = match (&mut buf, force) {
            (Access::Read(buf), false) => memory.read(at32, &mut buf[done..done + n]).await,
            (Access::Read(buf), true) => memory.force_read(at32, &mut buf[done..done + n]).await,
            (Access::Write(buf), false) => memory.write(at32, &buf[done..done + n]).await,
            (Access::Write(buf), true) => memory.force_write(at32, &buf[done..done + n]).await,
        };
        if copied.is_err() {
            break;
        }
        done += n;
    }
    done
}

// ---- the tracee's state ----

/// A tracee's link to its tracer (`task->ptrace` and `task->parent`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Link {
    /// The tracing task, whose ptrace requests alone the tracee answers.
    pub(crate) tracer: TaskId,
    /// Attached with `PTRACE_SEIZE` (`PT_SEIZED`): group stops are `PTRACE_EVENT_STOP`s, and
    /// `PTRACE_INTERRUPT` and `PTRACE_LISTEN` work.
    pub(crate) seized: bool,
    /// The `PTRACE_O_*` options.
    pub(crate) options: u32,
}

impl Link {
    pub(crate) fn has(&self, option: wasixv2_abi::PtraceOptions) -> bool {
        self.options & option.0 != 0
    }
}

/// The call a tracee stopped in, for `PTRACE_GET_SYSCALL_INFO`: the riscv32 number and
/// registers, and once it returned, its raw result.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CallInfo {
    pub(crate) nr: u32,
    pub(crate) args: [u64; 6],
    pub(crate) ret: Option<i64>,
    /// `SECCOMP_RET_DATA` of a `PTRACE_EVENT_SECCOMP` stop.
    pub(crate) seccomp_data: u32,
    /// The guest's stack pointer as the call found it.
    pub(crate) sp: u32,
}

/// What ptrace keeps on a task (`task->ptrace`, `exit_code`, `ptrace_message`,
/// `last_siginfo` and the jobctl trap bits), under its signal group's lock as Linux keeps it
/// under `siglock`.
#[derive(Clone, Debug, Default)]
pub(crate) struct TaskTrace {
    pub(crate) link: Option<Link>,
    /// In a ptrace stop (`JOBCTL_TRACED`, `TASK_TRACED`): only the tracer, SIGKILL, or the
    /// tracer's going ends it.
    pub(crate) traced: bool,
    /// What the stop reports until a wait takes it, and once the stop is over what the tracer
    /// resumed the task with (`exit_code`).
    pub(crate) exit_code: i32,
    pub(crate) message: u64,
    pub(crate) last_siginfo: Option<wasixv2_abi::Siginfo>,
    /// A stop [`Kernel::take_signal`] entered, whose end it acts on as the task comes back.
    pub(crate) sync_stop: Option<SyncStop>,
    /// `JOBCTL_LISTENING`: `PTRACE_LISTEN` lets the tracee sit in its stop without being
    /// reported, until a stop or SIGCONT notifies (`JOBCTL_TRAP_NOTIFY`).
    pub(crate) listening: bool,
    /// `JOBCTL_TRAP_STOP`: a trap stop is due (`PTRACE_INTERRUPT`, an attach to a stopped task,
    /// a seized task's group stop).
    pub(crate) trap_stop: bool,
    /// `JOBCTL_TRAP_NOTIFY`.
    pub(crate) trap_notify: bool,
    /// `SYSCALL_TRACE`: stop at every syscall entry and exit (`PTRACE_SYSCALL`).
    pub(crate) syscall: bool,
    /// The call the task is in while it is traced.
    pub(crate) call: Option<CallInfo>,
    /// The event stop a `clone` of the task owes its tracer once the child is made
    /// (`ptrace_event_pid`), with the child.
    pub(crate) clone_event: Option<(i32, TaskId)>,
}

/// A stop the return-to-guest path entered, which the execution layer waits in
/// ([`GuestAction::Stopped`]); the next [`Kernel::take_signal`] finishes it.
///
/// [`GuestAction::Stopped`]: wasixv2_syscalls::GuestAction::Stopped
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SyncStop {
    /// A signal-delivery stop: `get_signal` delivers what the tracer chose (`ptrace_signal`).
    Signal,
    /// A trap stop (`do_jobctl_trap`): nothing follows.
    Trap,
}

/// `PTRACE_EVENT_STOP`, the event of a seized tracee's trap stops.
pub(crate) const EVENT_STOP: i32 = wasixv2_abi::PtraceEvent::PTRACE_EVENT_STOP.0;

// ---- who traces whom ----

/// A traced task as its tracer's `wait` sees it (`tracer->ptraced`), until the tracer waited
/// for its end.
pub(crate) struct Tracee {
    pub(crate) tracer: TaskId,
    /// The tracer's thread group, whose every task may wait for the tracee.
    pub(crate) tracer_pid: u32,
    pub(crate) process: Arc<Process>,
    pub(crate) signals: Arc<crate::signal::TaskSignals>,
    /// The tracee ended (an exited thread, `EXIT_ZOMBIE` for its tracer): what `wait` reports.
    pub(crate) exit: Option<crate::process::Exit>,
}

/// Every traced task, by task id.
#[derive(Default)]
pub(crate) struct Tracees {
    map: std::sync::Mutex<std::collections::HashMap<TaskId, Tracee>>,
    /// Some task was ever traced: until then syscall entry and exit look at nothing.
    used: std::sync::atomic::AtomicBool,
}

impl Tracees {
    fn lock(&self) -> std::sync::MutexGuard<'_, std::collections::HashMap<TaskId, Tracee>> {
        self.map.lock_or_recover()
    }

    pub(crate) fn used(&self) -> bool {
        self.used.load(std::sync::atomic::Ordering::Acquire)
    }

    /// The tracer of `task`, while it is traced.
    pub(crate) fn tracer_of(&self, task: TaskId) -> Option<TaskId> {
        self.lock()
            .get(&task)
            .filter(|t| t.exit.is_none())
            .map(|t| t.tracer)
    }
}

/// `PTRACE_O_*` bit of the event `event` (`PT_EVENT_FLAG`).
fn event_flag(event: i32) -> u32 {
    1 << event
}

/// `valid_signal`.
fn valid_signal(data: u32) -> Option<i32> {
    (data <= crate::signal::NSIG as u32).then_some(data as i32)
}

impl Kernel {
    /// `ptrace_link`: `task` is traced by `link.tracer` from now on.
    fn link_tracee(
        &self,
        task: TaskId,
        link: Link,
        with_sigstop: bool,
    ) -> Result<(), wasixv2_abi::Errno> {
        let signals = self.task_signals(task);
        let process = self.task_process(task).ok_or(wasixv2_abi::Errno::ESRCH)?;
        let mut map = self.tracees.lock();
        if map.get(&task).is_some_and(|t| t.exit.is_none()) {
            return Err(wasixv2_abi::Errno::EPERM);
        }
        let tracer_pid = self
            .task_process(link.tracer)
            .ok_or(wasixv2_abi::Errno::ESRCH)?
            .pid
            .0;
        signals.group().ptrace_link(task, link, with_sigstop)?;
        map.insert(
            task,
            Tracee {
                tracer: link.tracer,
                tracer_pid,
                process,
                signals: signals.clone(),
                exit: None,
            },
        );
        self.tracees
            .used
            .store(true, std::sync::atomic::Ordering::Release);
        Ok(())
    }

    /// `__ptrace_unlink` and the tracer's list: `task` goes on with `exit_code` if given.
    fn unlink_tracee(&self, task: TaskId, exit_code: Option<i32>) {
        let tracee = self.tracees.lock().remove(&task);
        if let Some(tracee) = tracee {
            tracee.signals.group().ptrace_unlink(task, exit_code);
        }
        self.procs.changed.wake(wasixv2_wait::PollMask::empty());
    }

    /// `do_notify_parent_cldstop(tsk, true, why)`: the tracer of `task` hears of its stop
    /// (`CLD_TRAPPED`, or `CLD_STOPPED` in a group stop), with SIGCHLD unless it set
    /// `SA_NOCLDSTOP`, and its waiters look again.
    pub(crate) fn ptrace_notify_tracer(&self, task: TaskId, why: wasixv2_abi::SiCode, status: i32) {
        if let Some(tracer) = self.tracees.tracer_of(task) {
            self.signal_tracer(task, tracer, why, status, true);
        }
        self.procs.changed.wake(wasixv2_wait::PollMask::empty());
    }

    /// SIGCHLD to `tracer` about `task`, as `si_pid` the task's tid in the tracer's
    /// namespace; `stop` honours the tracer's `SA_NOCLDSTOP`.
    fn signal_tracer(
        &self,
        task: TaskId,
        tracer: TaskId,
        why: wasixv2_abi::SiCode,
        status: i32,
        stop: bool,
    ) {
        let (Some(them), Some(me)) = (self.task_process(tracer), self.task_process(task)) else {
            return;
        };
        let Some(signals) = self.group_signals(them.pid.0) else {
            return;
        };
        if stop && signals.nocldstop() {
            return;
        }
        let exit = crate::process::Exit {
            code: why,
            status,
            uid: me.cred().user.real,
        };
        let pid = self.pids.nr(&them.pid_ns, task.0);
        let sig = wasixv2_abi::Signal::SIGCHLD.0;
        let info = crate::process::child_info(sig, pid, exit, &them.cred().user_ns);
        let _ = signals.send_group(info);
    }

    /// `ptrace_stop` from a syscall: `task` stops for its tracer until resumed, SIGKILL comes
    /// or the tracer goes, and returns what it was resumed with. It does not stop when it is
    /// not traced or SIGKILL is pending, and returns `exit_code` then.
    pub(crate) async fn ptrace_stop(&self, task: TaskId, stop: crate::signal::TraceStop) -> i32 {
        let signals = self.task_signals(task);
        let (exit_code, why) = (stop.exit_code, stop.why);
        if signals.group().enter_trace_stop(task, stop).is_none() {
            return exit_code;
        }
        self.ptrace_notify_tracer(task, why, exit_code & 0x7f);
        let resumed = || (!signals.is_traced()).then_some(());
        wasixv2_wait::wait_event(signals.sent(), resumed, &wasixv2_wait::NoInterrupt)
            .await
            .expect("NoInterrupt never interrupts");
        signals.group().leave_trace_stop(task).0
    }

    /// `ptrace_notify`: a SIGTRAP stop with `exit_code` (`SIGTRAP` with an event or `0x80`)
    /// and `message`.
    pub(crate) async fn ptrace_notify(&self, task: TaskId, exit_code: i32, message: u64) -> i32 {
        let Some(process) = self.task_process(task) else {
            return 0;
        };
        let sig = wasixv2_abi::Signal::SIGTRAP.0;
        let stop = crate::signal::TraceStop {
            exit_code,
            why: wasixv2_abi::SiCode::CLD_TRAPPED,
            message,
            info: Some(self.ptrace_trap_info(&process, task, sig, exit_code)),
            sync: None,
        };
        self.ptrace_stop(task, stop).await
    }

    /// The link of `task`, if it is traced.
    pub(crate) fn trace_link(&self, task: TaskId) -> Option<Link> {
        if !self.tracees.used() {
            return None;
        }
        self.task_signals(task)
            .group()
            .with_trace(task, |t| t.link)
            .flatten()
    }

    /// `ptrace_event`: the event stop `event` with `message` when the tracer asked for it; an
    /// `execve` of a task attached the old way (no `PTRACE_O_TRACEEXEC`, not seized) gets
    /// SIGTRAP instead.
    pub(crate) async fn ptrace_event(
        &self,
        task: TaskId,
        event: wasixv2_abi::PtraceEvent,
        message: u64,
    ) {
        let Some(link) = self.trace_link(task) else {
            return;
        };
        let event = event.0;
        if link.options & event_flag(event) != 0 {
            let code = wasixv2_abi::Signal::SIGTRAP.0 | event << 8;
            self.ptrace_notify(task, code, message).await;
        } else if event == wasixv2_abi::PtraceEvent::PTRACE_EVENT_EXEC.0 && !link.seized {
            self.task_signals(task)
                .group()
                .send_task_kernel(task, wasixv2_abi::Signal::SIGTRAP.0);
        }
    }

    /// `ptrace_report_syscall_entry` (`syscall_trace_enter`): records the call `task` makes,
    /// and with `PTRACE_SYSCALL` stops for the tracer. Returns whether the call is to be
    /// skipped: a fatal signal came meanwhile.
    pub(crate) async fn ptrace_syscall_entry(
        &self,
        caller: &wasixv2_syscalls::Caller<'_>,
        nr: u32,
        args: [u64; 6],
    ) -> bool {
        let task = caller.task();
        let Some(link) = self.trace_link(task) else {
            return false;
        };
        let sp = caller.memory().stack_pointer().await;
        let signals = self.task_signals(task);
        let call = CallInfo {
            nr,
            args,
            ret: None,
            seccomp_data: 0,
            sp,
        };
        let trace = signals.group().with_trace(task, |t| {
            t.call = Some(call);
            t.syscall
        });
        if trace != Some(true) {
            return false;
        }
        let message = u64::from(wasixv2_abi::PtraceArg::PTRACE_EVENTMSG_SYSCALL_ENTRY.0);
        self.report_syscall(task, link, message).await;
        signals.group().sigkill_pending_for(task)
    }

    /// `ptrace_report_syscall_exit`: with `PTRACE_SYSCALL`, the call's end, with its raw
    /// result, stops for the tracer.
    pub(crate) async fn ptrace_syscall_exit(&self, task: TaskId, ret: i64) {
        let Some(link) = self.trace_link(task) else {
            return;
        };
        let signals = self.task_signals(task);
        let trace = signals.group().with_trace(task, |t| {
            if let Some(call) = &mut t.call {
                call.ret = Some(ret);
            }
            t.syscall
        });
        if trace != Some(true) {
            return;
        }
        let message = u64::from(wasixv2_abi::PtraceArg::PTRACE_EVENTMSG_SYSCALL_EXIT.0);
        self.report_syscall(task, link, message).await;
    }

    /// `ptrace_report_syscall`: SIGTRAP (with `0x80` for `PTRACE_O_TRACESYSGOOD`); a signal the
    /// tracer resumes with is sent to the task.
    async fn report_syscall(&self, task: TaskId, link: Link, message: u64) {
        let mut code = wasixv2_abi::Signal::SIGTRAP.0;
        if link.has(wasixv2_abi::PtraceOptions::PTRACE_O_TRACESYSGOOD) {
            code |= 0x80;
        }
        let sig = self.ptrace_notify(task, code, message).await;
        if sig != 0 {
            self.task_signals(task).group().send_task_kernel(task, sig);
        }
    }
}

// ---- the ptrace syscall ----

/// `sizeof(unsigned long)` on wasm32: what `PEEK*`, `POKE*` and `GETEVENTMSG` move.
const ULONG: usize = 4;
/// `sizeof(siginfo_t)`.
const SIGINFO_SIZE: u32 = wasixv2_abi::Siginfo::SIZE as u32;

impl Kernel {
    /// `ptrace(2)` (kernel/ptrace.c `SYSCALL_DEFINE4(ptrace)`, `ptrace_request`). Tracing is
    /// at the level of syscalls and signals: wasm has no registers a tracer could read or
    /// write and no instructions to step, so `PTRACE_PEEKUSR`, `PTRACE_POKEUSR`,
    /// `PTRACE_GETREGSET`, `PTRACE_SETREGSET` and `PTRACE_SINGLESTEP` fail with `EIO` as an
    /// architecture without them does. `PEEKTEXT` and `POKETEXT` are `PEEKDATA` and
    /// `POKEDATA`, as on Linux: an address is one of the linear memory, where no code is.
    pub(crate) async fn sys_ptrace(
        &self,
        caller: &wasixv2_syscalls::Caller<'_>,
        request: wasixv2_abi::PtraceRequest,
        pid: wasixv2_abi::Pid,
        addr: u32,
        data: u32,
    ) -> wasixv2_syscalls::SysResult<i32> {
        use wasixv2_abi::{Errno, PtraceRequest as R};
        let me = caller.task();
        if request == R::PTRACE_TRACEME {
            return Ok(self.ptrace_traceme(me)?);
        }
        let child = self
            .find_vpid(caller, pid)
            .map(TaskId)
            .filter(|&t| pid > 0 && self.task_process(t).is_some())
            .ok_or(Errno::ESRCH)?;
        if request == R::PTRACE_ATTACH || request == R::PTRACE_SEIZE {
            return Ok(self.ptrace_attach(me, child, request == R::PTRACE_SEIZE, addr, data)?);
        }
        let any_state = request == R::PTRACE_KILL || request == R::PTRACE_INTERRUPT;
        let signals = self.task_signals(child);
        let group = signals.group().clone();
        let link = group.ptrace_check(child, me, any_state)?;
        let memory = caller.memory();
        match request {
            R::PTRACE_PEEKTEXT | R::PTRACE_PEEKDATA => {
                let mut word = [0; ULONG];
                if self
                    .ptrace_access_vm(child, addr, Access::Read(&mut word))
                    .await
                    != ULONG
                {
                    return Err(Errno::EIO.into());
                }
                memory.write(data, &word).await?;
                Ok(0)
            }
            R::PTRACE_POKETEXT | R::PTRACE_POKEDATA => {
                let word = data.to_le_bytes();
                match self
                    .ptrace_access_vm(child, addr, Access::Write(&word))
                    .await
                {
                    ULONG => Ok(0),
                    _ => Err(Errno::EIO.into()),
                }
            }
            R::PTRACE_SETOPTIONS => {
                self.check_ptrace_options(me, data)?;
                group.with_trace(child, |t| {
                    if let Some(link) = &mut t.link {
                        link.options = data;
                    }
                });
                Ok(0)
            }
            R::PTRACE_GETEVENTMSG => {
                let message = group.with_trace(child, |t| t.message).unwrap_or(0);
                memory.write(data, &(message as u32).to_le_bytes()).await?;
                Ok(0)
            }
            R::PTRACE_GETSIGINFO => {
                let info = group.with_trace(child, |t| t.last_siginfo).flatten();
                let info = info.ok_or(Errno::EINVAL)?;
                wasixv2_syscalls::write_value(memory, wasixv2_abi::GuestPtrMut::new(data), &info)
                    .await?;
                Ok(0)
            }
            R::PTRACE_SETSIGINFO => {
                let info: wasixv2_abi::Siginfo =
                    wasixv2_syscalls::read_value(memory, wasixv2_abi::GuestPtr::new(data)).await?;
                let set = group.with_trace(child, |t| match &mut t.last_siginfo {
                    Some(last) => {
                        *last = info;
                        true
                    }
                    None => false,
                });
                match set {
                    Some(true) => Ok(0),
                    _ => Err(Errno::EINVAL.into()),
                }
            }
            R::PTRACE_PEEKSIGINFO => self.ptrace_peek_siginfo(caller, child, addr, data).await,
            R::PTRACE_GETSIGMASK => {
                if addr != 8 {
                    return Err(Errno::EINVAL.into());
                }
                let mask = group.trace_sigmask(child).unwrap_or(0);
                memory.write(data, &mask.to_le_bytes()).await?;
                Ok(0)
            }
            R::PTRACE_SETSIGMASK => {
                if addr != 8 {
                    return Err(Errno::EINVAL.into());
                }
                let mut raw = [0; 8];
                memory.read(data, &mut raw).await?;
                group.set_trace_sigmask(child, u64::from_le_bytes(raw));
                Ok(0)
            }
            R::PTRACE_INTERRUPT => {
                if !link.seized {
                    return Err(Errno::EIO.into());
                }
                group.ptrace_interrupt(child);
                Ok(0)
            }
            R::PTRACE_LISTEN => {
                if !link.seized {
                    return Err(Errno::EIO.into());
                }
                group.ptrace_listen(child)?;
                Ok(0)
            }
            R::PTRACE_DETACH => {
                let sig = valid_signal(data).ok_or(Errno::EIO)?;
                self.unlink_tracee(child, Some(sig));
                Ok(0)
            }
            R::PTRACE_CONT | R::PTRACE_SYSCALL => {
                let sig = valid_signal(data).ok_or(Errno::EIO)?;
                group.ptrace_resume(child, request == R::PTRACE_SYSCALL, sig);
                Ok(0)
            }
            R::PTRACE_KILL => {
                group.send_task_kernel(child, wasixv2_abi::Signal::SIGKILL.0);
                Ok(0)
            }
            R::PTRACE_GET_SYSCALL_INFO => {
                self.ptrace_get_syscall_info(caller, child, addr, data)
                    .await
            }
            // No registers, no single-stepping: what an architecture without them answers.
            _ => Err(Errno::EIO.into()),
        }
    }

    /// `ptrace_traceme`: the caller's parent traces it from now on, if Yama and Landlock let
    /// the parent; `EPERM` for a task traced already. A parent on its way out is not linked,
    /// and the call still succeeds, as on Linux.
    fn ptrace_traceme(&self, me: TaskId) -> Result<i32, wasixv2_abi::Errno> {
        if self.trace_link(me).is_some() {
            return Err(wasixv2_abi::Errno::EPERM);
        }
        let process = self.process(me);
        let parent = {
            let tree = self.procs.tree.lock_or_recover();
            tree.nodes.get(&process.pid.0).map_or(0, |node| node.parent)
        };
        let parent = self
            .task_process(TaskId(parent))
            .ok_or(wasixv2_abi::Errno::EPERM)?;
        if !self.may_traceme(&process, &parent) {
            return Err(wasixv2_abi::Errno::EPERM);
        }
        if parent.group.exiting() {
            return Ok(0);
        }
        let link = Link {
            tracer: parent.pid,
            seized: false,
            options: 0,
        };
        self.link_tracee(me, link, false)?;
        Ok(0)
    }

    /// `ptrace_attach`: `PTRACE_SEIZE` takes options in `data` and no `addr` (`EIO`); a task of
    /// the caller's own group, one it may not attach to (`PTRACE_MODE_ATTACH_REALCREDS`), one
    /// exiting or traced already are `EPERM`. `PTRACE_ATTACH` stops the task with SIGSTOP.
    fn ptrace_attach(
        &self,
        me: TaskId,
        child: TaskId,
        seize: bool,
        addr: u32,
        data: u32,
    ) -> Result<i32, wasixv2_abi::Errno> {
        use wasixv2_abi::Errno;
        let options = match seize {
            true => {
                if addr != 0 || data & !wasixv2_abi::PtraceOptions::PTRACE_O_MASK.0 != 0 {
                    return Err(Errno::EIO);
                }
                self.check_ptrace_options(me, data)?;
                data
            }
            false => 0,
        };
        let (mine, theirs) = (
            self.process(me),
            self.task_process(child).ok_or(Errno::ESRCH)?,
        );
        if Arc::ptr_eq(&mine, &theirs) {
            return Err(Errno::EPERM);
        }
        if !self.process_may_access(&mine, &theirs, MODE_ATTACH_REALCREDS) {
            return Err(Errno::EPERM);
        }
        let link = Link {
            tracer: me,
            seized: seize,
            options,
        };
        self.link_tracee(child, link, !seize)?;
        Ok(0)
    }

    /// `check_ptrace_options`: only known options; `PTRACE_O_SUSPEND_SECCOMP` needs
    /// `CAP_SYS_ADMIN` and a tracer outside seccomp.
    fn check_ptrace_options(&self, me: TaskId, data: u32) -> Result<(), wasixv2_abi::Errno> {
        use wasixv2_abi::{Errno, PtraceOptions};
        if data & !PtraceOptions::PTRACE_O_MASK.0 != 0 {
            return Err(Errno::EINVAL);
        }
        if data & PtraceOptions::PTRACE_O_SUSPEND_SECCOMP.0 != 0 {
            let cred = self.process(me).cred();
            if !cred.capable_init(Cap::SysAdmin) {
                return Err(Errno::EPERM);
            }
            let filtered = self.seccomp_active(me);
            let suspended = self
                .trace_link(me)
                .is_some_and(|link| link.has(PtraceOptions::PTRACE_O_SUSPEND_SECCOMP));
            if filtered || suspended {
                return Err(Errno::EPERM);
            }
        }
        Ok(())
    }

    /// `ptrace_access_vm`: `FOLL_FORCE` access to the tracee's memory, by its tracer, of a
    /// dumpable image or with `CAP_SYS_PTRACE` over it (`ptracer_capable`).
    async fn ptrace_access_vm(&self, child: TaskId, addr: u32, buf: Access<'_>) -> usize {
        let Some(process) = self.task_process(child) else {
            return 0;
        };
        let dumpable = process.ident.lock_or_recover().dumpable;
        let tracer = self
            .tracees
            .tracer_of(child)
            .and_then(|t| self.task_process(t));
        let capable =
            tracer.is_some_and(|t| t.cred().ns_capable(&process.cred().user_ns, Cap::SysPtrace));
        if !dumpable && !capable {
            return 0;
        }
        let mm = {
            let state = self.state.read_or_recover();
            state.tasks.get(&child).map(|t| t.mm.clone())
        };
        match mm {
            Some(mm) => access_remote_vm(&mm, addr, buf, true).await,
            None => 0,
        }
    }

    /// `ptrace_peek_siginfo`: up to `nr` signals pending for the tracee (or its group) from
    /// the `off`-th on, written from `data`; how many, or `EFAULT` when none could be.
    async fn ptrace_peek_siginfo(
        &self,
        caller: &wasixv2_syscalls::Caller<'_>,
        child: TaskId,
        addr: u32,
        data: u32,
    ) -> wasixv2_syscalls::SysResult<i32> {
        use wasixv2_abi::{Errno, PtraceArg, PtracePeeksiginfoArgs};
        let memory = caller.memory();
        let args: PtracePeeksiginfoArgs =
            wasixv2_syscalls::read_value(memory, wasixv2_abi::GuestPtr::new(addr)).await?;
        let shared = PtraceArg::PTRACE_PEEKSIGINFO_SHARED.0;
        if args.flags & !shared != 0 || args.nr < 0 {
            return Err(Errno::EINVAL.into());
        }
        // `arg.off > ULONG_MAX`.
        if args.off > u64::from(u32::MAX) {
            return Ok(0);
        }
        let signals = self.task_signals(child);
        let infos = signals.group().peek_siginfo(
            child,
            args.flags & shared != 0,
            args.off,
            args.nr as usize,
        );
        let mut copied = 0;
        for info in infos {
            let at = data.wrapping_add(copied * SIGINFO_SIZE);
            let ptr = wasixv2_abi::GuestPtrMut::new(at);
            if wasixv2_syscalls::write_value(memory, ptr, &info)
                .await
                .is_err()
            {
                return match copied {
                    0 => Err(Errno::EFAULT.into()),
                    n => Ok(n as i32),
                };
            }
            copied += 1;
        }
        Ok(copied as i32)
    }

    /// `ptrace_get_syscall_info`: the call the tracee stopped in, as much of it as fits in
    /// `size` bytes at `data`; returns the size of the whole.
    async fn ptrace_get_syscall_info(
        &self,
        caller: &wasixv2_syscalls::Caller<'_>,
        child: TaskId,
        size: u32,
        data: u32,
    ) -> wasixv2_syscalls::SysResult<i32> {
        use wasixv2_abi::{AuditArch, PtraceArg, PtraceEvent, PtraceSyscallInfo, Signal};
        let signals = self.task_signals(child);
        let (code, message, call) = signals
            .group()
            .with_trace(child, |t| {
                (
                    t.last_siginfo.map(|i| i.si_code),
                    t.message,
                    t.call.unwrap_or_default(),
                )
            })
            .unwrap_or_default();
        let sysgood = Signal::SIGTRAP.0 | 0x80;
        let seccomp = Signal::SIGTRAP.0 | PtraceEvent::PTRACE_EVENT_SECCOMP.0 << 8;
        let op = match code {
            Some(code) if code == sysgood => match PtraceArg(message as u32) {
                PtraceArg::PTRACE_EVENTMSG_SYSCALL_ENTRY => PtraceArg::PTRACE_SYSCALL_INFO_ENTRY,
                PtraceArg::PTRACE_EVENTMSG_SYSCALL_EXIT => PtraceArg::PTRACE_SYSCALL_INFO_EXIT,
                _ => PtraceArg::PTRACE_SYSCALL_INFO_NONE,
            },
            Some(code) if code == seccomp => PtraceArg::PTRACE_SYSCALL_INFO_SECCOMP,
            _ => PtraceArg::PTRACE_SYSCALL_INFO_NONE,
        };
        let mut bytes = [0u8; PtraceSyscallInfo::SIZE];
        bytes[0] = op.0 as u8;
        bytes[4..8].copy_from_slice(&AuditArch::AUDIT_ARCH_RISCV32.0.to_le_bytes());
        bytes[16..24].copy_from_slice(&u64::from(call.sp).to_le_bytes());
        let nr_args = |bytes: &mut [u8]| {
            bytes[24..32].copy_from_slice(&u64::from(call.nr).to_le_bytes());
            for (i, arg) in call.args.iter().enumerate() {
                bytes[32 + 8 * i..40 + 8 * i].copy_from_slice(&arg.to_le_bytes());
            }
        };
        // `offsetofend` of the last member each kind fills.
        let actual = match op {
            PtraceArg::PTRACE_SYSCALL_INFO_ENTRY => {
                nr_args(&mut bytes);
                80
            }
            PtraceArg::PTRACE_SYSCALL_INFO_EXIT => {
                let rval = call.ret.unwrap_or(0);
                bytes[24..32].copy_from_slice(&rval.to_le_bytes());
                // `IS_ERR_VALUE`.
                bytes[32] = u8::from((-4095..0).contains(&rval));
                33
            }
            PtraceArg::PTRACE_SYSCALL_INFO_SECCOMP => {
                nr_args(&mut bytes);
                bytes[80..84].copy_from_slice(&call.seccomp_data.to_le_bytes());
                84
            }
            _ => 24,
        };
        let len = actual.min(size as usize);
        caller.memory().write(data, &bytes[..len]).await?;
        Ok(actual as i32)
    }
}

impl Kernel {
    /// Whether `task` is on its way out: its exit is queued, or its group exits.
    pub(crate) fn exiting_task(&self, task: TaskId) -> bool {
        let state = self.state.read_or_recover();
        state
            .tasks
            .get(&task)
            .is_none_or(|t| t.work.exit().is_some() || t.process.group.exiting())
    }
}

// ---- wait, exit and clone ----

impl Kernel {
    /// `ptrace_do_wait` for every task of `me`: the first tracee `which` names, by tid, that
    /// ended and was not waited for (with `WEXITED`), or sits in a ptrace stop not reported
    /// yet (`wait_task_stopped` with `ptrace`, which needs no `WSTOPPED`). `WNOWAIT` leaves it
    /// to be reported again. Returns whether any tracee matched, and what to report: its tid as
    /// `me` sees it, the report, its process.
    #[allow(clippy::type_complexity)]
    pub(crate) fn ptrace_wait_once(
        &self,
        me: &Process,
        which: crate::process::Which,
        options: u32,
    ) -> (
        bool,
        Option<(wasixv2_abi::Pid, crate::process::Exit, Arc<Process>)>,
    ) {
        use crate::process::Which;
        use wasixv2_abi::WaitOptions;
        if !self.tracees.used() {
            return (false, None);
        }
        let nowait = options & WaitOptions::WNOWAIT.0 != 0;
        let mut map = self.tracees.lock();
        let mut ids: Vec<TaskId> = map
            .iter()
            .filter(|&(id, t)| {
                t.tracer_pid == me.pid.0
                    && match which {
                        Which::All => true,
                        Which::Pid(pid) => id.0 == pid,
                        Which::Pgid(pgid) => t.process.ident.lock_or_recover().pgid == pgid,
                    }
            })
            .map(|(&id, _)| id)
            .collect();
        ids.sort_unstable_by_key(|id| id.0);
        let any = !ids.is_empty();
        for id in ids {
            let tracee = &map[&id];
            let process = tracee.process.clone();
            let report = match tracee.exit {
                Some(exit) if options & WaitOptions::WEXITED.0 != 0 => {
                    if !nowait {
                        map.remove(&id);
                    }
                    Some(exit)
                }
                Some(_) => None,
                None => tracee
                    .signals
                    .group()
                    .ptrace_report(id, nowait)
                    .map(|code| crate::process::Exit {
                        code: wasixv2_abi::SiCode::CLD_TRAPPED,
                        status: code,
                        uid: process.cred().user.real,
                    }),
            };
            if let Some(report) = report {
                let nr = self.pids.nr(&me.pid_ns, id.0);
                return (true, Some((nr, report, process)));
            }
        }
        (any, None)
    }

    /// A task's end as ptrace sees it: as a tracer, its tracees are let go (`exit_ptrace`),
    /// killed first with `PTRACE_O_EXITKILL`; as a tracee, its tracer hears of the end and may
    /// wait for it, unless the tracer is its parent, which waits for the zombie as such.
    pub(crate) fn ptrace_exit_task(&self, task: TaskId, code: i32) {
        if !self.tracees.used() {
            return;
        }
        let tracees: Vec<(TaskId, Arc<crate::signal::TaskSignals>)> = self
            .tracees
            .lock()
            .iter()
            .filter(|(_, t)| t.tracer == task)
            .map(|(&id, t)| (id, t.signals.clone()))
            .collect();
        for (id, signals) in tracees {
            let exitkill = signals
                .group()
                .with_trace(id, |t| t.link)
                .flatten()
                .is_some_and(|link| link.has(wasixv2_abi::PtraceOptions::PTRACE_O_EXITKILL));
            if exitkill {
                signals
                    .group()
                    .send_task_kernel(id, wasixv2_abi::Signal::SIGKILL.0);
            }
            self.unlink_tracee(id, None);
        }
        let Some(process) = self.task_process(task) else {
            return;
        };
        let (tracer, tracer_pid) = match self.tracees.lock().get(&task) {
            Some(t) if t.exit.is_none() => (t.tracer, t.tracer_pid),
            _ => return,
        };
        if task == process.pid && self.is_parent_of(tracer_pid, process.pid.0) {
            self.tracees.lock().remove(&task);
            return;
        }
        let exit = match self.killed_by(process.pid.0) {
            Some(sig) => crate::process::Exit {
                code: wasixv2_abi::SiCode::CLD_KILLED,
                status: sig,
                uid: process.cred().user.real,
            },
            None => crate::process::Exit {
                code: wasixv2_abi::SiCode::CLD_EXITED,
                status: code & 0xff,
                uid: process.cred().user.real,
            },
        };
        if let Some(t) = self.tracees.lock().get_mut(&task) {
            t.exit = Some(exit);
        }
        self.signal_tracer(task, tracer, exit.code, exit.status, false);
        self.procs.changed.wake(wasixv2_wait::PollMask::empty());
    }

    /// `ptrace_init_task` for a task `parent` just made: a traced parent's child is traced by
    /// the same tracer when `CLONE_PTRACE` asks or the tracer wants the clone's event
    /// (`PTRACE_O_TRACEFORK`, `..VFORK`, `..CLONE`; never with `CLONE_UNTRACED`). It starts with
    /// SIGSTOP pending, or a trap stop when seized. The event stop itself comes once the call
    /// made the child ([`Kernel::ptrace_clone_event`]).
    pub(crate) fn ptrace_init_task(
        &self,
        parent: TaskId,
        child: TaskId,
        flags: wasixv2_abi::CloneFlags,
        exit_signal: i32,
    ) {
        use wasixv2_abi::{CloneFlags, PtraceEvent};
        let Some(link) = self.trace_link(parent) else {
            return;
        };
        let event = match () {
            _ if flags.contains(CloneFlags::CLONE_UNTRACED) => None,
            _ if flags.contains(CloneFlags::CLONE_VFORK) => Some(PtraceEvent::PTRACE_EVENT_VFORK),
            _ if exit_signal != wasixv2_abi::Signal::SIGCHLD.0 => {
                Some(PtraceEvent::PTRACE_EVENT_CLONE)
            }
            _ => Some(PtraceEvent::PTRACE_EVENT_FORK),
        };
        let event = event.filter(|event| link.options & event_flag(event.0) != 0);
        if !flags.contains(CloneFlags::CLONE_PTRACE) && event.is_none() {
            return;
        }
        if self.link_tracee(child, link, !link.seized).is_err() {
            return;
        }
        if link.seized {
            self.task_signals(child).group().ptrace_interrupt(child);
        }
        if let Some(event) = event {
            let signals = self.task_signals(parent);
            signals
                .group()
                .with_trace(parent, |t| t.clone_event = Some((event.0, child)));
        }
    }

    /// `ptrace_event_pid` once a clone made its child: the event stop the tracer asked for,
    /// with the child's pid in the tracer's namespace as the message.
    pub(crate) async fn ptrace_clone_event(&self, task: TaskId) {
        if !self.tracees.used() {
            return;
        }
        let signals = self.task_signals(task);
        let event = signals
            .group()
            .with_trace(task, |t| t.clone_event.take())
            .flatten();
        let Some((event, child)) = event else {
            return;
        };
        let tracer = self
            .tracees
            .tracer_of(task)
            .and_then(|t| self.task_process(t));
        let message = tracer.map_or(0, |tracer| self.pids.nr(&tracer.pid_ns, child.0));
        let event = wasixv2_abi::PtraceEvent(event);
        self.ptrace_event(task, event, message as u32 as u64).await;
    }

    /// What `execve` owes a traced caller once it committed (`exec_binprm`): with
    /// `PTRACE_O_TRACEEXEC`, an event stop whose message is the caller's old tid in its
    /// tracer's namespace (`old_vpid`); else, attached the old way, SIGTRAP.
    pub(crate) async fn ptrace_exec_event(&self, task: TaskId, old_vpid: wasixv2_abi::Pid) {
        let event = wasixv2_abi::PtraceEvent::PTRACE_EVENT_EXEC;
        self.ptrace_event(task, event, old_vpid as u32 as u64).await;
    }

    /// The caller's tid as its tracer sees it, 0 when untraced.
    pub(crate) fn tid_for_tracer(&self, task: TaskId) -> wasixv2_abi::Pid {
        let tracer = self
            .tracees
            .tracer_of(task)
            .and_then(|t| self.task_process(t));
        tracer.map_or(0, |tracer| self.pids.nr(&tracer.pid_ns, task.0))
    }

    /// `LSM_UNSAFE_PTRACE` (`check_unsafe_exec`): a traced `execve` gains no privileges
    /// unless the tracer has `CAP_SYS_PTRACE` over the task's user namespace
    /// (`ptracer_capable`).
    pub(crate) fn ptrace_unsafe_exec(&self, task: TaskId) -> bool {
        let Some(tracer) = self
            .tracees
            .tracer_of(task)
            .and_then(|t| self.task_process(t))
        else {
            return false;
        };
        let ns = self.process(task).cred().user_ns;
        !tracer.cred().ns_capable(&ns, Cap::SysPtrace)
    }
}
