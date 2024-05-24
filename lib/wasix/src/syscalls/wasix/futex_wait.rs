use std::task::Waker;

use super::*;
use crate::syscalls::*;

/// Poller returns true if its triggered and false if it times out
struct FutexPoller {
    state: Arc<WasiState>,
    poller_idx: u64,
    futex_idx: u64,
    expected: u32,
    timeout: Option<Pin<Box<dyn Future<Output = ()> + Send + Sync + 'static>>>,
}
impl Future for FutexPoller {
    type Output = bool;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<bool> {
        let mut guard = self.state.futexs.lock().unwrap();

        // If the futex itself is no longer registered then it was likely
        // woken by a wake call
        let futex = match guard.futexes.get_mut(&self.futex_idx) {
            Some(f) => f,
            None => return Poll::Ready(true),
        };
        let waker = match futex.wakers.get_mut(&self.poller_idx) {
            Some(w) => w,
            None => return Poll::Ready(true),
        };

        // Register the waker
        waker.replace(cx.waker().clone());

        // Check for timeout
        drop(guard);
        if let Some(timeout) = self.timeout.as_mut() {
            let timeout = timeout.as_mut();
            if timeout.poll(cx).is_ready() {
                self.timeout.take();
                return Poll::Ready(false);
            }
        }

        // We will now wait to be woken
        Poll::Pending
    }
}
impl Drop for FutexPoller {
    fn drop(&mut self) {
        let mut guard = self.state.futexs.lock().unwrap();

        let mut should_remove = false;
        if let Some(futex) = guard.futexes.get_mut(&self.futex_idx) {
            if let Some(Some(waker)) = futex.wakers.remove(&self.poller_idx) {
                waker.wake();
            }
            should_remove = futex.wakers.is_empty();
        }
        if should_remove {
            guard.futexes.remove(&self.futex_idx);
        }
    }
}

/// Wait for a futex_wake operation to wake us.
/// Returns with EINVAL if the futex doesn't hold the expected value.
/// Returns false on timeout, and true in all other cases.
///
/// ## Parameters
///
/// * `futex` - Memory location that holds the value that will be checked
/// * `expected` - Expected value that should be currently held at the memory location
/// * `timeout` - Timeout should the futex not be triggered in the allocated time
#[instrument(level = "trace", skip_all, fields(futex_idx = field::Empty, poller_idx = field::Empty, %expected, timeout = field::Empty, woken = field::Empty))]
pub fn futex_wait<M: MemorySize + 'static>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    futex_ptr: WasmPtr<u32, M>,
    expected: u32,
    timeout: WasmPtr<OptionTimestamp, M>,
    ret_woken: WasmPtr<Bool, M>,
) -> Result<Errno, WasiError> {
    futex_wait_internal(ctx, futex_ptr, expected, timeout, ret_woken)
}

pub(super) fn futex_wait_internal<M: MemorySize + 'static>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    futex_ptr: WasmPtr<u32, M>,
    expected: u32,
    timeout: WasmPtr<OptionTimestamp, M>,
    ret_woken: WasmPtr<Bool, M>,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    ctx = wasi_try_ok!(maybe_backoff::<M>(ctx)?);
    ctx = wasi_try_ok!(maybe_snapshot::<M>(ctx)?);

    // If we were just restored then we were woken after a deep sleep
    // and thus we repeat all the checks again, we do not immediately
    // exit here as it could be the case that we were woken but the
    // expected value does not match
    if let Some(_woken) = unsafe { handle_rewind::<M, bool>(&mut ctx) } {
        // fall through so the normal checks kick in, this will
        // ensure that the expected value has changed before
        // this syscall returns even if it was woken
    }

    // Determine the timeout
    let mut env = ctx.data();
    let timeout = {
        let memory = unsafe { env.memory_view(&ctx) };
        wasi_try_mem_ok!(timeout.read(&memory))
    };
    let timeout = match timeout.tag {
        OptionTag::Some => Some(Duration::from_nanos(timeout.u)),
        _ => None,
    };
    Span::current().record("timeout", &format!("{:?}", timeout));

    let state = env.state.clone();
    let futex_idx: u64 = wasi_try_ok!(futex_ptr.offset().try_into().map_err(|_| Errno::Overflow));
    Span::current().record("futex_idx", futex_idx);

    // We generate a new poller which also registers in the
    // shared state futex lookup. When this object is dropped
    // it will remove itself from the lookup. It can also be
    // removed whenever the wake call is invoked (which could
    // be before the poller is polled).
    let poller = {
        let mut guard = env.state.futexs.lock().unwrap();
        guard.poller_seed += 1;
        let poller_idx = guard.poller_seed;

        // Create the timeout if one exists
        let timeout = timeout.map(|timeout| env.tasks().sleep_now(timeout));

        // We insert the futex before we check the condition variable to avoid
        // certain race conditions
        let futex = guard.futexes.entry(futex_idx).or_default();
        futex.wakers.insert(poller_idx, Default::default());

        Span::current().record("poller_idx", poller_idx);
        FutexPoller {
            state: env.state.clone(),
            poller_idx,
            futex_idx,
            expected,
            timeout,
        }
    };

    // We check if the expected value has changed
    let memory = unsafe { env.memory_view(&ctx) };
    let val = wasi_try_mem_ok!(futex_ptr.read(&memory));
    if val != expected {
        // We have been triggered so do not go into a wait
        wasi_try_mem_ok!(ret_woken.write(&memory, Bool::True));
        return Ok(Errno::Success);
    }

    // We clear the woken flag (so if the poller fails to trigger
    // then the value is not set) - the poller will set it to true
    wasi_try_mem_ok!(ret_woken.write(&memory, Bool::False));

    // We use asyncify on the poller and potentially go into deep sleep
    tracing::trace!("wait on {futex_idx}");
    let res = __asyncify_with_deep_sleep::<M, _, _>(ctx, Box::pin(poller))?;
    if let AsyncifyAction::Finish(ctx, res) = res {
        let mut env = ctx.data();
        let memory = unsafe { env.memory_view(&ctx) };
        if res {
            wasi_try_mem_ok!(ret_woken.write(&memory, Bool::True));
        } else {
            wasi_try_mem_ok!(ret_woken.write(&memory, Bool::False));
        }
    }
    Ok(Errno::Success)
}
