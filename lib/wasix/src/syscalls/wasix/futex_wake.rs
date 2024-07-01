use super::*;
use crate::syscalls::*;

/// Wake up one thread that's blocked on futex_wait on this futex.
/// Returns true if this actually woke up such a thread,
/// or false if no thread was waiting on this futex.
///
/// ## Parameters
///
/// * `futex` - Memory location that holds a futex that others may be waiting on
#[instrument(level = "trace", skip_all, fields(futex_idx = field::Empty, woken = field::Empty), ret)]
pub fn futex_wake<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    futex_ptr: WasmPtr<u32, M>,
    ret_woken: WasmPtr<Bool, M>,
) -> Errno {
    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    let state = env.state.deref();

    let pointer: u64 = futex_ptr.offset().into();
    Span::current().record("futex_idx", pointer);

    let mut woken = false;
    let woken = {
        let mut guard = state.futexs.lock().unwrap();
        if let Some(futex) = guard.futexes.get_mut(&pointer) {
            let first = futex.wakers.keys().copied().next();
            if let Some(id) = first {
                if let Some(Some(w)) = futex.wakers.remove(&id) {
                    w.wake();
                }
            }
            if futex.wakers.is_empty() {
                guard.futexes.remove(&pointer);
            }
            tracing::trace!("wake(hit) on {pointer}");
            true
        } else {
            tracing::trace!("wake(miss) on {pointer}");
            true
        }
    };
    Span::current().record("woken", woken);

    let woken = match woken {
        false => Bool::False,
        true => Bool::True,
    };
    wasi_try_mem!(ret_woken.write(&memory, woken));

    Errno::Success
}
