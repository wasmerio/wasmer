//! Exits and identity: `proc_exit`, `proc_exit2`, `thread_exit`, `proc_id`, `thread_id`,
//! `sched_yield`.

use wasixv2_syscalls::{Control, Syscalls};
use wasmer::RuntimeError;

use crate::shim::{self, Env};

/// `exit_group`. v1's exit code is a `u32`; Linux keeps its low 8 bits.
pub(crate) async fn proc_exit<S: Syscalls + Control + 'static>(
    env: Env<S>,
    code: i32,
) -> Result<(), RuntimeError> {
    shim::void(&env, "proc_exit", async |frame| {
        frame.syscalls.exit_group(&frame.caller(), code).await?;
        Ok(())
    })
    .await
}

/// `exit` of the calling thread.
pub(crate) async fn thread_exit<S: Syscalls + Control + 'static>(
    env: Env<S>,
    code: i32,
) -> Result<(), RuntimeError> {
    shim::void(&env, "thread_exit", async |frame| {
        frame.syscalls.exit(&frame.caller(), code).await?;
        Ok(())
    })
    .await
}

pub(crate) async fn proc_id<S: Syscalls + Control + 'static>(
    env: Env<S>,
    out: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let pid = frame.syscalls.getpid(&frame.caller()).await?;
        frame.write(out as u32, &(pid as u32)).await
    })
    .await
}

pub(crate) async fn thread_id<S: Syscalls + Control + 'static>(
    env: Env<S>,
    out: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let tid = frame.syscalls.gettid(&frame.caller()).await?;
        frame.write(out as u32, &(tid as u32)).await
    })
    .await
}

pub(crate) async fn sched_yield<S: Syscalls + Control + 'static>(
    env: Env<S>,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        frame.syscalls.sched_yield(&frame.caller()).await?;
        Ok(())
    })
    .await
}
