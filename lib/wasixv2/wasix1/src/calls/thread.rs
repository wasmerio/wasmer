//! Threads: `thread_spawn_v2` (and its older name `thread_spawn`), wasi-threads'
//! `thread-spawn`, `thread_sleep`, `thread_parallelism`.
//!
//! A v1 thread is a Linux thread (`CLONE_VM | CLONE_FS | CLONE_FILES | CLONE_SIGHAND |
//! CLONE_THREAD`). The execution layer starts it on a new instance of the module, on the shared
//! memory, at `wasi_thread_start(tid, start_arg)`; the guest's trampoline sets up its stack and
//! TLS from `start_arg`, and returning from it ends the thread (see [`crate::instance`]).
//! `thread_exit` and `thread_id` are in `exit.rs`, `thread_signal` in `signal.rs`.

use wasixv2_abi::{
    CloneFlags, ConstKernelTimespecPtr, KernelTimespec, KernelTimespecPtr, S32Ptr, UlongArray,
};
use wasixv2_syscalls::{Control, Syscalls};
use wasmer::RuntimeError;

use crate::{
    errno::V1Result,
    shim::{self, Done, Env, Frame},
};

/// The `clone` flags of a v1 thread. TLS is the guest trampoline's business, so there is no
/// `CLONE_SETTLS`.
const THREAD: CloneFlags = CloneFlags(
    CloneFlags::CLONE_VM.0
        | CloneFlags::CLONE_FS.0
        | CloneFlags::CLONE_FILES.0
        | CloneFlags::CLONE_SIGHAND.0
        | CloneFlags::CLONE_THREAD.0
        | CloneFlags::CLONE_SYSVSEM.0,
);

/// Creates a thread that starts at `wasi_thread_start(tid, start_arg)`; returns its tid. A hold
/// on signals that libc's `pthread_create` would never end goes first (see
/// [`super::signal::end_spawn_hold`]).
async fn spawn<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    start_arg: u32,
) -> V1Result<i32> {
    super::signal::end_spawn_hold(frame).await?;
    Ok(frame
        .syscalls
        .thread_spawn(
            &frame.caller(),
            THREAD.0 as u32,
            start_arg,
            S32Ptr::NULL,
            0,
            S32Ptr::NULL,
        )
        .await?)
}

/// `thread_spawn_v2(start, tid_out)`, and `thread_spawn`, its older name. `start` points to
/// libc's `thread_start`, which only the guest reads.
pub(crate) async fn thread_spawn_v2<S: Syscalls + Control + 'static>(
    env: Env<S>,
    start: i32,
    tid_out: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let tid = spawn(frame, start as u32).await? as u32;
        frame.write(tid_out as u32, &tid).await
    })
    .await
}

/// wasi-threads' `thread-spawn(start_arg)`: the new thread's tid, or a negative errno.
pub(crate) async fn wasi_thread_spawn<S: Syscalls + Control + 'static>(
    env: Env<S>,
    start: i32,
) -> Result<i32, RuntimeError> {
    Ok(
        match shim::call(&env, async |frame| spawn(frame, start as u32).await).await? {
            Done::Ok(tid) => tid,
            Done::Err(errno) => -i32::from(errno.0),
            Done::Switched(_) | Done::Unwinding => {
                return Err(RuntimeError::new("thread-spawn switched tasks"));
            }
        },
    )
}

/// `thread_sleep(ns)`: `clock_nanosleep` on the monotonic clock. A signal ends it early with
/// `INTR`.
pub(crate) async fn thread_sleep<S: Syscalls + Control + 'static>(
    env: Env<S>,
    duration: i64,
) -> Result<i32, RuntimeError> {
    let duration = duration as u64;
    shim::errno(&env, async |frame| {
        let ts = frame.mem.alloc(&KernelTimespec {
            tv_sec: (duration / 1_000_000_000) as i64,
            tv_nsec: (duration % 1_000_000_000) as i64,
        })?;
        frame
            .syscalls
            .clock_nanosleep_time64(
                &frame.caller(),
                wasixv2_abi::ClockId::CLOCK_MONOTONIC,
                wasixv2_abi::TimerFlags(0),
                ConstKernelTimespecPtr::new(ts.addr()),
                KernelTimespecPtr::NULL,
            )
            .await?;
        Ok(())
    })
    .await
}

/// Bytes of the CPU mask `thread_parallelism` asks for: 1024 CPUs, as glibc's `cpu_set_t`.
const CPU_MASK_BYTES: u32 = 128;

/// `thread_parallelism(out)`: the CPUs the calling thread may run on.
pub(crate) async fn thread_parallelism<S: Syscalls + Control + 'static>(
    env: Env<S>,
    out: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let mask = frame.mem.alloc_zeroed(CPU_MASK_BYTES)?;
        let len = frame
            .syscalls
            .sched_getaffinity(
                &frame.caller(),
                0,
                CPU_MASK_BYTES,
                UlongArray::new(mask.addr),
            )
            .await?;
        let bytes = frame.mem.take_bytes(mask, len.max(0) as usize);
        let cpus = bytes.iter().map(|b| b.count_ones()).sum::<u32>().max(1);
        frame.write(out as u32, &cpus).await
    })
    .await
}
