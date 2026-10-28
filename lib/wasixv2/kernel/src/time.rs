//! Clocks, sleeps and their timespec conversions, after Linux's `kernel/time/posix-timers.c`
//! and, for the CPU-time clocks of any process or thread, `posix-cpu-timers.c`.

use std::{sync::Arc, time::Duration};

use wasixv2_abi::{ClockId, ConstKernelTimespecPtr, KernelTimespec, KernelTimespecPtr, TimerFlags};
use wasixv2_host::Host;
use wasixv2_syscalls::{
    Caller, CpuBase, CpuTime, Errno, SysErr, SysResult, TaskId, read_value, write_value,
};
use wasixv2_wait::{Interrupted, wait_event};

use crate::{
    Kernel,
    cputime::{CpuClock, CpuSleep},
    restart::Restart,
};

const NANOS_PER_SEC: i64 = 1_000_000_000;
/// `CLOCK_TAI`: the kernel's TAI offset is 0 until something sets it, as on a fresh Linux boot.
const CLOCK_TAI: ClockId = ClockId(11);
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
    Monotonic,
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
        | ClockId::CLOCK_REALTIME_ALARM
        | CLOCK_TAI => Clock::Realtime,
        ClockId::CLOCK_MONOTONIC
        | ClockId::CLOCK_MONOTONIC_RAW
        | ClockId::CLOCK_MONOTONIC_COARSE
        | ClockId::CLOCK_BOOTTIME
        | ClockId::CLOCK_BOOTTIME_ALARM => Clock::Monotonic,
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

/// What `clock` reads; `cpu` is the CPU clock a CPU clock is, which reads as of the last
/// charges.
pub(crate) fn read(host: &dyn Host, clock: Clock, cpu: Option<&CpuClock>) -> Duration {
    match clock {
        Clock::Realtime => host.realtime(),
        Clock::Monotonic => host.monotonic(),
        Clock::ProcessCpu | Clock::ThreadCpu => cpu.map_or(Duration::ZERO, CpuClock::now),
    }
}

/// What a reading of [`ClockOffset`] may be off by beyond its bracket: the coarsest host wall
/// clock counts milliseconds (a browser's `Date.now`). A set by less goes unnoticed.
const OFFSET_SLACK: i128 = 1_000_000;

/// The realtime clock's offset from the monotonic one (`ktime_mono_to_real(0)`), in
/// nanoseconds, as the host lets it be read: one realtime reading between two monotonic ones
/// gives a range. The offset moves only when the realtime clock is set, by a step or a leap
/// second (slewing moves both clocks alike), or when the host slept, the events after which
/// Linux runs `clock_was_set`; the wall clock is the host's, so nothing in the machine sets
/// it, and a set is seen by comparing readings.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ClockOffset {
    low: i128,
    high: i128,
}

impl ClockOffset {
    pub(crate) fn read(host: &dyn Host) -> Self {
        let before = host.monotonic().as_nanos() as i128;
        let real = host.realtime().as_nanos() as i128;
        let after = host.monotonic().as_nanos() as i128;
        Self {
            low: real - after,
            high: real - before,
        }
    }

    /// Whether the realtime clock was set between this reading and `now`.
    pub(crate) fn moved(&self, now: &Self) -> bool {
        now.low > self.high + OFFSET_SLACK || now.high + OFFSET_SLACK < self.low
    }
}

impl Kernel {
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
            (tid, true) if self.same_thread_group(caller, tid) => (me, Some(TaskId(tid as u32))),
            (_, true) => return Err(Errno::EINVAL),
            (pid, false) if gettime && pid as u32 == caller.task().0 => (me, None),
            // A process clock names a thread group by its leader's pid.
            (pid, false) => match self.procs.find(pid as u32) {
                Some(process) if process.pid.0 == pid as u32 => (process, None),
                _ => return Err(Errno::EINVAL),
            },
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

    /// Whether `tid` is a live thread of the caller's thread group (`same_thread_group`).
    pub(crate) fn same_thread_group(&self, caller: &Caller<'_>, tid: i32) -> bool {
        let state = self.state.read_or_recover();
        let me = &state.tasks[&caller.task()];
        u32::try_from(tid)
            .ok()
            .and_then(|tid| state.tasks.get(&TaskId(tid)))
            .is_some_and(|t| Arc::ptr_eq(&t.process, &me.process) && !t.thread.exited())
    }

    /// The current value of `clock` for the caller, who sees its own CPU time up to now
    /// ([`Kernel::cpu_clock_now`]).
    pub(crate) fn clock_now(&self, caller: &Caller<'_>, clock: &ClockRef) -> Duration {
        match &clock.cpu {
            Some(cpu) => self.cpu_clock_now(caller.task(), cpu),
            None => read(&*self.host, clock.clock, None),
        }
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
    /// are `EINVAL` before the timespec is read.
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
        from_timespec(ts)?;
        // Setting the wall clock needs CAP_SYS_TIME, and the wall clock is the host's, so not
        // even that is enough.
        Err(Errno::EPERM.into())
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
            // Realtime and monotonic clocks advance at the same rate, so a relative sleep
            // ends at a fixed point of the monotonic clock.
            let deadline = self.host.monotonic().saturating_add(request);
            return self.nanosleep_until(caller, deadline, rmtp).await;
        }
        // Interrupted, an absolute sleep reruns as it was, and `rmtp` is not written
        // (`-ERESTARTNOHAND`).
        let interrupted = |_: Interrupted| SysErr::RestartNoHand;
        match clock.clock {
            Clock::Monotonic => self
                .interruptible_sleep(caller, self.host.sleep_until(request))
                .await
                .map_err(interrupted)?,
            _ => {
                // Re-check the clock: a realtime deadline is not a monotonic one.
                let mut now = self.clock_now(caller, &clock);
                while now < request {
                    self.interruptible_sleep(caller, self.host.sleep(request - now))
                        .await
                        .map_err(interrupted)?;
                    now = self.clock_now(caller, &clock);
                }
            }
        }
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

    /// The relative sleep of `clock_nanosleep`, until `deadline` on the monotonic clock.
    /// Interrupted, it writes the time left to `rmtp` (`nanosleep_copyout`), and without a
    /// handler `restart_syscall` sleeps the rest.
    pub(crate) async fn nanosleep_until(
        &self,
        caller: &Caller<'_>,
        deadline: Duration,
        rmtp: KernelTimespecPtr,
    ) -> SysResult<i32> {
        let sleep = self.host.sleep_until(deadline);
        if self.interruptible_sleep(caller, sleep).await.is_ok() {
            return Ok(0);
        }
        if !rmtp.is_null() {
            let left = deadline.saturating_sub(self.host.monotonic());
            write_value(caller.memory(), rmtp, &to_timespec(left)).await?;
        }
        Err(self.restart_with(caller, Restart::Nanosleep { deadline, rmtp }))
    }
}
