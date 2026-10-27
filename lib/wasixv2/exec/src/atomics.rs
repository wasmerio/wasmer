//! Wasm's `memory.atomic.wait32`, `wait64` and `notify` on the kernel's futexes. The loader
//! turns the instructions into calls of these imports ([`crate::preempt`]), which trap as the
//! instructions do and otherwise ask the kernel ([`AtomicWait`]). A wait is a futex wait: a
//! signal interrupts it, its handler runs on the return-to-guest path, and the group's exit ends
//! it.
//!
//! A handler that restarts calls makes an untimed wait go on; a timed wait, or one whose handler
//! does not restart, returns as woken, the result that has its caller look at the word again,
//! as `futex` failing with `EINTR` has.
//!
//! [`AtomicWait`]: wasixv2_syscalls::AtomicWait

use std::time::Duration;

use wasixv2_syscalls::{AtomicWord, Caller, Control, Errno, Syscalls, UserMemory};
use wasmer::{AsyncFunctionEnvMut, Function, FunctionEnv, Imports, RuntimeError, Store};
use wasmer_types::TrapCode;

use crate::{ExecEnv, GuestMemory, caller_parts, preempt, return_to_guest};

/// Defines the imports that replace the instructions.
pub(crate) fn define<S>(imports: &mut Imports, store: &mut Store, env: &FunctionEnv<ExecEnv<S>>)
where
    S: Syscalls + Control + 'static,
{
    let module = preempt::HOOK_MODULE;
    imports.define(
        module,
        preempt::WAIT32_NAME,
        Function::new_typed_with_env_async(store, env, wait32::<S>),
    );
    imports.define(
        module,
        preempt::WAIT64_NAME,
        Function::new_typed_with_env_async(store, env, wait64::<S>),
    );
    imports.define(
        module,
        preempt::NOTIFY_NAME,
        Function::new_typed_with_env_async(store, env, notify::<S>),
    );
}

async fn wait32<S>(
    env: AsyncFunctionEnvMut<ExecEnv<S>>,
    addr: i32,
    expected: i32,
    timeout: i64,
    offset: i32,
) -> Result<i32, RuntimeError>
where
    S: Syscalls + Control + 'static,
{
    wait(
        &env,
        addr,
        offset,
        AtomicWord::U32(expected as u32),
        timeout,
    )
    .await
}

async fn wait64<S>(
    env: AsyncFunctionEnvMut<ExecEnv<S>>,
    addr: i32,
    expected: i64,
    timeout: i64,
    offset: i32,
) -> Result<i32, RuntimeError>
where
    S: Syscalls + Control + 'static,
{
    wait(
        &env,
        addr,
        offset,
        AtomicWord::U64(expected as u64),
        timeout,
    )
    .await
}

async fn notify<S>(
    env: AsyncFunctionEnvMut<ExecEnv<S>>,
    addr: i32,
    count: i32,
    offset: i32,
) -> Result<i32, RuntimeError>
where
    S: Syscalls + Control + 'static,
{
    let addr = effective_address(&env, addr, offset, 4).await?;
    let (syscalls, task) = caller_parts(&env).await;
    Ok(syscalls.atomic_notify(task, addr, count as u32) as i32)
}

async fn wait<S>(
    env: &AsyncFunctionEnvMut<ExecEnv<S>>,
    addr: i32,
    offset: i32,
    expected: AtomicWord,
    timeout: i64,
) -> Result<i32, RuntimeError>
where
    S: Syscalls + Control + 'static,
{
    let size = match expected {
        AtomicWord::U32(_) => 4,
        AtomicWord::U64(_) => 8,
    };
    let addr = effective_address(env, addr, offset, size).await?;
    let (syscalls, task) = caller_parts(env).await;
    let memory = GuestMemory { env };
    let caller = Caller::new(task, &memory);
    // A negative timeout waits forever.
    let timeout = u64::try_from(timeout).ok().map(Duration::from_nanos);
    loop {
        let ret = syscalls.atomic_wait(&caller, addr, expected, timeout).await;
        if let Some(ret) = return_to_guest(env, &*syscalls, task, ret).await {
            return ret.map(wait_result);
        }
    }
}

/// The instruction's result for the futex wait's: 0 for woken (or interrupted), 1 for a word
/// that did not hold the expected value, 2 for a timeout.
fn wait_result(ret: i32) -> i32 {
    match Errno(-ret) {
        Errno::EAGAIN => 1,
        Errno::ETIMEDOUT => 2,
        _ => 0,
    }
}

/// `addr + offset`, trapping as the instruction does when the `size` bytes there are out of
/// bounds or not aligned to `size`.
async fn effective_address<S: 'static>(
    env: &AsyncFunctionEnvMut<ExecEnv<S>>,
    addr: i32,
    offset: i32,
    size: u32,
) -> Result<u32, RuntimeError> {
    let addr = u64::from(addr as u32) + u64::from(offset as u32);
    let len = GuestMemory { env }.size().await;
    if addr + u64::from(size) > len {
        return Err(trap(TrapCode::HeapAccessOutOfBounds));
    }
    if !addr.is_multiple_of(u64::from(size)) {
        return Err(trap(TrapCode::UnalignedAtomic));
    }
    // In bounds of a 32-bit memory.
    Ok(addr as u32)
}

#[cfg(feature = "sys")]
fn trap(code: TrapCode) -> RuntimeError {
    wasmer::sys::vm::Trap::lib(code).into()
}

#[cfg(not(feature = "sys"))]
fn trap(code: TrapCode) -> RuntimeError {
    RuntimeError::new(code.message())
}
