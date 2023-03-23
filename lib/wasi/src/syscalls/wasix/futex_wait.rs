use std::task::Waker;

use super::*;
use crate::syscalls::*;

#[derive(Clone)]
struct FutexPoller<M>
where
    M: MemorySize,
{
    state: Arc<WasiState>,
    woken: Arc<Mutex<bool>>,
    futex_idx: u64,
    futex_ptr: WasmPtr<u32, M>,
    expected: u32,
}
impl<M> AsyncifyFuture for FutexPoller<M>
where
    M: MemorySize,
{
    fn poll(
        &mut self,
        env: &WasiEnv,
        store: &dyn AsStoreRef,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Errno>> {
        let waker = cx.waker();
        let view = env.memory_view(store);
        let mut guard = env.state.futexs.lock().unwrap();

        {
            let val = match self.futex_ptr.read(&view) {
                Ok(a) => a,
                Err(err) => {
                    {
                        let mut guard = self.woken.lock().unwrap();
                        *guard = true;
                    }
                    return Poll::Ready(Err(mem_error_to_wasi(err)));
                }
            };
            if val != self.expected {
                {
                    let mut guard = self.woken.lock().unwrap();
                    *guard = true;
                }
                return Poll::Ready(Ok(()));
            }
        }

        let futex = guard
            .entry(self.futex_idx)
            .or_insert_with(|| WasiFutex { wakers: vec![] });
        if !futex.wakers.iter().any(|w| w.will_wake(waker)) {
            futex.wakers.push(waker.clone());
        }

        Poll::Pending
    }
}
impl<M> Drop for FutexPoller<M>
where
    M: MemorySize,
{
    fn drop(&mut self) {
        let futex = {
            let mut guard = self.state.futexs.lock().unwrap();
            guard.remove(&self.futex_idx)
        };
        if let Some(futex) = futex {
            futex.wakers.into_iter().for_each(|w| w.wake());
        }
    }
}

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
#[instrument(level = "trace", skip_all, fields(futex_idx = field::Empty, %expected, timeout = field::Empty, woken = field::Empty), err)]
pub fn futex_wait<M: MemorySize + 'static>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    futex_ptr: WasmPtr<u32, M>,
    expected: u32,
    timeout: WasmPtr<OptionTimestamp, M>,
    ret_woken: WasmPtr<Bool, M>,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    // If we were just restored then we were woken after a deep sleep
    if handle_rewind::<M>(&mut ctx) {
        return Ok(Errno::Success);
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

    // We clear the woken flag (so if the poller fails to trigger
    // then the value is not set) - the poller will set it to true
    let memory = env.memory_view(&ctx);
    wasi_try_mem_ok!(ret_woken.write(&memory, Bool::False));

    // Create a poller which will register ourselves against
    // this futex event and check when it has changed
    let woken = Arc::new(Mutex::new(false));
    let poller = FutexPoller {
        state: env.state.clone(),
        woken: woken.clone(),
        futex_idx,
        futex_ptr,
        expected,
    };
    let after = FutexAfter { woken, ret_woken };

    // We use asyncify on the poller and potentially go into deep sleep
    __asyncify_with_deep_sleep::<M, _, _>(ctx, timeout, poller, after)?;
    Ok(Errno::Success)
}
