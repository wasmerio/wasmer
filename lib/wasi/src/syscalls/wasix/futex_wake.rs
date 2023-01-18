use super::*;
use crate::syscalls::*;

/// Wake up one thread that's blocked on futex_wait on this futex.
/// Returns true if this actually woke up such a thread,
/// or false if no thread was waiting on this futex.
///
/// ## Parameters
///
/// * `futex` - Memory location that holds a futex that others may be waiting on
pub fn futex_wake<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    futex: WasmPtr<u32, M>,
    ret_woken: WasmPtr<Bool, M>,
) -> Errno {
    trace!(
        "wasi[{}:{}]::futex_wake(offset={})",
        ctx.data().pid(),
        ctx.data().tid(),
        futex.offset()
    );
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let state = env.state.deref();

    let pointer: u64 = wasi_try!(futex.offset().try_into().map_err(|_| Errno::Overflow));
    let mut woken = false;

    let mut guard = state.futexs.read().unwrap();
    if let Some(futex) = guard.get(&pointer) {
        woken = futex.waker.receiver_count() > 0;
        let _ = futex.waker.send(());
    } else {
        trace!(
            "wasi[{}:{}]::futex_wake - nothing waiting!",
            ctx.data().pid(),
            ctx.data().tid()
        );
    }

    let woken = match woken {
        false => Bool::False,
        true => Bool::True,
    };
    wasi_try_mem!(ret_woken.write(&memory, woken));

    Errno::Success
}
