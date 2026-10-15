//! Signals, after Linux's `kernel/signal.c`: generation into per-task and per-thread-group
//! pending sets, masks, dispositions, dequeueing on the return-to-guest path, `sigaltstack`,
//! `sigsuspend`, `sigtimedwait` and the `ITIMER_REAL` interval timer.
//!
//! Handlers run in the guest: [`Kernel::take_signal`] hands the execution layer a
//! [`SignalFrame`] and [`Kernel::signal_return`] is the matching sigreturn.

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};

use wasixv2_abi::{
    ConstKernelOldItimervalPtr, ConstKernelTimespecPtr, ConstSigactionPtr, ConstSigaltstackPtr,
    ConstSiginfoPtr, ConstSigsetPtr, ItimerWhich, KernelOldItimerval, KernelOldItimervalPtr,
    KernelOldTimeval, Pid, SaFlags, SiCode, Sigaction, SigactionPtr, Sigaltstack, SigaltstackFlags,
    SigaltstackPtr, Siginfo, SiginfoPtr, Signal, SignalHandler, SignalLimits, SigprocmaskHow,
    Sigset, SigsetPtr,
};
use wasixv2_host::Host;
use wasixv2_syscalls::{
    Caller, Errno, GuestAction, SignalFrame, SysErr, SysResult, TaskId, read_value, write_value,
};
use wasixv2_wait::{Interrupt, WaitQueue, Waiter, Wakeup};

use crate::{ID, Kernel, time};

const NSIG: i32 = 64;
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
}

struct GroupState {
    actions: [Action; NSIG as usize],
    /// The guest's trampoline (`signal-callback-register`).
    trampoline: Option<u32>,
    shared: Pending,
    tasks: HashMap<TaskId, TaskState>,
    real_timer: RealTimer,
}

impl Default for GroupState {
    fn default() -> Self {
        Self {
            actions: [Action::default(); NSIG as usize],
            trampoline: None,
            shared: Pending::default(),
            tasks: HashMap::new(),
            real_timer: RealTimer::default(),
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

/// A pending set plus the `siginfo` of every queued instance, oldest first.
#[derive(Default)]
struct Pending {
    set: u64,
    queue: VecDeque<Siginfo>,
}

impl Pending {
    /// `__sigqueue_alloc` and `sigaddset`. Standard signals are not queued twice.
    fn add(&mut self, info: Siginfo) {
        let sig = info.si_signo;
        if sig < SIGRTMIN && self.set & bit(sig) != 0 {
            return;
        }
        self.set |= bit(sig);
        self.queue.push_back(info);
    }

    /// `collect_signal`: takes the oldest instance of `sig`.
    fn take(&mut self, sig: i32) -> Siginfo {
        let at = self.queue.iter().position(|info| info.si_signo == sig);
        let info = at.and_then(|at| self.queue.remove(at));
        if !self.queue.iter().any(|info| info.si_signo == sig) {
            self.set &= !bit(sig);
        }
        info.unwrap_or_else(|| kernel_info(sig))
    }

    fn flush(&mut self, mask: u64) {
        self.set &= !mask;
        self.queue.retain(|info| mask & bit(info.si_signo) == 0);
    }
}

struct TaskState {
    blocked: u64,
    /// The mask to restore once a `sigsuspend` or `ppoll` mask has served its signal
    /// (`TIF_RESTORE_SIGMASK` and `saved_sigmask`).
    saved: Option<u64>,
    pending: Pending,
    altstack: AltStack,
    /// One entry per handler running on this task, innermost last: whether it runs on the
    /// alternate stack, and the alternate stack to re-arm afterwards (`SS_AUTODISARM`).
    frames: Vec<(bool, Option<AltStack>)>,
}

impl TaskState {
    fn new(blocked: u64) -> Self {
        Self {
            blocked,
            saved: None,
            pending: Pending::default(),
            altstack: AltStack::default(),
            frames: Vec::new(),
        }
    }

    fn on_altstack(&self) -> bool {
        self.frames.iter().any(|(on, _)| *on)
    }
}

#[derive(Clone, Copy)]
struct AltStack {
    sp: u32,
    size: u32,
    /// `SS_DISABLE`, or `SS_AUTODISARM` for an armed stack.
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
    fn enabled(&self) -> bool {
        self.flags & SigaltstackFlags::SS_DISABLE.0 == 0
    }
}

/// `ITIMER_REAL`. Each arming bumps `generation`, so the timer task of an older arming stops.
#[derive(Default)]
struct RealTimer {
    generation: u64,
    /// On the host's monotonic clock.
    deadline: Option<Duration>,
    interval: Duration,
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

/// A `siginfo` for a signal a process sent: `si_pid` and `si_uid` are its first fields.
fn user_info(sig: i32, code: SiCode, pid: Pid) -> Siginfo {
    let mut info = kernel_info(sig);
    info.si_code = code.0;
    info.sifields[0..4].copy_from_slice(&pid.to_le_bytes());
    info.sifields[4..8].copy_from_slice(&ID.to_le_bytes());
    info
}

impl Signals {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::default(),
            queue: WaitQueue::new(),
        })
    }

    /// Adds `task` to the group, with the mask it inherits.
    pub(crate) fn attach(self: &Arc<Self>, task: TaskId, blocked: u64) -> Arc<TaskSignals> {
        let mut state = self.state.lock().unwrap();
        state.tasks.insert(task, TaskState::new(blocked));
        Arc::new(TaskSignals {
            group: self.clone(),
            task,
        })
    }

    // ---- proc ----
    /// `copy_sighand` and `copy_signal` for a new process: the dispositions and the trampoline,
    /// with nothing pending and no timers.
    pub(crate) fn fork(&self) -> Arc<Self> {
        let new = Self::new();
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
    pub(crate) fn send_group(&self, info: Siginfo) {
        self.send(None, info);
    }
    // ---- end proc ----

    /// `zap_other_threads`: queues SIGKILL for the group, so every task blocked in an
    /// interruptible wait wakes up and unwinds through its pending exit.
    pub(crate) fn kill_all(&self) {
        self.send(None, kernel_info(Signal::SIGKILL.0));
    }

    /// `send_signal`: queues `info` for the group, or for `task` alone, unless it would be
    /// ignored, and wakes the group's waiters.
    fn send(&self, task: Option<TaskId>, info: Siginfo) {
        let sig = info.si_signo;
        let mut state = self.state.lock().unwrap();
        // `prepare_signal`: SIGCONT and the stop signals cancel each other.
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
            state.shared.flush(cancelled);
            for task in state.tasks.values_mut() {
                task.pending.flush(cancelled);
            }
        }
        // `sig_ignored`: a blocked signal stays queued, since the handler may change before it
        // is unblocked.
        let blocked = match task {
            Some(task) => state
                .tasks
                .get(&task)
                .is_some_and(|t| t.blocked & bit(sig) != 0),
            None => state.tasks.values().all(|t| t.blocked & bit(sig) != 0),
        };
        if !blocked && state.ignores(sig) {
            return;
        }
        match task.and_then(|task| state.tasks.get_mut(&task)) {
            Some(task) => task.pending.add(info),
            None => state.shared.add(info),
        }
        drop(state);
        self.queue.wake(wasixv2_wait::PollMask::empty());
    }
}

impl GroupState {
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
    /// set first, then the group's.
    fn dequeue(&mut self, task: TaskId, wanted: u64) -> Option<Siginfo> {
        let own = self.task(task).pending.set & wanted;
        if own != 0 {
            let sig = own.trailing_zeros() as i32 + 1;
            return Some(self.task(task).pending.take(sig));
        }
        let shared = self.shared.set & wanted;
        if shared != 0 {
            let sig = shared.trailing_zeros() as i32 + 1;
            return Some(self.shared.take(sig));
        }
        None
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
    /// previous mask.
    pub(crate) fn replace_blocked(&self, blocked: u64) -> u64 {
        self.with(|task| std::mem::replace(&mut task.blocked, blocked & !UNBLOCKABLE))
    }

    pub(crate) fn blocked(&self) -> u64 {
        self.with(|task| task.blocked)
    }

    /// Ends a temporary mask (`sigsuspend`, `ppoll`, `pselect`, `epoll_pwait`). When the call
    /// was interrupted, the temporary mask stays until the handler's frame is built, whose
    /// `uc_sigmask` then holds `saved` (`restore_saved_sigmask_unless`).
    pub(crate) fn restore_blocked(&self, saved: u64, interrupted: bool) {
        self.with(|task| match interrupted {
            true => task.saved = Some(saved),
            false => task.blocked = saved,
        });
    }

    /// Removes the task from its group once it is gone (`__exit_signal`).
    pub(crate) fn detach(&self) {
        self.group.state.lock().unwrap().tasks.remove(&self.task);
    }

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
        let blocked = state.tasks[&self.task].blocked;
        state.deliverable(self.task, !blocked) != 0
    }

    fn queue(&self) -> Option<&WaitQueue> {
        Some(&self.group.queue)
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
        let blocked = state.tasks[&self.signals.task].blocked;
        state.deliverable(self.signals.task, self.these | !blocked) != 0
    }

    fn queue(&self) -> Option<&WaitQueue> {
        Some(&self.signals.group.queue)
    }
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
        self.with_task(task, |task| task.signals.clone())
    }

    /// The signal state a blocked call watches.
    pub(crate) fn interrupt(&self, caller: &Caller<'_>) -> Arc<TaskSignals> {
        self.task_signals(caller.task())
    }

    /// The thread groups whose leader is `pid`, or every group for `None`.
    fn groups(&self, pid: Option<Pid>) -> Vec<(Pid, Arc<Signals>)> {
        let state = self.state.lock().unwrap();
        let mut groups: Vec<(Pid, Arc<Signals>)> = Vec::new();
        for task in state.tasks.values() {
            let leader = task.process.pid.0 as Pid;
            if pid.is_some_and(|pid| pid != leader) || groups.iter().any(|(p, _)| *p == leader) {
                continue;
            }
            // ---- proc ----
            // A reaped process's tasks may linger while their instance unwinds.
            if self.procs.find(leader as u32).is_none() {
                continue;
            }
            groups.push((leader, task.signals.group().clone()));
        }
        groups
    }

    fn tgid(&self, task: TaskId) -> Pid {
        self.process(task).pid.0 as Pid
    }

    /// `send_sig(SIGPIPE, current, 0)` when a write failed with `EPIPE`.
    pub(crate) fn sigpipe<T>(&self, caller: &Caller<'_>, ret: SysResult<T>) -> SysResult<T> {
        if ret.as_ref().err() == Some(&SysErr::Errno(Errno::EPIPE)) {
            let pid = self.tgid(caller.task());
            let info = user_info(Signal::SIGPIPE.0, SiCode::SI_USER, pid);
            let signals = self.task_signals(caller.task());
            signals.group.send(Some(caller.task()), info);
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
        match Waiter::new().sleep(&*signals, Some(sleep.as_mut())).await {
            Wakeup::Interrupted => Err(wasixv2_wait::Interrupted),
            _ => Ok(()),
        }
    }

    /// The return-to-guest half of `get_signal`: dequeues the next signal `task` takes and
    /// either applies its default action or prepares its handler's frame (`handle_signal`).
    pub(crate) fn take_signal(&self, task: TaskId) -> Option<GuestAction> {
        let signals = self.task_signals(task);
        let mut state = signals.group.state.lock().unwrap();
        loop {
            let blocked = state.task(task).blocked;
            let Some(info) = state.dequeue(task, !blocked) else {
                // `restore_saved_sigmask`: no handler took the temporary mask's signal.
                let task = state.task(task);
                if let Some(saved) = task.saved.take() {
                    task.blocked = saved;
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
                    // `wasmer run` reports death by a signal as the shell does: 128 + signal.
                    // The whole thread group dies (`do_group_exit`).
                    DefaultAction::Terminate => {
                        drop(state);
                        self.note_killed(task, sig);
                        self.do_group_exit(task, 128 + sig);
                        return self.with_task(task, |t| t.pending.take());
                    }
                }
            };
            let task = state.task(task);
            let mask = task.saved.take().unwrap_or(task.blocked);
            let alt = task.altstack;
            let onstack =
                action.flags & SaFlags::SA_ONSTACK.0 != 0 && alt.enabled() && !task.on_altstack();
            let mut rearm = None;
            if onstack && alt.flags & SigaltstackFlags::SS_AUTODISARM.0 != 0 {
                rearm = Some(alt);
                task.altstack = AltStack::default();
            }
            let altstack = (
                alt.sp,
                match (task.on_altstack() || onstack, alt.enabled()) {
                    (true, _) => SigaltstackFlags::SS_ONSTACK.0 as i32,
                    (false, true) => (alt.flags & SigaltstackFlags::SS_AUTODISARM.0) as i32,
                    (false, false) => SigaltstackFlags::SS_DISABLE.0 as i32,
                },
                alt.size,
            );
            task.frames.push((onstack, rearm));
            let mut block = action.mask;
            if action.flags & SaFlags::SA_NODEFER.0 == 0 {
                block |= bit(sig);
            }
            task.blocked = (task.blocked | block) & !UNBLOCKABLE;
            if action.flags & SaFlags::SA_RESETHAND.0 != 0 {
                state.actions[sig as usize - 1] = Action::default();
            }
            return Some(GuestAction::Signal(SignalFrame {
                trampoline,
                info,
                handler: action.handler,
                flags: action.flags,
                mask,
                stack_top: onstack.then(|| alt.sp.wrapping_add(alt.size)),
                altstack,
            }));
        }
    }

    /// `rt_sigreturn`: the innermost handler of `task` finished.
    pub(crate) fn signal_return(&self, task: TaskId, mask: Option<u64>) {
        let signals = self.task_signals(task);
        signals.with(|task| {
            if let Some((_, Some(alt))) = task.frames.pop() {
                task.altstack = alt;
            }
            if let Some(mask) = mask {
                task.blocked = mask & !UNBLOCKABLE;
            }
        });
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
                    state.shared.flush(bit(sig));
                    for task in state.tasks.values_mut() {
                        task.pending.flush(bit(sig));
                    }
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
            signals
                .group
                .state
                .lock()
                .unwrap()
                .dequeue(caller.task(), these)
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
        let sig = check_signal(sig)?;
        // Only the kernel may claim to be the kernel or `tkill`, except towards oneself.
        if (info.si_code >= 0 || info.si_code == SiCode::SI_TKILL.0)
            && tgid != self.tgid(caller.task())
        {
            return Err(Errno::EPERM.into());
        }
        info.si_signo = sig;
        match tid {
            Some(tid) => self.send_to_task(Some(tgid), tid, info)?,
            None => self.send_to_group(tgid, info)?,
        }
        Ok(0)
    }

    fn send_to_group(&self, pid: Pid, info: Siginfo) -> Result<(), Errno> {
        let groups = self.groups(Some(pid));
        let Some((_, group)) = groups.first() else {
            return Err(Errno::ESRCH);
        };
        if info.si_signo != 0 {
            group.send(None, info);
        }
        Ok(())
    }

    /// Sends `sig` to the process `pid` from outside the machine, like `kill(2)` from a process
    /// in an ancestor PID namespace: `si_code` is `SI_USER` and `si_pid` 0. The guest's
    /// dispositions apply as for any other signal, so with no handler the default action
    /// runs. A signal reaches a task at its next interruption point (a blocking call or a
    /// return from a syscall).
    pub fn kill(&self, pid: TaskId, sig: Signal) -> Result<(), Errno> {
        let sig = check_signal(sig)?;
        self.send_to_group(pid.0 as Pid, user_info(sig, SiCode::SI_USER, 0))
    }

    /// `do_tkill`: `tgid` must own `tid` when given.
    fn send_to_task(&self, tgid: Option<Pid>, tid: Pid, info: Siginfo) -> Result<(), Errno> {
        if tid <= 0 || tgid.is_some_and(|tgid| tgid <= 0) {
            return Err(Errno::EINVAL);
        }
        let target = {
            let state = self.state.lock().unwrap();
            state
                .tasks
                .get(&TaskId(tid as u32))
                .filter(|task| tgid.is_none_or(|tgid| task.process.pid.0 as Pid == tgid))
                .map(|task| task.signals.group().clone())
        };
        let group = target.ok_or(Errno::ESRCH)?;
        if info.si_signo != 0 {
            group.send(Some(TaskId(tid as u32)), info);
        }
        Ok(())
    }

    pub(crate) async fn sys_kill(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        sig: Signal,
    ) -> SysResult<i32> {
        if !(0..=NSIG).contains(&sig.0) {
            return Err(Errno::EINVAL.into());
        }
        let me = self.tgid(caller.task());
        let info = user_info(sig.0, SiCode::SI_USER, me);
        let targets: Vec<_> = match pid {
            // ---- proc ----
            0 => self.pgrp_groups(self.pgid_of(me)),
            -1 => self
                .groups(None)
                .into_iter()
                .filter(|(pid, _)| *pid != me)
                .collect(),
            pid if pid < 0 => self.pgrp_groups(pid.checked_abs().unwrap_or(0)),
            pid => self.groups(Some(pid)),
        };
        if targets.is_empty() {
            return Err(Errno::ESRCH.into());
        }
        if sig.0 != 0 {
            for (_, group) in targets {
                group.send(None, info);
            }
        }
        Ok(0)
    }

    pub(crate) async fn sys_tgkill(
        &self,
        caller: &Caller<'_>,
        tgid: Option<Pid>,
        tid: Pid,
        sig: Signal,
    ) -> SysResult<i32> {
        if !(0..=NSIG).contains(&sig.0) {
            return Err(Errno::EINVAL.into());
        }
        let info = user_info(sig.0, SiCode::SI_TKILL, self.tgid(caller.task()));
        self.send_to_task(tgid, tid, info)?;
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
        let signals = self.task_signals(caller.task());
        let old = signals.with(|task| -> Result<Sigaltstack, Errno> {
            let on = task.on_altstack();
            let alt = task.altstack;
            let old = Sigaltstack {
                ss_sp: alt.sp,
                ss_flags: match on {
                    true => SigaltstackFlags::SS_ONSTACK.0 as i32,
                    false => alt.flags as i32,
                },
                ss_size: alt.size,
            };
            if let Some(new) = new {
                if on {
                    return Err(Errno::EPERM);
                }
                let flags = new.ss_flags as u32;
                let autodisarm = flags & SigaltstackFlags::SS_AUTODISARM.0;
                let mode = flags & !SigaltstackFlags::SS_AUTODISARM.0;
                task.altstack = match SigaltstackFlags(mode) {
                    SigaltstackFlags::SS_DISABLE => AltStack::default(),
                    m if m.0 == 0 || m == SigaltstackFlags::SS_ONSTACK => {
                        if new.ss_size < SignalLimits::MINSIGSTKSZ.0 {
                            return Err(Errno::ENOMEM);
                        }
                        AltStack {
                            sp: new.ss_sp,
                            size: new.ss_size,
                            flags: autodisarm,
                        }
                    }
                    _ => return Err(Errno::EINVAL),
                };
            }
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
    /// interval. A host task sends SIGALRM at each expiry.
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
        let timer = &mut state.real_timer;
        timer.generation += 1;
        timer.interval = interval;
        timer.deadline = (!value.is_zero()).then(|| now + value);
        if let Some(deadline) = timer.deadline {
            let run = real_timer(self.host.clone(), group.clone(), timer.generation, deadline);
            self.host.executor().spawn_local(Box::pin(run));
        }
        old
    }
}

/// The body of an armed `ITIMER_REAL`: sleeps to each deadline and sends SIGALRM to the group,
/// until a later arming replaces it.
async fn real_timer(host: Arc<dyn Host>, group: Arc<Signals>, generation: u64, first: Duration) {
    let mut deadline = first;
    loop {
        host.sleep_until(deadline).await;
        {
            let mut state = group.state.lock().unwrap();
            let timer = &mut state.real_timer;
            if timer.generation != generation {
                return;
            }
            timer.deadline = (!timer.interval.is_zero()).then(|| deadline + timer.interval);
        }
        group.send(None, kernel_info(Signal::SIGALRM.0));
        let state = group.state.lock().unwrap();
        match state.real_timer.deadline {
            Some(next) if state.real_timer.generation == generation => deadline = next,
            _ => return,
        }
    }
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
