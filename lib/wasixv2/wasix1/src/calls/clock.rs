//! V1 clock reads and per-process clock_time_set offsets over Linux clocks.

use std::sync::Mutex;

use wasixv2_abi::KernelTimespec;
use wasixv2_syscalls::{Control, Syscalls};
use wasmer::RuntimeError;

use crate::{
    abi::{Clockid, Errno},
    errno::V1Result,
    shim::{self, Env, Frame},
};

/// V1 clock_time_set is an unprivileged per-process offset, including monotonic and
/// CPU clocks. It is deliberately separate from Linux clock_settime (CAP_SYS_TIME).
#[derive(Default)]
pub(crate) struct State(Mutex<[i128; 4]>);

impl State {
    pub(crate) fn fork(&self) -> Self {
        Self(Mutex::new(*self.0.lock().unwrap()))
    }

    fn index(id: Clockid) -> V1Result<usize> {
        id.to_linux().ok_or(crate::errno::Error::V1(Errno::INVAL))?;
        Ok(id.0 as usize)
    }

    fn set(&self, id: Clockid, raw: u64, target: u64) -> V1Result<()> {
        let index = Self::index(id)?;
        self.0.lock().unwrap()[index] = i128::from(target) - i128::from(raw);
        Ok(())
    }

    pub(crate) fn read(&self, id: Clockid, raw: u64) -> V1Result<u64> {
        let index = Self::index(id)?;
        let value = i128::from(raw) + self.0.lock().unwrap()[index];
        Ok(value.clamp(0, i128::from(u64::MAX)) as u64)
    }
}

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
        let raw = read_clock(frame, id, false).await?;
        let now = frame.proc().await?.clocks.read(Clockid(id as u32), raw)?;
        frame.write(out as u32, &now).await
    })
    .await
}

/// V1 ABI semantics: lib/wasix/src/syscalls/wasi/clock_time_set.rs,
/// clock_time_set_internal. Each set replaces the offset relative to the raw clock.
pub(crate) async fn clock_time_set<S: Syscalls + Control + 'static>(
    env: Env<S>,
    id: i32,
    time: i64,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let raw = read_clock(frame, id, false).await?;
        frame
            .proc()
            .await?
            .clocks
            .set(Clockid(id as u32), raw, time as u64)
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
    #[test]
    fn offsets_are_per_clock_and_copied_at_fork() {
        let parent = State::default();
        parent.set(Clockid::REALTIME, 1_000, 200).unwrap();
        assert_eq!(parent.read(Clockid::REALTIME, 1_100).unwrap(), 300);
        assert_eq!(parent.read(Clockid::MONOTONIC, 1_100).unwrap(), 1_100);
        let child = parent.fork();
        child.set(Clockid::REALTIME, 1_100, 5_000).unwrap();
        assert_eq!(child.read(Clockid::REALTIME, 1_200).unwrap(), 5_100);
        assert_eq!(parent.read(Clockid::REALTIME, 1_200).unwrap(), 400);
        assert_eq!(
            State::default().read(Clockid::REALTIME, 1_200).unwrap(),
            1_200
        );
        parent.set(Clockid::REALTIME, 1_200, 50).unwrap();
        assert_eq!(
            parent.read(Clockid::REALTIME, 1_200).unwrap(),
            50,
            "repeated sets replace the offset"
        );
        assert!(parent.set(Clockid(4), 0, 0).is_err());
    }
}
