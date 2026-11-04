//! Futexes: `futex_wait`, `futex_wake`, `futex_wake_all` as Linux's private futexes, which the
//! kernel keys per address space, as v1 keys them per memory.
//!
//! wasix-libc traps when `futex_wait` fails, and reads "not woken and the value is unchanged"
//! as a timeout. So every outcome but a bad address is a success: a changed value (`EAGAIN`)
//! and an interruption by a signal (whose handler runs before the call returns) count as
//! woken, which a futex waiter must expect anyway.

use wasixv2_abi::{FutexOp, GuestLayout, KernelTimespec, U32Ptr};
use wasixv2_syscalls::{Control, Errno, SysErr, Syscalls};
use wasmer::RuntimeError;

use crate::{
    abi::{
        Bool,
        proc::{OptionTag, OptionTimestamp},
    },
    errno::V1Result,
    shim::{self, Env, Frame},
};

/// `futex_wait(futex, expected, timeout, woken_out)`; `timeout` points to an
/// `option_timestamp` of nanoseconds, relative to now.
pub(crate) async fn futex_wait<S: Syscalls + Control + 'static>(
    env: Env<S>,
    futex: i32,
    expected: i32,
    timeout: i32,
    woken_out: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let timeout = match timeout as u32 {
            0 => OptionTimestamp::ZERO,
            addr => frame.read::<OptionTimestamp>(addr).await?,
        };
        let utime = match timeout.tag {
            OptionTag::SOME => {
                let ns = timeout.timestamp;
                let ts = KernelTimespec {
                    tv_sec: (ns / 1_000_000_000) as i64,
                    tv_nsec: (ns % 1_000_000_000) as i64,
                };
                frame.mem.alloc(&ts)?.addr()
            }
            _ => 0,
        };
        let woken = match frame
            .syscalls
            .futex_time64(
                &frame.caller(),
                U32Ptr::new(futex as u32),
                FutexOp::FUTEX_WAIT_PRIVATE,
                expected as u32,
                utime,
                U32Ptr::NULL,
                0,
            )
            .await
        {
            Ok(_) => true,
            Err(SysErr::Errno(Errno::ETIMEDOUT)) => false,
            Err(SysErr::Errno(Errno::EAGAIN) | SysErr::Restart | SysErr::RestartNoHand) => true,
            Err(err) => return Err(err.into()),
        };
        write_bool(frame, woken_out, woken).await
    })
    .await
}

/// Wakes at most `count` waiters on `futex`; whether any woke.
async fn wake<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    futex: i32,
    count: u32,
) -> V1Result<bool> {
    let woken = frame
        .syscalls
        .futex_time64(
            &frame.caller(),
            U32Ptr::new(futex as u32),
            FutexOp::FUTEX_WAKE_PRIVATE,
            count,
            0,
            U32Ptr::NULL,
            0,
        )
        .await?;
    Ok(woken > 0)
}

async fn write_bool<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    addr: i32,
    value: bool,
) -> V1Result<()> {
    let value = if value { Bool::TRUE } else { Bool::FALSE };
    frame.write(addr as u32, &value).await
}

/// `futex_wake(futex, woken_out)`: one waiter.
pub(crate) async fn futex_wake<S: Syscalls + Control + 'static>(
    env: Env<S>,
    futex: i32,
    woken_out: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let woken = wake(frame, futex, 1).await?;
        write_bool(frame, woken_out, woken).await
    })
    .await
}

/// `futex_wake_all(futex, woken_out)`: every waiter.
pub(crate) async fn futex_wake_all<S: Syscalls + Control + 'static>(
    env: Env<S>,
    futex: i32,
    woken_out: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let woken = wake(frame, futex, i32::MAX as u32).await?;
        write_bool(frame, woken_out, woken).await
    })
    .await
}
