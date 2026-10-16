//! `restart_syscall` (`kernel/signal.c`, `restart_block`): how a call that a signal interrupted
//! with [`SysErr::RestartBlock`] goes on when no handler ran. The interrupted call saves what it
//! needs on the task; the execution layer then calls `restart_syscall` instead of rerunning the
//! call with its original arguments.

use std::time::Duration;

use wasixv2_abi::KernelTimespecPtr;
use wasixv2_syscalls::{Caller, Errno, SysErr, SysResult};

use crate::{Kernel, futex::Timeout};

/// What an interrupted call left for its restart. Deadlines are on the host's monotonic clock.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Restart {
    /// `hrtimer_nanosleep_restart`: a relative sleep, with where to write the time left.
    Nanosleep {
        deadline: Duration,
        rmtp: KernelTimespecPtr,
    },
    /// `futex_wait_restart`: a timed `FUTEX_WAIT`.
    FutexWait {
        addr: u32,
        val: u32,
        bitset: u32,
        timeout: Timeout,
    },
}

impl Kernel {
    /// Saves `restart` on the caller and returns the error that asks for it.
    pub(crate) fn restart_with(&self, caller: &Caller<'_>, restart: Restart) -> SysErr {
        self.with_task(caller.task(), |t| t.restart = Some(restart));
        SysErr::RestartBlock
    }

    /// `restart_syscall`: continues the interrupted call, or fails with `EINTR` when there is
    /// none to continue (`do_no_restart_syscall`).
    pub(crate) async fn sys_restart_syscall(&self, caller: &Caller<'_>) -> SysResult<i32> {
        match self.with_task(caller.task(), |t| t.restart.take()) {
            None => Err(Errno::EINTR.into()),
            Some(Restart::Nanosleep { deadline, rmtp }) => {
                self.nanosleep_until(caller, deadline, rmtp).await
            }
            Some(Restart::FutexWait {
                addr,
                val,
                bitset,
                timeout,
            }) => {
                self.futex_wait_restart(caller, addr, val, bitset, timeout)
                    .await
            }
        }
    }
}
