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
    Caller, Errno, GuestAction, SignalFrame, SysErr, SysResult, TaskId, WorkFlag, read_value,
    write_value,
};
use wasixv2_wait::{Interrupt, WaitQueue, Waiter, Wakeup};

use crate::{
    Kernel, Process,
    posix_timer::{self, PosixTimer},
    time,
    timer::{self, Timer},
};

pub(crate) const NSIG: i32 = 64;
const SIGRTMIN: i32 = 32;
const UNBLOCKABLE: u64 = bit(Signal::SIGKILL.0) | bit(Signal::SIGSTOP.0);

const fn bit(sig: i32) -> u64 {
    1 << (sig - 1)
}

/// The signal state of one thread group: dispositions (`sighand_struct`), the shared pending
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
pub(crate) struct SigqueueUsers(Mutex<HashMap<u32, Arc<AtomicU64>>>);

/// The charge of one queued signal to its user (`sigqueue->ucounts`), given back when the
/// signal leaves its queue or its timer goes.
pub(crate) struct Charge(Arc<AtomicU64>);

/// A user is at `RLIMIT_SIGPENDING`.
pub(crate) struct Overflow;

impl SigqueueUsers {
    /// `inc_rlimit_get_ucounts`: charges one signal to `uid`, unless that takes it past `limit`.
    /// `None` overrides the limit; the signal still counts.
    pub(crate) fn charge(&self, uid: u32, limit: Option<u64>) -> Result<Charge, Overflow> {
        let count = self.0.lock().unwrap().entry(uid).or_default().clone();
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
    fn charge(&self, info: &Siginfo) -> Result<Option<Charge>, Overflow> {
        let sig = info.si_signo;
        let Some(process) = self.process.upgrade().filter(|_| sig != Signal::SIGKILL.0) else {
            return Ok(None);
        };
        let uid = process.cred.lock().unwrap().user.real;
        let limit = match sig < SIGRTMIN && info.si_code >= 0 {
            true => None,
            false => Some(process.resources.lock().unwrap().sigpending()),
        };
        self.users.charge(uid, limit).map(Some)
    }
}

struct GroupState {
    actions: [Action; NSIG as usize],
    /// The guest's trampoline (`signal-callback-register`).
    trampoline: Option<u32>,
    shared: Pending,
    tasks: HashMap<TaskId, TaskState>,
    real_timer: RealTimer,
    /// The group's POSIX timers (`signal_struct::posix_timers`).
    posix_timers: posix_timer::Table,
    /// Periodic POSIX timers whose signal was ignored when it fired; they are queued again
    /// once the signal is no longer ignored (`ignored_posix_timers`).
    ignored_timers: Vec<Arc<PosixTimer>>,
}

impl Default for GroupState {
    fn default() -> Self {
        Self {
            actions: [Action::default(); NSIG as usize],
            trampoline: None,
            shared: Pending::default(),
            tasks: HashMap::new(),
            real_timer: RealTimer::default(),
            posix_timers: posix_timer::Table::default(),
            ignored_timers: Vec::new(),
        }
    }
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
    /// The task's work flag, raised whenever a signal may have become deliverable to it.
    flag: Arc<WorkFlag>,
    /// Kernel work makes the task leave a blocking wait without a signal (`TIF_NOTIFY_SIGNAL`).
    notify: bool,
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
}

impl TaskState {
    fn new(inherited: Inherited, flag: Arc<WorkFlag>) -> Self {
        Self {
            flag,
            notify: false,
            blocked: inherited.blocked,
            saved: None,
            pending: Pending::default(),
            altstack: inherited.altstack,
            frames: Vec::new(),
        }
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
fn kernel_info(sig: i32) -> Siginfo {
    Siginfo {
        si_signo: sig,
        si_errno: 0,
        si_code: SiCode::SI_KERNEL.0,
        sifields: [0; 116],
    }
}

/// A `siginfo` for a signal a process sent: `si_pid` and `si_uid` (its real uid) are its first
/// fields.
pub(crate) fn user_info(sig: i32, code: SiCode, pid: Pid, uid: u32) -> Siginfo {
    let mut info = kernel_info(sig);
    info.si_code = code.0;
    info.sifields[0..4].copy_from_slice(&pid.to_le_bytes());
    info.sifields[4..8].copy_from_slice(&uid.to_le_bytes());
    info
}

impl Signals {
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
        let mut state = self.state.lock().unwrap();
        state.tasks.insert(task, TaskState::new(inherited, flag));
        Arc::new(TaskSignals {
            group: self.clone(),
            task,
        })
    }

    // ---- proc ----
    /// `copy_sighand` and `copy_signal` for a new process, whose queued signals `account` pays
    /// for: the dispositions and the trampoline, with nothing pending and no timers.
    pub(crate) fn fork(&self, account: Account) -> Arc<Self> {
        let new = Self::new(Some(account));
        {
            let state = self.state.lock().unwrap();
            let mut copy = new.state.lock().unwrap();
            copy.actions = state.actions;
            copy.trampoline = state.trampoline;
        }
        new
    }

    /// `flush_signal_handlers` and `sas_ss_reset` at `execve`: caught signals go back to their
    /// default action, ignored ones stay ignored, and the trampoline and alternate stacks
    /// belonged to the old image. Masks and pending signals stay.
    pub(crate) fn exec_reset(&self) {
        let mut state = self.state.lock().unwrap();
        for action in state.actions.iter_mut() {
            *action = match action.is_ignore() {
                true => Action {
                    handler: action.handler,
                    ..Action::default()
                },
                false => Action::default(),
            };
        }
        state.trampoline = None;
        for task in state.tasks.values_mut() {
            task.altstack = AltStack::default();
            task.frames.clear();
        }
    }

    /// How the group treats its children's exits: SIGCHLD is ignored (`SIG_IGN`), and
    /// `SA_NOCLDWAIT` is set.
    pub(crate) fn child_reaping(&self) -> (bool, bool) {
        let state = self.state.lock().unwrap();
        let action = state.actions[Signal::SIGCHLD.0 as usize - 1];
        (
            action.is_ignore(),
            action.flags & SaFlags::SA_NOCLDWAIT.0 != 0,
        )
    }

    /// Queues `info` for the whole group (`__group_send_sig_info`).
    pub(crate) fn send_group(&self, info: Siginfo) -> Result<(), Errno> {
        self.send(None, info)
    }
    // ---- end proc ----

    /// `zap_other_threads`: queues SIGKILL for the group, so every task blocked in an
    /// interruptible wait wakes up and unwinds through its pending exit.
    pub(crate) fn kill_all(&self) {
        // SIGKILL needs no room in a queue.
        let _ = self.send(None, kernel_info(Signal::SIGKILL.0));
    }

    /// Whether `sig` is pending for the group or one of its tasks.
    fn is_pending(&self, sig: i32) -> bool {
        let state = self.state.lock().unwrap();
        (state.shared.set | state.tasks.values().fold(0, |set, t| set | t.pending.set)) & bit(sig)
            != 0
    }

    pub(crate) fn clear_notify(&self, task: TaskId) {
        if let Some(task) = self.state.lock().unwrap().tasks.get_mut(&task) {
            task.notify = false;
        }
    }

    /// Makes `task` leave an interruptible wait for kernel work, without a signal
    /// (`task_work_add` with `TWA_SIGNAL`).
    pub(crate) fn notify(&self, task: TaskId) {
        let mut state = self.state.lock().unwrap();
        let Some(task) = state.tasks.get_mut(&task) else {
            return;
        };
        task.notify = true;
        task.flag.raise();
        drop(state);
        self.queue.wake(wasixv2_wait::PollMask::empty());
    }

    /// `de_thread` after an `execve` from `from`: `to`, the leader, goes on with `from`'s mask
    /// and pending signals, and `from` is gone from the group.
    pub(crate) fn take_over(&self, from: TaskId, to: TaskId) {
        let mut state = self.state.lock().unwrap();
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
        // The charge reads the process's credentials and limits, which come before the group.
        let charge = self.account.as_ref().map_or(Ok(None), |a| a.charge(&info));
        let mut state = self.state.lock().unwrap();
        if !state.prepare(task, info.si_signo) {
            return Ok(());
        }
        let target = task.filter(|task| state.tasks.contains_key(task));
        state.pending(target).add(info, charge)?;
        state.raise(target, info.si_signo);
        drop(state);
        self.queue.wake(wasixv2_wait::PollMask::empty());
        Ok(())
    }

    /// `posixtimer_send_sigqueue`: queues the signal of `timer`, which just expired, for its
    /// target thread or the group. Called with the timer's state locked. `periodic` says
    /// whether the timer re-arms once its signal is delivered.
    pub(crate) fn send_timer(&self, timer: &Arc<PosixTimer>, periodic: bool) {
        let target = timer.target_thread();
        let info = timer.info();
        let mut state = self.state.lock().unwrap();
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
        if !state.prepare(target, info.si_signo) {
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
        state.raise(target, info.si_signo);
        drop(state);
        self.queue.wake(wasixv2_wait::PollMask::empty());
    }

    /// `posix_timer_delete`'s signal half: `timer` stops being a timer of the group. A
    /// signal of it still queued stays, and is dropped when it is dequeued.
    pub(crate) fn unlink_timer(&self, timer: &Arc<PosixTimer>, id: i32) {
        let mut state = self.state.lock().unwrap();
        timer.invalidate();
        state.posix_timers.remove(id);
        state.ignored_timers.retain(|t| !Arc::ptr_eq(t, timer));
    }

    /// Runs `f` on the group's POSIX timer table, with the group locked.
    pub(crate) fn with_posix_timers<R>(&self, f: impl FnOnce(&mut posix_timer::Table) -> R) -> R {
        f(&mut self.state.lock().unwrap().posix_timers)
    }

    /// Whether `task` still belongs to the group.
    pub(crate) fn has_task(&self, task: TaskId) -> bool {
        self.state.lock().unwrap().tasks.contains_key(&task)
    }

    /// `flush_itimer_signals` at `execve`: the old image's POSIX timer signals go.
    pub(crate) fn flush_timer_signals(&self) {
        let mut state = self.state.lock().unwrap();
        state.shared.flush_timers();
        for task in state.tasks.values_mut() {
            task.pending.flush_timers();
        }
    }

    /// `hrtimer_cancel(&sig->real_timer)` once the group is dead.
    pub(crate) fn cancel_real_timer(&self) {
        let mut state = self.state.lock().unwrap();
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
            state = self.state.lock().unwrap();
            if counts {
                return (state, Some(info));
            }
        }
    }
}

impl GroupState {
    /// `prepare_signal`: SIGCONT and the stop signals cancel each other; returns whether
    /// `sig` is to be queued for `task`, or for the group with `None`. A blocked signal is
    /// never ignored, since the handler may change before it is unblocked (`sig_ignored`).
    fn prepare(&mut self, task: Option<TaskId>, sig: i32) -> bool {
        let stops = [
            Signal::SIGSTOP,
            Signal::SIGTSTP,
            Signal::SIGTTIN,
            Signal::SIGTTOU,
        ]
        .iter()
        .fold(0, |mask, sig| mask | bit(sig.0));
        let cancelled = match Signal(sig) {
            Signal::SIGCONT => stops,
            _ if stops & bit(sig) != 0 => bit(Signal::SIGCONT.0),
            _ => 0,
        };
        if cancelled != 0 {
            self.flush(cancelled);
        }
        let blocked = match task {
            Some(task) => self
                .tasks
                .get(&task)
                .is_some_and(|t| t.blocked & bit(sig) != 0),
            None => self.tasks.values().all(|t| t.blocked & bit(sig) != 0),
        };
        blocked || !self.ignores(sig)
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

    /// `signal_wake_up` for `sig`, just queued for `task` or for the group with `None`: raises
    /// the work flag of the task that takes it, or of every task that may (`complete_signal`).
    fn raise(&mut self, task: Option<TaskId>, sig: i32) {
        match task.and_then(|task| self.tasks.get(&task)) {
            Some(task) => task.flag.raise(),
            None => {
                for task in self.tasks.values() {
                    if task.blocked & bit(sig) == 0 {
                        task.flag.raise();
                    }
                }
            }
        }
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
            self.raise(target, sig);
        }
    }

    /// `sig_task_ignored`: the disposition throws the signal away. SIGKILL and SIGSTOP never are.
    fn ignores(&self, sig: i32) -> bool {
        if UNBLOCKABLE & bit(sig) != 0 {
            return false;
        }
        let action = &self.actions[sig as usize - 1];
        action.is_ignore()
            || (action.is_default() && matches!(default_action(sig), DefaultAction::Ignore))
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

    /// `dequeue_signal`: the lowest-numbered signal in `wanted`, from the task's own pending
    /// set first, then the group's. A POSIX timer's signal comes with its timer, which the
    /// caller asks, with the group unlocked, whether the signal still counts.
    fn dequeue(&mut self, task: TaskId, wanted: u64) -> Option<(Siginfo, Option<Arc<PosixTimer>>)> {
        let own = self.task(task).pending.set & wanted;
        if own != 0 {
            let sig = own.trailing_zeros() as i32 + 1;
            return Some(self.task(task).pending.take(sig));
        }
        let shared = self.shared.set & wanted;
        if shared != 0 {
            return Some(self.take_shared(shared.trailing_zeros() as i32 + 1));
        }
        None
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

/// One task's view of its group's signals: Linux's `signal_pending` for blocking waits.
pub(crate) struct TaskSignals {
    group: Arc<Signals>,
    task: TaskId,
}

impl TaskSignals {
    pub(crate) fn group(&self) -> &Arc<Signals> {
        &self.group
    }

    fn with<R>(&self, f: impl FnOnce(&mut TaskState) -> R) -> R {
        let mut state = self.group.state.lock().unwrap();
        f(state.task(self.task))
    }

    /// `sigprocmask`'s `set_current_blocked`: SIGKILL and SIGSTOP are never blocked. Returns the
    /// previous mask. A signal it unblocks is delivered on the way back to the guest
    /// (`recalc_sigpending`).
    pub(crate) fn replace_blocked(&self, blocked: u64) -> u64 {
        self.with(|task| {
            task.flag.raise();
            std::mem::replace(&mut task.blocked, blocked & !UNBLOCKABLE)
        })
    }

    pub(crate) fn blocked(&self) -> u64 {
        self.with(|task| task.blocked)
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
        let state = self.group.state.lock().unwrap();
        let task = state.tasks.get(&self.task);
        let (pending, blocked) = task.map_or((0, 0), |task| (task.pending.set, task.blocked));
        let mut ignored = 0;
        let mut caught = 0;
        for (index, action) in state.actions.iter().enumerate() {
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
        self.with(|task| {
            task.flag.raise();
            match interrupted {
                true => task.saved = Some(saved),
                false => task.blocked = saved,
            }
        });
    }

    /// The signal half of `rt_sigreturn` for the task's innermost handler (see
    /// [`Kernel::signal_return`]). A task whose `execve` handed its signal state to the leader
    /// (`de_thread`) has none left: its handler unwinds with the old image, and nothing of it
    /// is restored.
    fn sigreturn(&self, mask: Option<u64>) {
        let mut state = self.group.state.lock().unwrap();
        let Some(task) = state.tasks.get_mut(&self.task) else {
            return;
        };
        let saved = task.frames.pop();
        if let Some(mask) = mask {
            if let Some(alt) = saved {
                task.altstack = alt;
            }
            task.blocked = mask & !UNBLOCKABLE;
            task.flag.raise();
        }
    }

    /// The fatal signal pending for the task, as a killable wait sees it
    /// (`__fatal_signal_pending`): one the task does not block whose action is a default that
    /// kills, SIGKILL among them, for the task or its group. Linux's `complete_signal` makes such
    /// a signal a SIGKILL for every thread as it is sent.
    pub(crate) fn fatal_pending(&self) -> Option<i32> {
        let state = self.group.state.lock().unwrap();
        let task = state.tasks.get(&self.task)?;
        let mut pending = (task.pending.set | state.shared.set) & !task.blocked;
        while pending != 0 {
            let sig = pending.trailing_zeros() as i32 + 1;
            let action = state.actions[sig as usize - 1];
            if action.is_default() && matches!(default_action(sig), DefaultAction::Terminate) {
                return Some(sig);
            }
            pending &= !bit(sig);
        }
        None
    }

    /// What ends a killable wait of the task (`TASK_KILLABLE`): a fatal signal only.
    pub(crate) fn killable(&self) -> Killable<'_> {
        Killable(self)
    }

    /// Queues SIGKILL for this task alone, as `zap_other_threads` does for each thread.
    pub(crate) fn kill(&self) {
        let _ = self
            .group
            .send(Some(self.task), kernel_info(Signal::SIGKILL.0));
    }

    /// Removes the task from its group once it is gone (`__exit_signal`).
    pub(crate) fn detach(&self) {
        self.group.state.lock().unwrap().tasks.remove(&self.task);
    }

    // ---- ipc: signalfd ----
    /// `dequeue_signal` for a signalfd: the next signal in `wanted` pending for the task or its
    /// group, blocked or not. Once the task is gone, only the group's.
    pub(crate) fn dequeue_from(&self, wanted: u64) -> Option<Siginfo> {
        let mut state = self.group.state.lock().unwrap();
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
            state = self.group.state.lock().unwrap();
        }
    }

    /// Whether a signal in `wanted` is pending for the task or its group.
    pub(crate) fn has_pending(&self, wanted: u64) -> bool {
        let state = self.group.state.lock().unwrap();
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
    fn pending(&self) -> bool {
        let state = self.group.state.lock().unwrap();
        // A task taken over by an `execve` from another thread has nothing left to wait for.
        let Some(task) = state.tasks.get(&self.task) else {
            return true;
        };
        task.notify || state.deliverable(self.task, !task.blocked) != 0
    }

    fn queue(&self) -> Option<&WaitQueue> {
        Some(&self.group.queue)
    }
}

/// See [`TaskSignals::killable`].
pub(crate) struct Killable<'a>(&'a TaskSignals);

impl Interrupt for Killable<'_> {
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
    fn pending(&self) -> bool {
        let state = self.signals.group.state.lock().unwrap();
        let Some(task) = state.tasks.get(&self.signals.task) else {
            return true;
        };
        task.notify || state.deliverable(self.signals.task, self.these | !task.blocked) != 0
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

fn check_sigsetsize(size: u32) -> Result<(), Errno> {
    match size as usize == Sigset::SIZE {
        true => Ok(()),
        false => Err(Errno::EINVAL),
    }
}

fn check_signal(sig: Signal) -> Result<i32, Errno> {
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

impl Kernel {
    pub(crate) fn task_signals(&self, task: TaskId) -> Arc<TaskSignals> {
        self.task_ref(task, |task| task.signals.clone())
    }

    /// The signal state a blocked call watches.
    pub(crate) fn interrupt(&self, caller: &Caller<'_>) -> Arc<TaskSignals> {
        self.task_signals(caller.task())
    }

    /// The thread group `pid` names, or every group for `None`. As for Linux's `kill_pid_info`,
    /// any thread's id names its whole group, and the group id still does once its leader thread
    /// has exited.
    fn groups(&self, pid: Option<Pid>) -> Vec<(Pid, Arc<Signals>)> {
        let state = self.state.read().unwrap();
        let mut groups: Vec<(Pid, Arc<Signals>)> = Vec::new();
        for (id, task) in &state.tasks {
            let leader = task.process.pid.0 as Pid;
            let named = pid.is_none_or(|pid| pid == leader || pid == id.0 as Pid);
            if !named || groups.iter().any(|(p, _)| *p == leader) {
                continue;
            }
            // ---- proc ----
            // A reaped process's tasks may linger while their instance unwinds.
            if self.procs.find(leader as u32).is_none() {
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
            && pid > 0
            && self.procs.find(pid as u32).is_some()
        {
            groups.push((pid, Signals::new(None)));
        }
        groups
    }

    fn tgid(&self, task: TaskId) -> Pid {
        self.process(task).pid.0 as Pid
    }

    /// `send_sig(SIGPIPE, current, 0)` when a write failed with `EPIPE`.
    pub(crate) fn sigpipe<T>(&self, caller: &Caller<'_>, ret: SysResult<T>) -> SysResult<T> {
        if ret.as_ref().err() == Some(&SysErr::Errno(Errno::EPIPE)) {
            let info = self.sender_info(caller, Signal::SIGPIPE.0, SiCode::SI_USER);
            let signals = self.task_signals(caller.task());
            // SIGPIPE needs no room in a queue.
            let _ = signals.group.send(Some(caller.task()), info);
        }
        ret
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

    /// The return-to-guest half of `get_signal`: dequeues the next signal `task` takes and
    /// either applies its default action or prepares its handler's frame (`handle_signal`).
    pub(crate) fn take_signal(&self, task: TaskId, signals: &TaskSignals) -> Option<GuestAction> {
        let mut state = signals.group.state.lock().unwrap();
        state.tasks.get_mut(&task)?.notify = false;
        loop {
            let blocked = state.task(task).blocked;
            let info;
            (state, info) = signals.group.dequeue(state, task, !blocked);
            let Some(info) = info else {
                // `restore_saved_sigmask`: no handler took the temporary mask's signal.
                let task = state.task(task);
                if let Some(saved) = task.saved.take() {
                    // What the old mask unblocks is delivered on the next return.
                    task.blocked = saved;
                    task.flag.raise();
                }
                return None;
            };
            let sig = info.si_signo;
            let action = state.actions[sig as usize - 1];
            if action.is_ignore() {
                continue;
            }
            let trampoline = state.trampoline;
            let (false, Some(trampoline)) = (action.is_default(), trampoline) else {
                match default_action(sig) {
                    // TODO: stop the group until SIGCONT once there is job control.
                    DefaultAction::Ignore | DefaultAction::Stop => continue,
                    DefaultAction::Terminate => {
                        drop(state);
                        self.force_fatal_signal(task, sig);
                        return self.with_task(task, |t| t.work.take());
                    }
                }
            };
            let task = state.task(task);
            let mask = task.saved.take().unwrap_or(task.blocked);
            // The frame saves the alternate stack (`__save_altstack`), and the execution layer
            // puts it there unless the guest runs on it already (`sigsp`). Delivery disarms an
            // `SS_AUTODISARM` stack until the handler returns (`signal_delivered`).
            let alt = task.altstack;
            task.frames.push(alt);
            if alt.flags & SigaltstackFlags::SS_AUTODISARM.0 != 0 {
                task.altstack = AltStack::default();
            }
            let mut block = action.mask;
            if action.flags & SaFlags::SA_NODEFER.0 == 0 {
                block |= bit(sig);
            }
            task.blocked = (task.blocked | block) & !UNBLOCKABLE;
            // `SA_ONESHOT` resets only the handler; the flags and mask stay.
            if action.flags & SaFlags::SA_RESETHAND.0 != 0 {
                state.actions[sig as usize - 1].handler = SignalHandler::SIG_DFL.0 as u32;
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

    /// The default action of a signal that kills: the whole thread group dies of `sig`
    /// (`do_group_exit`), so its parent sees `WIFSIGNALED`. No core is written, so the core
    /// signals (SIGSEGV, SIGABRT, ...) end the same way, without `WCOREDUMP`. Also what a trap
    /// comes to: a fault whose handler could not run (`force_sig_fault`, `force_sigsegv`).
    /// `wasmer run` reports the death as the shell does: 128 + signal.
    /// False when the group was on its way out already, which the signal does not change.
    pub(crate) fn force_fatal_signal(&self, task: TaskId, sig: i32) -> bool {
        if self.process(task).group.exiting() {
            // The exit is pending on every task; `task` unwinds through it.
            return false;
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
            let state = self.state.read().unwrap();
            match state.tasks.get(&task) {
                Some(task) => (task.signals.clone(), task.restart.is_some()),
                None => return,
            }
        };
        if restart && let Some(task) = self.state.lock().unwrap().tasks.get_mut(&task) {
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
        signals.group.state.lock().unwrap().trampoline = Some(callback);
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
            let mut state = signals.group.state.lock().unwrap();
            let old = state.actions[sig as usize - 1];
            if let Some(new) = new {
                state.actions[sig as usize - 1] = Action {
                    handler: new.sa_handler,
                    flags: new.sa_flags,
                    mask: sigset_bits(new.sa_mask) & !UNBLOCKABLE,
                };
                // POSIX: setting a disposition to ignore discards the pending signal.
                if state.ignores(sig) {
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
            signals.replace_blocked(mask);
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
            let state = signals.group.state.lock().unwrap();
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
                .dequeue(group.state.lock().unwrap(), caller.task(), these)
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
        if reserved_to_kernel(info.si_code) && tid.unwrap_or(tgid) != caller.task().0 as Pid {
            return Err(Errno::EPERM.into());
        }
        let sender = Some(&*self.process(caller.task()));
        match tid {
            Some(tid) => self.send_to_task(sender, Some(tgid), tid, info)?,
            None => self.send_to_group(sender, tgid, info)?,
        }
        Ok(0)
    }

    /// A `siginfo` naming the caller's process as the sender.
    pub(crate) fn sender_info(&self, caller: &Caller<'_>, sig: i32, code: SiCode) -> Siginfo {
        let process = self.process(caller.task());
        let uid = process.cred().user.real;
        user_info(sig, code, process.pid.0 as Pid, uid)
    }

    /// `check_kill_permission` for a signal from `sender` (`None`: from outside the machine,
    /// which may signal anything) to the process `target`: `kill_ok_by_cred`, or `SIGCONT`
    /// within the sender's session.
    fn may_signal(&self, sender: Option<&Process>, target: Pid, sig: i32) -> bool {
        let (Some(sender), Some(target)) = (sender, self.procs.find(target as u32)) else {
            return true;
        };
        sender.cred().may_signal(&target.cred())
            || (sig == Signal::SIGCONT.0
                && sender.ident.lock().unwrap().sid == target.ident.lock().unwrap().sid)
    }

    /// Queues `info` for the thread group `pid` (`kill_pid_info`), if `sender` may signal it.
    pub(crate) fn send_to_group(
        &self,
        sender: Option<&Process>,
        pid: Pid,
        info: Siginfo,
    ) -> Result<(), Errno> {
        let groups = self.groups(Some(pid));
        let Some((_, group)) = groups.first() else {
            return Err(Errno::ESRCH);
        };
        check_sendable(info.si_signo)?;
        if !self.may_signal(sender, pid, info.si_signo) {
            return Err(Errno::EPERM);
        }
        match info.si_signo {
            0 => Ok(()),
            _ => group.send(None, info),
        }
    }

    /// Sends `sig` to the process `pid` from outside the machine, like `kill(2)` from a process
    /// in an ancestor PID namespace: `si_code` is `SI_USER` and `si_pid` 0. The guest's
    /// dispositions apply as for any other signal, so with no handler the default action
    /// runs. A signal reaches a task at its next interruption point: a blocking call, the
    /// return from a syscall, or a preemption point of the execution layer.
    pub fn kill(&self, pid: TaskId, sig: Signal) -> Result<(), Errno> {
        let sig = check_signal(sig)?;
        self.send_to_group(None, pid.0 as Pid, user_info(sig, SiCode::SI_USER, 0, 0))
    }

    /// Sends `sig` to every process in the process group of `pid`, as a terminal's line
    /// discipline sends `^C` and `^\` to its foreground group (`isig`, `kill_pgrp` with
    /// `SI_KERNEL`).
    // TODO: the terminal's foreground group, once the kernel has terminals.
    pub fn kill_process_group(&self, pid: TaskId, sig: Signal) -> Result<(), Errno> {
        let sig = check_signal(sig)?;
        let groups = self.pgrp_groups(self.pgid_of(pid.0 as Pid));
        if groups.is_empty() {
            return Err(Errno::ESRCH);
        }
        for (_, group) in groups {
            // A kernel signal goes past the limit, or without its `siginfo`.
            let _ = group.send(None, kernel_info(sig));
        }
        Ok(())
    }

    /// Whether `sig` is still pending, not yet taken by any task, for a process in the
    /// process group of `pid`.
    pub fn process_group_pending(&self, pid: TaskId, sig: Signal) -> bool {
        let Ok(sig) = check_signal(sig) else {
            return false;
        };
        let groups = self.pgrp_groups(self.pgid_of(pid.0 as Pid));
        groups.iter().any(|(_, group)| group.is_pending(sig))
    }

    /// `do_tkill`: `tgid` must own `tid` when given.
    fn send_to_task(
        &self,
        sender: Option<&Process>,
        tgid: Option<Pid>,
        tid: Pid,
        info: Siginfo,
    ) -> Result<(), Errno> {
        if tid <= 0 || tgid.is_some_and(|tgid| tgid <= 0) {
            return Err(Errno::EINVAL);
        }
        let target = {
            let state = self.state.read().unwrap();
            state
                .tasks
                .get(&TaskId(tid as u32))
                .filter(|task| tgid.is_none_or(|tgid| task.process.pid.0 as Pid == tgid))
                // A thread that exited is gone, though its instance may still be unwinding; a
                // zombie leader stays until its group ends.
                .filter(|task| !task.thread.exited() || task.process.pid.0 as Pid == tid)
                .map(|task| (task.process.pid.0 as Pid, task.signals.group().clone()))
        };
        let (pid, group) = target.ok_or(Errno::ESRCH)?;
        check_sendable(info.si_signo)?;
        if !self.may_signal(sender, pid, info.si_signo) {
            return Err(Errno::EPERM);
        }
        match info.si_signo {
            0 => Ok(()),
            _ => group.send(Some(TaskId(tid as u32)), info),
        }
    }

    pub(crate) async fn sys_kill(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        sig: Signal,
    ) -> SysResult<i32> {
        let me = self.tgid(caller.task());
        let info = self.sender_info(caller, sig.0, SiCode::SI_USER);
        let targets: Vec<_> = match pid {
            // ---- proc ----
            0 => self.pgrp_groups(self.pgid_of(me)),
            // Every process but init and the caller.
            -1 => self
                .groups(None)
                .into_iter()
                .filter(|(pid, _)| *pid > 1 && *pid != me)
                .collect(),
            pid if pid < 0 => self.pgrp_groups(pid.checked_abs().unwrap_or(0)),
            pid => self.groups(Some(pid)),
        };
        if targets.is_empty() {
            return Err(Errno::ESRCH.into());
        }
        check_sendable(sig.0)?;
        let sender = self.process(caller.task());
        let mut sent = false;
        for (target, group) in targets {
            if !self.may_signal(Some(&sender), target, sig.0) {
                continue;
            }
            sent = true;
            if sig.0 != 0 {
                // `kill`'s signals are never refused for want of room (`SI_USER`).
                let _ = group.send(None, info);
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
        let sender = self.process(caller.task());
        self.send_to_task(Some(&sender), tgid, tid, info)?;
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
        let old = match which {
            ItimerWhich::ITIMER_REAL => {
                let signals = self.task_signals(caller.task());
                self.arm_real_timer(signals.group(), new)
            }
            // TODO: CPU-time timers; they never fire.
            _ => (Duration::ZERO, Duration::ZERO),
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
                let state = signals.group.state.lock().unwrap();
                real_timer_value(&state.real_timer, self.host.monotonic())
            }
            ItimerWhich::ITIMER_VIRTUAL | ItimerWhich::ITIMER_PROF => {
                (Duration::ZERO, Duration::ZERO)
            }
            _ => return Err(Errno::EINVAL.into()),
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
        let mut state = group.state.lock().unwrap();
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
        let mut state = group.state.lock().unwrap();
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
