use super::*;
use crate::syscalls::*;

/// Wake up all threads that are waiting on futex_wait on this futex.
///
/// ## Parameters
///
/// * `futex` - Memory location that holds a futex that others may be waiting on
#[instrument(level = "trace", skip_all, fields(futex_idx = field::Empty, woken = field::Empty), ret)]
pub fn futex_wake_all<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    futex_ptr: WasmPtr<u32, M>,
    ret_woken: WasmPtr<Bool, M>,
) -> Errno {
    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    let state = env.state.deref();

    let pointer: u64 = wasi_try!(futex_ptr.offset().try_into().map_err(|_| Errno::Overflow));
    //Span::current().record("futex_idx", pointer);

    let mut woken = false;
    let woken = {
        let mut guard = state.futexs.lock().unwrap();
        if let Some(futex) = guard.futexes.remove(&pointer) {
            for waker in futex.wakers {
                if let Some(waker) = waker.1 {
                    waker.wake();
                }
            }
            tracing::trace!("wake_all (hit) on {pointer}");
            true
        } else {
            tracing::trace!("wake_all (miss) on {pointer}");
            true
        }
    };
    //Span::current().record("woken", woken);

    let woken = match woken {
        false => Bool::False,
        true => Bool::True,
    };
    wasi_try_mem!(ret_woken.write(&memory, woken));
    Errno::Success
}
