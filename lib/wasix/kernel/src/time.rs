//! Clocks, sleeps and their timespec conversions, after Linux's `kernel/time/posix-timers.c`
//! and, for the CPU-time clocks of any process or thread, `posix-cpu-timers.c`. The wall clock
//! is the machine's ([`Timekeeper`]); the alarm clocks (`alarmtimer.c`) are their base clocks,
//! as a machine never suspends.

use std::{sync::Arc, time::Duration};

use wasixv2_abi::{
    ClockId, ConstKernelTimespecPtr, GuestPtr, KernelTimespec, KernelTimespecPtr, KernelTimex,
    KernelTimexPtr, TimerFlags,
};
use wasixv2_host::Host;
use wasixv2_syscalls::{
    Caller, CpuBase, CpuTime, Errno, SysErr, SysResult, TaskId, read_value, write_value,
};
use wasixv2_vfs::Cap;
use wasixv2_wait::{Interrupted, wait_event};

use crate::{
    Kernel,
    cputime::{CpuClock, CpuSleep},
    restart::Restart,
    timekeeping::{self, Timekeeper},
};

const NANOS_PER_SEC: i64 = 1_000_000_000;
/// Resolution of the coarse clocks: one tick at `HZ=250`.
const COARSE_RES: Duration = Duration::from_millis(4);
/// `CPUCLOCK_SCHED`, the CPU clock `clock_getcpuclockid` and `pthread_getcpuclockid` build.
/// `CPUCLOCK_PROF` (0) and `CPUCLOCK_VIRT` (1) count in ticks; 3 is `CPUCLOCK_MAX`.
const CPUCLOCK_SCHED: i32 = 2;
const CPUCLOCK_VIRT: i32 = 1;
const CPUCLOCK_MAX: i32 = 3;

/// What a clock id reads.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Clock {
    Realtime,
    /// `CLOCK_TAI`: the wall clock plus the TAI offset.
    Tai,
    /// `CLOCK_MONOTONIC` and its raw and coarse forms, which a time namespace offsets alike.
    Monotonic,
    /// `CLOCK_BOOTTIME`: the monotonic clock, as a machine never suspends, with a time
    /// namespace offset of its own.
    Boottime,
    /// CPU time of a process or one of its threads, as the execution layer charges it
    /// ([`crate::cputime`]).
    ProcessCpu,
    ThreadCpu,
}

/// The owner a CPU clock id names: `CPUCLOCK_PID` (0 for the caller) and
/// `CPUCLOCK_PERTHREAD`.
#[derive(Clone, Copy)]
struct CpuOwner {
    pid: i32,
    thread: bool,
    which: i32,
}

/// Decodes `id` (`clockid_to_kclock`), including the per-pid and per-thread CPU clock ids
/// `clock_getcpuclockid` and `pthread_getcpuclockid` build (`CPUCLOCK_PID`,
/// `CPUCLOCK_PERTHREAD`, `CPUCLOCK_WHICH`). The caller's own CPU clocks are those of pid 0.
fn decode(id: ClockId) -> Result<(Clock, Option<CpuOwner>), Errno> {
    let own = |thread| CpuOwner {
        pid: 0,
        thread,
        which: CPUCLOCK_SCHED,
    };
    let clock = match id {
        ClockId::CLOCK_REALTIME
        | ClockId::CLOCK_REALTIME_COARSE
        | ClockId::CLOCK_REALTIME_ALARM => Clock::Realtime,
        ClockId::CLOCK_TAI => Clock::Tai,
        ClockId::CLOCK_MONOTONIC
        | ClockId::CLOCK_MONOTONIC_RAW
        | ClockId::CLOCK_MONOTONIC_COARSE => Clock::Monotonic,
        ClockId::CLOCK_BOOTTIME | ClockId::CLOCK_BOOTTIME_ALARM => Clock::Boottime,
        ClockId::CLOCK_PROCESS_CPUTIME_ID => return Ok((Clock::ProcessCpu, Some(own(false)))),
        ClockId::CLOCK_THREAD_CPUTIME_ID => return Ok((Clock::ThreadCpu, Some(own(true)))),
        // Dynamic (`CLOCKFD`) clocks come out as `CPUCLOCK_MAX`, which no clock has.
        ClockId(raw) if raw < 0 => {
            let owner = CpuOwner {
                pid: !(raw >> 3),
                thread: raw & 4 != 0,
                which: raw & 3,
            };
            if owner.which >= CPUCLOCK_MAX {
                return Err(Errno::EINVAL);
            }
            let clock = match owner.thread {
                true => Clock::ThreadCpu,
                false => Clock::ProcessCpu,
            };
            return Ok((clock, Some(owner)));
        }
        _ => return Err(Errno::EINVAL),
    };
    Ok((clock, None))
}

/// A clock id resolved for its caller: what it reads, and for a CPU clock whose.
#[derive(Clone)]
pub(crate) struct ClockRef {
    pub(crate) clock: Clock,
    /// For a CPU clock, whose CPU time it counts (`it.cpu.pid`, held as Linux holds the pid),
    /// a process or a thread of the caller's thread group, and which part.
    pub(crate) cpu: Option<CpuClock>,
    /// `CPUCLOCK_WHICH` of a CPU clock.
    which: i32,
}

/// `clock_getres`: the coarse clocks and the tick-based CPU clocks (`CPUCLOCK_PROF`,
/// `CPUCLOCK_VIRT`) count in ticks, everything else in nanoseconds.
fn resolution(id: ClockId, clock: &ClockRef) -> Duration {
    match id {
        ClockId::CLOCK_REALTIME_COARSE | ClockId::CLOCK_MONOTONIC_COARSE => COARSE_RES,
        _ if clock.cpu.is_some() && clock.which != CPUCLOCK_SCHED => COARSE_RES,
        _ => Duration::from_nanos(1),
    }
}

pub(crate) fn to_timespec(time: Duration) -> KernelTimespec {
    KernelTimespec {
        tv_sec: time.as_secs() as i64,
        tv_nsec: time.subsec_nanos().into(),
    }
}

/// A timespec the guest passed in. Only the low 32 bits of `tv_nsec` count on 32-bit ABIs, and
/// a negative or unnormalized value is `EINVAL`.
pub(crate) fn from_timespec(ts: KernelTimespec) -> Result<Duration, Errno> {
    let nanos = i64::from(ts.tv_nsec as i32);
    if ts.tv_sec < 0 || !(0..NANOS_PER_SEC).contains(&nanos) {
        return Err(Errno::EINVAL);
    }
    Ok(Duration::new(ts.tv_sec as u64, nanos as u32))
}

impl Clock {
    /// Whether the clock can be set, so that timers on it are realtime timers
    /// ([`crate::timer::Timers::realtime_timer`]) and its deadlines move with a set.
    pub(crate) fn settable(self) -> bool {
        matches!(self, Clock::Realtime | Clock::Tai)
    }
}

/// What `clock` reads outside any time namespace; `cpu` is the CPU clock a CPU clock is,
/// which reads as of the last charges.
pub(crate) fn read(time: &Timekeeper, clock: Clock, cpu: Option<&CpuClock>) -> Duration {
    match clock {
        Clock::Realtime => time.realtime(),
        Clock::Tai => time.tai(),
        Clock::Monotonic | Clock::Boottime => time.monotonic(),
        Clock::ProcessCpu | Clock::ThreadCpu => cpu.map_or(Duration::ZERO, CpuClock::now),
    }
}

/// A deadline on the settable `clock` as one on the host's monotonic clock, which kernel
/// timers run on; a set of the clock moves it ([`crate::Kernel::clock_was_set`]).
pub(crate) fn monotonic_deadline(time: &Timekeeper, clock: Clock, deadline: Duration) -> Duration {
    match clock.settable() {
        true => {
            let left = deadline.saturating_sub(read(time, clock, None));
            time.monotonic().saturating_add(left)
        }
        false => deadline,
    }
}

/// `timens_add_*`: `time` plus a time namespace's `offset` in nanoseconds.
pub(crate) fn timens_add(time: Duration, offset: i64) -> Duration {
    let nanos = time.as_nanos() as i128 + i128::from(offset);
    duration_from_nanos(nanos)
}

/// `do_timens_ktime_to_host`: an absolute time `at` in a time namespace whose offset is
/// `offset` as the host's; one before the offset has passed already (0), and the result is at
/// most `KTIME_MAX`.
pub(crate) fn timens_to_host(at: Duration, offset: i64) -> Duration {
    let at = at.as_nanos() as i128;
    let offset = i128::from(offset);
    if at < offset {
        return Duration::ZERO;
    }
    duration_from_nanos((at - offset).min(i128::from(i64::MAX)))
}

fn duration_from_nanos(nanos: i128) -> Duration {
    let nanos = nanos.max(0);
    let secs = u64::try_from(nanos / i128::from(NANOS_PER_SEC)).unwrap_or(u64::MAX);
    Duration::new(secs, (nanos % i128::from(NANOS_PER_SEC)) as u32)
}

/// What a reading of [`ClockOffset`] may be off by beyond its bracket: the coarsest host wall
/// clock counts milliseconds (a browser's `Date.now`). A set by less goes unnoticed.
const OFFSET_SLACK: i128 = 1_000_000;

/// The realtime clock's offset from the monotonic one (`ktime_mono_to_real(0)`), in
/// nanoseconds, as the host lets it be read: one realtime reading between two monotonic ones
/// gives a range, with the count of the machine's own sets of the clock. The offset moves
/// when the machine sets the clock, by a step or a leap second (slewing moves both clocks
/// alike), and when the host's wall clock is stepped or the host slept, which the machine is
/// not told of and sees by comparing readings: the events after which Linux runs
/// `clock_was_set`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ClockOffset {
    low: i128,
    high: i128,
    seq: u64,
}

impl ClockOffset {
    pub(crate) fn read(time: &Timekeeper) -> Self {
        let seq = time.seq();
        let before = time.monotonic().as_nanos() as i128;
        let real = time.realtime().as_nanos() as i128;
        let after = time.monotonic().as_nanos() as i128;
        Self {
            low: real - after,
            high: real - before,
            seq,
        }
    }

    /// Whether the realtime clock was set between this reading and `now`.
    pub(crate) fn moved(&self, now: &Self) -> bool {
        now.seq != self.seq
            || now.low > self.high + OFFSET_SLACK
            || now.high + OFFSET_SLACK < self.low
    }
}

impl Kernel {
    /// `hrtimer_nanosleep` and `futex_setup_timer`: the caller's allowed expiry range.
    pub(crate) fn timer_slack(&self, caller: &Caller<'_>) -> Duration {
        Duration::from_nanos(self.with_task(caller.task(), |t| t.sched.timer_slack()))
    }

    /// Resolves `id` for the caller, as `clock_gettime` does: a process CPU clock may also
    /// name the process by the caller's own thread id (`pid_for_clock` with `gettime`).
    pub(crate) fn clock(&self, caller: &Caller<'_>, id: ClockId) -> Result<ClockRef, Errno> {
        self.resolve_clock(caller, id, true)
    }

    /// Resolves `id` for the caller. A CPU clock names the caller (pid 0), a thread of its
    /// thread group, or any process, a zombie included, by its pid; anything else is `EINVAL`
    /// (`pid_for_clock`, `validate_clock_permissions`). Reading another process's CPU time
    /// needs no permission on Linux.
    pub(crate) fn resolve_clock(
        &self,
        caller: &Caller<'_>,
        id: ClockId,
        gettime: bool,
    ) -> Result<ClockRef, Errno> {
        let (clock, owner) = decode(id)?;
        let Some(owner) = owner else {
            return Ok(ClockRef {
                clock,
                cpu: None,
                which: 0,
            });
        };
        let me = self.process(caller.task());
        let (process, thread) = match (owner.pid, owner.thread) {
            (0, true) => (me, Some(caller.task())),
            (0, false) => (me, None),
            // A thread clock names a thread of the caller's own group.
            (tid, true) => match self.same_thread_group(caller, tid) {
                Some(thread) => (me, Some(thread)),
                None => return Err(Errno::EINVAL),
            },
            (pid, false) if gettime && pid == self.own_tid(caller.task()) => (me, None),
            // A process clock names a thread group by its leader's pid, in the caller's pid
            // namespace.
            (pid, false) => {
                let id = self.find_vpid(caller, pid).ok_or(Errno::EINVAL)?;
                match self.procs.find(id) {
                    Some(process) if process.pid.0 == id => (process, None),
                    _ => return Err(Errno::EINVAL),
                }
            }
        };
        // The clock holds the thread's account, as Linux holds the pid: it reads the time the
        // thread was charged with even after it exited.
        let thread = thread.map(|tid| (tid, self.cpu_account(tid)));
        let base = match owner.which {
            CPUCLOCK_VIRT => CpuBase::Virt,
            _ => CpuBase::Prof,
        };
        Ok(ClockRef {
            clock,
            cpu: Some(CpuClock {
                process,
                thread,
                base,
            }),
            which: owner.which,
        })
    }

    /// The live thread of the caller's thread group its `tid` names (`same_thread_group`).
    pub(crate) fn same_thread_group(&self, caller: &Caller<'_>, tid: i32) -> Option<TaskId> {
        let id = TaskId(self.find_vpid(caller, tid)?);
        let state = self.state.read_or_recover();
        let me = &state.tasks[&caller.task()];
        let thread = state.tasks.get(&id)?;
        (Arc::ptr_eq(&thread.process, &me.process) && !thread.thread.exited()).then_some(id)
    }

    /// The current value of `clock` for the caller, who sees its own CPU time up to now
    /// ([`Kernel::cpu_clock_now`]) and the monotonic and boot-time clocks with its time
    /// namespace's offsets (`timens_add_monotonic`, `timens_add_boottime`).
    pub(crate) fn clock_now(&self, caller: &Caller<'_>, clock: &ClockRef) -> Duration {
        match &clock.cpu {
            Some(cpu) => self.cpu_clock_now(caller.task(), cpu),
            None => {
                let now = read(&self.time, clock.clock, None);
                timens_add(now, self.timens_offset(caller, clock.clock))
            }
        }
    }

    /// The offset in nanoseconds the caller's time namespace gives `clock`.
    pub(crate) fn timens_offset(&self, caller: &Caller<'_>, clock: Clock) -> i64 {
        self.task_timens_offset(caller.task(), clock)
    }

    /// [`Kernel::timens_offset`] of `task`.
    pub(crate) fn task_timens_offset(&self, task: TaskId, clock: Clock) -> i64 {
        let (monotonic, boottime) = match clock {
            Clock::Monotonic | Clock::Boottime => self.task_ns(task).time.offsets(),
            _ => return 0,
        };
        match clock {
            Clock::Monotonic => monotonic,
            _ => boottime,
        }
    }

    /// `timens_ktime_to_host`: an absolute time on the caller's `clock` as the host's.
    pub(crate) fn timens_to_host(
        &self,
        caller: &Caller<'_>,
        clock: Clock,
        at: Duration,
    ) -> Duration {
        timens_to_host(at, self.timens_offset(caller, clock))
    }

    /// `clock_was_set`: after a set of the wall clock (or of the TAI offset), the machine's
    /// realtime timers fire or move to their new deadlines (`hrtimers_resume`/
    /// `retrigger_next_event`), and a timer descriptor that cancels on a set is canceled.
    /// The absolute waits on the wall clock already woke ([`Timekeeper`]).
    pub(crate) fn clock_was_set(&self) {
        self.timers.clock_was_set();
    }

    pub(crate) async fn sys_clock_gettime(
        &self,
        caller: &Caller<'_>,
        id: ClockId,
        tp: KernelTimespecPtr,
    ) -> SysResult<i32> {
        let now = self.clock_now(caller, &self.clock(caller, id)?);
        write_value(caller.memory(), tp, &to_timespec(now)).await?;
        Ok(0)
    }

    pub(crate) async fn sys_clock_getres(
        &self,
        caller: &Caller<'_>,
        id: ClockId,
        tp: KernelTimespecPtr,
    ) -> SysResult<i32> {
        let clock = self.resolve_clock(caller, id, false)?;
        if !tp.is_null() {
            write_value(caller.memory(), tp, &to_timespec(resolution(id, &clock))).await?;
        }
        Ok(0)
    }

    /// Only `CLOCK_REALTIME` and the CPU clocks named by a pid have a `clock_set`; the others
    /// are `EINVAL` before the timespec is read. The wall clock is set as
    /// `do_sys_settimeofday64` does: a time past what it can hold is `EINVAL`, then setting
    /// it needs `CAP_SYS_TIME` in the initial user namespace (`cap_settime`). It is the
    /// machine's, not the host's.
    pub(crate) async fn sys_clock_settime(
        &self,
        caller: &Caller<'_>,
        id: ClockId,
        tp: ConstKernelTimespecPtr,
    ) -> SysResult<i32> {
        let (_, cpu) = decode(id)?;
        let pid_clock = id.0 < 0 && cpu.is_some();
        if id != ClockId::CLOCK_REALTIME && !pid_clock {
            return Err(Errno::EINVAL.into());
        }
        let ts = read_value(caller.memory(), tp).await?;
        if pid_clock {
            // "You can never reset a CPU clock" (`posix_cpu_clock_set`), once it names one.
            self.resolve_clock(caller, id, false)?;
            return Err(Errno::EPERM.into());
        }
        let ts = from_timespec(ts)?;
        if !timekeeping::valid_settod(ts) {
            return Err(Errno::EINVAL.into());
        }
        if !self.cred(caller).capable_init(Cap::SysTime) {
            return Err(Errno::EPERM.into());
        }
        self.time.settime(ts)?;
        self.clock_was_set();
        Ok(0)
    }

    /// `clock_adjtime`: `adjtimex` of the wall clock (`do_adjtimex`); the other clocks have
    /// no `clock_adj` (`EOPNOTSUPP`). The whole `timex` goes back to the caller, its padding
    /// as the caller passed it.
    pub(crate) async fn sys_clock_adjtime(
        &self,
        caller: &Caller<'_>,
        id: ClockId,
        utx: KernelTimexPtr,
    ) -> SysResult<i32> {
        let mut txc = read_value(caller.memory(), GuestPtr::<KernelTimex>::new(utx.addr())).await?;
        decode(id)?;
        if id != ClockId::CLOCK_REALTIME {
            return Err(Errno::EOPNOTSUPP.into());
        }
        let sys_time = self.cred(caller).capable_init(Cap::SysTime);
        let (state, set) = self.time.adjtimex(&mut txc, sys_time)?;
        if set {
            self.clock_was_set();
        }
        write_value(caller.memory(), utx, &txc).await?;
        Ok(state.0)
    }

    pub(crate) async fn sys_clock_nanosleep(
        &self,
        caller: &Caller<'_>,
        id: ClockId,
        flags: TimerFlags,
        rqtp: ConstKernelTimespecPtr,
        rmtp: KernelTimespecPtr,
    ) -> SysResult<i32> {
        decode(id)?;
        // The clocks without `nsleep`.
        if let ClockId::CLOCK_THREAD_CPUTIME_ID
        | ClockId::CLOCK_MONOTONIC_RAW
        | ClockId::CLOCK_REALTIME_COARSE
        | ClockId::CLOCK_MONOTONIC_COARSE = id
        {
            return Err(Errno::EOPNOTSUPP.into());
        }
        let clock = self.resolve_clock(caller, id, false)?;
        let alarm = matches!(
            id,
            ClockId::CLOCK_REALTIME_ALARM | ClockId::CLOCK_BOOTTIME_ALARM
        );
        // `posix_cpu_nsleep`: a thread cannot sleep on its own CPU clock.
        let own = |cpu: &CpuClock| {
            cpu.thread
                .as_ref()
                .is_some_and(|(t, _)| *t == caller.task())
        };
        if clock.cpu.as_ref().is_some_and(own) {
            return Err(Errno::EINVAL.into());
        }
        let request = from_timespec(read_value(caller.memory(), rqtp).await?)?;
        // `alarm_timer_nsleep`: an alarm sleep needs `CAP_WAKE_ALARM`.
        if alarm {
            if flags.0 & !TimerFlags::TIMER_ABSTIME.0 != 0 {
                return Err(Errno::EINVAL.into());
            }
            if !self.cred(caller).capable_init(Cap::WakeAlarm) {
                return Err(Errno::EPERM.into());
            }
        }
        let absolute = flags.contains(TimerFlags::TIMER_ABSTIME);
        if let Some(cpu) = &clock.cpu {
            let expires = match absolute {
                true => request,
                false => cpu.now().saturating_add(request),
            };
            let rmtp = (!absolute).then_some(rmtp);
            return self.cpu_nanosleep(caller, id, cpu, expires, rmtp).await;
        }
        if !absolute {
            // Every clock advances at the monotonic clock's rate, so a relative sleep ends at
            // a fixed point of it.
            let deadline = self.host.monotonic().saturating_add(request);
            let slack = if alarm {
                Duration::ZERO
            } else {
                self.timer_slack(caller)
            };
            return self.nanosleep_range(caller, deadline, slack, rmtp).await;
        }
        // Interrupted, an absolute sleep reruns as it was, and `rmtp` is not written
        // (`-ERESTARTNOHAND`).
        let interrupted = |_: Interrupted| SysErr::RestartNoHand;
        // `alarm_timer_nsleep` does not use hrtimer_nanosleep's task slack.
        let slack = if alarm {
            Duration::ZERO
        } else {
            self.timer_slack(caller)
        };
        let sleep = match clock.clock.settable() {
            // A set of the clock moves the deadline.
            true => self
                .time
                .sleep_until_with_slack(request, clock.clock == Clock::Tai, slack),
            false => {
                let request = self.timens_to_host(caller, clock.clock, request);
                self.host.sleep_until_range(request, slack)
            }
        };
        self.interruptible_sleep(caller, sleep)
            .await
            .map_err(interrupted)?;
        Ok(0)
    }

    /// `do_cpu_nanosleep`: sleeps until the CPU clock `id`, `cpu`, reaches `expires`.
    /// Interrupted, an absolute sleep (no `rmtp`) reruns as it was; a relative one writes the
    /// time left to `rmtp` and goes on through `restart_syscall` as an absolute sleep on the
    /// same clock (`posix_cpu_nsleep_restart`), which looks the clock up again.
    pub(crate) async fn cpu_nanosleep(
        &self,
        caller: &Caller<'_>,
        id: ClockId,
        cpu: &CpuClock,
        expires: Duration,
        rmtp: Option<KernelTimespecPtr>,
    ) -> SysResult<i32> {
        if self.cpu_sleep(caller, cpu, expires).await.is_ok() {
            return Ok(0);
        }
        let Some(rmtp) = rmtp else {
            return Err(SysErr::RestartNoHand);
        };
        if !rmtp.is_null() {
            let left = expires.saturating_sub(cpu.now());
            write_value(caller.memory(), rmtp, &to_timespec(left)).await?;
        }
        let restart = Restart::CpuNanosleep {
            clock: id,
            expires,
            rmtp,
        };
        Err(self.restart_with(caller, restart))
    }

    /// Waits until `cpu` reads `expires`, as a timer on the clock does: a charge to its owner
    /// that passes the expiry ends the wait. The clock of a task or process that exited stops,
    /// after which only a signal ends the wait, as no CPU timer of a dead process fires on
    /// Linux.
    async fn cpu_sleep(
        &self,
        caller: &Caller<'_>,
        cpu: &CpuClock,
        expires: Duration,
    ) -> Result<(), Interrupted> {
        let interrupt = self.interrupt(caller);
        let _sleep = CpuSleep::new(cpu, expires);
        let over = || (cpu.now() >= expires).then_some(());
        wait_event(&cpu.process.cpu_timers.woken, over, &*interrupt).await
    }

    /// Restarts a relative `clock_nanosleep` at its saved hard expiry on the monotonic
    /// clock. Interrupted again, it writes the time left to `rmtp` (`nanosleep_copyout`).
    pub(crate) async fn nanosleep_until(
        &self,
        caller: &Caller<'_>,
        deadline: Duration,
        rmtp: KernelTimespecPtr,
    ) -> SysResult<i32> {
        // `hrtimer_nanosleep_restart`: the saved hard expiry gets no additional slack.
        self.nanosleep_range(caller, deadline, Duration::ZERO, rmtp)
            .await
    }

    async fn nanosleep_range(
        &self,
        caller: &Caller<'_>,
        deadline: Duration,
        slack: Duration,
        rmtp: KernelTimespecPtr,
    ) -> SysResult<i32> {
        let sleep = self.host.sleep_until_range(deadline, slack);
        if self.interruptible_sleep(caller, sleep).await.is_ok() {
            return Ok(0);
        }
        // `do_nanosleep` reports hrtimer_expires_remaining; the restart block also
        // retains the hard expiry (kernel/time/hrtimer.c hrtimer_nanosleep).
        let deadline = deadline.saturating_add(slack);
        if !rmtp.is_null() {
            let left = deadline.saturating_sub(self.host.monotonic());
            write_value(caller.memory(), rmtp, &to_timespec(left)).await?;
        }
        Err(self.restart_with(caller, Restart::Nanosleep { deadline, rmtp }))
    }
}
