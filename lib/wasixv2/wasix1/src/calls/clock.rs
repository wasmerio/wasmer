//! `clock_res_get`, `clock_time_get`: Linux clocks in nanoseconds.

use wasixv2_abi::KernelTimespec;
use wasixv2_syscalls::{Control, Syscalls};
use wasmer::RuntimeError;

use crate::{
    abi::{Clockid, Errno},
    errno::V1Result,
    shim::{self, Env, Frame},
};

/// A timespec as v1's nanosecond timestamp, saturating instead of wrapping.
fn nanos(ts: &KernelTimespec) -> u64 {
    (ts.tv_sec.max(0) as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add(ts.tv_nsec.max(0) as u64)
}

/// Reads `id` with `clock_gettime64` (or `clock_getres_time64` when `resolution`).
async fn read_clock<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    id: i32,
    resolution: bool,
) -> V1Result<u64> {
    let Some(clock) = Clockid(id as u32).to_linux() else {
        return shim::fail(Errno::INVAL);
    };
    let ts = frame.mem.alloc_mut::<KernelTimespec>()?;
    let caller = frame.caller();
    if resolution {
        frame
            .syscalls
            .clock_getres_time64(&caller, clock, ts)
            .await?;
    } else {
        frame.syscalls.clock_gettime64(&caller, clock, ts).await?;
    }
    Ok(nanos(&frame.mem.take(ts)))
}

pub(crate) async fn clock_res_get<S: Syscalls + Control + 'static>(
    env: Env<S>,
    id: i32,
    out: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let res = read_clock(frame, id, true).await?;
        frame.write(out as u32, &res).await
    })
    .await
}

/// `precision` is a hint Linux has no use for.
pub(crate) async fn clock_time_get<S: Syscalls + Control + 'static>(
    env: Env<S>,
    id: i32,
    _precision: i64,
    out: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let now = read_clock(frame, id, false).await?;
        frame.write(out as u32, &now).await
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timespec_to_nanos() {
        let ts = KernelTimespec {
            tv_sec: 2,
            tv_nsec: 5,
        };
        assert_eq!(nanos(&ts), 2_000_000_005);
        let huge = KernelTimespec {
            tv_sec: i64::MAX,
            tv_nsec: 0,
        };
        assert_eq!(nanos(&huge), u64::MAX);
    }
}
