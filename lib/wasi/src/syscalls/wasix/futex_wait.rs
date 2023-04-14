use std::task::Waker;

use super::*;
use crate::syscalls::*;

#[derive(Clone)]
struct FutexPoller {
    state: Arc<WasiState>,
    woken: Arc<Mutex<bool>>,
    poller_idx: u64,
    futex_idx: u64,
    expected: u32,
}
impl Future for FutexPoller {
    type Output = Result<(), Errno>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Errno>> {
        let mut guard = self.state.futexs.lock().unwrap();

        // If the futex itself is no longer registered then it was likely
        // woken by a wake call
        let futex = match guard.futexes.get_mut(&self.futex_idx) {
            Some(f) => f,
            None => return Poll::Ready(Ok(())),
        };
        let waker = match futex.wakers.get_mut(&self.poller_idx) {
            Some(w) => w,
            None => return Poll::Ready(Ok(())),
        };

        // Register the waker if its not set
        if waker.is_none() {
            waker.replace(cx.waker().clone());
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
            futex.wakers.remove(&self.poller_idx);
            should_remove = futex.wakers.is_empty();
        }
        if should_remove {
            guard.futexes.remove(&self.futex_idx);
        }
    }
}

/// The futex after struct will write the response of whether the
/// futex was actually woken or not to the return memory of the syscall
/// callee after the wake event has been triggered.
///
/// It is encased in this struct so that it can be passed around
/// between threads and execute after the threads are rewound (in an
/// asynchronous threading situation).
///
/// The same implementation is used for both synchronous and
/// asynchronous threading.
///
/// It is not possible to include this logic directly in the poller
/// as the poller runs before the stack is rewound and the memory
/// that this writes to is often a pointer to the stack hence a
/// rewind would override whatever is written.
struct FutexAfter<M>
where
    M: MemorySize,
{
    woken: Arc<Mutex<bool>>,
    ret_woken: WasmPtr<Bool, M>,
}
impl<M> RewindPostProcess for FutexAfter<M>
where
    M: MemorySize,
{
    fn finish(
        &mut self,
        env: &WasiEnv,
        store: &dyn AsStoreRef,
        res: Result<(), Errno>,
    ) -> Result<(), ExitCode> {
        let woken = self.woken.lock().unwrap();
        if *woken {
            let view = env.memory_view(store);
            self.ret_woken
                .write(&view, Bool::True)
                .map_err(mem_error_to_wasi)
                .map_err(ExitCode::Errno)
        } else {
            Ok(())
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
#[instrument(level = "trace", skip_all, fields(futex_idx = field::Empty, poller_idx = field::Empty, %expected, timeout = field::Empty, woken = field::Empty), err)]
pub fn futex_wait<M: MemorySize + 'static>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    futex_ptr: WasmPtr<u32, M>,
    expected: u32,
    timeout: WasmPtr<OptionTimestamp, M>,
    ret_woken: WasmPtr<Bool, M>,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    // If we were just restored then we were woken after a deep sleep
    // and thus we repeat all the checks again, we do not immediately
    // exit here as it could be the case that we were woken but the
    // expected value does not match
    if handle_rewind::<M>(&mut ctx) {
        // fall through so the normal checks kick in, this will
        // ensure that the expected value has changed before
        // this syscall returns even if it was woken
    }

    // Determine the timeout
    let mut env = ctx.data();
    let timeout = {
        let memory = env.memory_view(&ctx);
        wasi_try_mem_ok!(timeout.read(&memory))
    };
    let timeout = match timeout.tag {
        OptionTag::Some => Some(Duration::from_nanos(timeout.u as u64)),
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
    let woken = Arc::new(Mutex::new(false));
    let poller = {
        let mut guard = env.state.futexs.lock().unwrap();
        guard.poller_seed += 1;
        let poller_idx = guard.poller_seed;

        // We insert the futex before we check the condition variable to avoid
        // certain race conditions
        let futex = guard
            .futexes
            .entry(futex_idx)
            .or_insert_with(|| Default::default());
        futex.wakers.insert(poller_idx, Default::default());

        Span::current().record("poller_idx", poller_idx);
        FutexPoller {
            state: env.state.clone(),
            woken: woken.clone(),
            poller_idx,
            futex_idx,
            expected,
        }
    };

    // We check if the expected value has changed
    let memory = env.memory_view(&ctx);
    let val = wasi_try_mem_ok!(futex_ptr.read(&memory));
    if val != expected {
        // We have been triggered so do not go into a wait
        wasi_try_mem_ok!(ret_woken.write(&memory, Bool::True));
        return Ok(Errno::Success);
    }

    // We clear the woken flag (so if the poller fails to trigger
    // then the value is not set) - the poller will set it to true
    wasi_try_mem_ok!(ret_woken.write(&memory, Bool::False));

    // Create a poller which will register ourselves against
    // this futex event and check when it has changed
    let after = FutexAfter { woken, ret_woken };

    // We use asyncify on the poller and potentially go into deep sleep
    __asyncify_with_deep_sleep::<M, _>(
        ctx,
        timeout,
        Duration::from_millis(50),
        Box::pin(poller),
        after,
    )?;
    Ok(Errno::Success)
}
