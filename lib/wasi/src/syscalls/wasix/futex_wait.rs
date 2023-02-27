use std::task::Waker;

use super::*;
use crate::syscalls::*;

struct FutexPoller<'a, M>
where
    M: MemorySize,
{
    env: &'a WasiEnv,
    view: MemoryView<'a>,
    futex_idx: u64,
    futex_ptr: WasmPtr<u32, M>,
    expected: u32,
}
impl<'a, M> Future for FutexPoller<'a, M>
where
    M: MemorySize,
{
    type Output = Result<(), Errno>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let waker = cx.waker();
        let mut guard = self.env.state.futexs.lock().unwrap();

        {
            let val = match self.futex_ptr.read(&self.view) {
                Ok(a) => a,
                Err(err) => return Poll::Ready(Err(mem_error_to_wasi(err))),
            };
            if val != self.expected {
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
impl<'a, M> Drop for FutexPoller<'a, M>
where
    M: MemorySize,
{
    fn drop(&mut self) {
        let futex = {
            let mut guard = self.env.state.futexs.lock().unwrap();
            guard.remove(&self.futex_idx)
        };
        if let Some(futex) = futex {
            futex.wakers.into_iter().for_each(|w| w.wake());
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
pub fn futex_wait<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    futex_ptr: WasmPtr<u32, M>,
    expected: u32,
    timeout: WasmPtr<OptionTimestamp, M>,
    ret_woken: WasmPtr<Bool, M>,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

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

    trace!(
        "wasi[{}:{}]::futex_wait(offset={}, timeout={:?})",
        ctx.data().pid(),
        ctx.data().tid(),
        futex_ptr.offset(),
        timeout
    );

    let state = env.state.clone();
    let futex_idx: u64 = wasi_try_ok!(futex_ptr.offset().try_into().map_err(|_| Errno::Overflow));

    // Create a poller which will register ourselves against
    // this futex event and check when it has changed
    let view = env.memory_view(&ctx);
    let poller = FutexPoller {
        env,
        view,
        futex_idx,
        futex_ptr,
        expected,
    };

    // Wait for the futex to trigger or a timeout to occur
    let res = __asyncify_light(env, timeout, poller)?;

    // Process it and return the result
    let mut ret = Errno::Success;
    let woken = match res {
        Err(Errno::Timedout) => Bool::False,
        Err(err) => {
            ret = err;
            Bool::True
        }
        Ok(_) => Bool::True,
    };
    let memory = env.memory_view(&ctx);
    let mut env = ctx.data();
    wasi_try_mem_ok!(ret_woken.write(&memory, woken));
    Ok(ret)
}
