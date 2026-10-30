//! Signals, after Linux's `kernel/signal.c`: generation into per-task and per-thread-group
//! pending sets, masks, dispositions, dequeueing on the return-to-guest path, `sigaltstack`,
//! `sigsuspend`, `sigtimedwait`, the `ITIMER_REAL` interval timer, and the signal side of POSIX
//! timers ([`crate::posix_timer`]): their preallocated queue entries and the ignored list.
//!
//! Handlers run in the guest: [`Kernel::take_signal`] hands the execution layer a
//! [`SignalFrame`] and [`Kernel::signal_return`] is the matching sigreturn.

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex, MutexGuard, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use wasixv2_abi::{
    CloneFlags, ConstKernelOldItimervalPtr, ConstKernelTimespecPtr, ConstSigactionPtr,
    ConstSigaltstackPtr, ConstSiginfoPtr, ConstSigsetPtr, ItimerWhich, KernelOldItimerval,
    KernelOldItimervalPtr, KernelOldTimeval, Pid, SaFlags, SiCode, Sigaction, SigactionPtr,
    Sigaltstack, SigaltstackFlags, SigaltstackPtr, Siginfo, SiginfoPtr, Signal, SignalHandler,
    SignalLimits, SigprocmaskHow, Sigset, SigsetPtr,
};
use wasixv2_host::Host;
use wasixv2_syscalls::{
    Caller, CpuBase, Errno, GuestAction, SignalFrame, SysErr, SysResult, TaskId, WorkFlag,
    read_value, write_value,
};
use wasixv2_wait::{Interrupt, NoInterrupt, WaitQueue, Waiter, Wakeup, wait_event};

use crate::{
    Kernel, Process,
    posix_timer::{self, PosixTimer},
    sync::MutexExt,
    time,
    timer::{self, Timer},
    uidgid::{Kuid, OVERFLOW_ID},
};

pub(crate) const NSIG: i32 = 64;
const SIGRTMIN: i32 = 32;
const UNBLOCKABLE: u64 = bit(Signal::SIGKILL.0) | bit(Signal::SIGSTOP.0);
/// The signals whose default action stops the thread group (`SIG_KERNEL_STOP_MASK`).
const STOP_SIGNALS: u64 = bit(Signal::SIGSTOP.0)
    | bit(Signal::SIGTSTP.0)
    | bit(Signal::SIGTTIN.0)
    | bit(Signal::SIGTTOU.0);

const fn bit(sig: i32) -> u64 {
    1 << (sig - 1)
}

/// The signal dispositions (`sighand_struct`): a thread group's own, or shared with the
/// processes made with `CLONE_SIGHAND` (which needs `CLONE_VM`), whose `sigaction` changes
/// them for all. Locked after the group's state and briefly, so a lock of it never waits for
/// another group.
pub(crate) struct Sighand(Mutex<[Action; NSIG as usize]>);

impl Default for Sighand {
    fn default() -> Self {
        Self(Mutex::new([Action::default(); NSIG as usize]))
    }
}

impl Sighand {
    fn get(&self, sig: i32) -> Action {
        self.0.lock_or_recover()[sig as usize - 1]
    }

    fn set(&self, sig: i32, action: Action) {
        self.0.lock_or_recover()[sig as usize - 1] = action;
    }

    fn all(&self) -> [Action; NSIG as usize] {
        *self.0.lock_or_recover()
    }

    /// A table of its own with the same dispositions (`copy_sighand`).
    fn copy(&self) -> Arc<Self> {
        Arc::new(Self(Mutex::new(self.all())))
    }
}

/// The signal state of one thread group: the dispositions ([`Sighand`]), the shared pending
/// set and timers (`signal_struct`), and the per-task masks and pending sets, under one lock.
pub(crate) struct Signals {
    state: Mutex<GroupState>,
    /// Woken on every signal sent to the group or one of its tasks; blocked tasks recheck.
    queue: WaitQueue,
    /// Whom the group's queued signals are charged to; `None` for a group that queues for
    /// nobody (a zombie's).
    account: Option<Account>,
}

/// The signals queued for each user, against `RLIMIT_SIGPENDING` (`UCOUNT_RLIMIT_SIGPENDING`).
#[derive(Default)]
pub(crate) struct SigqueueUsers(Mutex<HashMap<Kuid, Arc<AtomicU64>>>);

/// The charge of one queued signal to its user (`sigqueue->ucounts`), given back when the
/// signal leaves its queue or its timer goes.
pub(crate) struct Charge(Arc<AtomicU64>);

/// A user is at `RLIMIT_SIGPENDING`.
pub(crate) struct Overflow;

impl SigqueueUsers {
    /// `inc_rlimit_get_ucounts`: charges one signal to `uid`, unless that takes it past `limit`.
    /// `None` overrides the limit; the signal still counts.
    pub(crate) fn charge(&self, uid: Kuid, limit: Option<u64>) -> Result<Charge, Overflow> {
        let count = self.0.lock_or_recover().entry(uid).or_default().clone();
        let queued = count.fetch_add(1, Ordering::Relaxed) + 1;
        let charge = Charge(count);
        match limit {
            Some(limit) if queued > limit => Err(Overflow),
            _ => Ok(charge),
        }
    }
}

impl Drop for Charge {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Whom a group's queued signals count against: its process's real uid, up to the process's
/// `RLIMIT_SIGPENDING` (`task_ucounts`, `task_rlimit`).
pub(crate) struct Account {
    users: Arc<SigqueueUsers>,
    process: Weak<Process>,
}

impl Account {
    pub(crate) fn new(users: Arc<SigqueueUsers>, process: &Arc<Process>) -> Self {
        Self {
            users,
            process: Arc::downgrade(process),
        }
    }

    /// `sig_get_ucounts` for `info`: kernel signals and `kill`'s standard ones go past the
    /// limit (`override_rlimit`). SIGKILL is never queued, so it costs nothing.
    fn charge(
        &self,
        info: &Siginfo,
        cred: Option<&crate::cred::Cred>,
    ) -> Result<Option<Charge>, Overflow> {
        let sig = info.si_signo;
        let Some(process) = self.process.upgrade().filter(|_| sig != Signal::SIGKILL.0) else {
            return Ok(None);
        };
        let Some(cred) = cred else {
            return Ok(None);
        };
        let uid = cred.user.real;
        let limit = match sig < SIGRTMIN && info.si_code >= 0 {
            true => None,
            false => Some(process.resources.lock_or_recover().sigpending()),
        };
        self.users.charge(uid, limit).map(Some)
    }
}

#[derive(Default)]
struct GroupState {
    sighand: Arc<Sighand>,
    /// The guest's trampoline (`signal-callback-register`).
    trampoline: Option<u32>,
    shared: Pending,
    tasks: HashMap<TaskId, TaskState>,
    /// The task a signal for the whole group is offered first: the leader, which `kill` and
    /// the group's other senders name (`pid_task(pid, PIDTYPE_TGID)`).
    leader: Option<TaskId>,
    /// Where the search for a task to take a group signal the leader does not take starts
    /// (`signal_struct::curr_target`).
    curr_target: Option<TaskId>,
    real_timer: RealTimer,
    /// The group's POSIX timers (`signal_struct::posix_timers`).
    posix_timers: posix_timer::Table,
    /// Periodic POSIX timers whose signal was ignored when it fired; they are queued again
    /// once the signal is no longer ignored (`ignored_posix_timers`).
    ignored_timers: Vec<Arc<PosixTimer>>,
    /// The init of a pid namespace: a signal whose action is the default is dropped, but for
    /// SIGKILL and SIGSTOP from the kernel or from an ancestor namespace (`SIGNAL_UNKILLABLE`).
    unkillable: bool,
    /// The kernel's init, which not even those reach (`is_global_init`).
    global_init: bool,
    jobctl: JobCtl,
}

/// The thread group's half of job control (`signal_struct`'s `group_stop_count`,
/// `group_exit_code` and `SIGNAL_STOP_*` flags); each task's half is in [`TaskState`].
#[derive(Default)]
struct JobCtl {
    /// The tasks yet to take part in the group stop under way (`group_stop_count`).
    group_stop_count: usize,
    /// The signal that stopped the group, until a `waitid` with `WSTOPPED` reports it
    /// (`group_exit_code`); 0 once reported.
    stop_code: i32,
    /// Every task took part in the last group stop, and no SIGCONT came since
    /// (`SIGNAL_STOP_STOPPED`).
    stopped: bool,
    /// SIGCONT continued the group, and no `waitid` with `WCONTINUED` reported it yet
    /// (`SIGNAL_STOP_CONTINUED`).
    continued: bool,
    /// What the next task through [`Kernel::take_signal`] tells the parent after a SIGCONT
    /// (`SIGNAL_CLD_MASK`): `CLD_CONTINUED`, or `CLD_STOPPED` for a stop it cut short.
    report: Option<SiCode>,
}

#[derive(Clone, Copy, Default)]
struct Action {
    handler: u32,
    flags: u32,
    mask: u64,
}

impl Action {
    fn is_ignore(&self) -> bool {
        self.handler == SignalHandler::SIG_IGN.0 as u32
    }

    fn is_default(&self) -> bool {
        self.handler == SignalHandler::SIG_DFL.0 as u32
    }
}

/// A pending set plus every queued instance, oldest first.
#[derive(Default)]
struct Pending {
    set: u64,
    queue: VecDeque<Queued>,
}

/// One queued signal (`struct sigqueue`). A POSIX timer's signal names its timer, as the
/// timer's preallocated `sigqueue` does on Linux: a timer is queued at most once, and it
/// decides at dequeue whether its signal still counts (`posixtimer_deliver_signal`); the timer
/// holds its charge.
struct Queued {
    info: Siginfo,
    timer: Option<Arc<PosixTimer>>,
    /// Given back when the entry goes.
    _charge: Option<Charge>,
}

impl Pending {
    /// `__send_signal_locked` from `legacy_queue` on: a standard signal is not queued twice,
    /// and SIGKILL carries nothing. A signal its user has no room for (`charge`) is pending
    /// without its `siginfo`, except a realtime one sent other than by `kill`, which fails with
    /// `EAGAIN`.
    fn add(
        &mut self,
        info: Siginfo,
        charge: Result<Option<Charge>, Overflow>,
    ) -> Result<(), Errno> {
        let sig = info.si_signo;
        if sig < SIGRTMIN && self.set & bit(sig) != 0 {
            return Ok(());
        }
        if sig != Signal::SIGKILL.0 {
            match charge {
                Ok(charge) => self.queue.push_back(Queued {
                    info,
                    timer: None,
                    _charge: charge,
                }),
                Err(Overflow) if sig >= SIGRTMIN && info.si_code != SiCode::SI_USER.0 => {
                    return Err(Errno::EAGAIN);
                }
                Err(Overflow) => {}
            }
        }
        self.set |= bit(sig);
        Ok(())
    }

    /// `posixtimer_queue_sigqueue`: a timer's signal is queued even next to a pending
    /// standard signal of the same number.
    fn add_timer(&mut self, info: Siginfo, timer: Arc<PosixTimer>) {
        self.set |= bit(info.si_signo);
        self.queue.push_back(Queued {
            info,
            timer: Some(timer),
            _charge: None,
        });
    }

    /// The queued signals, oldest first (`pending->list`).
    fn list(&self) -> impl Iterator<Item = Siginfo> + '_ {
        self.queue.iter().map(|queued| queued.info)
    }

    fn has_timer(&self, timer: &Arc<PosixTimer>) -> bool {
        self.queue
            .iter()
            .any(|queued| queued.timer.as_ref().is_some_and(|t| Arc::ptr_eq(t, timer)))
    }

    /// `collect_signal`: takes the oldest instance of `sig`, and its timer if a POSIX timer
    /// sent it. A signal pending without its `siginfo` comes as `SI_USER` from pid 0.
    fn take(&mut self, sig: i32) -> (Siginfo, Option<Arc<PosixTimer>>) {
        let at = self.queue.iter().position(|q| q.info.si_signo == sig);
        let queued = at.and_then(|at| self.queue.remove(at));
        if !self.queue.iter().any(|q| q.info.si_signo == sig) {
            self.set &= !bit(sig);
        }
        match queued {
            Some(queued) => (queued.info, queued.timer),
            None => (user_info(sig, SiCode::SI_USER, 0, 0), None),
        }
    }

    /// `flush_sigqueue_mask`. Returns the timers whose signals it dropped.
    fn flush(&mut self, mask: u64) -> Vec<Arc<PosixTimer>> {
        self.set &= !mask;
        let mut timers = Vec::new();
        self.queue.retain_mut(|queued| {
            if mask & bit(queued.info.si_signo) == 0 {
                return true;
            }
            timers.extend(queued.timer.take());
            false
        });
        timers
    }

    /// `__flush_itimer_signals`: drops every POSIX timer signal, and the pending bits only
    /// they held.
    fn flush_timers(&mut self) {
        let (mut retain, mut dropped) = (0, 0);
        self.queue.retain(|queued| {
            let sig = bit(queued.info.si_signo);
            match queued.timer {
                Some(_) => dropped |= sig,
                None => retain |= sig,
            }
            queued.timer.is_none()
        });
        self.set = (self.set & !dropped) | retain;
    }
}

struct TaskState {
    cred: Option<Arc<crate::cred::Cred>>,
    /// The task's work flag, raised whenever a signal may have become deliverable to it.
    flag: Arc<WorkFlag>,
    /// Kernel work makes the task leave a blocking wait without a signal (`TIF_NOTIFY_SIGNAL`).
    notify: bool,
    /// TIF_NOTIFY_SIGNAL for io_uring, cleared by task_work_run independently of actions.
    uring_notify: bool,
    /// A signal waits for this task to take it (`TIF_SIGPENDING`): one sent to it, or a group
    /// signal it was picked to take ([`GroupState::complete`]), or one its own mask lets through
    /// ([`GroupState::recalc_sigpending`]). Only then does it leave an interruptible wait or
    /// dequeue on its way back to the guest, so a group signal reaches the one task picked
    /// for it rather than whichever task gets there first.
    sigpending: bool,
    /// The task is on its way out and takes no more signals (`PF_EXITING`).
    exiting: bool,
    blocked: u64,
    /// The mask to restore once a `sigsuspend` or `ppoll` mask has served its signal
    /// (`TIF_RESTORE_SIGMASK` and `saved_sigmask`).
    saved: Option<u64>,
    pending: Pending,
    altstack: AltStack,
    /// The alternate stack each handler running on this task found, innermost last: what its
    /// frame saved in `uc_stack`, which its sigreturn restores (`restore_altstack`), re-arming
    /// a stack that `SS_AUTODISARM` disarmed for the handler.
    frames: Vec<AltStack>,
    /// The vfork child running in this task's instance while the task waits for it, with the
    /// child's signal state: a fatal signal for the task wakes the child, whose next return to
    /// the guest ends this task's group (`wait_for_vfork_done` is `TASK_KILLABLE`).
    vfork_child: Option<(Arc<Signals>, TaskId)>,
    /// The task has yet to take part in the group stop under way (`JOBCTL_STOP_PENDING` with
    /// `JOBCTL_STOP_CONSUME`): it leaves its blocking wait and stops on its way back to the
    /// guest.
    stop_pending: bool,
    /// The task dequeued a stop signal and no SIGCONT came since (`JOBCTL_STOP_DEQUEUED`).
    stop_dequeued: bool,
    /// The task stopped and runs no guest code until it is continued (`JOBCTL_STOPPED`,
    /// `TASK_STOPPED`).
    stopped: bool,
    /// Whether and how the task is traced, and its ptrace stops (see `ptrace.rs`).
    ptrace: crate::ptrace::TaskTrace,
    /// Its cgroup freezes it (`JOBCTL_TRAP_FREEZE`): it leaves its blocking wait and, on its way
    /// back to the guest, waits in the freezer until thawed, unlike a stop unseen by its parent.
    trap_freeze: bool,
    /// A quota of its cgroups ran out: it runs no guest code until then, the end of the
    /// period (`throttle_cfs_rq`), unless a fatal signal comes.
    throttled_until: Option<Duration>,
}

impl TaskState {
    fn new(inherited: Inherited, flag: Arc<WorkFlag>) -> Self {
        Self {
            flag,
            notify: false,
            uring_notify: false,
            sigpending: false,
            exiting: false,
            cred: None,
            blocked: inherited.blocked,
            saved: None,
            pending: Pending::default(),
            altstack: inherited.altstack,
            frames: Vec::new(),
            vfork_child: None,
            stop_pending: false,
            stop_dequeued: false,
            stopped: false,
            ptrace: Default::default(),
            trap_freeze: false,
            throttled_until: None,
        }
    }

    /// `signal_wake_up`: the task has a signal to take, and leaves its interruptible wait for
    /// it once the caller wakes the group's queue.
    fn signal_wake_up(&mut self) {
        self.sigpending = true;
        self.flag.raise();
    }
}

/// What a new task takes over from the task that creates it (`copy_process`).
pub(crate) struct Inherited {
    blocked: u64,
    altstack: AltStack,
}

impl Inherited {
    /// A task that inherits nothing: no signal blocked, no alternate stack.
    pub(crate) fn none() -> Self {
        Self {
            blocked: 0,
            altstack: AltStack::default(),
        }
    }
}

/// `sas_ss_sp`, `sas_ss_size` and `sas_ss_flags`.
#[derive(Clone, Copy, PartialEq, Eq)]
struct AltStack {
    sp: u32,
    size: u32,
    /// As `sigaltstack` set them: the mode and `SS_AUTODISARM`.
    flags: u32,
}

impl Default for AltStack {
    fn default() -> Self {
        Self {
            sp: 0,
            size: 0,
            flags: SigaltstackFlags::SS_DISABLE.0,
        }
    }
}

impl AltStack {
    /// `on_sig_stack`: whether `sp` lies on the stack. An `SS_AUTODISARM` stack never does, as
    /// delivery disarms it before anything runs on it.
    fn holds(&self, sp: u32) -> bool {
        self.flags & SigaltstackFlags::SS_AUTODISARM.0 == 0
            && sp > self.sp
            && sp - self.sp <= self.size
    }

    /// The `ss_flags` `sigaltstack` reports for a task at `sp`: `SS_DISABLE` without a stack,
    /// `SS_ONSTACK` on it, and `SS_AUTODISARM` as set (`sas_ss_flags`).
    fn reported_flags(&self, sp: u32) -> u32 {
        let state = match (self.size, self.holds(sp)) {
            (0, _) => SigaltstackFlags::SS_DISABLE.0,
            (_, true) => SigaltstackFlags::SS_ONSTACK.0,
            (_, false) => 0,
        };
        state | (self.flags & SigaltstackFlags::SS_AUTODISARM.0)
    }
}

/// `ITIMER_REAL`.
#[derive(Default)]
struct RealTimer {
    /// On the host's monotonic clock; `None` while disarmed, and from an expiry until its
    /// SIGALRM is dequeued.
    deadline: Option<Duration>,
    /// `it_real_incr`: zero for a one-shot timer.
    interval: Duration,
    /// The deadline that last expired, which a periodic timer moves on from.
    expired: Option<Duration>,
    /// Sends SIGALRM at `deadline` (`it_real_fn`), on the host's clock; made at the first
    /// arming.
    timer: Option<(Timer, Arc<dyn Host>)>,
}

impl RealTimer {
    /// `posixtimer_rearm_itimer`, as SIGALRM is dequeued: a periodic timer that expired goes on
    /// to its first period after now. Re-arming here rather than at the expiry keeps a timer
    /// whose SIGALRM is blocked or ignored from running meanwhile, as on Linux.
    fn rearm(&mut self) {
        let (Some(expired), Some((timer, host))) = (self.expired, &self.timer) else {
            return;
        };
        if self.deadline.is_some() || self.interval.is_zero() {
            return;
        }
        let now = host.monotonic();
        self.deadline = match expired <= now {
            true => timer::forward(expired, self.interval, now).1,
            false => Some(expired),
        };
        if let Some(deadline) = self.deadline {
            timer.start(deadline);
        }
    }
}

/// What a signal does by default (`sig_kernel_*` in `include/linux/signal.h`).
enum DefaultAction {
    Ignore,
    Stop,
    Terminate,
}

fn default_action(sig: i32) -> DefaultAction {
    match Signal(sig) {
        Signal::SIGCHLD | Signal::SIGURG | Signal::SIGWINCH | Signal::SIGCONT => {
            DefaultAction::Ignore
        }
        Signal::SIGSTOP | Signal::SIGTSTP | Signal::SIGTTIN | Signal::SIGTTOU => {
            DefaultAction::Stop
        }
        _ => DefaultAction::Terminate,
    }
}

/// A `siginfo` with only the signal number and `SI_KERNEL`.
pub(crate) fn kernel_info(sig: i32) -> Siginfo {
    Siginfo {
        si_signo: sig,
        si_errno: 0,
        si_code: SiCode::SI_KERNEL.0,
        sifields: [0; 116],
    }
}

/// A `siginfo` for a signal a process sent: `si_pid` and `si_uid` (its real uid) are its first
/// fields, as the sender sees them; [`Kernel::deliver`] shows them as the receiver does.
pub(crate) fn user_info(sig: i32, code: SiCode, pid: Pid, uid: u32) -> Siginfo {
    let mut info = kernel_info(sig);
    info.si_code = code.0;
    info.sifields[0..4].copy_from_slice(&pid.to_le_bytes());
    info.sifields[4..8].copy_from_slice(&uid.to_le_bytes());
    info
}

impl Signals {
    /// Target snapshot for callbacks without a Kernel; never hold this lock across accounting.
    pub(crate) fn cred(&self, task: Option<TaskId>) -> Option<Arc<crate::cred::Cred>> {
        let state = self.state.lock_or_recover();
        let target = match task {
            Some(task) => state.tasks.get(&task),
            None => state
                .tasks
                .iter()
                .min_by_key(|(id, _)| id.0)
                .map(|(_, task)| task),
        }?;
        target.cred.clone()
    }

    /// A group whose queued signals `account` pays for.
    pub(crate) fn new(account: Option<Account>) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::default(),
            queue: WaitQueue::new(),
            account,
        })
    }

    /// Adds `task` to the group, with what it inherits and the work flag its signals raise.
    pub(crate) fn attach(
        self: &Arc<Self>,
        task: TaskId,
        inherited: Inherited,
        flag: Arc<WorkFlag>,
    ) -> Arc<TaskSignals> {
        let mut state = self.state.lock_or_recover();
        let mut new = TaskState::new(inherited, flag);
        // `task_join_group_stop`: a thread created while its group stops takes part too.
        if state.jobctl.group_stop_count != 0 {
            state.jobctl.group_stop_count += 1;
            new.stop_pending = true;
            new.flag.raise();
        }
        state.tasks.insert(task, new);
        // The group's first task is its leader (`copy_signal`'s `curr_target`).
        if state.leader.is_none() {
            (state.leader, state.curr_target) = (Some(task), Some(task));
        }
        Arc::new(TaskSignals {
            group: self.clone(),
            task,
            status: wasixv2_wait::TaskStatus::default(),
        })
    }

    /// The identity of the group's dispositions (`sighand_struct`), which `CLONE_SIGHAND`
    /// shares.
    pub(crate) fn sighand_id(&self) -> usize {
        Arc::as_ptr(&self.state.lock_or_recover().sighand) as usize
    }

    // ---- proc ----
    /// `copy_sighand` and `copy_signal` for a new process, whose queued signals `account` pays
    /// for: the dispositions, shared with `share_sighand` (`CLONE_SIGHAND`) and copied
    /// otherwise, and the trampoline, with nothing pending and no timers.
    pub(crate) fn fork(&self, account: Account, share_sighand: bool) -> Arc<Self> {
        let new = Self::new(Some(account));
        {
            let state = self.state.lock_or_recover();
            let mut copy = new.state.lock_or_recover();
            copy.sighand = match share_sighand {
                true => state.sighand.clone(),
                false => state.sighand.copy(),
            };
            copy.trampoline = state.trampoline;
        }
        new
    }

    /// `flush_signal_handlers`: caught signals go back to their default action and ignored
    /// ones stay ignored, as at `execve` and for a `CLONE_CLEAR_SIGHAND` child.
    pub(crate) fn flush_handlers(&self) {
        let state = self.state.lock_or_recover();
        let mut actions = state.sighand.0.lock_or_recover();
        for action in actions.iter_mut() {
            *action = match action.is_ignore() {
                true => Action {
                    handler: action.handler,
                    ..Action::default()
                },
                false => Action::default(),
            };
        }
    }

    /// `flush_signal_handlers` and `sas_ss_reset` at `execve`: caught signals go back to their
    /// default action, ignored ones stay ignored, and the trampoline and alternate stacks
    /// belonged to the old image. Masks and pending signals stay; a temporary mask's end
    /// (`sigsuspend`, `ppoll`) and a wakeup for kernel work were the old image's, and go.
    pub(crate) fn exec_reset(&self) {
        // `unshare_sighand`: dispositions shared with other processes stay theirs.
        {
            let mut state = self.state.lock_or_recover();
            if Arc::strong_count(&state.sighand) > 1 {
                state.sighand = state.sighand.copy();
            }
        }
        self.flush_handlers();
        let mut state = self.state.lock_or_recover();
        state.trampoline = None;
        for task in state.tasks.values_mut() {
            task.altstack = AltStack::default();
            task.frames.clear();
            task.saved = None;
            task.notify = false;
            task.uring_notify = false;
        }
    }

    /// How the group treats its children's exits: SIGCHLD is ignored (`SIG_IGN`), and
    /// `SA_NOCLDWAIT` is set.
    pub(crate) fn child_reaping(&self) -> (bool, bool) {
        let state = self.state.lock_or_recover();
        let action = state.sighand.get(Signal::SIGCHLD.0);
        (
            action.is_ignore(),
            action.flags & SaFlags::SA_NOCLDWAIT.0 != 0,
        )
    }

    /// Makes the group the kernel's init's: SIGCHLD is ignored, so its children are reaped as
    /// they exit, and no signal with the default action reaches it.
    pub(crate) fn make_init(&self) {
        let mut state = self.state.lock_or_recover();
        let mut sigchld = state.sighand.get(Signal::SIGCHLD.0);
        sigchld.handler = SignalHandler::SIG_IGN.0 as u32;
        state.sighand.set(Signal::SIGCHLD.0, sigchld);
        state.unkillable = true;
        state.global_init = true;
    }

    /// Whether the group is an init (`SIGNAL_UNKILLABLE`).
    pub(crate) fn unkillable(&self) -> bool {
        self.state.lock_or_recover().unkillable
    }

    /// Makes the group a pid namespace's init (`SIGNAL_UNKILLABLE`): only the signals it has
    /// handlers for reach it from inside its namespace.
    pub(crate) fn make_unkillable(&self) {
        self.state.lock_or_recover().unkillable = true;
    }

    /// `zap_pid_ns_processes`: the dying init of a pid namespace ignores SIGCHLD, so its
    /// children are reaped as they exit.
    pub(crate) fn ignore_sigchld(&self) {
        let state = self.state.lock_or_recover();
        let ignore = Action {
            handler: SignalHandler::SIG_IGN.0 as u32,
            ..Action::default()
        };
        state.sighand.set(Signal::SIGCHLD.0, ignore);
    }

    /// Queues `info` for the whole group (`__group_send_sig_info`).
    pub(crate) fn send_group(&self, info: Siginfo) -> Result<(), Errno> {
        self.send(None, info)
    }
    // ---- end proc ----

    // ---- job control ----
    /// Whether a stop or continue of a child is no reason for SIGCHLD: the group ignores
    /// SIGCHLD, or set `SA_NOCLDSTOP` (`do_notify_parent_cldstop`).
    pub(crate) fn nocldstop(&self) -> bool {
        let state = self.state.lock_or_recover();
        let action = state.sighand.get(Signal::SIGCHLD.0);
        action.is_ignore() || action.flags & SaFlags::SA_NOCLDSTOP.0 != 0
    }

    /// The `si_status` of the group's stop as its parent hears of it: the signal that stopped
    /// it, or 0 once reported.
    pub(crate) fn stop_status(&self) -> i32 {
        self.state.lock_or_recover().jobctl.stop_code & 0x7f
    }

    /// `wait_task_stopped`: the signal that stopped the group, if the group is stopped and no
    /// `waitid` reported it yet; reported from now on unless `nowait` (`WNOWAIT`).
    pub(crate) fn wait_stopped(&self, nowait: bool) -> Option<i32> {
        let mut state = self.state.lock_or_recover();
        let jobctl = &mut state.jobctl;
        if !jobctl.stopped || jobctl.stop_code == 0 {
            return None;
        }
        let code = jobctl.stop_code;
        if !nowait {
            jobctl.stop_code = 0;
        }
        Some(code)
    }

    /// `wait_task_continued`: whether SIGCONT continued the group and no `waitid` reported it
    /// yet; reported from now on unless `nowait` (`WNOWAIT`).
    pub(crate) fn wait_continued(&self, nowait: bool) -> bool {
        let mut state = self.state.lock_or_recover();
        let continued = state.jobctl.continued;
        if !nowait {
            state.jobctl.continued = false;
        }
        continued
    }

    /// The group stopped and is stopped still (`SIGNAL_STOP_STOPPED`).
    pub(crate) fn is_stopped(&self) -> bool {
        self.state.lock_or_recover().jobctl.stopped
    }

    /// Sends `sig` from the kernel (`SI_KERNEL`) to the whole group, as a terminal's `isig`
    /// does; returns whether it is pending now, `false` when the group ignores it.
    pub(crate) fn generate_kernel(&self, sig: i32) -> bool {
        // A kernel signal goes past the limit, or without its `siginfo`.
        matches!(self.generate(None, kernel_info(sig), false), Ok(true))
    }

    /// Sends `sig` from the kernel (`SEND_SIG_PRIV`, `SI_KERNEL`) to the whole group.
    pub(crate) fn send_kernel(&self, sig: i32) {
        // A kernel signal goes past the limit, or without its `siginfo`.
        let _ = self.send(None, kernel_info(sig));
    }

    /// See [`GroupState::cancel_stop`]; stopped tasks wake.
    pub(crate) fn cancel_stop(&self, dying: bool) {
        self.state.lock_or_recover().cancel_stop(dying);
        self.queue.wake(wasixv2_wait::PollMask::empty());
    }
    // ---- end job control ----

    /// `zap_other_threads`: queues SIGKILL for the group, so every task blocked in an
    /// interruptible wait wakes up and unwinds through its pending exit.
    pub(crate) fn kill_all(&self) {
        // SIGKILL needs no room in a queue.
        let _ = self.send(None, kernel_info(Signal::SIGKILL.0));
    }

    /// Whether `sig` is pending for the group or one of its tasks.
    pub(crate) fn is_pending(&self, sig: i32) -> bool {
        let state = self.state.lock_or_recover();
        (state.shared.set | state.tasks.values().fold(0, |set, t| set | t.pending.set)) & bit(sig)
            != 0
    }

    pub(crate) fn clear_notify(&self, task: TaskId) {
        if let Some(task) = self.state.lock_or_recover().tasks.get_mut(&task) {
            task.notify = false;
        }
    }

    /// Makes `task` leave an interruptible wait for kernel work, without a signal
    /// (`task_work_add` with `TWA_SIGNAL`).
    pub(crate) fn notify(&self, task: TaskId) {
        self.notify_work(task, false);
    }

    pub(crate) fn notify_uring(&self, task: TaskId) {
        self.notify_work(task, true);
    }

    pub(crate) fn clear_uring_notify(&self, task: TaskId) {
        if let Some(task) = self.state.lock_or_recover().tasks.get_mut(&task) {
            task.uring_notify = false;
        }
    }

    fn notify_work(&self, task: TaskId, uring: bool) {
        let mut state = self.state.lock_or_recover();
        let Some(task) = state.tasks.get_mut(&task) else {
            return;
        };
        if uring {
            task.uring_notify = true;
        } else {
            task.notify = true;
        }
        task.flag.raise();
        drop(state);
        self.queue.wake(wasixv2_wait::PollMask::empty());
    }

    /// `de_thread` after an `execve` from `from`: `to`, the leader, goes on with `from`'s mask
    /// and pending signals, and `from` is gone from the group.
    pub(crate) fn take_over(&self, from: TaskId, to: TaskId) {
        let mut state = self.state.lock_or_recover();
        let Some(mut moved) = state.tasks.remove(&from) else {
            return;
        };
        if let Some(leader) = state.tasks.get_mut(&to) {
            moved.flag = leader.flag.clone();
            *leader = moved;
            leader.flag.raise();
        }
    }

    /// `send_signal`: queues `info` for the group, or for `task` alone, unless it would be
    /// ignored, and wakes the group's waiters. `EAGAIN` when the target's user has no room for
    /// a realtime signal that must keep its `siginfo`.
    fn send(&self, task: Option<TaskId>, info: Siginfo) -> Result<(), Errno> {
        self.generate(task, info, false).map(drop)
    }

    /// [`Signals::send`] from a sender outside the pid namespace of the group, which a
    /// namespace's init takes SIGKILL and SIGSTOP from (`send_signal_locked`'s `force`); the
    /// sender's pid in `info` is the caller's to clear.
    pub(crate) fn send_from(
        &self,
        task: Option<TaskId>,
        info: Siginfo,
        outside: bool,
    ) -> Result<(), Errno> {
        self.generate(task, info, outside).map(drop)
    }

    /// [`Signals::send`], saying whether the signal is pending now: `false` when the group
    /// ignores it and it was discarded (`prepare_signal` failing). A signal from the kernel
    /// (`SI_KERNEL`), or from outside the group's pid namespace, is forced on a namespace's
    /// init.
    fn generate(&self, task: Option<TaskId>, info: Siginfo, outside: bool) -> Result<bool, Errno> {
        let force = outside || info.si_code == SiCode::SI_KERNEL.0;
        // The charge reads the process's credentials and limits, which come before the group.
        let cred = self.cred(task);
        let charge = self
            .account
            .as_ref()
            .map_or(Ok(None), |a| a.charge(&info, cred.as_deref()));
        let mut state = self.state.lock_or_recover();
        if !state.prepare(task, info.si_signo, force) {
            drop(state);
            // A SIGCONT continues the group though it is discarded.
            if info.si_signo == Signal::SIGCONT.0 {
                self.queue.wake(wasixv2_wait::PollMask::empty());
            }
            return Ok(false);
        }
        let target = task.filter(|task| state.tasks.contains_key(task));
        state.pending(target).add(info, charge)?;
        state.complete(target, info.si_signo);
        // `complete_signal`: SIGKILL wakes even stopped tasks, which go on to their end.
        if info.si_signo == Signal::SIGKILL.0 {
            state.cancel_stop(false);
        }
        let vfork_children = state.killed_vfork_children(target, info.si_signo);
        drop(state);
        self.queue.wake(wasixv2_wait::PollMask::empty());
        for (signals, child) in vfork_children {
            signals.notify(child);
        }
        Ok(true)
    }

    /// `posixtimer_send_sigqueue`: queues the signal of `timer`, which just expired, for its
    /// target thread or the group. Called with the timer's state locked. `periodic` says
    /// whether the timer re-arms once its signal is delivered.
    pub(crate) fn send_timer(&self, timer: &Arc<PosixTimer>, periodic: bool) {
        let target = timer.target_thread();
        let info = timer.info();
        let mut state = self.state.lock_or_recover();
        // `task_can_queue_signal`: a target thread that is gone takes nothing.
        if target.is_some_and(|task| !state.tasks.contains_key(&task)) {
            return;
        }
        timer.set_sig_periodic(periodic);
        let queued = state.pending(target).has_timer(timer);
        let ignored_at = state
            .ignored_timers
            .iter()
            .position(|t| Arc::ptr_eq(t, timer));
        if !state.prepare(target, info.si_signo, true) {
            if info.si_signo == Signal::SIGCONT.0 {
                self.queue.wake(wasixv2_wait::PollMask::empty());
            }
            // A queued signal that is ignored now stays, as a regular one does. Otherwise a
            // periodic timer waits on the ignored list for the signal to be caught again,
            // and a one-shot one is done.
            match (queued, periodic, ignored_at) {
                (true, _, _) | (false, true, Some(_)) => {}
                (false, true, None) => state.ignored_timers.push(timer.clone()),
                (false, false, Some(at)) => {
                    state.ignored_timers.swap_remove(at);
                }
                (false, false, None) => {}
            }
            return;
        }
        if queued {
            return;
        }
        // Blocked again after it was ignored: back to the pending list.
        if let Some(at) = ignored_at {
            state.ignored_timers.swap_remove(at);
        }
        state.pending(target).add_timer(info, timer.clone());
        state.complete(target, info.si_signo);
        drop(state);
        self.queue.wake(wasixv2_wait::PollMask::empty());
    }

    /// `posix_timer_delete`'s signal half: `timer` stops being a timer of the group. A
    /// signal of it still queued stays, and is dropped when it is dequeued.
    pub(crate) fn unlink_timer(&self, timer: &Arc<PosixTimer>, id: i32) {
        let mut state = self.state.lock_or_recover();
        timer.invalidate();
        state.posix_timers.remove(id);
        state.ignored_timers.retain(|t| !Arc::ptr_eq(t, timer));
    }

    /// Runs `f` on the group's POSIX timer table, with the group locked.
    pub(crate) fn with_posix_timers<R>(&self, f: impl FnOnce(&mut posix_timer::Table) -> R) -> R {
        f(&mut self.state.lock_or_recover().posix_timers)
    }

    /// Whether `task` still belongs to the group.
    pub(crate) fn has_task(&self, task: TaskId) -> bool {
        self.state.lock_or_recover().tasks.contains_key(&task)
    }

    /// `flush_itimer_signals` at `execve`: the old image's POSIX timer signals go.
    pub(crate) fn flush_timer_signals(&self) {
        let mut state = self.state.lock_or_recover();
        state.shared.flush_timers();
        for task in state.tasks.values_mut() {
            task.pending.flush_timers();
        }
    }

    /// `hrtimer_cancel(&sig->real_timer)` once the group is dead.
    pub(crate) fn cancel_real_timer(&self) {
        let mut state = self.state.lock_or_recover();
        let real = &mut state.real_timer;
        (real.deadline, real.expired) = (None, None);
        if let Some((timer, _)) = &real.timer {
            timer.cancel();
        }
    }

    /// `dequeue_signal` with `state` the group's locked state. A POSIX timer's signal is
    /// handed to its timer with the group unlocked, as the timer's lock comes first; the
    /// timer re-arms and fills in `si_overrun`, or drops a signal that no longer counts.
    fn dequeue<'a>(
        &'a self,
        mut state: MutexGuard<'a, GroupState>,
        task: TaskId,
        wanted: u64,
    ) -> (MutexGuard<'a, GroupState>, Option<Siginfo>) {
        loop {
            let Some((mut info, timer)) = state.dequeue(task, wanted) else {
                return (state, None);
            };
            let Some(timer) = timer else {
                return (state, Some(info));
            };
            drop(state);
            let counts = timer.deliver(&mut info);
            state = self.state.lock_or_recover();
            if counts {
                return (state, Some(info));
            }
        }
    }
}

impl GroupState {
    /// `prepare_signal`: SIGCONT and the stop signals cancel each other, and SIGCONT continues
    /// a stopped group whatever its disposition; returns whether `sig` is to be queued for
    /// `task`, or for the group with `None`. A blocked signal is never ignored, since the
    /// handler may change before it is unblocked (`sig_ignored`). The caller wakes the group's
    /// queue after a SIGCONT, queued or not.
    fn prepare(&mut self, task: Option<TaskId>, sig: i32, force: bool) -> bool {
        let cancelled = match Signal(sig) {
            Signal::SIGCONT => STOP_SIGNALS,
            _ if STOP_SIGNALS & bit(sig) != 0 => bit(Signal::SIGCONT.0),
            _ => 0,
        };
        if cancelled != 0 {
            self.flush(cancelled);
        }
        if sig == Signal::SIGCONT.0 {
            self.continue_group();
        }
        let blocked = match task {
            Some(task) => self
                .tasks
                .get(&task)
                .is_some_and(|t| t.blocked & bit(sig) != 0),
            None => self.tasks.values().all(|t| t.blocked & bit(sig) != 0),
        };
        // `sig_ignored`: a traced task's tracer sees even an ignored signal.
        let traced = match task {
            Some(task) => self
                .tasks
                .get(&task)
                .is_some_and(|t| t.ptrace.link.is_some()),
            None => self
                .leader
                .and_then(|l| self.tasks.get(&l))
                .is_some_and(|t| t.ptrace.link.is_some()),
        };
        blocked || (traced && sig != Signal::SIGKILL.0) || !self.ignores(sig, force)
    }

    /// `prepare_signal`'s SIGCONT: every task goes on, and a group that was stopped, or was
    /// stopping, has that reported to its parent by the first task through
    /// [`Kernel::take_signal`]. A stop SIGCONT cut short is reported as a stop: SIGCHLD does not
    /// queue, so the continue that would follow it is as good as lost.
    fn continue_group(&mut self) {
        for task in self.tasks.values_mut() {
            (task.stop_pending, task.stop_dequeued, task.stopped) = (false, false, false);
            // kernel/signal.c `prepare_signal`, `ptrace_trap_notify`: SIGCONT
            // schedules a sticky notification even before the tracer starts LISTEN.
            if task.ptrace.link.is_some_and(|link| link.seized) {
                task.ptrace.trap_notify = true;
                if task.ptrace.listening {
                    task.ptrace.traced = false;
                }
                task.signal_wake_up();
            }
        }
        let report = match (self.jobctl.stopped, self.jobctl.group_stop_count) {
            (true, _) => SiCode::CLD_CONTINUED,
            (false, 0) => return,
            (false, _) => SiCode::CLD_STOPPED,
        };
        self.jobctl = JobCtl {
            continued: true,
            report: Some(report),
            ..JobCtl::default()
        };
        for task in self.tasks.values() {
            task.flag.raise();
        }
    }

    /// `do_signal_stop` for `task`, which dequeued the stop signal `sig`, or with `sig` 0 has
    /// its part to take in the group stop under way: unless one is under way, a group stop
    /// starts, which every task of the group that is not stopped takes part in; then `task`
    /// takes its part and is stopped. `None` when nothing stops: SIGCONT came since `task`
    /// dequeued `sig`, or the group is exiting or in an `execve` (`exiting`). Otherwise
    /// whether `task` completed the group stop, which its parent hears of.
    fn do_signal_stop(&mut self, task: TaskId, sig: i32, exiting: bool) -> Option<bool> {
        let me = &self.tasks[&task];
        if !me.stop_pending {
            if !me.stop_dequeued || exiting {
                return None;
            }
            // A group already stopped keeps the code it reports.
            if !self.jobctl.stopped {
                self.jobctl.stop_code = sig;
            }
            let mut count = 0;
            for (&id, t) in self.tasks.iter_mut() {
                if id == task || !t.stopped {
                    t.stop_pending = true;
                    t.flag.raise();
                    count += 1;
                }
            }
            self.jobctl.group_stop_count = count;
        }
        let completed = self.participate(task);
        let me = self.task(task);
        me.stopped = true;
        // Once continued, the task goes on in `get_signal`'s loop, and dequeues what came for
        // the group while it was stopped and passed by (`wants_signal`).
        me.sigpending = true;
        Some(completed)
    }

    /// `task_participate_group_stop`: `task`, which had its part to take in the group stop under
    /// way, took it. Returns whether that completed the stop.
    fn participate(&mut self, task: TaskId) -> bool {
        let state = self.task(task);
        (state.stop_pending, state.stop_dequeued) = (false, false);
        let jobctl = &mut self.jobctl;
        jobctl.group_stop_count = jobctl.group_stop_count.saturating_sub(1);
        if jobctl.group_stop_count != 0 || jobctl.stopped {
            return false;
        }
        // `signal_set_stop_flags`: a SIGCONT not reported yet no longer is.
        (jobctl.stopped, jobctl.continued, jobctl.report) = (true, false, None);
        true
    }

    /// Ends the group stop under way and continues every stopped task, without a SIGCONT: the
    /// group dies (`complete_signal`'s group exit, with `dying`, has nothing left to report),
    /// or an `execve` ends every task but one (`zap_other_threads`).
    fn cancel_stop(&mut self, dying: bool) {
        for task in self.tasks.values_mut() {
            (task.stop_pending, task.stop_dequeued, task.stopped) = (false, false, false);
            // A fatal signal ends ptrace stops too (`signal_wake_up(t, 1)` wakes `__TASK_TRACED`).
            task.ptrace.traced = false;
        }
        self.jobctl.group_stop_count = 0;
        if dying {
            self.jobctl = JobCtl::default();
        }
    }

    /// The pending list of `task`, or the group's for `None` or a task that is gone.
    fn pending(&mut self, task: Option<TaskId>) -> &mut Pending {
        match task {
            Some(task) if self.tasks.contains_key(&task) => &mut self.task(task).pending,
            _ => &mut self.shared,
        }
    }

    /// `flush_sigqueue_mask` on the group's and every task's pending list. A dropped signal
    /// of a periodic POSIX timer moves to the ignored list (`sigqueue_free_ignored`).
    fn flush(&mut self, mask: u64) {
        let mut timers = self.shared.flush(mask);
        for task in self.tasks.values_mut() {
            timers.extend(task.pending.flush(mask));
        }
        for timer in timers {
            // `posixtimer_sig_ignore`
            if timer.is_valid() && timer.sig_periodic() {
                self.ignored_timers.push(timer);
            }
        }
    }

    /// `complete_signal` for `sig`, just queued for `task` or for the group with `None`: picks
    /// the task to take it and wakes that one alone (`signal_wake_up`), while the others go on
    /// undisturbed. A signal for a task is its own, taken once it can. A group signal goes to
    /// the leader if it takes it now, or else to the next task that does, from where the last
    /// search ended (`curr_target`); with none, it waits until a task unblocks it. One that
    /// kills the group wakes every task that could take it, as Linux queues SIGKILL for each.
    /// Every task counts as running (`task_curr`), as the kernel does not know which are.
    fn complete(&mut self, task: Option<TaskId>, sig: i32) {
        let chosen = match task {
            Some(task) => self.wants_signal(task, sig).then_some(task),
            None => self.pick(sig),
        };
        let Some(chosen) = chosen else {
            return;
        };
        if task.is_none() && self.kills_group(sig) {
            for task in self.tasks.values_mut() {
                if task.blocked & bit(sig) == 0 {
                    task.signal_wake_up();
                }
            }
            return;
        }
        self.task(chosen).signal_wake_up();
    }

    /// `wants_signal`: `task` could take `sig` now. A stopped task takes only SIGKILL.
    fn wants_signal(&self, task: TaskId, sig: i32) -> bool {
        self.tasks.get(&task).is_some_and(|t| {
            t.blocked & bit(sig) == 0 && !t.exiting && (sig == Signal::SIGKILL.0 || !t.stopped)
        })
    }

    /// `complete_signal`'s choice of a task for the group signal `sig`: the leader, or the
    /// next task in thread id order from `curr_target` on that takes it, which becomes
    /// `curr_target`.
    fn pick(&mut self, sig: i32) -> Option<TaskId> {
        if let Some(leader) = self.leader.filter(|&leader| self.wants_signal(leader, sig)) {
            return Some(leader);
        }
        let mut ids: Vec<TaskId> = self.tasks.keys().copied().collect();
        ids.sort_unstable_by_key(|task| task.0);
        let from = self
            .curr_target
            .map_or(0, |target| ids.partition_point(|task| task.0 < target.0));
        let (after, before) = ids.split_at(from);
        let chosen = before
            .iter()
            .chain(after)
            .copied()
            .find(|&task| self.wants_signal(task, sig))?;
        self.curr_target = Some(chosen);
        Some(chosen)
    }

    /// `sig_fatal`: the default action of `sig` kills the group.
    fn kills_group(&self, sig: i32) -> bool {
        let action = self.sighand.get(sig);
        sig == Signal::SIGKILL.0
            || (action.is_default() && matches!(default_action(sig), DefaultAction::Terminate))
    }

    /// `recalc_sigpending` for `task`, in its own context: whether a signal waits for it is
    /// whether it could take one now, from its own pending set or the group's.
    fn recalc_sigpending(&mut self, task: TaskId) {
        let blocked = self.tasks[&task].blocked;
        let pending = self.deliverable(task, !blocked) != 0;
        let task = self.task(task);
        task.sigpending = pending;
        if pending {
            task.flag.raise();
        }
    }

    /// `__set_task_blocked`: `task` blocks `blocked` from now on. The group signals it was to
    /// take and now blocks go to other tasks (`retarget_shared_pending`), and what it can still
    /// take is recalculated. Returns whether another task was woken, whose wait the caller
    /// ends by waking the group's queue.
    fn set_blocked(&mut self, task: TaskId, blocked: u64) -> bool {
        let blocked = blocked & !UNBLOCKABLE;
        let me = &self.tasks[&task];
        let newly = blocked & !me.blocked;
        let woke = me.sigpending && self.retarget(task, newly);
        self.task(task).blocked = blocked;
        self.recalc_sigpending(task);
        woke
    }

    /// `retarget_shared_pending`: the group's pending signals among `which`, which `task` will
    /// not take, each go to another task that takes it, woken unless a signal waits for it
    /// already. Returns whether a task was woken.
    fn retarget(&mut self, task: TaskId, which: u64) -> bool {
        let mut retarget = self.shared.set & which;
        let mut woke = false;
        for (&id, t) in self.tasks.iter_mut() {
            if retarget == 0 {
                break;
            }
            if id == task || t.exiting || retarget & !t.blocked == 0 {
                continue;
            }
            retarget &= t.blocked;
            if !t.sigpending {
                t.signal_wake_up();
                woke = true;
            }
        }
        woke
    }

    /// The vfork children to wake for `sig`, just queued for `task` or for the group with
    /// `None`: those of the tasks it kills while they wait for them, as a fatal signal ends a
    /// killable wait.
    fn killed_vfork_children(&self, task: Option<TaskId>, sig: i32) -> Vec<(Arc<Signals>, TaskId)> {
        let action = self.sighand.get(sig);
        if !action.is_default() || !matches!(default_action(sig), DefaultAction::Terminate) {
            return Vec::new();
        }
        self.tasks
            .iter()
            .filter(|(id, t)| task.is_none_or(|task| task == **id) && t.blocked & bit(sig) == 0)
            .filter_map(|(_, t)| t.vfork_child.clone())
            .collect()
    }

    /// `posixtimer_sig_unignore`: `sig` is caught again, so the periodic timers that fired
    /// while it was ignored queue their signal now.
    fn unignore_timers(&mut self, sig: i32) {
        let (requeue, keep) = std::mem::take(&mut self.ignored_timers)
            .into_iter()
            .partition(|timer| timer.signo() == sig);
        self.ignored_timers = keep;
        for timer in requeue {
            let target = timer.target_thread();
            if target.is_some_and(|task| !self.tasks.contains_key(&task)) {
                continue;
            }
            let info = timer.info();
            self.pending(target).add_timer(info, timer);
            self.complete(target, sig);
        }
    }

    /// `sig_task_ignored`: the disposition throws the signal away. SIGKILL and SIGSTOP never are,
    /// but for an init, which takes only the signals it has handlers for, and SIGKILL and
    /// SIGSTOP when they are `force`d on a namespace's init.
    fn ignores(&self, sig: i32, force: bool) -> bool {
        let action = self.sighand.get(sig);
        let kernel_only = UNBLOCKABLE & bit(sig) != 0;
        if self.global_init && kernel_only {
            return true;
        }
        if self.unkillable && action.is_default() && !(force && kernel_only) {
            return true;
        }
        if UNBLOCKABLE & bit(sig) != 0 {
            return false;
        }
        action.is_ignore()
            || (action.is_default() && matches!(default_action(sig), DefaultAction::Ignore))
    }

    /// The fatal signal `task` could take now (see [`TaskSignals::fatal_pending`]).
    fn fatal(&self, task: TaskId) -> Option<i32> {
        let me = &self.tasks[&task];
        let mut pending = (me.pending.set | self.shared.set) & !me.blocked;
        while pending != 0 {
            let sig = pending.trailing_zeros() as i32 + 1;
            let action = self.sighand.get(sig);
            let kills = !self.unkillable || UNBLOCKABLE & bit(sig) != 0;
            if action.is_default()
                && kills
                && matches!(default_action(sig), DefaultAction::Terminate)
            {
                return Some(sig);
            }
            pending &= !bit(sig);
        }
        None
    }

    fn task(&mut self, task: TaskId) -> &mut TaskState {
        self.tasks
            .get_mut(&task)
            .expect("signal state of an unknown task")
    }

    /// Signals `task` could take now, among `wanted`.
    fn deliverable(&self, task: TaskId, wanted: u64) -> u64 {
        let state = &self.tasks[&task];
        (state.pending.set | self.shared.set) & wanted
    }

    /// Whether `task` could take one of `hidden` now, the signals a call it makes would block
    /// or discard. If so its work flag is raised, so that it takes the signal before the call,
    /// which runs again after the handler (`ERESTARTNOINTR`). Linux delivers a signal to a task
    /// in user mode at once (`signal_wake_up` kicks it), before it gets to its next syscall;
    /// here the signal waits for the task's next interruption check, and a call that hid it
    /// could keep it from the task for good, as when a thread blocks every signal on its way out
    /// and no other thread takes it.
    fn take_first(&mut self, task: TaskId, hidden: u64) -> bool {
        let me = &self.tasks[&task];
        // Only a signal that waits for the task: Linux kicks the task a signal was given to.
        if !me.sigpending || self.deliverable(task, hidden & !me.blocked) == 0 {
            return false;
        }
        self.task(task).flag.raise();
        true
    }

    /// `dequeue_signal`: the lowest-numbered signal in `wanted`, from the task's own pending
    /// set first, then the group's, after which the task is pending only for what is left
    /// (`recalc_sigpending`). A POSIX timer's signal comes with its timer, which the caller
    /// asks, with the group unlocked, whether the signal still counts.
    fn dequeue(&mut self, task: TaskId, wanted: u64) -> Option<(Siginfo, Option<Arc<PosixTimer>>)> {
        let own = self.task(task).pending.set & wanted;
        let taken = if own != 0 {
            let sig = own.trailing_zeros() as i32 + 1;
            Some(self.task(task).pending.take(sig))
        } else {
            let shared = self.shared.set & wanted;
            (shared != 0).then(|| self.take_shared(shared.trailing_zeros() as i32 + 1))
        };
        self.recalc_sigpending(task);
        taken
    }

    /// Takes `sig` from the group's pending set; SIGALRM re-arms a periodic `ITIMER_REAL`.
    fn take_shared(&mut self, sig: i32) -> (Siginfo, Option<Arc<PosixTimer>>) {
        let taken = self.shared.take(sig);
        if sig == Signal::SIGALRM.0 {
            self.real_timer.rearm();
        }
        taken
    }
}

// ---- ptrace ----

/// A ptrace stop to enter (`ptrace_stop`'s arguments).
pub(crate) struct TraceStop {
    pub(crate) exit_code: i32,
    /// `CLD_TRAPPED`, or `CLD_STOPPED` for a stop that takes part in a group stop.
    pub(crate) why: SiCode,
    pub(crate) message: u64,
    pub(crate) info: Option<Siginfo>,
    pub(crate) sync: Option<crate::ptrace::SyncStop>,
}

impl GroupState {
    /// `__fatal_signal_pending`: SIGKILL waits for `task`.
    fn sigkill_pending(&self, task: TaskId) -> bool {
        let own = self.tasks.get(&task).map_or(0, |t| t.pending.set);
        (own | self.shared.set) & bit(Signal::SIGKILL.0) != 0
    }

    /// `ptrace_stop`'s first half: `task` enters a ptrace stop, unless it is not traced or
    /// SIGKILL is pending. Returns whether that completed a group stop it took part in.
    fn enter_trace_stop(&mut self, task: TaskId, stop: TraceStop) -> Option<bool> {
        let killed = self.sigkill_pending(task);
        let me = self.tasks.get_mut(&task)?;
        if me.ptrace.link.is_none() || killed {
            return None;
        }
        let trace = &mut me.ptrace;
        trace.traced = true;
        (trace.message, trace.exit_code) = (stop.message, stop.exit_code);
        trace.sync_stop = stop.sync;
        trace.trap_stop = false;
        if stop
            .info
            .is_some_and(|info| info.si_code >> 8 == crate::ptrace::EVENT_STOP)
        {
            trace.trap_notify = false;
        }
        trace.last_siginfo = stop.info;
        let pending = me.stop_pending;
        Some(stop.why == SiCode::CLD_STOPPED && pending && self.participate(task))
    }

    /// `ptrace_stop`'s second half, once the task is woken: what the tracer resumed it with,
    /// and the signal information of the stop as the tracer left it.
    fn leave_trace_stop(&mut self, task: TaskId) -> (i32, Option<Siginfo>) {
        let Some(me) = self.tasks.get_mut(&task) else {
            return (0, None);
        };
        let trace = &mut me.ptrace;
        let code = std::mem::take(&mut trace.exit_code);
        let info = trace.last_siginfo.take();
        (
            trace.message,
            trace.listening,
            trace.traced,
            trace.sync_stop,
        ) = (0, false, false, None);
        self.recalc_sigpending(task);
        (code, info)
    }
}

impl Signals {
    /// `send_sig(sig, task, 1)`: `sig` from the kernel, to `task` alone.
    pub(crate) fn send_task_kernel(&self, task: TaskId, sig: i32) {
        // kernel/signal.c:send_sig_info validates kernel senders too. If SIGKILL
        // prevented a syscall stop, ptrace_stop can return SIGTRAP | 0x80.
        if check_sendable(sig).is_err() {
            return;
        }
        let _ = self.send(Some(task), kernel_info(sig));
    }

    /// `fatal_signal_pending` for `task`.
    pub(crate) fn sigkill_pending_for(&self, task: TaskId) -> bool {
        self.state.lock_or_recover().sigkill_pending(task)
    }

    /// Runs `f` on `task`'s ptrace state, if the task is in the group.
    pub(crate) fn with_trace<R>(
        &self,
        task: TaskId,
        f: impl FnOnce(&mut crate::ptrace::TaskTrace) -> R,
    ) -> Option<R> {
        let mut state = self.state.lock_or_recover();
        state.tasks.get_mut(&task).map(|t| f(&mut t.ptrace))
    }

    /// `ptrace_link` and `ptrace_set_stopped` (`PTRACE_ATTACH`, `PTRACE_SEIZE`,
    /// `PTRACE_TRACEME` and a traced parent's new child): `EPERM` for a task that is exiting or
    /// traced already. An attach that does not seize sends SIGSTOP (`with_sigstop`); a task in
    /// a group stop leaves it for a trap stop, which its tracer is told of.
    pub(crate) fn ptrace_link(
        &self,
        task: TaskId,
        link: crate::ptrace::Link,
        with_sigstop: bool,
    ) -> Result<(), Errno> {
        let mut state = self.state.lock_or_recover();
        let me = state.tasks.get_mut(&task).ok_or(Errno::EPERM)?;
        if me.exiting || me.ptrace.link.is_some() {
            return Err(Errno::EPERM);
        }
        me.ptrace = crate::ptrace::TaskTrace {
            link: Some(link),
            ..Default::default()
        };
        if with_sigstop {
            let info = kernel_info(Signal::SIGSTOP.0);
            let _ = state.pending(Some(task)).add(info, Ok(None));
            state.complete(Some(task), Signal::SIGSTOP.0);
        }
        let me = state.task(task);
        if me.stopped {
            me.stopped = false;
            me.ptrace.trap_stop = true;
            me.signal_wake_up();
        }
        drop(state);
        self.queue.wake(wasixv2_wait::PollMask::empty());
        Ok(())
    }

    /// `__ptrace_unlink`: `task` is traced no more. It goes on with `exit_code` if it was in
    /// a ptrace stop (`PTRACE_DETACH`'s signal), and back into a group stop under way or done.
    pub(crate) fn ptrace_unlink(&self, task: TaskId, exit_code: Option<i32>) {
        let mut state = self.state.lock_or_recover();
        let group_stop = state.jobctl.stopped || state.jobctl.group_stop_count != 0;
        let Some(me) = state.tasks.get_mut(&task) else {
            return;
        };
        let was_traced = me.ptrace.traced;
        let resume = me.ptrace.sync_stop;
        me.ptrace = crate::ptrace::TaskTrace {
            exit_code: exit_code.unwrap_or(me.ptrace.exit_code),
            last_siginfo: me.ptrace.last_siginfo,
            // A stop the task still has to come back from keeps what it needs for that.
            sync_stop: resume,
            ..Default::default()
        };
        if !me.exiting && group_stop {
            me.stop_pending = true;
        }
        if me.stop_pending || was_traced {
            me.signal_wake_up();
        }
        drop(state);
        self.queue.wake(wasixv2_wait::PollMask::empty());
    }

    /// `ptrace_check_attach` (with `ptrace_freeze_traced`): `task` is traced by `tracer` and,
    /// unless `any_state`, sits in a ptrace stop it is not listening in, with no SIGKILL
    /// pending; `ESRCH` otherwise.
    pub(crate) fn ptrace_check(
        &self,
        task: TaskId,
        tracer: TaskId,
        any_state: bool,
    ) -> Result<crate::ptrace::Link, Errno> {
        let state = self.state.lock_or_recover();
        let me = state.tasks.get(&task).ok_or(Errno::ESRCH)?;
        let link = me.ptrace.link.filter(|link| link.tracer == tracer);
        let link = link.ok_or(Errno::ESRCH)?;
        let stopped = me.ptrace.traced && !me.ptrace.listening && !state.sigkill_pending(task);
        match any_state || stopped {
            true => Ok(link),
            false => Err(Errno::ESRCH),
        }
    }

    /// `ptrace_resume`: `task` leaves its ptrace stop with `data`, stopping at syscalls from
    /// now on with `syscall` (`PTRACE_SYSCALL`).
    pub(crate) fn ptrace_resume(&self, task: TaskId, syscall: bool, data: i32) {
        let mut state = self.state.lock_or_recover();
        if let Some(me) = state.tasks.get_mut(&task) {
            me.ptrace.syscall = syscall;
            me.ptrace.exit_code = data;
            me.ptrace.traced = false;
            me.flag.raise();
        }
        drop(state);
        self.queue.wake(wasixv2_wait::PollMask::empty());
    }

    /// `PTRACE_INTERRUPT`: a trap stop is due (`JOBCTL_TRAP_STOP`); a listening task leaves
    /// its stop to take it.
    pub(crate) fn ptrace_interrupt(&self, task: TaskId) {
        let mut state = self.state.lock_or_recover();
        let Some(me) = state.tasks.get_mut(&task) else {
            return;
        };
        me.ptrace.trap_stop = true;
        if me.ptrace.listening {
            me.ptrace.traced = false;
        }
        me.signal_wake_up();
        drop(state);
        self.queue.wake(wasixv2_wait::PollMask::empty());
    }

    /// `PTRACE_LISTEN`: in a `PTRACE_EVENT_STOP` trap stop, the task listens: a wait does not
    /// report it, and a notification (`JOBCTL_TRAP_NOTIFY`) traps it again. `EIO` in any other
    /// stop.
    pub(crate) fn ptrace_listen(&self, task: TaskId) -> Result<(), Errno> {
        let mut state = self.state.lock_or_recover();
        let me = state.tasks.get_mut(&task).ok_or(Errno::EIO)?;
        let event_stop = me
            .ptrace
            .last_siginfo
            .is_some_and(|info| info.si_code >> 8 == crate::ptrace::EVENT_STOP);
        if !event_stop {
            return Err(Errno::EIO);
        }
        me.ptrace.listening = true;
        if me.ptrace.trap_notify {
            me.ptrace.traced = false;
            me.signal_wake_up();
        }
        drop(state);
        self.queue.wake(wasixv2_wait::PollMask::empty());
        Ok(())
    }

    /// `task_stopped_code(p, ptrace=1)` for a tracer's `wait`: the code of `task`'s ptrace
    /// stop that no wait reported yet, which a wait without `nowait` takes.
    pub(crate) fn ptrace_report(&self, task: TaskId, nowait: bool) -> Option<i32> {
        let mut state = self.state.lock_or_recover();
        let trace = &mut state.tasks.get_mut(&task)?.ptrace;
        if !trace.traced || trace.listening || trace.exit_code == 0 {
            return None;
        }
        let code = trace.exit_code;
        if !nowait {
            trace.exit_code = 0;
        }
        Some(code)
    }

    /// `ptrace_stop`'s first half for a stop outside [`Kernel::take_signal`].
    pub(crate) fn enter_trace_stop(&self, task: TaskId, stop: TraceStop) -> Option<bool> {
        self.state.lock_or_recover().enter_trace_stop(task, stop)
    }

    /// kernel/signal.c `get_signal` dequeues the fatal signal before kernel/exit.c
    /// `do_exit` reports PTRACE_EVENT_EXIT. Group exit already queued every task's exit.
    pub(crate) fn enter_exit_stop(&self, task: TaskId, stop: TraceStop) -> Option<bool> {
        let mut state = self.state.lock_or_recover();
        while state.dequeue(task, bit(Signal::SIGKILL.0)).is_some() {}
        state.enter_trace_stop(task, stop)
    }

    /// `ptrace_stop`'s second half for a stop outside [`Kernel::take_signal`].
    pub(crate) fn leave_trace_stop(&self, task: TaskId) -> (i32, Option<Siginfo>) {
        self.state.lock_or_recover().leave_trace_stop(task)
    }

    /// `ptrace_peek_siginfo`: up to `nr` signals queued for `task` (or the group, `shared`)
    /// from the `off`-th on.
    pub(crate) fn peek_siginfo(
        &self,
        task: TaskId,
        shared: bool,
        off: u64,
        nr: usize,
    ) -> Vec<Siginfo> {
        let state = self.state.lock_or_recover();
        let pending = match shared {
            true => &state.shared,
            false => match state.tasks.get(&task) {
                Some(t) => &t.pending,
                None => return Vec::new(),
            },
        };
        let skip = usize::try_from(off).unwrap_or(usize::MAX);
        pending.list().skip(skip).take(nr).collect()
    }

    /// The mask `PTRACE_GETSIGMASK` reads: a `sigsuspend`'s saved one while it lasts.
    pub(crate) fn trace_sigmask(&self, task: TaskId) -> Option<u64> {
        let state = self.state.lock_or_recover();
        let me = state.tasks.get(&task)?;
        Some(me.saved.unwrap_or(me.blocked))
    }

    /// `PTRACE_SETSIGMASK`: the mask, minus SIGKILL and SIGSTOP, with any saved one forgotten.
    pub(crate) fn set_trace_sigmask(&self, task: TaskId, mask: u64) {
        let mut state = self.state.lock_or_recover();
        if state.tasks.contains_key(&task) {
            state.task(task).saved = None;
            let woke = state.set_blocked(task, mask);
            drop(state);
            if woke {
                self.queue.wake(wasixv2_wait::PollMask::empty());
            }
        }
    }
}

/// One task's view of its group's signals: Linux's `signal_pending` for blocking waits.
pub(crate) struct TaskSignals {
    group: Arc<Signals>,
    task: TaskId,
    pub(crate) status: wasixv2_wait::TaskStatus,
}

impl TaskSignals {
    pub(crate) fn publish_cred(&self, cred: Arc<crate::cred::Cred>) {
        self.with(|task| task.cred = Some(cred));
    }

    pub(crate) fn group(&self) -> &Arc<Signals> {
        &self.group
    }

    fn with<R>(&self, f: impl FnOnce(&mut TaskState) -> R) -> R {
        let mut state = self.group.state.lock_or_recover();
        f(state.task(self.task))
    }

    /// `force_sig_info_to_task` with `HANDLER_CURRENT`: a synchronous signal for the task that
    /// it cannot block or ignore. Blocked or ignored, it goes back to its default action and
    /// is unblocked; with the default action, a namespace's init is killable by it.
    pub(crate) fn force(&self, info: Siginfo) {
        let sig = info.si_signo;
        {
            let mut state = self.group.state.lock_or_recover();
            let action = state.sighand.get(sig);
            let blocked = state.task(self.task).blocked & bit(sig) != 0;
            if blocked || action.is_ignore() {
                let handler = SignalHandler::SIG_DFL.0 as u32;
                state.sighand.set(sig, Action { handler, ..action });
                state.task(self.task).blocked &= !bit(sig);
            }
            if state.sighand.get(sig).is_default() {
                state.unkillable = false;
            }
        }
        // The task's own signal: no queue limit stops it.
        let _ = self.group.send(Some(self.task), info);
    }

    /// `sigprocmask`'s `set_current_blocked`: SIGKILL and SIGSTOP are never blocked. Returns the
    /// previous mask. A signal it unblocks is delivered on the way back to the guest
    /// (`recalc_sigpending`).
    pub(crate) fn replace_blocked(&self, blocked: u64) -> u64 {
        let mut state = self.group.state.lock_or_recover();
        let old = state.task(self.task).blocked;
        let woke = state.set_blocked(self.task, blocked);
        drop(state);
        self.wake_if(woke);
        old
    }

    /// Ends the waits of the tasks a mask change handed the group's signals to.
    fn wake_if(&self, woke: bool) {
        if woke {
            self.group.queue.wake(wasixv2_wait::PollMask::empty());
        }
    }

    /// [`TaskSignals::replace_blocked`] for `sigprocmask`, unless the new mask blocks a signal
    /// the task could take now: then `None`, the mask stays, and the task takes that signal
    /// before it makes the call again ([`GroupState::take_first`]).
    pub(crate) fn block_after_pending(&self, blocked: u64) -> Option<u64> {
        let mut state = self.group.state.lock_or_recover();
        let blocked = blocked & !UNBLOCKABLE;
        let old = state.task(self.task).blocked;
        if state.take_first(self.task, blocked & !old) {
            return None;
        }
        let woke = state.set_blocked(self.task, blocked);
        drop(state);
        self.wake_if(woke);
        Some(old)
    }

    pub(crate) fn blocked(&self) -> u64 {
        self.with(|task| task.blocked)
    }

    /// `is_ignored` of `tty_jobctrl.c`: the task blocks `sig`, or its group ignores it.
    pub(crate) fn ignores_or_blocks(&self, sig: i32) -> bool {
        let mut state = self.group.state.lock_or_recover();
        let ignored = state.sighand.get(sig).is_ignore();
        ignored || state.task(self.task).blocked & bit(sig) != 0
    }

    /// What a task this one creates with `flags` inherits: the mask, and the alternate stack
    /// unless the new task shares the address space and runs alongside (`sas_ss_reset` for
    /// `CLONE_VM` without `CLONE_VFORK`: a thread).
    pub(crate) fn inherit(&self, flags: CloneFlags) -> Inherited {
        let thread =
            flags.contains(CloneFlags::CLONE_VM) && !flags.contains(CloneFlags::CLONE_VFORK);
        self.with(|task| Inherited {
            blocked: task.blocked,
            altstack: match thread {
                true => AltStack::default(),
                false => task.altstack,
            },
        })
    }

    /// The masks `/proc/<pid>/status` shows: pending for the task and for the group, blocked,
    /// ignored and caught.
    pub(crate) fn proc_status(&self) -> [u64; 5] {
        let state = self.group.state.lock_or_recover();
        let task = state.tasks.get(&self.task);
        let (pending, blocked) = task.map_or((0, 0), |task| (task.pending.set, task.blocked));
        let mut ignored = 0;
        let mut caught = 0;
        for (index, action) in state.sighand.all().iter().enumerate() {
            let mask = 1 << index;
            if action.is_ignore() {
                ignored |= mask;
            } else if !action.is_default() {
                caught |= mask;
            }
        }
        [pending, state.shared.set, blocked, ignored, caught]
    }

    /// Ends a temporary mask (`sigsuspend`, `ppoll`, `pselect`, `epoll_pwait`). When the call
    /// was interrupted, the temporary mask stays until the handler's frame is built, whose
    /// `uc_sigmask` then holds `saved` (`restore_saved_sigmask_unless`).
    pub(crate) fn restore_blocked(&self, saved: u64, interrupted: bool) {
        let mut state = self.group.state.lock_or_recover();
        let task = state.task(self.task);
        task.flag.raise();
        if interrupted {
            task.saved = Some(saved);
            return;
        }
        let woke = state.set_blocked(self.task, saved);
        drop(state);
        self.wake_if(woke);
    }

    /// The signal half of `rt_sigreturn` for the task's innermost handler (see
    /// [`Kernel::signal_return`]). A task whose `execve` handed its signal state to the leader
    /// (`de_thread`) has none left: its handler unwinds with the old image, and nothing of it
    /// is restored.
    fn sigreturn(&self, mask: Option<u64>) {
        let mut state = self.group.state.lock_or_recover();
        let Some(task) = state.tasks.get_mut(&self.task) else {
            return;
        };
        let saved = task.frames.pop();
        let Some(mask) = mask else {
            return;
        };
        if let Some(alt) = saved {
            task.altstack = alt;
        }
        task.flag.raise();
        let woke = state.set_blocked(self.task, mask);
        drop(state);
        self.wake_if(woke);
    }

    /// The fatal signal pending for the task, as a killable wait sees it
    /// (`__fatal_signal_pending`): one the task does not block whose action is a default that
    /// kills, SIGKILL among them, for the task or its group. Linux's `complete_signal` makes such
    /// a signal a SIGKILL for every thread as it is sent.
    pub(crate) fn fatal_pending(&self) -> Option<i32> {
        let state = self.group.state.lock_or_recover();
        state.tasks.contains_key(&self.task).then_some(())?;
        state.fatal(self.task)
    }

    /// What ends a killable wait of the task (`TASK_KILLABLE`): a fatal signal only.
    pub(crate) fn killable(&self) -> Killable<'_> {
        Killable(self, wasixv2_wait::TaskState::Uninterruptible)
    }

    /// Queues SIGKILL for this task alone, as `zap_other_threads` does for each thread.
    pub(crate) fn kill(&self) {
        let _ = self
            .group
            .send(Some(self.task), kernel_info(Signal::SIGKILL.0));
    }

    /// Records the vfork child the task waits for, or with `None` that it waits no more.
    pub(crate) fn set_vfork_child(&self, child: Option<(Arc<Signals>, TaskId)>) {
        if let Some(task) = self.group.state.lock_or_recover().tasks.get_mut(&self.task) {
            task.vfork_child = child;
        }
    }

    /// Removes the task from its group once it is gone (`__exit_signal`). Returns whether that
    /// completed a group stop the task had its part to take in, which only a task that never
    /// started still has (an exiting one took its part on its way out,
    /// [`Kernel::exit_signals`]).
    pub(crate) fn detach(&self) -> bool {
        let mut state = self.group.state.lock_or_recover();
        let Some(me) = state.tasks.get(&self.task) else {
            return false;
        };
        // A task that never got to its exit (one that never started) hands on the group
        // signals it was to take, as [`Kernel::exit_signals`] does for the others.
        let (pending, retarget, unblocked) =
            (me.stop_pending, me.sigpending && !me.exiting, !me.blocked);
        let woke = retarget && state.retarget(self.task, unblocked);
        let completed = pending && state.participate(self.task);
        state.tasks.remove(&self.task);
        drop(state);
        self.wake_if(woke);
        completed
    }

    /// The task is stopped (`TASK_STOPPED`) or in a ptrace stop (`TASK_TRACED`), as its stop
    /// wait waits on.
    pub(crate) fn is_stopped(&self) -> bool {
        let state = self.group.state.lock_or_recover();
        state
            .tasks
            .get(&self.task)
            .is_some_and(|t| t.stopped || t.ptrace.traced)
    }

    /// The task tracing this one (`ptrace_parent`).
    pub(crate) fn tracer(&self) -> Option<TaskId> {
        let state = self.group.state.lock_or_recover();
        state
            .tasks
            .get(&self.task)?
            .ptrace
            .link
            .map(|link| link.tracer)
    }

    /// The task is in a ptrace stop (`TASK_TRACED`), as `/proc` shows it (`t`).
    pub(crate) fn is_traced(&self) -> bool {
        let state = self.group.state.lock_or_recover();
        state.tasks.get(&self.task).is_some_and(|t| t.ptrace.traced)
    }

    /// `cgroup_freeze_task`: the task's cgroup freezes it, and it leaves its blocking wait for
    /// the freezer (`signal_wake_up`), or thaws it, and it goes on. A task on its way out is
    /// not bothered.
    pub(crate) fn freeze(&self, freeze: bool) {
        let mut state = self.group.state.lock_or_recover();
        let Some(task) = state.tasks.get_mut(&self.task).filter(|t| !t.exiting) else {
            return;
        };
        task.trap_freeze = freeze;
        task.flag.raise();
        drop(state);
        self.group.queue.wake(wasixv2_wait::PollMask::empty());
    }

    /// The task's cgroup throttles it until `until` (`throttle_cfs_rq`): it waits on its way
    /// back to the guest.
    pub(crate) fn throttle(&self, until: Duration) {
        let mut state = self.group.state.lock_or_recover();
        if let Some(task) = state.tasks.get_mut(&self.task).filter(|t| !t.exiting) {
            task.throttled_until = Some(until);
            task.flag.raise();
        }
    }

    /// Takes the end of the task's throttling, if it is throttled.
    fn take_throttle(&self) -> Option<Duration> {
        let mut state = self.group.state.lock_or_recover();
        state.tasks.get_mut(&self.task)?.throttled_until.take()
    }

    fn throttled_until(&self) -> Option<Duration> {
        let state = self.group.state.lock_or_recover();
        state.tasks.get(&self.task)?.throttled_until
    }

    /// What holds the task where it is: a stop until SIGCONT, or the freezer until it is thawed
    /// or a fatal signal comes, which it goes on to take.
    fn held(&self) -> bool {
        let state = self.group.state.lock_or_recover();
        state.tasks.get(&self.task).is_some_and(|t| {
            t.stopped || t.ptrace.traced || (t.trap_freeze && state.fatal(self.task).is_none())
        })
    }

    // ---- ipc: signalfd ----
    /// `dequeue_signal` for a signalfd: the next signal in `wanted` pending for the task or its
    /// group, blocked or not. Once the task is gone, only the group's.
    pub(crate) fn dequeue_from(&self, wanted: u64) -> Option<Siginfo> {
        let mut state = self.group.state.lock_or_recover();
        if state.tasks.contains_key(&self.task) {
            return self.group.dequeue(state, self.task, wanted).1;
        }
        // As `Signals::dequeue`, from the group's queue only.
        loop {
            let shared = state.shared.set & wanted;
            if shared == 0 {
                return None;
            }
            let (mut info, timer) = state.take_shared(shared.trailing_zeros() as i32 + 1);
            let Some(timer) = timer else {
                return Some(info);
            };
            drop(state);
            if timer.deliver(&mut info) {
                return Some(info);
            }
            state = self.group.state.lock_or_recover();
        }
    }

    /// Whether a signal in `wanted` is pending for the task or its group.
    pub(crate) fn has_pending(&self, wanted: u64) -> bool {
        let state = self.group.state.lock_or_recover();
        let own = state
            .tasks
            .get(&self.task)
            .map_or(0, |task| task.pending.set);
        (own | state.shared.set) & wanted != 0
    }

    /// Woken whenever a signal is sent to the group or one of its tasks.
    pub(crate) fn sent(&self) -> &WaitQueue {
        &self.group.queue
    }
    // ---- end ipc ----

    /// Waits until a signal the task does not block is pending.
    async fn wait_unblocked(&self) {
        let waiter = Waiter::new();
        while !self.pending() {
            waiter.sleep(self, None).await;
        }
    }
}

impl Interrupt for TaskSignals {
    fn task_status(&self) -> Option<(&wasixv2_wait::TaskStatus, wasixv2_wait::TaskState)> {
        Some((&self.status, wasixv2_wait::TaskState::Interruptible))
    }

    fn pending(&self) -> bool {
        let state = self.group.state.lock_or_recover();
        // A task taken over by an `execve` from another thread has nothing left to wait for.
        let Some(task) = state.tasks.get(&self.task) else {
            return true;
        };
        // A group stop and the freezer set `TIF_SIGPENDING` too (`recalc_sigpending_tsk`).
        (task.notify || task.uring_notify)
            || task.stop_pending
            || task.trap_freeze
            || task.sigpending
    }

    fn queue(&self) -> Option<&WaitQueue> {
        Some(&self.group.queue)
    }
}

/// See [`TaskSignals::killable`].
pub(crate) struct Killable<'a>(&'a TaskSignals, wasixv2_wait::TaskState);

impl Interrupt for Killable<'_> {
    fn task_status(&self) -> Option<(&wasixv2_wait::TaskStatus, wasixv2_wait::TaskState)> {
        Some((&self.0.status, self.1))
    }

    fn pending(&self) -> bool {
        self.0.fatal_pending().is_some()
    }

    fn queue(&self) -> Option<&WaitQueue> {
        Some(&self.0.group.queue)
    }
}

/// `sigtimedwait`'s wake condition: one of `these`, or any signal the task takes.
struct WaitFor<'a> {
    signals: &'a TaskSignals,
    these: u64,
}

impl Interrupt for WaitFor<'_> {
    fn task_status(&self) -> Option<(&wasixv2_wait::TaskStatus, wasixv2_wait::TaskState)> {
        self.signals.task_status()
    }

    fn pending(&self) -> bool {
        let state = self.signals.group.state.lock_or_recover();
        let Some(task) = state.tasks.get(&self.signals.task) else {
            return true;
        };
        // A group stop ends the wait too, which then fails with EINTR, as on Linux. The signals
        // waited for count while blocked, as Linux unblocks them for the wait.
        task.notify
            || task.uring_notify
            || task.stop_pending
            || task.sigpending
            || state.deliverable(self.signals.task, self.these) != 0
    }

    fn queue(&self) -> Option<&WaitQueue> {
        Some(&self.signals.group.queue)
    }
}

/// `si_code_reserved_to_kernel`: codes only the kernel sends, `SI_USER` and `SI_KERNEL` among
/// them, and `tkill`'s.
pub(crate) fn reserved_to_kernel(code: i32) -> bool {
    code >= 0 || code == SiCode::SI_TKILL.0
}

/// `has_si_pid_and_uid`: whether `info`'s layout carries the sender's pid and uid
/// (`SIL_KILL`, `SIL_CHLD`, `SIL_RT` of `siginfo_layout`).
fn has_si_pid_and_uid(info: &Siginfo) -> bool {
    let code = info.si_code;
    match code {
        _ if code == SiCode::SI_TIMER.0 || code == SiCode::SI_SIGIO.0 => false,
        _ if code <= 0 || code == SiCode::SI_KERNEL.0 => true,
        // The codes of a child's state change carry the child's.
        _ => info.si_signo == Signal::SIGCHLD.0,
    }
}

fn check_sigsetsize(size: u32) -> Result<(), Errno> {
    match size as usize == Sigset::SIZE {
        true => Ok(()),
        false => Err(Errno::EINVAL),
    }
}

pub(crate) fn check_signal(sig: Signal) -> Result<i32, Errno> {
    match sig.0 {
        1..=NSIG => Ok(sig.0),
        _ => Err(Errno::EINVAL),
    }
}

/// `valid_signal`: a signal a process may send, where 0 only checks that the target exists.
/// Linux checks it once the target is found, so a missing target is `ESRCH` first.
fn check_sendable(sig: i32) -> Result<(), Errno> {
    match (0..=NSIG).contains(&sig) {
        true => Ok(()),
        false => Err(Errno::EINVAL),
    }
}

pub(crate) fn sigset_bits(set: Sigset) -> u64 {
    u64::from(set.sig[0]) | (u64::from(set.sig[1]) << 32)
}

fn to_sigset(bits: u64) -> Sigset {
    Sigset {
        sig: [bits as u32, (bits >> 32) as u32],
    }
}

use wasixv2_abi::GuestLayout;

pub(crate) struct Sender {
    process: Arc<Process>,
    cred: Arc<crate::cred::Cred>,
}

impl std::ops::Deref for Sender {
    type Target = Process;
    fn deref(&self) -> &Process {
        &self.process
    }
}

impl Kernel {
    pub(crate) fn signal_sender(&self, task: TaskId) -> Sender {
        Sender {
            process: self.process(task),
            cred: self.current_cred(task),
        }
    }

    pub(crate) fn task_signals(&self, task: TaskId) -> Arc<TaskSignals> {
        self.task_ref(task, |task| task.signals.clone())
    }

    /// The signal state a blocked call watches.
    pub(crate) fn interrupt(&self, caller: &Caller<'_>) -> Arc<TaskSignals> {
        self.task_signals(caller.task())
    }

    /// The thread group the id `pid` names, or every group for `None`, by its leader's id. As
    /// for Linux's `kill_pid_info`, any thread's id names its whole group, and the group id still
    /// does once its leader thread has exited.
    fn groups(&self, pid: Option<u32>) -> Vec<(u32, Arc<Signals>)> {
        let state = self.state.read_or_recover();
        let mut groups: Vec<(u32, Arc<Signals>)> = Vec::new();
        for (id, task) in &state.tasks {
            let leader = task.process.pid.0;
            let named = pid.is_none_or(|pid| pid == leader || pid == id.0);
            if !named || groups.iter().any(|(p, _)| *p == leader) {
                continue;
            }
            // ---- proc ----
            // A reaped process's tasks may linger while their instance unwinds.
            if self.procs.find(leader).is_none() {
                continue;
            }
            groups.push((leader, task.signals.group().clone()));
        }
        drop(state);
        // ---- proc ----
        // A zombie has no tasks left, but it is there until reaped: a signal to it succeeds and
        // goes nowhere.
        if let Some(pid) = pid
            && groups.is_empty()
            && self.procs.find(pid).is_some()
        {
            groups.push((pid, Signals::new(None)));
        }
        groups
    }

    /// `send_signal_locked`'s view of a signal from `sender` (`None`: from outside the machine)
    /// to the process `target`: a sender that has no pid in the target's pid namespace is
    /// outside it, so it shows as pid 0 and forces SIGKILL and SIGSTOP on the namespace's
    /// init. A uid the sender names in its user namespace shows as the target's user namespace
    /// sees it. Queues `info` for the group, or for `task` alone.
    fn deliver(
        &self,
        sender: Option<&Sender>,
        target: u32,
        group: &Signals,
        task: Option<TaskId>,
        mut info: Siginfo,
    ) -> Result<(), Errno> {
        let target = self.procs.find(target);
        let outside = match (sender, &target) {
            (Some(sender), Some(target)) => self.pids.nr(&target.pid_ns, sender.pid.0) == 0,
            (Some(_), None) => false,
            (None, _) => true,
        };
        if has_si_pid_and_uid(&info) {
            if let (Some(sender), Some(_target)) = (sender, &target) {
                let (from, to) = (
                    sender.cred.user_ns.clone(),
                    group
                        .cred(task)
                        .map_or_else(|| self.init_user_ns.clone(), |cred| cred.user_ns.clone()),
                );
                if !Arc::ptr_eq(&from, &to) {
                    let uid = u32::from_le_bytes(info.sifields[4..8].try_into().unwrap());
                    let seen = from
                        .make_kuid(uid)
                        .map_or(OVERFLOW_ID, |kuid| to.munged_uid(kuid));
                    info.sifields[4..8].copy_from_slice(&seen.to_le_bytes());
                }
            }
            if outside {
                info.sifields[0..4].copy_from_slice(&0i32.to_le_bytes());
            }
        }
        group.send_from(task, info, outside)
    }

    /// `send_sig(SIGPIPE, current, 0)` when a write failed with `EPIPE`.
    pub(crate) fn sigpipe<T>(&self, caller: &Caller<'_>, ret: SysResult<T>) -> SysResult<T> {
        if ret.as_ref().err() == Some(&SysErr::Errno(Errno::EPIPE)) {
            self.send_sigpipe(caller);
        }
        ret
    }

    /// `send_sig(SIGPIPE, current, 0)`.
    pub(crate) fn send_sigpipe(&self, caller: &Caller<'_>) {
        let info = self.sender_info(caller, Signal::SIGPIPE.0, SiCode::SI_USER);
        let signals = self.task_signals(caller.task());
        // SIGPIPE needs no room in a queue.
        let _ = signals.group.send(Some(caller.task()), info);
    }

    /// fs/read_write.c:generic_write_check_limits and fs/attr.c:inode_newsize_ok
    /// use send_sig(SIGXFSZ, current, 0), with the sender's ordinary siginfo.
    pub(crate) fn send_sigxfsz(&self, caller: &Caller<'_>) {
        let info = self.sender_info(caller, Signal::SIGXFSZ.0, SiCode::SI_USER);
        let signals = self.task_signals(caller.task());
        let _ = signals.group.send(Some(caller.task()), info);
    }

    /// Sleeps for `sleep` unless a signal interrupts it first.
    pub(crate) async fn interruptible_sleep(
        &self,
        caller: &Caller<'_>,
        sleep: wasixv2_host::LocalFuture<'_, ()>,
    ) -> Result<(), wasixv2_wait::Interrupted> {
        let signals = self.interrupt(caller);
        let mut sleep = sleep;
        let waiter = Waiter::new();
        loop {
            match waiter.sleep(&*signals, Some(sleep.as_mut())).await {
                Wakeup::Interrupted => return Err(wasixv2_wait::Interrupted),
                Wakeup::TimedOut => return Ok(()),
                // The group's signal queue also wakes for signals other threads take; only a
                // signal for this task ends the sleep.
                Wakeup::Woken(_) => {}
            }
        }
    }

    /// The return-to-guest half of `get_signal`: takes the task's part in a group stop, or
    /// dequeues the next signal `task` takes and either applies its default action or
    /// prepares its handler's frame (`handle_signal`).
    pub(crate) fn take_signal(&self, task: TaskId, signals: &TaskSignals) -> Option<GuestAction> {
        let process = self.process(task);
        let css = self.task_ref(task, |t| t.css.clone());
        let mut state = signals.group.state.lock_or_recover();
        state.tasks.get_mut(&task)?.notify = false;
        // Every task continued goes here first; one tells the parent (`SIGNAL_CLD_MASK`).
        if let Some(why) = state.jobctl.report.take() {
            let status = match why {
                SiCode::CLD_CONTINUED => Signal::SIGCONT.0,
                _ => state.jobctl.stop_code & 0x7f,
            };
            drop(state);
            self.notify_parent_cldstop(&process, why, status);
            state = signals.group.state.lock_or_recover();
        }
        // ---- ptrace ----
        // Back from a ptrace stop this function entered: a signal-delivery stop delivers what
        // the tracer chose (`ptrace_signal`).
        let mut from_tracer = None;
        let (sync_stop, traced) = {
            let trace = &state.tasks.get(&task)?.ptrace;
            (trace.sync_stop, trace.traced)
        };
        match sync_stop {
            Some(_) if traced => return Some(GuestAction::Stopped),
            Some(kind) => {
                let tracer = state.task(task).ptrace.link.map(|link| link.tracer);
                let (code, info) = state.leave_trace_stop(task);
                if kind == crate::ptrace::SyncStop::Signal {
                    drop(state);
                    let info = info
                        .map(|info| self.ptrace_signal_info(task, &process, tracer, code, info));
                    state = signals.group.state.lock_or_recover();
                    if !state.tasks.contains_key(&task) {
                        return None;
                    }
                    from_tracer = info.flatten();
                    // Blocked, or SIGKILL comes first: it waits in the queue.
                    if let Some(info) = from_tracer
                        && (state.task(task).blocked & bit(info.si_signo) != 0
                            || state.sigkill_pending(task))
                    {
                        let _ = state.pending(Some(task)).add(info, Ok(None));
                        state.recalc_sigpending(task);
                        from_tracer = None;
                    }
                }
            }
            None => {}
        }
        loop {
            // kernel/fork.c `kernel_clone`: VFORK_DONE precedes the parent's return.
            let trace = &state.task(task).ptrace;
            if trace.link.is_some()
                && trace.vfork_done.is_some()
                && !state.sigkill_pending(task)
                && state.fatal(task).is_none()
            {
                let child = state.task(task).ptrace.vfork_done.take().unwrap();
                let tracer = state.task(task).ptrace.link.map(|link| link.tracer);
                drop(state);
                let message = tracer
                    .and_then(|tracer| self.task_process(tracer))
                    .map_or(0, |tracer| child.nr(&tracer.pid_ns));
                drop(child);
                state = signals.group.state.lock_or_recover();
                let code =
                    Signal::SIGTRAP.0 | wasixv2_abi::PtraceEvent::PTRACE_EVENT_VFORK_DONE.0 << 8;
                let stop = TraceStop {
                    exit_code: code,
                    why: SiCode::CLD_TRAPPED,
                    message: message as u32 as u64,
                    info: Some(
                        self.ptrace_trap_info(
                            state.tasks[&task]
                                .cred
                                .as_deref()
                                .expect("attached task credentials"),
                            task,
                            Signal::SIGTRAP.0,
                            code,
                        ),
                    ),
                    sync: Some(crate::ptrace::SyncStop::Trap),
                };
                if state.enter_trace_stop(task, stop).is_some() {
                    drop(state);
                    self.ptrace_notify_tracer(task, SiCode::CLD_TRAPPED, Signal::SIGTRAP.0);
                    return Some(GuestAction::Stopped);
                }
            }
            // `do_jobctl_trap`: a trap stop is due.
            let trace = &state.task(task).ptrace;
            if trace.link.is_some()
                && (trace.trap_stop || trace.trap_notify)
                && !state.sigkill_pending(task)
                && state.fatal(task).is_none()
            {
                let stop = self.jobctl_trap(task, &state);
                let sig = stop.exit_code & 0x7f;
                if state.enter_trace_stop(task, stop).is_some() {
                    drop(state);
                    self.ptrace_notify_tracer(task, SiCode::CLD_STOPPED, sig);
                    return Some(GuestAction::Stopped);
                }
            }
            // A pending fatal signal comes before a stop, as Linux's `complete_signal` makes it
            // a group exit as it is sent.
            if state.task(task).stop_pending && state.fatal(task).is_none() {
                // Lock order: the signal state, then the thread group's.
                let exiting = process.group.signal_group_exit();
                if let Some(completed) = state.do_signal_stop(task, 0, exiting) {
                    return Some(self.stopped_traced(&process, signals, state, task, completed));
                }
            }
            // Lock order: the signal state, then the thread group's.
            let fatal = state.fatal(task).is_some() || process.group.signal_group_exit();
            // `do_freezer_trap`: the freezer holds the task, its signals queued, unless it has
            // a stop to take or a fatal signal ends it.
            let me = state.task(task);
            if me.trap_freeze && !me.stop_pending && !fatal {
                drop(state);
                css.enter_frozen();
                return Some(GuestAction::Stopped);
            }
            // Throttled: the task waits for its runtime, unless it dies.
            let me = state.task(task);
            if me.throttled_until.is_some() {
                if !fatal {
                    drop(state);
                    return Some(GuestAction::Stopped);
                }
                me.throttled_until = None;
            }
            // Leaving the frozen state: for good, or back into the freezer while the cgroup
            // still freezes (`cgroup_leave_frozen`). The tree comes before the signal state.
            if css.frozen() {
                drop(state);
                css.leave_frozen(fatal, || signals.freeze(true));
                state = signals.group.state.lock_or_recover();
                if !state.tasks.contains_key(&task) {
                    return None;
                }
                continue;
            }
            let me = state.task(task);
            let (blocked, sigpending) = (me.blocked, me.sigpending);
            // Only a task a signal waits for dequeues (`get_signal` runs on `TIF_SIGPENDING`):
            // one woken for another reason leaves the group's signals to the task picked.
            let mut info = from_tracer.take();
            let tracer_chose = info.is_some();
            if sigpending && info.is_none() {
                (state, info) = signals.group.dequeue(state, task, !blocked);
            }
            // `ptrace_signal`: a traced task stops for each signal but SIGKILL, and goes on
            // with what its tracer chose.
            if let Some(info) = info
                && !tracer_chose
                && info.si_signo != Signal::SIGKILL.0
                && state.task(task).ptrace.link.is_some()
            {
                state.task(task).stop_dequeued = true;
                let stop = TraceStop {
                    exit_code: info.si_signo,
                    why: SiCode::CLD_TRAPPED,
                    message: 0,
                    info: Some(info),
                    sync: Some(crate::ptrace::SyncStop::Signal),
                };
                if state.enter_trace_stop(task, stop).is_some() {
                    drop(state);
                    self.ptrace_notify_tracer(task, SiCode::CLD_TRAPPED, info.si_signo);
                    return Some(GuestAction::Stopped);
                }
            }
            let Some(info) = info else {
                // `restore_saved_sigmask`: no handler took the temporary mask's signal. What the
                // old mask unblocks is delivered on the next return.
                if let Some(saved) = state.task(task).saved.take() {
                    let woke = state.set_blocked(task, saved);
                    drop(state);
                    if woke {
                        signals.group.queue.wake(wasixv2_wait::PollMask::empty());
                    }
                }
                return None;
            };
            let sig = info.si_signo;
            if STOP_SIGNALS & bit(sig) != 0 && !tracer_chose {
                state.task(task).stop_dequeued = true;
            }
            let action = state.sighand.get(sig);
            if action.is_ignore() {
                continue;
            }
            let trampoline = state.trampoline;
            let (false, Some(trampoline)) = (action.is_default(), trampoline) else {
                // An init takes no default action but SIGKILL's and SIGSTOP's, which were
                // forced on it.
                if state.unkillable && UNBLOCKABLE & bit(sig) == 0 {
                    continue;
                }
                match default_action(sig) {
                    DefaultAction::Ignore => continue,
                    DefaultAction::Stop => {
                        // The job control signals do nothing in an orphaned process group,
                        // SIGSTOP always stops. The tree comes before the signal state, which a
                        // SIGCONT may change meanwhile: the stop then does not happen.
                        if sig != Signal::SIGSTOP.0 {
                            drop(state);
                            let orphaned = self.is_pgrp_orphaned(&process);
                            state = signals.group.state.lock_or_recover();
                            // Taken over by an `execve` from another thread meanwhile.
                            if !state.tasks.contains_key(&task) {
                                return None;
                            }
                            if orphaned {
                                continue;
                            }
                        }
                        let exiting = process.group.signal_group_exit();
                        match state.do_signal_stop(task, sig, exiting) {
                            Some(completed) => {
                                return Some(
                                    self.stopped_traced(&process, signals, state, task, completed),
                                );
                            }
                            // SIGCONT came first, or the group is on its way out.
                            None => continue,
                        }
                    }
                    DefaultAction::Terminate => {
                        drop(state);
                        // `do_coredump`, which the task does on its way out.
                        if crate::coredump::dumps_core(sig) {
                            self.coredump_request(task, info);
                        }
                        self.force_fatal_signal(task, sig);
                        return wasixv2_syscalls::Control::take_pending(self, task);
                    }
                }
            };
            let me = state.task(task);
            let mask = me.saved.take().unwrap_or(me.blocked);
            // The frame saves the alternate stack (`__save_altstack`), and the execution layer
            // puts it there unless the guest runs on it already (`sigsp`). Delivery disarms an
            // `SS_AUTODISARM` stack until the handler returns (`signal_delivered`).
            let alt = me.altstack;
            me.frames.push(alt);
            if alt.flags & SigaltstackFlags::SS_AUTODISARM.0 != 0 {
                me.altstack = AltStack::default();
            }
            let mut block = action.mask;
            if action.flags & SaFlags::SA_NODEFER.0 == 0 {
                block |= bit(sig);
            }
            // `signal_delivered`'s `set_current_blocked`.
            let blocked = me.blocked | block;
            let woke = state.set_blocked(task, blocked);
            // `SA_ONESHOT` resets only the handler; the flags and mask stay.
            if action.flags & SaFlags::SA_RESETHAND.0 != 0 {
                let reset = Action {
                    handler: SignalHandler::SIG_DFL.0 as u32,
                    ..action
                };
                state.sighand.set(sig, reset);
            }
            drop(state);
            if woke {
                signals.group.queue.wake(wasixv2_wait::PollMask::empty());
            }
            return Some(GuestAction::Signal(SignalFrame {
                trampoline,
                info,
                handler: action.handler,
                flags: action.flags,
                mask,
                altstack: (alt.sp, alt.flags as i32, alt.size),
            }));
        }
    }

    // ---- ptrace ----

    /// `do_jobctl_trap`'s stop for `task`: a seized tracee reports a `PTRACE_EVENT_STOP` with
    /// the group stop's signal, or SIGTRAP outside one; another its group stop's signal.
    fn jobctl_trap(&self, task: TaskId, state: &GroupState) -> TraceStop {
        let seized = state.tasks[&task]
            .ptrace
            .link
            .is_some_and(|link| link.seized);
        let in_group_stop = state.jobctl.group_stop_count != 0 || state.jobctl.stopped;
        let mut sig = match state.jobctl.stop_code & 0x7f {
            0 => Signal::SIGSTOP.0,
            sig => sig,
        };
        if !seized {
            return TraceStop {
                exit_code: sig,
                why: SiCode::CLD_STOPPED,
                message: 0,
                info: None,
                sync: Some(crate::ptrace::SyncStop::Trap),
            };
        }
        if !in_group_stop {
            sig = Signal::SIGTRAP.0;
        }
        let exit_code = sig | crate::ptrace::EVENT_STOP << 8;
        TraceStop {
            exit_code,
            why: SiCode::CLD_STOPPED,
            message: 0,
            info: Some(
                self.ptrace_trap_info(
                    state.tasks[&task]
                        .cred
                        .as_deref()
                        .expect("attached task credentials"),
                    task,
                    sig,
                    exit_code,
                ),
            ),
            sync: Some(crate::ptrace::SyncStop::Trap),
        }
    }

    /// `ptrace_do_notify`'s `siginfo`: `sig` with `code`, from the task itself.
    pub(crate) fn ptrace_trap_info(
        &self,
        cred: &crate::cred::Cred,
        task: TaskId,
        sig: i32,
        code: i32,
    ) -> Siginfo {
        let uid = cred.user_ns.munged_uid(cred.user.real);
        let mut info = user_info(sig, SiCode(code), self.pids.own_nr(task.0), uid);
        info.si_code = code;
        info
    }

    /// `ptrace_signal`'s tail: what a traced task delivers once its tracer resumed it from a
    /// signal-delivery stop with `code`: nothing for 0, the signal as it was (or as
    /// `PTRACE_SETSIGINFO` left it) for its own number, else `code` as sent by the tracer.
    fn ptrace_signal_info(
        &self,
        task: TaskId,
        process: &Process,
        tracer: Option<TaskId>,
        code: i32,
        info: Siginfo,
    ) -> Option<Siginfo> {
        if code == 0 || code > NSIG {
            return None;
        }
        if code == info.si_signo {
            return Some(info);
        }
        // The tracer's pid in the tracee's namespace and its uid as the tracee sees it.
        let (pid, uid) = match tracer.and_then(|t| self.find_task_cred(t).map(|cred| (t, cred))) {
            Some((tracer, cred)) => {
                let uid = self.task_cred(task).user_ns.munged_uid(cred.user.real);
                (self.pids.nr(&process.pid_ns, tracer.0), uid)
            }
            None => (0, 0),
        };
        Some(user_info(code, SiCode::SI_USER, pid, uid))
    }

    /// The task's part in a group stop is taken: a traced task goes into a trap stop for its
    /// tracer instead of `TASK_STOPPED` (`do_signal_stop` with `current->ptrace`); either way
    /// the parent hears of a group stop it completed.
    fn stopped_traced(
        &self,
        process: &Process,
        signals: &TaskSignals,
        mut state: std::sync::MutexGuard<'_, GroupState>,
        task: TaskId,
        completed: bool,
    ) -> GuestAction {
        let link = state.task(task).ptrace.link;
        let Some(link) = link else {
            drop(state);
            return self.stopped(process, signals, completed);
        };
        state.task(task).stopped = false;
        let stop = self.jobctl_trap(task, &state);
        let sig = stop.exit_code & 0x7f;
        let entered = state.enter_trace_stop(task, stop).is_some();
        drop(state);
        signals.group.queue.wake(wasixv2_wait::PollMask::empty());
        if entered {
            self.ptrace_notify_tracer(task, SiCode::CLD_STOPPED, sig);
        }
        // `ptrace_reparented`: a parent that is not the tracer hears of the group stop too.
        let tracer_is_parent = self.task_process(link.tracer).is_some_and(|tracer| {
            let tree = self.procs.tree.lock_or_recover();
            tree.nodes
                .get(&process.pid.0)
                .is_some_and(|node| node.parent == tracer.pid.0)
        });
        if completed && !tracer_is_parent {
            self.notify_parent_cldstop(process, SiCode::CLD_STOPPED, signals.group.stop_status());
        }
        GuestAction::Stopped
    }

    /// The task just stopped: the group's other tasks, whose part in the stop is pending, leave
    /// their blocking waits (`signal_wake_up`), and the parent hears of the group stop the task
    /// `completed`.
    fn stopped(&self, process: &Process, signals: &TaskSignals, completed: bool) -> GuestAction {
        signals.group.queue.wake(wasixv2_wait::PollMask::empty());
        if completed {
            self.notify_parent_cldstop(process, SiCode::CLD_STOPPED, signals.group.stop_status());
        }
        // A stopped task counts as frozen in its cgroup (`do_signal_stop`).
        self.task_ref(signals.task, |task| task.css.clone())
            .enter_frozen();
        GuestAction::Stopped
    }

    /// `do_signal_stop`'s `schedule` in `TASK_STOPPED`: `task` waits until SIGCONT continues
    /// it, or SIGKILL, its group's exit or an `execve` ends the stop. Nothing else wakes it:
    /// signals that are not SIGKILL wait in its queue until it goes on, fatal ones included
    /// (`wants_signal` passes stopped tasks by).
    pub(crate) async fn stop_wait(&self, task: TaskId) {
        let signals = self.task_signals(task);
        // CFS bandwidth: until the period refills the runtime (`unthrottle_cfs_rq`), or a
        // fatal signal.
        if let Some(until) = signals.throttled_until() {
            let waiter = Waiter::new();
            let mut sleep = self.host.sleep_until(until);
            while self.host.monotonic() < until && signals.fatal_pending().is_none() {
                // kernel/sched/fair.c throttle_cfs_rq removes a runnable entity from the
                // runqueue; it does not put the task into a sleeping state.
                let killable = Killable(&signals, wasixv2_wait::TaskState::Running);
                if let Wakeup::TimedOut = waiter.sleep(&killable, Some(sleep.as_mut())).await {
                    break;
                }
            }
            signals.take_throttle();
        }
        let continued = || (!signals.held()).then_some(());
        // kernel/signal.c:do_freezer_trap sleeps TASK_INTERRUPTIBLE|TASK_FREEZABLE.
        // Job-control and ptrace stops keep their procfs state precedence.
        signals
            .status
            .during(
                wasixv2_wait::TaskState::Interruptible,
                wait_event(signals.sent(), continued, &NoInterrupt),
            )
            .await
            .expect("NoInterrupt never interrupts");
    }

    /// `exit_signals` for `task`, which leaves its thread group: it takes no more signals
    /// (`PF_EXITING`), and if the group goes on, the group's signals it was to take go to other
    /// tasks and its part in a group stop under way is taken.
    pub(crate) fn exit_signals(&self, task: TaskId, signals: &TaskSignals) {
        // kernel/exit.c:do_exit runs this before exit_notify/release_task. A parent
        // may have reaped our task after take_pending selected its exit action.
        let Some(process) = self.task_process(task) else {
            return;
        };
        let mut state = signals.group.state.lock_or_recover();
        let Some(me) = state.tasks.get_mut(&task) else {
            return;
        };
        me.exiting = true;
        // Lock order: the signal state, then the thread group's.
        if process.group.signal_group_exit() {
            return;
        }
        // The group's signals the task was to take go to the others (`retarget_shared_pending`).
        let (sigpending, unblocked, stop_pending) = (me.sigpending, !me.blocked, me.stop_pending);
        let woke = sigpending && state.retarget(task, unblocked);
        let completed = stop_pending && state.participate(task);
        drop(state);
        if woke {
            signals.group.queue.wake(wasixv2_wait::PollMask::empty());
        }
        if completed {
            self.notify_parent_cldstop(&process, SiCode::CLD_STOPPED, signals.group.stop_status());
        }
    }

    /// The default action of a signal that kills: the whole thread group dies of `sig`
    /// (`do_group_exit`), so its parent sees `WIFSIGNALED`. No core is written, so the core
    /// signals (SIGSEGV, SIGABRT, ...) end the same way, without `WCOREDUMP`. Also what a trap
    /// comes to: a fault whose handler could not run (`force_sig_fault`, `force_sigsegv`).
    /// `wasmer run` reports the death as the shell does: 128 + signal.
    /// False when the group was on its way out already, which the signal does not change.
    pub(crate) fn force_fatal_signal(&self, task: TaskId, sig: i32) -> bool {
        // A reaped process's task traps only while it unwinds: part of its teardown.
        let process = {
            let state = self.state.read_or_recover();
            state.tasks.get(&task).map(|t| t.process.clone())
        };
        let Some(process) = process else {
            return false;
        };
        if process.group.exiting() {
            // The exit is pending on every task; `task` unwinds through it.
            return false;
        }
        // `do_coredump`, which the task does in its `exit_mm`, after this: a signal taken on
        // the way back to the guest asked for it with its own `siginfo` already, a fault
        // (`force_sig_fault`) does here.
        if crate::coredump::dumps_core(sig) {
            self.coredump_request(task, kernel_info(sig));
        }
        self.note_killed(task, sig);
        self.do_group_exit(task, 128 + sig);
        true
    }

    /// `rt_sigreturn`: the innermost handler of `task` finished. A call it interrupted is not
    /// restarted through `restart_syscall` any more (`do_no_restart_syscall`), and the alternate
    /// stack is again what the handler's frame saved (`restore_altstack`; a change the handler
    /// made to `uc_stack` is not seen). A handler that did not return (`siglongjmp`, `mask` is
    /// `None`) restores nothing, so a stack it disarmed stays disarmed. A handler that ended its
    /// thread (`pthread_exit`, a cancellation) unwinds through here after the task is gone.
    pub(crate) fn signal_return(&self, task: TaskId, mask: Option<u64>) {
        let (signals, restart) = {
            let state = self.state.read_or_recover();
            match state.tasks.get(&task) {
                Some(task) => (task.signals.clone(), task.restart.is_some()),
                None => return,
            }
        };
        if restart && let Some(task) = self.state.lock_or_recover().tasks.get_mut(&task) {
            task.restart = None;
        }
        signals.sigreturn(mask);
    }

    pub(crate) async fn sys_signal_callback_register(
        &self,
        caller: &Caller<'_>,
        callback: u32,
    ) -> SysResult<i32> {
        let signals = self.task_signals(caller.task());
        signals.group.state.lock_or_recover().trampoline = Some(callback);
        Ok(0)
    }

    pub(crate) async fn sys_rt_sigaction(
        &self,
        caller: &Caller<'_>,
        sig: Signal,
        act: ConstSigactionPtr,
        oact: SigactionPtr,
        sigsetsize: u32,
    ) -> SysResult<i32> {
        check_sigsetsize(sigsetsize)?;
        let sig = check_signal(sig)?;
        let new = match act.is_null() {
            true => None,
            false => Some(read_value(caller.memory(), act).await?),
        };
        if new.is_some() && UNBLOCKABLE & bit(sig) != 0 {
            return Err(Errno::EINVAL.into());
        }
        let signals = self.task_signals(caller.task());
        let old = {
            let mut state = signals.group.state.lock_or_recover();
            let old = state.sighand.get(sig);
            if let Some(new) = new {
                let action = Action {
                    handler: new.sa_handler,
                    flags: new.sa_flags,
                    mask: sigset_bits(new.sa_mask) & !UNBLOCKABLE,
                };
                state.sighand.set(sig, action);
                // POSIX: setting a disposition to ignore discards the pending signal.
                if state.ignores(sig, false) {
                    if state.take_first(caller.task(), bit(sig)) {
                        state.sighand.set(sig, old);
                        return Err(SysErr::RestartNoIntr);
                    }
                    state.flush(bit(sig));
                } else if old.is_ignore() {
                    state.unignore_timers(sig);
                    signals.group.queue.wake(wasixv2_wait::PollMask::empty());
                }
            }
            old
        };
        if !oact.is_null() {
            let old = Sigaction {
                sa_handler: old.handler,
                sa_flags: old.flags,
                sa_mask: to_sigset(old.mask),
            };
            write_value(caller.memory(), oact, &old).await?;
        }
        Ok(0)
    }

    pub(crate) async fn sys_rt_sigprocmask(
        &self,
        caller: &Caller<'_>,
        how: SigprocmaskHow,
        nset: ConstSigsetPtr,
        oset: SigsetPtr,
        sigsetsize: u32,
    ) -> SysResult<i32> {
        check_sigsetsize(sigsetsize)?;
        let signals = self.task_signals(caller.task());
        let old = signals.blocked();
        if !nset.is_null() {
            let set = sigset_bits(read_value(caller.memory(), nset).await?);
            let mask = match how {
                SigprocmaskHow::SIG_BLOCK => old | set,
                SigprocmaskHow::SIG_UNBLOCK => old & !set,
                SigprocmaskHow::SIG_SETMASK => set,
                _ => return Err(Errno::EINVAL.into()),
            };
            if signals.block_after_pending(mask).is_none() {
                return Err(SysErr::RestartNoIntr);
            }
        }
        if !oset.is_null() {
            write_value(caller.memory(), oset, &to_sigset(old)).await?;
        }
        Ok(0)
    }

    pub(crate) async fn sys_rt_sigpending(
        &self,
        caller: &Caller<'_>,
        uset: SigsetPtr,
        sigsetsize: u32,
    ) -> SysResult<i32> {
        check_sigsetsize(sigsetsize)?;
        let signals = self.task_signals(caller.task());
        let pending = {
            let state = signals.group.state.lock_or_recover();
            let blocked = state.tasks[&caller.task()].blocked;
            state.deliverable(caller.task(), blocked)
        };
        write_value(caller.memory(), uset, &to_sigset(pending)).await?;
        Ok(0)
    }

    pub(crate) async fn sys_rt_sigsuspend(
        &self,
        caller: &Caller<'_>,
        unewset: ConstSigsetPtr,
        sigsetsize: u32,
    ) -> SysResult<i32> {
        check_sigsetsize(sigsetsize)?;
        let set = sigset_bits(read_value(caller.memory(), unewset).await?);
        let signals = self.task_signals(caller.task());
        let saved = signals.replace_blocked(set);
        signals.wait_unblocked().await;
        signals.restore_blocked(saved, true);
        Err(SysErr::RestartNoHand)
    }

    pub(crate) async fn sys_rt_sigtimedwait(
        &self,
        caller: &Caller<'_>,
        uthese: ConstSigsetPtr,
        uinfo: SiginfoPtr,
        uts: ConstKernelTimespecPtr,
        sigsetsize: u32,
    ) -> SysResult<i32> {
        check_sigsetsize(sigsetsize)?;
        let these = sigset_bits(read_value(caller.memory(), uthese).await?) & !UNBLOCKABLE;
        let timeout = match uts.is_null() {
            true => None,
            false => Some(time::from_timespec(
                read_value(caller.memory(), uts).await?,
            )?),
        };
        let signals = self.task_signals(caller.task());
        let take = || {
            let group = &signals.group;
            group
                .dequeue(group.state.lock_or_recover(), caller.task(), these)
                .1
        };
        let info = match take() {
            Some(info) => Some(info),
            None if timeout.is_some_and(|t| t.is_zero()) => None,
            None => {
                let mut timer = timeout.map(|t| self.host.sleep(t));
                let wait = WaitFor {
                    signals: &signals,
                    these,
                };
                let waiter = Waiter::new();
                loop {
                    let timer = timer.as_mut().map(|timer| timer.as_mut());
                    let wakeup = waiter.sleep(&wait, timer).await;
                    if let Some(info) = take() {
                        break Some(info);
                    }
                    match wakeup {
                        Wakeup::TimedOut => break None,
                        Wakeup::Interrupted => return Err(Errno::EINTR.into()),
                        Wakeup::Woken(_) => {}
                    }
                }
            }
        };
        let Some(info) = info else {
            return Err(Errno::EAGAIN.into());
        };
        if !uinfo.is_null() {
            write_value(caller.memory(), uinfo, &info).await?;
        }
        Ok(info.si_signo)
    }

    pub(crate) async fn sys_rt_sigqueueinfo(
        &self,
        caller: &Caller<'_>,
        tgid: Pid,
        tid: Option<Pid>,
        sig: Signal,
        uinfo: ConstSiginfoPtr,
    ) -> SysResult<i32> {
        let mut info = read_value(caller.memory(), uinfo).await?;
        info.si_signo = sig.0;
        // `rt_tgsigqueueinfo` targets a single task.
        if tid.is_some_and(|tid| tid <= 0 || tgid <= 0) {
            return Err(Errno::EINVAL.into());
        }
        // Only the kernel may claim to be the kernel or `tkill`, except towards the calling
        // thread itself.
        if reserved_to_kernel(info.si_code) && tid.unwrap_or(tgid) != self.own_tid(caller.task()) {
            return Err(Errno::EPERM.into());
        }
        let sender = self.signal_sender(caller.task());
        let sender = Some(&sender);
        match tid {
            Some(tid) => self.send_to_task(caller, sender, Some(tgid), tid, info)?,
            None => {
                // `kill_proc_info`: a pid outside the caller's namespace names nothing.
                let pid = self.find_vpid(caller, tgid).ok_or(Errno::ESRCH)?;
                self.send_to_group(sender, pid, info)?
            }
        }
        Ok(0)
    }

    /// A `siginfo` naming the caller's process as the sender, by its pid as it sees it
    /// (`task_tgid_vnr(current)`).
    pub(crate) fn sender_info(&self, caller: &Caller<'_>, sig: i32, code: SiCode) -> Siginfo {
        let process = self.process(caller.task());
        let cred = self.cred(caller);
        let uid = cred.user_ns.munged_uid(cred.user.real);
        user_info(sig, code, self.pids.own_nr(process.pid.0), uid)
    }

    /// `check_kill_permission` for a signal from `sender` (`None`: from outside the machine,
    /// which may signal anything) to the process `target`: `kill_ok_by_cred`, or `SIGCONT`
    /// within the sender's session.
    fn may_signal(&self, sender: Option<&Sender>, target: u32, sig: i32) -> bool {
        let target_cred = {
            let state = self.state.read_or_recover();
            state
                .tasks
                .get(&TaskId(target))
                .map(|t| t.cred.clone())
                .or_else(|| state.retired_creds.get(&TaskId(target)).cloned())
        };
        let (Some(sender), Some(target)) = (
            sender,
            self.task_process(TaskId(target))
                .or_else(|| self.procs.find(target)),
        ) else {
            return true;
        };
        // `security_task_kill`: Landlock's signal scope, which never holds within a thread
        // group.
        if let Some(domain) = &sender.cred.landlock
            && sender.pid != target.pid
            && domain.scoped(
                target_cred.as_ref().and_then(|c| c.landlock.as_ref()),
                crate::landlock::SCOPE_SIGNAL,
            )
        {
            return false;
        }
        if target_cred
            .as_ref()
            .is_none_or(|cred| sender.cred.may_signal(cred))
        {
            return true;
        }
        // One process's lock at a time: two processes may check each other at once.
        let sid = |process: &Process| process.ident.lock_or_recover().sid;
        sig == Signal::SIGCONT.0 && sid(sender) == sid(&target)
    }

    /// Queues `info`, which the kernel makes on behalf of `sender`, for the thread group of the
    /// id `pid`, which `sender` needs no permission to signal. `info`'s pid is the sender's as
    /// the target sees it.
    pub(crate) fn send_on_behalf(
        &self,
        sender: Option<&Process>,
        pid: u32,
        info: Siginfo,
    ) -> Result<(), Errno> {
        let groups = self.groups(Some(pid));
        let (leader, group) = groups.first().ok_or(Errno::ESRCH)?;
        let sender = sender
            .and_then(|p| self.procs.find(p.pid.0))
            .map(|p| Sender {
                cred: self.task_cred(p.pid),
                process: p,
            });
        self.deliver(sender.as_ref(), *leader, group, None, info)
    }

    /// Queues `info` for the thread group of the id `pid` (`kill_pid_info`), if `sender` may
    /// signal it.
    pub(crate) fn send_to_group(
        &self,
        sender: Option<&Sender>,
        pid: u32,
        info: Siginfo,
    ) -> Result<(), Errno> {
        let groups = self.groups(Some(pid));
        let Some((leader, group)) = groups.first() else {
            return Err(Errno::ESRCH);
        };
        check_sendable(info.si_signo)?;
        if !self.may_signal(sender, pid, info.si_signo) {
            return Err(Errno::EPERM);
        }
        match info.si_signo {
            0 => Ok(()),
            _ => self.deliver(sender, *leader, group, None, info),
        }
    }

    /// Sends `sig` to the process `pid` from outside the machine, like `kill(2)` from a process
    /// in an ancestor PID namespace: `si_code` is `SI_USER` and `si_pid` 0. The guest's
    /// dispositions apply as for any other signal, so with no handler the default action
    /// runs. A signal reaches a task at its next interruption point: a blocking call, the
    /// return from a syscall, or a preemption point of the execution layer.
    pub fn kill(&self, pid: TaskId, sig: Signal) -> Result<(), Errno> {
        let sig = check_signal(sig)?;
        self.send_to_group(None, pid.0, user_info(sig, SiCode::SI_USER, 0, 0))
    }

    /// Sends `sig` to every process in the process group of `pid`, as a terminal's line
    /// discipline sends `^C` and `^\` to its foreground group (`isig`, `kill_pgrp` with
    /// `SI_KERNEL`). Returns whether some process of the group has it pending now; `false`
    /// when each one ignores it, so it was discarded where it was generated.
    // TODO: the terminal's foreground group, once the kernel has terminals.
    pub fn kill_process_group(&self, pid: TaskId, sig: Signal) -> Result<bool, Errno> {
        let sig = check_signal(sig)?;
        let groups = self.pgrp_groups(self.pgid_of(pid.0));
        if groups.is_empty() {
            return Err(Errno::ESRCH);
        }
        let mut pending = false;
        for (_, group) in groups {
            // A kernel signal goes past the limit, or without its `siginfo`.
            pending |= matches!(group.generate(None, kernel_info(sig), false), Ok(true));
        }
        Ok(pending)
    }

    /// Whether `sig` is still pending, not yet taken by any task, for a process in the
    /// process group of `pid`.
    pub fn process_group_pending(&self, pid: TaskId, sig: Signal) -> bool {
        let Ok(sig) = check_signal(sig) else {
            return false;
        };
        let groups = self.pgrp_groups(self.pgid_of(pid.0));
        groups.iter().any(|(_, group)| group.is_pending(sig))
    }

    /// `do_tkill`: the caller's `tgid` must own its `tid` when given (`find_task_by_vpid`).
    fn send_to_task(
        &self,
        caller: &Caller<'_>,
        sender: Option<&Sender>,
        tgid: Option<Pid>,
        tid: Pid,
        info: Siginfo,
    ) -> Result<(), Errno> {
        if tid <= 0 || tgid.is_some_and(|tgid| tgid <= 0) {
            return Err(Errno::EINVAL);
        }
        let tid = self.find_vpid(caller, tid).ok_or(Errno::ESRCH)?;
        let tgid = match tgid {
            Some(tgid) => Some(self.find_vpid(caller, tgid).ok_or(Errno::ESRCH)?),
            None => None,
        };
        let target = {
            let state = self.state.read_or_recover();
            state
                .tasks
                .get(&TaskId(tid))
                .filter(|task| tgid.is_none_or(|tgid| task.process.pid.0 == tgid))
                // A thread that exited is gone, though its instance may still be unwinding; a
                // zombie leader stays until its group ends.
                .filter(|task| !task.thread.exited() || task.process.pid.0 == tid)
                .map(|task| (task.process.pid.0, task.signals.group().clone()))
        };
        let (pid, group) = target.ok_or(Errno::ESRCH)?;
        check_sendable(info.si_signo)?;
        if !self.may_signal(sender, tid, info.si_signo) {
            return Err(Errno::EPERM);
        }
        match info.si_signo {
            0 => Ok(()),
            _ => self.deliver(sender, pid, &group, Some(TaskId(tid)), info),
        }
    }

    /// `kill_pid_info_type(sig, info, task_pid(current), PIDTYPE_PID)`: `info` to the calling
    /// task `task` alone, as `pidfd_send_signal(PIDFD_SELF_THREAD)` sends it.
    pub(crate) fn send_to_self(
        &self,
        sender: &Sender,
        task: TaskId,
        info: Siginfo,
    ) -> Result<(), Errno> {
        check_sendable(info.si_signo)?;
        let group = self.task_signals(task).group().clone();
        match info.si_signo {
            0 => Ok(()),
            _ => self.deliver(Some(sender), sender.pid.0, &group, Some(task), info),
        }
    }

    pub(crate) async fn sys_kill(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        sig: Signal,
    ) -> SysResult<i32> {
        let me = self.process(caller.task()).pid.0;
        let info = self.sender_info(caller, sig.0, SiCode::SI_USER);
        let ns = self.active_pid_ns(caller.task());
        let targets: Vec<(u32, Arc<Signals>)> = match pid {
            // ---- proc ----
            0 => self.pgrp_groups(self.pgid_of(me)),
            // Every process of the caller's pid namespace (and below) but its init and the
            // caller (`task_pid_vnr(p) > 1`).
            -1 => self
                .groups(None)
                .into_iter()
                .filter(|(pid, _)| *pid != me && self.pids.nr(&ns, *pid) > 1)
                .collect(),
            pid if pid < 0 => match ns.find(pid.checked_abs().unwrap_or(0)) {
                Some(pgid) => self.pgrp_groups(pgid),
                None => Vec::new(),
            },
            pid => match ns.find(pid) {
                Some(pid) => self.groups(Some(pid)),
                None => Vec::new(),
            },
        };
        if targets.is_empty() {
            return Err(Errno::ESRCH.into());
        }
        check_sendable(sig.0)?;
        let sender = self.signal_sender(caller.task());
        let mut sent = false;
        for (target, group) in targets {
            if !self.may_signal(
                Some(&sender),
                if pid > 0 {
                    ns.find(pid).unwrap_or(target)
                } else {
                    target
                },
                sig.0,
            ) {
                continue;
            }
            sent = true;
            if sig.0 != 0 {
                // `kill`'s signals are never refused for want of room (`SI_USER`).
                let _ = self.deliver(Some(&sender), target, &group, None, info);
            }
        }
        // A process group or a single process none of which may be signalled is `EPERM`;
        // `kill(-1)` only reports targets that do not exist.
        match sent || pid == -1 {
            true => Ok(0),
            false => Err(Errno::EPERM.into()),
        }
    }

    pub(crate) async fn sys_tgkill(
        &self,
        caller: &Caller<'_>,
        tgid: Option<Pid>,
        tid: Pid,
        sig: Signal,
    ) -> SysResult<i32> {
        let info = self.sender_info(caller, sig.0, SiCode::SI_TKILL);
        let sender = self.signal_sender(caller.task());
        self.send_to_task(caller, Some(&sender), tgid, tid, info)?;
        Ok(0)
    }

    pub(crate) async fn sys_sigaltstack(
        &self,
        caller: &Caller<'_>,
        uss: ConstSigaltstackPtr,
        uoss: SigaltstackPtr,
    ) -> SysResult<i32> {
        let new = match uss.is_null() {
            true => None,
            false => Some(read_value(caller.memory(), uss).await?),
        };
        // `do_sigaltstack` at the caller's stack pointer.
        let sp = caller.memory().stack_pointer().await;
        let signals = self.task_signals(caller.task());
        let old = signals.with(|task| -> Result<Sigaltstack, Errno> {
            let alt = task.altstack;
            let old = Sigaltstack {
                ss_sp: alt.sp,
                ss_flags: alt.reported_flags(sp) as i32,
                ss_size: alt.size,
            };
            let Some(new) = new else {
                return Ok(old);
            };
            if alt.holds(sp) {
                return Err(Errno::EPERM);
            }
            let flags = new.ss_flags as u32;
            let mode = SigaltstackFlags(flags & !SigaltstackFlags::SS_AUTODISARM.0);
            if !matches!(
                mode,
                SigaltstackFlags::SS_DISABLE | SigaltstackFlags::SS_ONSTACK
            ) && mode.0 != 0
            {
                return Err(Errno::EINVAL);
            }
            let requested = AltStack {
                sp: new.ss_sp,
                size: new.ss_size,
                flags,
            };
            if requested == alt {
                return Ok(old);
            }
            task.altstack = match mode {
                SigaltstackFlags::SS_DISABLE => AltStack {
                    sp: 0,
                    size: 0,
                    flags,
                },
                _ if new.ss_size < SignalLimits::MINSIGSTKSZ.0 => return Err(Errno::ENOMEM),
                _ => requested,
            };
            Ok(old)
        })?;
        if !uoss.is_null() {
            write_value(caller.memory(), uoss, &old).await?;
        }
        Ok(0)
    }

    pub(crate) async fn sys_setitimer(
        &self,
        caller: &Caller<'_>,
        which: ItimerWhich,
        value: ConstKernelOldItimervalPtr,
        ovalue: KernelOldItimervalPtr,
    ) -> SysResult<i32> {
        if !matches!(
            which,
            ItimerWhich::ITIMER_REAL | ItimerWhich::ITIMER_VIRTUAL | ItimerWhich::ITIMER_PROF
        ) {
            return Err(Errno::EINVAL.into());
        }
        let new = match value.is_null() {
            true => None,
            false => {
                let v = read_value(caller.memory(), value).await?;
                Some((from_timeval(v.it_value)?, from_timeval(v.it_interval)?))
            }
        };
        let old = match cpu_itimer_base(which) {
            None => {
                let signals = self.task_signals(caller.task());
                self.arm_real_timer(signals.group(), new)
            }
            // `set_cpu_itimer`, or without a new value `get_cpu_itimer`.
            Some(base) => {
                let process = self.process(caller.task());
                match new {
                    Some((value, interval)) => process.set_itimer(base, value, interval),
                    None => process.get_itimer(base),
                }
            }
        };
        if !ovalue.is_null() {
            write_value(caller.memory(), ovalue, &to_itimerval(old)).await?;
        }
        Ok(0)
    }

    pub(crate) async fn sys_getitimer(
        &self,
        caller: &Caller<'_>,
        which: ItimerWhich,
        value: KernelOldItimervalPtr,
    ) -> SysResult<i32> {
        let current = match which {
            ItimerWhich::ITIMER_REAL => {
                let signals = self.task_signals(caller.task());
                let state = signals.group.state.lock_or_recover();
                real_timer_value(&state.real_timer, self.host.monotonic())
            }
            _ => match cpu_itimer_base(which) {
                Some(base) => self.process(caller.task()).get_itimer(base),
                None => return Err(Errno::EINVAL.into()),
            },
        };
        write_value(caller.memory(), value, &to_itimerval(current)).await?;
        Ok(0)
    }

    /// Arms (or with a zero value disarms) `ITIMER_REAL` and returns its previous value and
    /// interval. A kernel timer sends SIGALRM at each expiry.
    fn arm_real_timer(
        &self,
        group: &Arc<Signals>,
        new: Option<(Duration, Duration)>,
    ) -> (Duration, Duration) {
        let now = self.host.monotonic();
        let mut state = group.state.lock_or_recover();
        let old = real_timer_value(&state.real_timer, now);
        let Some((value, interval)) = new else {
            return old;
        };
        let real = &mut state.real_timer;
        // A zero value disarms, and forgets the interval too (`do_setitimer`).
        real.interval = match value.is_zero() {
            true => Duration::ZERO,
            false => interval,
        };
        real.deadline = (!value.is_zero()).then(|| now.saturating_add(value));
        real.expired = None;
        let (timer, _) = real.timer.get_or_insert_with(|| {
            let group = Arc::downgrade(group);
            let host = self.host.clone();
            let fire = move || {
                let group = group.upgrade()?;
                real_timer_expired(&group, host.monotonic())
            };
            (self.timers.timer(Box::new(fire)), self.host.clone())
        });
        match real.deadline {
            Some(deadline) => timer.start(deadline),
            None => timer.cancel(),
        }
        old
    }
}

/// `it_real_fn`: sends SIGALRM. A periodic timer goes on once the signal is dequeued
/// ([`RealTimer::rearm`]), so missed periods collapse into the one signal. Returns the deadline
/// to re-arm at, for a timer that fired early.
fn real_timer_expired(group: &Signals, now: Duration) -> Option<Duration> {
    {
        let mut state = group.state.lock_or_recover();
        let real = &mut state.real_timer;
        let deadline = real.deadline?;
        if deadline > now {
            return Some(deadline);
        }
        (real.deadline, real.expired) = (None, Some(deadline));
    }
    // A kernel signal overrides RLIMIT_SIGPENDING (`SEND_SIG_PRIV`), so it cannot fail.
    let _ = group.send(None, kernel_info(Signal::SIGALRM.0));
    None
}

fn real_timer_value(timer: &RealTimer, now: Duration) -> (Duration, Duration) {
    let left = timer.deadline.map_or(Duration::ZERO, |deadline| {
        // An expired timer that has not fired yet still reports a tick, as Linux does.
        deadline.saturating_sub(now).max(Duration::from_micros(1))
    });
    (left, timer.interval)
}

/// The process CPU clock an interval timer runs on: `CPUCLOCK_VIRT` for `ITIMER_VIRTUAL`,
/// `CPUCLOCK_PROF` for `ITIMER_PROF`.
fn cpu_itimer_base(which: ItimerWhich) -> Option<CpuBase> {
    match which {
        ItimerWhich::ITIMER_VIRTUAL => Some(CpuBase::Virt),
        ItimerWhich::ITIMER_PROF => Some(CpuBase::Prof),
        _ => None,
    }
}

fn from_timeval(tv: KernelOldTimeval) -> Result<Duration, Errno> {
    if tv.tv_sec < 0 || !(0..1_000_000).contains(&tv.tv_usec) {
        return Err(Errno::EINVAL);
    }
    Ok(Duration::new(tv.tv_sec as u64, tv.tv_usec as u32 * 1000))
}

fn to_itimerval((value, interval): (Duration, Duration)) -> KernelOldItimerval {
    let tv = |d: Duration| KernelOldTimeval {
        tv_sec: d.as_secs() as i32,
        tv_usec: d.subsec_micros() as i32,
    };
    KernelOldItimerval {
        it_interval: tv(interval),
        it_value: tv(value),
    }
}
