//! Clocks, sleeps and their timespec conversions.

use std::time::Duration;

use wasixv2_abi::{ClockId, ConstKernelTimespecPtr, KernelTimespec, KernelTimespecPtr, TimerFlags};
use wasixv2_host::Host;
use wasixv2_syscalls::{Caller, Errno, SysErr, SysResult, read_value, write_value};

use crate::{Kernel, restart::Restart};

const NANOS_PER_SEC: i64 = 1_000_000_000;
/// `CLOCK_TAI`: the kernel's TAI offset is 0 until something sets it, as on a fresh Linux boot.
const CLOCK_TAI: ClockId = ClockId(11);
/// Resolution of the coarse clocks: one tick at `HZ=250`.
const COARSE_RES: Duration = Duration::from_millis(4);

/// What a clock id reads.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Clock {
    Realtime,
    Monotonic,
    /// CPU time of the process or one of its threads. A guest is charged for all the time it
    /// exists, as the host cannot tell CPU time from waiting.
    // TODO: subtract time spent blocked in the kernel.
    ProcessCpu,
    ThreadCpu,
}

/// Decodes `id`, including the per-pid and per-thread CPU clock ids `clock_getcpuclockid` and
/// `pthread_getcpuclockid` build (`CPUCLOCK_PID`, `CPUCLOCK_PERTHREAD`, `CPUCLOCK_WHICH`).
pub(crate) fn resolve(id: ClockId, pid: i32, tid: i32) -> Result<Clock, Errno> {
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
        ClockId::CLOCK_PROCESS_CPUTIME_ID => Clock::ProcessCpu,
        ClockId::CLOCK_THREAD_CPUTIME_ID => Clock::ThreadCpu,
        ClockId(raw) if raw < 0 => {
            let owner = !(raw >> 3);
            if raw & 3 == 3 {
                return Err(Errno::EINVAL);
            }
            let thread = raw & 4 != 0;
            let me = if thread { tid } else { pid };
            if owner != 0 && owner != me {
                return Err(Errno::EINVAL);
            }
            if thread {
                Clock::ThreadCpu
            } else {
                Clock::ProcessCpu
            }
        }
        _ => return Err(Errno::EINVAL),
    };
    Ok(clock)
}

fn resolution(id: ClockId) -> Duration {
    match id {
        ClockId::CLOCK_REALTIME_COARSE | ClockId::CLOCK_MONOTONIC_COARSE => COARSE_RES,
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

/// The host clock behind `clock` for the calling process.
pub(crate) fn read(host: &dyn Host, clock: Clock, process_start: Duration) -> Duration {
    match clock {
        Clock::Realtime => host.realtime(),
        Clock::Monotonic => host.monotonic(),
        Clock::ProcessCpu | Clock::ThreadCpu => host.monotonic().saturating_sub(process_start),
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
    pub(crate) fn clock(&self, caller: &Caller<'_>, id: ClockId) -> Result<Clock, Errno> {
        let pid = self.process(caller.task()).pid.0 as i32;
        resolve(id, pid, caller.task().0 as i32)
    }

    /// The current value of `clock` for the caller.
    pub(crate) fn clock_now(&self, caller: &Caller<'_>, clock: Clock) -> Duration {
        read(&*self.host, clock, self.process(caller.task()).start)
    }

    pub(crate) async fn sys_clock_gettime(
        &self,
        caller: &Caller<'_>,
        id: ClockId,
        tp: KernelTimespecPtr,
    ) -> SysResult<i32> {
        let now = self.clock_now(caller, self.clock(caller, id)?);
        write_value(caller.memory(), tp, &to_timespec(now)).await?;
        Ok(0)
    }

    pub(crate) async fn sys_clock_getres(
        &self,
        caller: &Caller<'_>,
        id: ClockId,
        tp: KernelTimespecPtr,
    ) -> SysResult<i32> {
        self.clock(caller, id)?;
        if !tp.is_null() {
            write_value(caller.memory(), tp, &to_timespec(resolution(id))).await?;
        }
        Ok(0)
    }

    pub(crate) async fn sys_clock_settime(
        &self,
        caller: &Caller<'_>,
        id: ClockId,
        tp: ConstKernelTimespecPtr,
    ) -> SysResult<i32> {
        let clock = self.clock(caller, id)?;
        from_timespec(read_value(caller.memory(), tp).await?)?;
        match clock {
            // Setting the wall clock needs CAP_SYS_TIME, and the wall clock is the host's, so
            // not even that is enough.
            Clock::Realtime if id == ClockId::CLOCK_REALTIME => Err(Errno::EPERM.into()),
            _ => Err(Errno::EINVAL.into()),
        }
    }

    pub(crate) async fn sys_clock_nanosleep(
        &self,
        caller: &Caller<'_>,
        id: ClockId,
        flags: TimerFlags,
        rqtp: ConstKernelTimespecPtr,
        rmtp: KernelTimespecPtr,
    ) -> SysResult<i32> {
        let clock = self.clock(caller, id)?;
        if clock == Clock::ThreadCpu {
            return Err(Errno::EINVAL.into());
        }
        let request = from_timespec(read_value(caller.memory(), rqtp).await?)?;
        if !flags.contains(TimerFlags::TIMER_ABSTIME) {
            // Every clock advances at the monotonic clock's rate, so a relative sleep ends at a
            // fixed point of it.
            let deadline = self.host.monotonic().saturating_add(request);
            return self.nanosleep_until(caller, deadline, rmtp).await;
        }
        // Interrupted, an absolute sleep reruns as it was, and `rmtp` is not written
        // (`-ERESTARTNOHAND`).
        let interrupted = |_: wasixv2_wait::Interrupted| SysErr::RestartNoHand;
        match clock {
            Clock::Monotonic => self
                .interruptible_sleep(caller, self.host.sleep_until(request))
                .await
                .map_err(interrupted)?,
            _ => {
                // Re-check the clock: a realtime deadline is not a monotonic one.
                let mut now = self.clock_now(caller, clock);
                while now < request {
                    self.interruptible_sleep(caller, self.host.sleep(request - now))
                        .await
                        .map_err(interrupted)?;
                    now = self.clock_now(caller, clock);
                }
            }
        }
        Ok(0)
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
