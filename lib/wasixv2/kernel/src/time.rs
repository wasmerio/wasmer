use std::time::Duration;

use wasixv2_abi::{ClockId, KernelTimespec};
use wasixv2_host::Host;
use wasixv2_syscalls::Errno;

const NANOS_PER_SEC: i64 = 1_000_000_000;

/// The host clock behind `clock`. The CPU-time and alarm clocks are not supported.
pub(crate) fn now(host: &dyn Host, clock: ClockId) -> Result<Duration, Errno> {
    match clock {
        ClockId::CLOCK_REALTIME | ClockId::CLOCK_REALTIME_COARSE => Ok(host.realtime()),
        ClockId::CLOCK_MONOTONIC
        | ClockId::CLOCK_MONOTONIC_RAW
        | ClockId::CLOCK_MONOTONIC_COARSE
        | ClockId::CLOCK_BOOTTIME => Ok(host.monotonic()),
        _ => Err(Errno::EINVAL),
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
