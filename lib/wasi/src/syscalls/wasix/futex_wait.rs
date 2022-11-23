use super::*;
use crate::syscalls::*;

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
    trace!(
        "wasi[{}:{}]::futex_wait(offset={})",
        ctx.data().pid(),
        ctx.data().tid(),
        futex_ptr.offset()
    );
    
    wasi_try_ok!(ctx.data().clone().process_signals_and_exit(&mut ctx)?);

    let mut env = ctx.data();
    let state = env.state.clone();

    let pointer: u64 = wasi_try_ok!(futex_ptr.offset().try_into().map_err(|_| Errno::Overflow));

    // Register the waiting futex (if its not already registered)
    let futex = {
        use std::collections::hash_map::Entry;
        let mut guard = state.futexs.lock().unwrap();
        match guard.entry(pointer) {
            Entry::Occupied(entry) => entry.get().clone(),
            Entry::Vacant(entry) => {
                let futex = WasiFutex {
                    refcnt: Arc::new(AtomicU32::new(1)),
                    inner: Arc::new(Mutex::new(tokio::sync::broadcast::channel(1).0)),
                };
                entry.insert(futex.clone());
                futex
            }
        }
    };

    // Determine the timeout
    let timeout = {
        let memory = env.memory_view(&ctx);
        wasi_try_mem_ok!(timeout.read(&memory))
    };
    let timeout = match timeout.tag {
        OptionTag::Some => Some(timeout.u as u128),
        _ => None,
    };

    // Loop until we either hit a yield error or the futex is woken
    let mut woken = Bool::False;
    let start = platform_clock_time_get(Snapshot0Clockid::Monotonic, 1).unwrap() as u128;
    loop {
        let mut rx = {
            let futex_lock = futex.inner.lock().unwrap();
            // If the value of the memory is no longer the expected value
            // then terminate from the loop (we do this under a futex lock
            // so that its protected)
            {
                let view = env.memory_view(&ctx);
                let val = wasi_try_mem_ok!(futex_ptr.read(&view));
                if val != expected {
                    woken = Bool::True;
                    break;
                }
            }
            futex_lock.subscribe()
        };

        // Check if we have timed out
        let mut sub_timeout = None;
        if let Some(timeout) = timeout.as_ref() {
            let now = platform_clock_time_get(Snapshot0Clockid::Monotonic, 1).unwrap() as u128;
            let delta = now.checked_sub(start).unwrap_or(0);
            if delta >= *timeout {
                break;
            }
            let remaining = *timeout - delta;
            sub_timeout = Some(Duration::from_nanos(remaining as u64));
        }

        // Now wait for it to be triggered
        wasi_try_ok!(__asyncify(&mut ctx, sub_timeout, async move {
            let _ = rx.recv().await;
            Ok(())
        }));
        env = ctx.data();
    }

    // Drop the reference count to the futex (and remove it if the refcnt hits zero)
    {
        let mut guard = state.futexs.lock().unwrap();
        if guard
            .get(&pointer)
            .map(|futex| futex.refcnt.fetch_sub(1, Ordering::AcqRel) == 1)
            .unwrap_or(false)
        {
            guard.remove(&pointer);
        }
    }

    let memory = env.memory_view(&ctx);
    wasi_try_mem_ok!(ret_woken.write(&memory, woken));

    Ok(Errno::Success)
}
