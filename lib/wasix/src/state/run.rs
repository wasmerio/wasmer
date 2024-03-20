use virtual_mio::InlineWaker;
use wasmer::{RuntimeError, Store};
use wasmer_wasix_types::wasi::ExitCode;

use crate::{os::task::thread::RewindResultType, RewindStateOption, WasiError, WasiRuntimeError};

use super::*;

impl WasiFunctionEnv {
    #[allow(clippy::result_large_err)]
    pub fn run_async(self, mut store: Store) -> Result<(Self, Store), WasiRuntimeError> {
        // If no handle or runtime exists then create one
        #[cfg(feature = "sys-thread")]
        let _guard = if tokio::runtime::Handle::try_current().is_err() {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap();
            Some(runtime)
        } else {
            None
        };
        #[cfg(feature = "sys-thread")]
        let _guard = _guard.as_ref().map(|r| r.enter());

        self.data(&store).thread.set_status_running();

        let tasks = self.data(&store).tasks().clone();
        let pid = self.data(&store).pid();
        let tid = self.data(&store).tid();

        // The return value is passed synchronously and will block until the result
        // is returned this is because the main thread can go into a deep sleep and
        // exit the dedicated thread
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let this = self.clone();
        tasks.task_dedicated(Box::new(move || {
            // Unsafe: The bootstrap must be executed in the same thread that runs the
            //         actual WASM code
            let rewind_state = unsafe {
                match this.bootstrap(&mut store) {
                    Ok(a) => a,
                    Err(err) => {
                        tracing::warn!("failed to bootstrap - {}", err);
                        this.on_exit(&mut store, None);
                        tx.send(Err(err)).ok();
                        return;
                    }
                }
            };
            run_with_deep_sleep(store, rewind_state, this, tx);
        }))?;

        let result = InlineWaker::block_on(rx.recv());
        let store = match result {
            Some(result) => {
                tracing::trace!(
                    %pid,
                    %tid,
                    error=result.as_ref().err().map(|e| e as &dyn std::error::Error),
                    "main exit",
                );
                result?
            }
            None => {
                tracing::trace!(
                    %pid,
                    %tid,
                    "main premature termination",
                );
                return Err(WasiRuntimeError::Runtime(RuntimeError::new(
                    "main thread terminated without a result, this normally means a panic occurred",
                )));
            }
        };
        Ok((self, store))
    }
}

fn run_with_deep_sleep(
    mut store: Store,
    rewind_state: RewindStateOption,
    env: WasiFunctionEnv,
    sender: tokio::sync::mpsc::UnboundedSender<Result<Store, WasiRuntimeError>>,
) {
    if let Some((rewind_state, rewind_result)) = rewind_state {
        tracing::trace!("Rewinding");
        let mut ctx = env.env.clone().into_mut(&mut store);
        let errno = if rewind_state.is_64bit {
            crate::rewind_ext::<wasmer_types::Memory64>(
                &mut ctx,
                Some(rewind_state.memory_stack),
                rewind_state.rewind_stack,
                rewind_state.store_data,
                rewind_result,
            )
        } else {
            crate::rewind_ext::<wasmer_types::Memory32>(
                &mut ctx,
                Some(rewind_state.memory_stack),
                rewind_state.rewind_stack,
                rewind_state.store_data,
                rewind_result,
            )
        };

        if errno != Errno::Success {
            let exit_code = ExitCode::from(errno);
            env.on_exit(&mut store, Some(exit_code));
            if exit_code.is_success() {
                let _ = sender.send(Ok(store));
            } else {
                let _ = sender.send(Err(WasiRuntimeError::Wasi(WasiError::Exit(exit_code))));
            }
            return;
        }
    }

    let instance = match env.data(&store).try_clone_instance() {
        Some(instance) => instance,
        None => {
            tracing::debug!("Unable to clone the instance");
            env.on_exit(&mut store, None);
            let _ = sender.send(Err(WasiRuntimeError::Wasi(WasiError::Exit(
                Errno::Noexec.into(),
            ))));
            return;
        }
    };

    let start = match instance.exports.get_function("_start") {
        Ok(start) => start,
        Err(e) => {
            tracing::debug!("Unable to get the _start function");
            env.on_exit(&mut store, None);
            let _ = sender.send(Err(e.into()));
            return;
        }
    };

    let result = start.call(&mut store, &[]);
    handle_result(store, env, result, sender);
}

fn handle_result(
    mut store: Store,
    env: WasiFunctionEnv,
    result: Result<Box<[wasmer::Value]>, RuntimeError>,
    sender: tokio::sync::mpsc::UnboundedSender<Result<Store, WasiRuntimeError>>,
) {
    let result: Result<_, WasiRuntimeError> = match result.map_err(|e| e.downcast::<WasiError>()) {
        Err(Ok(WasiError::DeepSleep(work))) => {
            let pid = env.data(&store).pid();
            let tid = env.data(&store).tid();
            tracing::trace!(%pid, %tid, "entered a deep sleep");

            let tasks = env.data(&store).tasks().clone();
            let rewind = work.rewind;
            let respawn = move |ctx, store, res| {
                run_with_deep_sleep(
                    store,
                    Some((rewind, RewindResultType::RewindWithResult(res))),
                    ctx,
                    sender,
                )
            };

            // Spawns the WASM process after a trigger
            unsafe {
                tasks
                    .resume_wasm_after_poller(Box::new(respawn), env, store, work.trigger)
                    .unwrap();
            }

            return;
        }
        Ok(_) => Ok(()),
        Err(Ok(other)) => Err(other.into()),
        Err(Err(e)) => Err(e.into()),
    };

    let (result, exit_code) = wasi_exit_code(result);
    env.on_exit(&mut store, Some(exit_code));
    sender.send(result.map(|_| store)).ok();
}

/// Extract the exit code from a `Result<(), WasiRuntimeError>`.
///
/// We need this because calling `exit(0)` inside a WASI program technically
/// triggers [`WasiError`] with an exit code of `0`, but the end user won't want
/// that treated as an error.
pub(super) fn wasi_exit_code(
    mut result: Result<(), WasiRuntimeError>,
) -> (Result<(), WasiRuntimeError>, ExitCode) {
    let exit_code = match &result {
        Ok(_) => Errno::Success.into(),
        Err(err) => match err.as_exit_code() {
            Some(code) if code.is_success() => {
                // This is actually not an error, so we need to fix up the
                // result
                result = Ok(());
                Errno::Success.into()
            }
            Some(other) => other,
            None => Errno::Noexec.into(),
        },
    };

    (result, exit_code)
}
