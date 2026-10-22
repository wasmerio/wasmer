use std::task::Waker;

use super::*;
use crate::syscalls::*;

/// ### `thread_sleep()`
/// Sends the current thread to sleep for a period of time
///
/// ## Parameters
///
/// * `duration` - Amount of time that the thread should sleep
#[instrument(level = "trace", skip_all, fields(%duration), ret)]
pub fn thread_sleep<M: MemorySize + 'static>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    duration: Timestamp,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    thread_sleep_internal::<M>(ctx, duration)
}

pub(crate) fn thread_sleep_internal<M: MemorySize + 'static>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    duration: Timestamp,
) -> Result<Errno, WasiError> {
    if let Some(()) = unsafe { handle_rewind::<M, _>(&mut ctx) } {
        return Ok(Errno::Success);
    }

    ctx = wasi_try_ok!(maybe_backoff::<M>(ctx)?);
    ctx = wasi_try_ok!(maybe_snapshot::<M>(ctx)?);

    let env = ctx.data();

    #[cfg(feature = "sys-thread")]
    if duration == 0 {
        std::thread::yield_now();
    }

    if duration > 0 {
        let duration = Duration::from_nanos(duration);
        let tasks = env.tasks().clone();
        let res = __asyncify_with_deep_sleep::<M, _, _>(ctx, async move {
            tasks.sleep_now(duration).await;
        })?;
    }
    Ok(Errno::Success)
}

#[cfg(all(test, feature = "sys", not(target_arch = "wasm32")))]
mod tests {
    use wasmer::{Module, Store, TypedFunction};

    use crate::WasiEnv;

    /// Every blocking syscall waits for signals with a fresh waker. Syscalls
    /// that complete without being interrupted must not leave it registered
    /// with the thread.
    #[tokio::test(flavor = "multi_thread")]
    async fn blocking_syscalls_do_not_accumulate_signal_wakers() {
        tokio::task::spawn_blocking(|| {
            let mut store = Store::default();
            let module = Module::new(
                &store,
                r#"(module
                    (import "wasix_32v1" "thread_sleep" (func $sleep (param i64) (result i32)))
                    (memory (export "memory") 1)
                    (func (export "sleep_n") (param $n i32)
                        (loop $again
                            (drop (call $sleep (i64.const 1000)))
                            (local.set $n (i32.sub (local.get $n) (i32.const 1)))
                            (br_if $again (local.get $n)))))"#,
            )
            .unwrap();
            let (instance, env) = WasiEnv::builder("signal-wakers")
                .engine(store.engine().clone())
                .instantiate(module, &mut store)
                .unwrap();
            let sleep_n: TypedFunction<i32, ()> = instance
                .exports
                .get_typed_function(&store, "sleep_n")
                .unwrap();

            sleep_n.call(&mut store, 100).unwrap();

            let thread = env.data(&store).thread.clone();
            assert_eq!(thread.signals().lock().unwrap().1.len(), 0);
        })
        .await
        .unwrap();
    }
}
