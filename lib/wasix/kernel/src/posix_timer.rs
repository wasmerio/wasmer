//! POSIX interval timers, after Linux's `kernel/time/posix-timers.c` and, for the CPU-time
//! clocks, `posix-cpu-timers.c`: `timer_create`, `timer_settime`, `timer_gettime`,
//! `timer_getoverrun` and `timer_delete`.
//!
//! A timer runs on a kernel timer ([`crate::timer`]) and queues its one signal when it expires.
//! A periodic timer re-arms only once that signal is delivered (`POSIX_TIMER_REQUEUE_PENDING`),
//! and the periods that went by meanwhile become the overrun count. A timer whose signal stays
//! blocked therefore costs nothing until it is taken.
//!
//! A timer on a CPU clock (`posix-cpu-timers.c`) runs on no kernel timer: it waits on the clock
//! of the process or thread it counts ([`crate::cputime`]), and fires at the first charge of CPU
//! time that takes the clock past its expiry. It may be on the clock of any process, or of a
//! thread of the creator's group, and stops with its process's exit or its thread.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use wasixv2_abi::{
    ClockId, ConstKernelItimerspecPtr, ConstSigeventPtr, GuestPtr, KernelItimerspec,
    KernelItimerspecPtr, PrctlArg, SiCode, SigevNotify, Siginfo, Sigval, TimerFlags, TimerId,
    TimerIdPtr, Ulong,
};
use wasixv2_syscalls::{Caller, Errno, SysResult, TaskId, read_value, write_value};
use wasixv2_vfs::Cap;

use crate::{
    Kernel, Process,
    cputime::CpuClock,
    signal::{Charge, Signals},
    sync::MutexExt,
    time::{self, Clock, ClockRef},
    timekeeping::Timekeeper,
    timer::{self, Timer},
};

const SIGRTMAX: i32 = 64;
/// `KTIME_MAX`: timer values saturate at the largest `ktime_t`.
const KTIME_MAX: Duration = Duration::from_nanos(i64::MAX as u64);

/// The POSIX timers of one thread group (`signal_struct::posix_timers` and its id hash).
#[derive(Default)]
pub(crate) struct Table {
    /// `None` holds the id of a timer `timer_create` has not finished yet.
    timers: HashMap<TimerId, Option<Arc<PosixTimer>>>,
    /// `next_posix_timer_id`
    next_id: u32,
    /// `PR_TIMER_CREATE_RESTORE_IDS`: `timer_create` takes the id it is handed, for
    /// checkpoint/restore.
    restore_ids: bool,
}

impl Table {
    /// `posix_timer_add`: the requested id, or the next free one counting up from the last.
    fn reserve(&mut self, requested: Option<TimerId>) -> Result<TimerId, Errno> {
        if let Some(id) = requested {
            if self.timers.contains_key(&id) {
                return Err(Errno::EBUSY);
            }
            self.timers.insert(id, None);
            // Later ids start past the restored ones.
            self.next_id = id as u32 + 1;
            return Ok(id);
        }
        for _ in 0..=i32::MAX as u32 {
            let id = (self.next_id & i32::MAX as u32) as TimerId;
            self.next_id = self.next_id.wrapping_add(1);
            if let std::collections::hash_map::Entry::Vacant(slot) = self.timers.entry(id) {
                slot.insert(None);
                return Ok(id);
            }
        }
        Err(Errno::EAGAIN)
    }

    /// Every timer, by id (`/proc/<pid>/timers`).
    pub(crate) fn all(&self) -> Vec<Arc<PosixTimer>> {
        let mut timers: Vec<Arc<PosixTimer>> = self.timers.values().flatten().cloned().collect();
        timers.sort_by_key(|timer| timer.id);
        timers
    }

    pub(crate) fn remove(&mut self, id: TimerId) {
        self.timers.remove(&id);
    }

    fn get(&self, id: TimerId) -> Option<Arc<PosixTimer>> {
        self.timers.get(&id).cloned().flatten()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Disarmed,
    Armed,
    /// Expired with an interval; re-arms when its signal is delivered.
    RequeuePending,
}

/// The fields Linux guards with `k_itimer::it_lock`.
struct State {
    status: Status,
    /// The clock `expires` is on. A relative `CLOCK_REALTIME` timer runs on the monotonic
    /// clock, as clock changes must not move it.
    base: Clock,
    /// Kept while disarmed: a `SIGEV_NONE` timer is never queued but still reports the time
    /// left.
    expires: Duration,
    interval: Duration,
    /// `it_overrun`: periods missed since the signal was queued, minus one.
    overrun: i64,
    /// `it_overrun_last`: the overrun of the last delivered signal.
    overrun_last: i64,
    /// `it_signal_seq`: bumped by every change that makes a queued signal stale.
    signal_seq: u64,
    /// `it_sigqueue_seq`: the `signal_seq` the queued signal was sent with.
    sigqueue_seq: u64,
    /// While armed on a CPU clock, its place there ([`Process::arm_cpu_timer`]).
    cpu_entry: Option<u64>,
}

/// One POSIX timer (`struct k_itimer`).
pub(crate) struct PosixTimer {
    id: TimerId,
    clock_id: ClockId,
    clock: Clock,
    /// For a CPU clock, whose clock it is (`it.cpu.pid`).
    cpu: Option<CpuClock>,
    sigev_none: bool,
    /// `SIGEV_THREAD_ID`: the thread the signal goes to instead of the group.
    thread: Option<TaskId>,
    signo: i32,
    value: Sigval,
    group: Weak<Signals>,
    time: Arc<Timekeeper>,
    /// Cleared, with the group's signal state locked, once the timer is deleted.
    valid: AtomicBool,
    /// Whether the last signal sent came from a periodic timer; read and written with the
    /// group's signal state locked (`it_sig_periodic`).
    sig_periodic: AtomicBool,
    state: Mutex<State>,
    /// Calls [`PosixTimer::expired`] at the expiry.
    timer: Timer,
    /// The preallocated signal's charge to the creator's user, held for the timer's life.
    _charge: Charge,
}

/// The parameters of a new timer, before it has an id.
struct Spec {
    clock_id: ClockId,
    clock: Clock,
    cpu: Option<CpuClock>,
    sigev_none: bool,
    thread: Option<TaskId>,
    signo: i32,
    /// `None` for the default `sigev_value`, the timer id.
    value: Option<Sigval>,
}

fn ktime(d: Duration) -> Duration {
    d.min(KTIME_MAX)
}

/// The siginfo and `timer_getoverrun` fields are `int`.
fn overrun_to_int(overrun: i64) -> i32 {
    overrun.clamp(0, i64::from(i32::MAX)) as i32
}

impl PosixTimer {
    /// What `show_timer` shows: the id, the clock, how it notifies (`None` for
    /// `SIGEV_NONE`, whether to a thread), its signal, the value it carries, and the thread it
    /// signals (`None` for the process).
    pub(crate) fn proc_show(&self) -> (i32, i32, Option<bool>, i32, u32, Option<TaskId>) {
        let notify = (!self.sigev_none).then_some(self.thread.is_some());
        (
            self.id,
            self.clock_id.0,
            notify,
            self.signo,
            u32::from_le_bytes(self.value.0),
            self.thread,
        )
    }

    pub(crate) fn signo(&self) -> i32 {
        self.signo
    }

    pub(crate) fn target_thread(&self) -> Option<TaskId> {
        self.thread
    }

    pub(crate) fn is_valid(&self) -> bool {
        self.valid.load(Ordering::Relaxed)
    }

    pub(crate) fn invalidate(&self) {
        self.valid.store(false, Ordering::Relaxed);
    }

    pub(crate) fn sig_periodic(&self) -> bool {
        self.sig_periodic.load(Ordering::Relaxed)
    }

    pub(crate) fn set_sig_periodic(&self, periodic: bool) {
        self.sig_periodic.store(periodic, Ordering::Relaxed);
    }

    /// The timer's `siginfo`: `SI_TIMER`, its id and value. `si_overrun` is filled in at
    /// delivery.
    pub(crate) fn info(&self) -> Siginfo {
        let mut info = Siginfo {
            si_signo: self.signo,
            si_errno: 0,
            si_code: SiCode::SI_TIMER.0,
            sifields: [0; 116],
        };
        info.sifields[0..4].copy_from_slice(&self.id.to_le_bytes());
        info.sifields[8..12].copy_from_slice(&self.value.0);
        info
    }

    fn now(&self, base: Clock) -> Duration {
        time::read(&self.time, base, self.cpu.as_ref())
    }

    /// `expires` on `base` as a deadline on the host's monotonic clock, which kernel timers
    /// run on. An expiry on a settable clock is checked again when the timer fires, since
    /// that clock can move.
    fn monotonic_deadline(&self, base: Clock, expires: Duration) -> Duration {
        time::monotonic_deadline(&self.time, base, expires)
    }

    /// Whether the thread a thread CPU clock belongs to is still there.
    fn thread_alive(&self, group: &Signals) -> bool {
        let thread = self.cpu.as_ref().and_then(|cpu| cpu.thread.as_ref());
        thread.is_none_or(|(task, _)| group.has_task(*task))
    }

    /// Whether the timer's clock still runs: a CPU clock stops with its thread, and with its
    /// process's exit (`posix_cpu_timers_exit_group`, `task_can_enqueue_timer`).
    fn clock_alive(&self, group: &Signals) -> bool {
        let exiting = |cpu: &CpuClock| cpu.process.group.exiting();
        self.thread_alive(group) && !self.cpu.as_ref().is_some_and(exiting)
    }

    /// `hrtimer_forward`: moves an expired interval timer past `now`; returns the periods
    /// that went by.
    fn forward(state: &mut State, now: Duration) -> i64 {
        if state.expires > now || state.interval.is_zero() {
            return 0;
        }
        let (periods, next) = timer::forward(state.expires, state.interval, now);
        state.expires = next.map_or(KTIME_MAX, ktime);
        i64::try_from(periods).unwrap_or(i64::MAX)
    }

    /// `hrtimer_start_expires_user`, or for a CPU clock `arm_timer`: arms the timer for
    /// `expires`. Returns false when that has passed already, as the caller then queues the
    /// signal itself.
    fn start(self: &Arc<Self>, state: &mut State) -> bool {
        if state.expires <= self.now(state.base) {
            return false;
        }
        match &self.cpu {
            Some(cpu) => {
                self.cancel(state);
                let entry = cpu
                    .process
                    .arm_cpu_timer(cpu, state.expires, Arc::downgrade(self));
                state.cpu_entry = Some(entry);
            }
            None => self
                .timer
                .start(self.monotonic_deadline(state.base, state.expires)),
        }
        true
    }

    /// Stops the timer from expiring: takes it off its kernel timer or CPU clock.
    fn cancel(&self, state: &mut State) {
        self.timer.cancel();
        if let (Some(cpu), Some(entry)) = (&self.cpu, state.cpu_entry.take()) {
            cpu.process.disarm_cpu(entry);
        }
    }

    /// `posix_timer_queue_signal`
    fn queue_signal(self: &Arc<Self>, state: &mut State) {
        if !self.is_valid() {
            return;
        }
        let periodic = !state.interval.is_zero();
        state.status = match periodic {
            true => Status::RequeuePending,
            false => Status::Disarmed,
        };
        state.sigqueue_seq = state.signal_seq;
        if let Some(group) = self.group.upgrade() {
            group.send_timer(self, periodic);
        }
    }

    /// `cpu_timer_fire`: a charge took the timer's CPU clock past the expiry it was armed with
    /// as `entry`, which is off the clock now.
    pub(crate) fn cpu_expired(self: &Arc<Self>, entry: u64) {
        let mut state = self.state.lock_or_recover();
        if state.cpu_entry != Some(entry) || state.status != Status::Armed {
            return;
        }
        state.cpu_entry = None;
        // A CPU clock stops with its thread or process, and never reaches the expiry then.
        if !self.group.upgrade().is_some_and(|g| self.clock_alive(&g)) {
            state.status = Status::Disarmed;
            return;
        }
        self.queue_signal(&mut state);
    }

    /// `posix_timer_fn`, on the kernel timer's thread. Returns the monotonic deadline to
    /// check again at when the timer's clock has not reached the expiry yet.
    fn expired(self: &Arc<Self>) -> Option<Duration> {
        let mut state = self.state.lock_or_recover();
        if state.status != Status::Armed {
            return None;
        }
        // A CPU clock stops with its thread or process, and never reaches the expiry then.
        if !self.group.upgrade().is_some_and(|g| self.clock_alive(&g)) {
            state.status = Status::Disarmed;
            return None;
        }
        if self.now(state.base) < state.expires {
            return Some(self.monotonic_deadline(state.base, state.expires));
        }
        self.queue_signal(&mut state);
        None
    }

    /// `posixtimer_deliver_signal`, with the group's signal state unlocked: whether the
    /// dequeued signal still counts. A periodic timer re-arms here and reports the periods it
    /// missed in `si_overrun`.
    pub(crate) fn deliver(self: &Arc<Self>, info: &mut Siginfo) -> bool {
        let mut state = self.state.lock_or_recover();
        // Re-armed, disarmed or deleted since the signal was queued.
        if state.signal_seq != state.sigqueue_seq || !self.is_valid() {
            return false;
        }
        if state.interval.is_zero() || state.status != Status::RequeuePending {
            return true;
        }
        // `common_hrtimer_rearm`
        let now = self.now(state.base);
        let periods = Self::forward(&mut state, now);
        state.overrun = state.overrun.saturating_add(periods);
        let queued = self.start(&mut state);
        state.overrun_last = state.overrun;
        state.overrun = -1;
        state.signal_seq += 1;
        let overrun = overrun_to_int(state.overrun_last);
        info.sifields[4..8].copy_from_slice(&overrun.to_le_bytes());
        match queued {
            true => state.status = Status::Armed,
            false => self.queue_signal(&mut state),
        }
        true
    }

    /// `common_timer_get`: the time left and the interval. An expired interval timer whose
    /// signal is not delivered yet, and a `SIGEV_NONE` one, are moved past now first.
    fn get(&self, state: &mut State, cpu_now: Option<Duration>) -> KernelItimerspec {
        let mut setting = KernelItimerspec::default();
        if !state.interval.is_zero() {
            setting.it_interval = time::to_timespec(state.interval);
        } else if state.status == Status::Disarmed && !self.sigev_none {
            return setting;
        }
        let now = cpu_now.unwrap_or_else(|| self.now(state.base));
        if !state.interval.is_zero() && state.status != Status::Armed {
            let periods = Self::forward(state, now);
            state.overrun = state.overrun.saturating_add(periods);
        }
        match state
            .expires
            .checked_sub(now)
            .filter(|left| !left.is_zero())
        {
            Some(left) => setting.it_value = time::to_timespec(left),
            // Expired, but the signal is not delivered yet. An expired one-shot `SIGEV_NONE`
            // timer reports 0.
            None if !self.sigev_none => setting.it_value.tv_nsec = 1,
            None => {}
        }
        setting
    }

    /// `common_timer_set`: `disarm` for a zero value; an absolute `value` is the host's
    /// already (`timens_ktime_to_host`).
    fn set(
        self: &Arc<Self>,
        state: &mut State,
        absolute: bool,
        disarm: bool,
        value: Duration,
        interval: Duration,
        cpu_now: Option<Duration>,
    ) {
        self.cancel(state);
        state.status = Status::Disarmed;
        state.interval = match disarm {
            true => Duration::ZERO,
            false => ktime(interval),
        };
        state.overrun_last = 0;
        state.overrun = -1;
        if disarm {
            return;
        }
        // `common_hrtimer_arm`: a relative `CLOCK_REALTIME` timer runs on the monotonic clock.
        state.base = match (self.clock_id, absolute) {
            (ClockId::CLOCK_REALTIME, false) => Clock::Monotonic,
            _ => self.clock,
        };
        // posix_cpu_timer_set samples the CPU clock at arming, including the running
        // task's time since its last tick (kernel/time/posix-cpu-timers.c).
        let now = cpu_now.unwrap_or_else(|| self.now(state.base));
        state.expires = match absolute {
            true => ktime(value),
            false => ktime(now.saturating_add(value)),
        };
        // A `SIGEV_NONE` timer only pretends to be queued.
        if self.sigev_none {
            return;
        }
        match state.expires > now && self.start(state) {
            true => state.status = Status::Armed,
            false => self.queue_signal(state),
        }
    }

    /// `posix_timer_delete`: the timer stops and leaves the group. A signal of it still
    /// queued is dropped when it is dequeued.
    fn delete(self: &Arc<Self>, group: &Signals) -> Result<(), Errno> {
        let mut state = self.state.lock_or_recover();
        if !self.is_valid() {
            return Err(Errno::EINVAL);
        }
        state.signal_seq += 1;
        group.unlink_timer(self, self.id);
        self.cancel(&mut state);
        state.status = Status::Disarmed;
        Ok(())
    }
}

/// `exit_itimers`, when the thread group dies or execs: every POSIX timer goes.
pub(crate) fn exit_itimers(group: &Signals) {
    let timers: Vec<_> = group.with_posix_timers(|table| {
        table.restore_ids = false;
        table.timers.values().flatten().cloned().collect()
    });
    for timer in timers {
        // Only fails for a timer another thread just deleted.
        let _ = timer.delete(group);
    }
}

/// What `clockid_to_kclock` makes of `id` for a timer, before any thread is looked up.
fn timer_clock(id: ClockId) -> Result<(), Errno> {
    match id {
        ClockId::CLOCK_REALTIME
        | ClockId::CLOCK_MONOTONIC
        | ClockId::CLOCK_BOOTTIME
        | ClockId::CLOCK_TAI
        | ClockId::CLOCK_REALTIME_ALARM
        | ClockId::CLOCK_BOOTTIME_ALARM
        | ClockId::CLOCK_PROCESS_CPUTIME_ID
        | ClockId::CLOCK_THREAD_CPUTIME_ID => Ok(()),
        // Clocks without timers, and dynamic (`CLOCKFD`) clocks.
        ClockId::CLOCK_MONOTONIC_RAW
        | ClockId::CLOCK_REALTIME_COARSE
        | ClockId::CLOCK_MONOTONIC_COARSE => Err(Errno::EOPNOTSUPP),
        ClockId(raw) if raw < 0 && raw & 7 == 3 => Err(Errno::EOPNOTSUPP),
        // The remaining negative ids are CPU clocks, checked when the timer is made.
        ClockId(raw) if raw < 0 => Ok(()),
        _ => Err(Errno::EINVAL),
    }
}

impl Kernel {
    fn timer_group(&self, caller: &Caller<'_>) -> Arc<Signals> {
        self.task_signals(caller.task()).group().clone()
    }

    /// `lock_timer`: the caller's group's timer `id`.
    fn posix_timer(&self, caller: &Caller<'_>, id: TimerId) -> Result<Arc<PosixTimer>, Errno> {
        if id < 0 {
            return Err(Errno::EINVAL);
        }
        let timer = self
            .timer_group(caller)
            .with_posix_timers(|table| table.get(id));
        timer.filter(|timer| timer.is_valid()).ok_or(Errno::EINVAL)
    }

    /// `good_sigevent` and the defaults for a null `sigevent`: the notification, the target
    /// thread, the signal and its value.
    fn timer_notify(
        &self,
        caller: &Caller<'_>,
        event: Option<wasixv2_abi::Sigevent>,
    ) -> Result<(bool, Option<TaskId>, i32, Option<Sigval>), Errno> {
        let Some(event) = event else {
            return Ok((false, None, wasixv2_abi::Signal::SIGALRM.0, None));
        };
        let valid_signo = (1..=SIGRTMAX).contains(&event.sigev_signo);
        let thread = match SigevNotify(event.sigev_notify) {
            SigevNotify::SIGEV_NONE => {
                return Ok((true, None, event.sigev_signo, Some(event.sigev_value)));
            }
            SigevNotify::SIGEV_SIGNAL | SigevNotify::SIGEV_THREAD => None,
            SigevNotify::SIGEV_THREAD_ID => {
                let tid = event.sigev_un.tid();
                Some(self.same_thread_group(caller, tid).ok_or(Errno::EINVAL)?)
            }
            _ => return Err(Errno::EINVAL),
        };
        if !valid_signo {
            return Err(Errno::EINVAL);
        }
        Ok((false, thread, event.sigev_signo, Some(event.sigev_value)))
    }

    /// `kc->timer_create`: the alarm clocks need `CAP_WAKE_ALARM` (`alarm_timer_create`),
    /// and run as their base clocks, as a machine never suspends; a CPU clock must name a
    /// process, or a thread of the caller's group (`posix_cpu_timer_create`).
    fn timer_spec_clock(&self, caller: &Caller<'_>, id: ClockId) -> Result<ClockRef, Errno> {
        if matches!(
            id,
            ClockId::CLOCK_REALTIME_ALARM | ClockId::CLOCK_BOOTTIME_ALARM
        ) && !self.cred(caller).capable_init(Cap::WakeAlarm)
        {
            return Err(Errno::EPERM);
        }
        self.resolve_clock(caller, id, false)
    }

    pub(crate) async fn sys_timer_create(
        &self,
        caller: &Caller<'_>,
        which_clock: ClockId,
        event: ConstSigeventPtr,
        created: TimerIdPtr,
    ) -> SysResult<i32> {
        let event = match event.is_null() {
            true => None,
            false => Some(read_value(caller.memory(), event).await?),
        };
        timer_clock(which_clock)?;
        let group = self.timer_group(caller);
        let requested = match group.with_posix_timers(|table| table.restore_ids) {
            true => {
                let id =
                    read_value(caller.memory(), GuestPtr::<TimerId>::new(created.addr())).await?;
                if id < 0 {
                    return Err(Errno::EINVAL.into());
                }
                Some(id)
            }
            false => None,
        };
        // Every timer holds a preallocated signal, which counts against the caller's user's
        // RLIMIT_SIGPENDING with the signals queued for it (`posixtimer_init_sigqueue`).
        let charge = {
            let process = self.process(caller.task());
            let limit = process.resources.lock_or_recover().sigpending();
            let uid = self.cred(caller).user.real;
            self.sigqueue_users
                .charge(uid, Some(limit))
                .map_err(|_| Errno::EAGAIN)?
        };
        let id = group.with_posix_timers(|table| table.reserve(requested))?;
        let made = self
            .make_posix_timer(caller, &group, id, which_clock, event, created, charge)
            .await;
        group.with_posix_timers(|table| match &made {
            Ok(timer) => {
                table.timers.insert(id, Some(timer.clone()));
            }
            Err(_) => table.remove(id),
        });
        made.map(|_| 0)
    }

    /// The part of `do_timer_create` between reserving the id and publishing the timer.
    #[allow(clippy::too_many_arguments)]
    async fn make_posix_timer(
        &self,
        caller: &Caller<'_>,
        group: &Arc<Signals>,
        id: TimerId,
        clock_id: ClockId,
        event: Option<wasixv2_abi::Sigevent>,
        created: TimerIdPtr,
        charge: Charge,
    ) -> SysResult<Arc<PosixTimer>> {
        let (sigev_none, thread, signo, value) = self.timer_notify(caller, event)?;
        write_value(caller.memory(), created, &id).await?;
        let clock = self.timer_spec_clock(caller, clock_id)?;
        let spec = Spec {
            clock_id,
            clock: clock.clock,
            cpu: clock.cpu,
            sigev_none,
            thread,
            signo,
            value,
        };
        Ok(self.new_posix_timer(group, id, spec, charge))
    }

    fn new_posix_timer(
        &self,
        group: &Arc<Signals>,
        id: TimerId,
        spec: Spec,
        charge: Charge,
    ) -> Arc<PosixTimer> {
        Arc::new_cyclic(|weak: &Weak<PosixTimer>| {
            let weak = weak.clone();
            let expired = move || weak.upgrade().and_then(|timer| timer.expired());
            PosixTimer {
                id,
                clock_id: spec.clock_id,
                clock: spec.clock,
                cpu: spec.cpu,
                sigev_none: spec.sigev_none,
                thread: spec.thread,
                signo: spec.signo,
                value: spec.value.unwrap_or(Sigval(id.to_le_bytes())),
                group: Arc::downgrade(group),
                time: self.time.clone(),
                valid: AtomicBool::new(true),
                sig_periodic: AtomicBool::new(false),
                state: Mutex::new(State {
                    status: Status::Disarmed,
                    base: spec.clock,
                    expires: Duration::ZERO,
                    interval: Duration::ZERO,
                    overrun: -1,
                    overrun_last: 0,
                    signal_seq: 0,
                    sigqueue_seq: 0,
                    cpu_entry: None,
                }),
                timer: match spec.clock.settable() {
                    true => self.timers.realtime_timer(Box::new(expired)),
                    false => self.timers.timer(Box::new(expired)),
                },
                _charge: charge,
            }
        })
    }

    pub(crate) async fn sys_timer_settime(
        &self,
        caller: &Caller<'_>,
        id: TimerId,
        flags: TimerFlags,
        new: ConstKernelItimerspecPtr,
        old: KernelItimerspecPtr,
    ) -> SysResult<i32> {
        if new.is_null() {
            return Err(Errno::EINVAL.into());
        }
        let new = read_value(caller.memory(), new).await?;
        let interval = time::from_timespec(new.it_interval)?;
        let value = time::from_timespec(new.it_value)?;
        let timer = self.posix_timer(caller, id)?;
        let absolute = flags.contains(TimerFlags::TIMER_ABSTIME);
        let disarm = value.is_zero();
        let value = match absolute && !disarm {
            true => self.timens_to_host(caller, timer.clock, value),
            false => value,
        };
        let account = timer
            .cpu
            .as_ref()
            .and_then(|cpu| self.cpu_clock_account(caller.task(), cpu));
        let previous = {
            let mut state = timer.state.lock_or_recover();
            if !timer.is_valid() {
                return Err(Errno::EINVAL.into());
            }
            // `posix_cpu_timer_set`: the thread of a thread CPU clock is gone, or the process
            // of a process CPU clock was reaped.
            let group = self.timer_group(caller);
            let reaped = |p: &Arc<Process>| {
                self.procs
                    .find(p.pid.0)
                    .is_none_or(|listed| !Arc::ptr_eq(&listed, p))
            };
            let reaped = |cpu: &CpuClock| reaped(&cpu.process);
            if !timer.thread_alive(&group) || timer.cpu.as_ref().is_some_and(reaped) {
                return Err(Errno::ESRCH.into());
            }
            // A signal still queued from the old setting is stale now.
            state.signal_seq += 1;
            let cpu_now = timer
                .cpu
                .as_ref()
                .map(|cpu| cpu.sample(&*self.host, account.as_deref()));
            let previous = timer.get(&mut state, cpu_now);
            timer.set(&mut state, absolute, disarm, value, interval, cpu_now);
            previous
        };
        if !old.is_null() {
            write_value(caller.memory(), old, &previous).await?;
        }
        Ok(0)
    }

    pub(crate) async fn sys_timer_gettime(
        &self,
        caller: &Caller<'_>,
        id: TimerId,
        setting: KernelItimerspecPtr,
    ) -> SysResult<i32> {
        let timer = self.posix_timer(caller, id)?;
        // posix_cpu_timer_get samples the current clock, rather than returning the
        // last accounting tick's remaining time (kernel/time/posix-cpu-timers.c).
        let account = timer
            .cpu
            .as_ref()
            .and_then(|cpu| self.cpu_clock_account(caller.task(), cpu));
        let current = {
            let mut state = timer.state.lock_or_recover();
            let cpu_now = timer
                .cpu
                .as_ref()
                .map(|cpu| cpu.sample(&*self.host, account.as_deref()));
            timer.get(&mut state, cpu_now)
        };
        write_value(caller.memory(), setting, &current).await?;
        Ok(0)
    }

    pub(crate) fn sys_timer_getoverrun(&self, caller: &Caller<'_>, id: TimerId) -> SysResult<i32> {
        let timer = self.posix_timer(caller, id)?;
        let overrun = timer.state.lock_or_recover().overrun_last;
        Ok(overrun_to_int(overrun))
    }

    pub(crate) fn sys_timer_delete(&self, caller: &Caller<'_>, id: TimerId) -> SysResult<i32> {
        let timer = self.posix_timer(caller, id)?;
        timer.delete(&self.timer_group(caller))?;
        Ok(0)
    }

    /// `prctl(PR_TIMER_CREATE_RESTORE_IDS, ...)`.
    pub(crate) fn timer_create_prctl(
        &self,
        caller: &Caller<'_>,
        ctrl: Ulong,
        rest: [Ulong; 3],
    ) -> SysResult<i32> {
        if rest != [0; 3] {
            return Err(Errno::EINVAL.into());
        }
        let group = self.timer_group(caller);
        group.with_posix_timers(|table| match PrctlArg(ctrl as _) {
            PrctlArg::PR_TIMER_CREATE_RESTORE_IDS_OFF => {
                table.restore_ids = false;
                Ok(0)
            }
            PrctlArg::PR_TIMER_CREATE_RESTORE_IDS_ON => {
                table.restore_ids = true;
                Ok(0)
            }
            PrctlArg::PR_TIMER_CREATE_RESTORE_IDS_GET => Ok(table.restore_ids as i32),
            _ => Err(Errno::EINVAL.into()),
        })
    }
}
