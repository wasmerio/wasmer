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
    futex_ptr: WasmPtr<u32, M>,
    ret_woken: WasmPtr<Bool, M>,
) -> Errno {
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let state = env.state.deref();

    let pointer: u64 = wasi_try!(futex_ptr.offset().try_into().map_err(|_| Errno::Overflow));
    let mut woken = false;

    let woken = {
        let mut guard = state.futexs.lock().unwrap();
        if let Some(futex) = guard.get_mut(&pointer) {
            if let Some(w) = futex.wakers.pop() {
                w.wake()
            }
            if futex.wakers.is_empty() {
                guard.remove(&pointer);
            }
            true
        } else {
            false
        }
    };
    if woken {
        trace!(
            %woken,
            "wasi[{}:{}]::futex_wake(offset={})",
            ctx.data().pid(),
            ctx.data().tid(),
            futex_ptr.offset()
        );
    } else {
        trace!(
            "wasi[{}:{}]::futex_wake(offset={}) - nothing waiting",
            ctx.data().pid(),
            ctx.data().tid(),
            futex_ptr.offset()
        );
    }

    let woken = match woken {
        false => Bool::False,
        true => Bool::True,
    };
    wasi_try_mem!(ret_woken.write(&memory, woken));

    Errno::Success
}
