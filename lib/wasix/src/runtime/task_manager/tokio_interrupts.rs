//! Host interrupts for WASM threads run by
//! [`TokioTaskManager`](super::tokio::TokioTaskManager).
//!
//! WASIX delivers `SIGKILL` by marking threads finished and waking threads
//! blocked in syscalls. A thread running WASM code that never calls into the
//! runtime (a busy loop) never observes that, so its executor would keep a
//! core forever. With the `experimental-host-interrupt` feature on Unix,
//! every running WASM thread registers its store's [`wasmer::Interrupter`],
//! and terminating the thread traps its WASM code with `HostInterrupt`.

pub(crate) use imp::RunningThreads;

#[cfg(all(unix, feature = "experimental-host-interrupt"))]
mod imp {
    use std::{
        collections::HashMap,
        sync::{
            Arc, Mutex,
            atomic::{AtomicU64, Ordering},
        },
        time::Duration,
    };

    use tokio::runtime::Handle;
    use wasmer::{Interrupter, Store};
    use wasmer_wasix_types::wasi::Signal;

    use crate::{
        WasiFunctionEnv, WasiProcess, WasiProcessId, WasiThreadId,
        os::task::thread::WasiThreadError,
    };

    /// Delay before the first interrupt retry; doubled up to
    /// [`MAX_RETRY_DELAY`] after each attempt.
    const FIRST_RETRY_DELAY: Duration = Duration::from_millis(1);
    const MAX_RETRY_DELAY: Duration = Duration::from_millis(100);

    /// WASM threads currently running on a task manager.
    #[derive(Default)]
    pub(crate) struct RunningThreads {
        next_id: AtomicU64,
        threads: Mutex<HashMap<u64, RunningThread>>,
    }

    struct RunningThread {
        pid: WasiProcessId,
        tid: WasiThreadId,
        process: WasiProcess,
        interrupter: Interrupter,
    }

    /// Unregisters a running thread when its task ends.
    pub(crate) struct RunningThreadGuard {
        threads: Arc<RunningThreads>,
        id: u64,
    }

    impl Drop for RunningThreadGuard {
        fn drop(&mut self) {
            self.threads.threads.lock().unwrap().remove(&self.id);
        }
    }

    impl std::fmt::Debug for RunningThreads {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("RunningThreads")
                .field("count", &self.threads.lock().unwrap().len())
                .finish()
        }
    }

    impl RunningThreads {
        #[cfg(test)]
        pub(crate) fn len(&self) -> usize {
            self.threads.lock().unwrap().len()
        }

        #[cfg(test)]
        pub(crate) fn is_empty(&self) -> bool {
            self.len() == 0
        }

        /// Record the WASM thread that `ctx` and `store` are about to run.
        /// Only stores of the `sys` backend can be interrupted.
        pub(crate) fn register(
            self: &Arc<Self>,
            ctx: &WasiFunctionEnv,
            store: &Store,
        ) -> Option<RunningThreadGuard> {
            if !store.engine().is_sys() {
                return None;
            }
            let env = ctx.data(store);
            let thread = RunningThread {
                pid: env.pid(),
                tid: env.tid(),
                process: env.process.clone(),
                interrupter: store.interrupter(),
            };
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            self.threads.lock().unwrap().insert(id, thread);
            Some(RunningThreadGuard {
                threads: Arc::clone(self),
                id,
            })
        }

        /// Interrupt the WASM code of thread `tid` of process `pid`.
        pub(crate) fn terminate(
            self: &Arc<Self>,
            runtime: &Handle,
            pid: WasiProcessId,
            tid: WasiThreadId,
        ) -> Result<(), WasiThreadError> {
            let targets: Vec<_> = self
                .threads
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, thread)| thread.pid == pid && thread.tid == tid)
                .map(|(id, thread)| (*id, thread.interrupter.clone(), thread.process.clone()))
                .collect();
            let Some((_, _, process)) = targets.first() else {
                return Err(WasiThreadError::Unsupported);
            };
            // Interrupts do not reach a thread parked in `memory.atomic.wait`,
            // so wake those as WASIX does for task managers without this hook.
            process.wake_atomic_waiters(Signal::Sigkill);

            for (id, interrupter, _) in targets {
                interrupter.interrupt();
                // An interrupt can be missed, for example when the thread is
                // about to enter WASM code, so repeat it until the thread's
                // task has ended.
                let threads = Arc::clone(self);
                runtime.spawn(async move {
                    let mut delay = FIRST_RETRY_DELAY;
                    loop {
                        tokio::time::sleep(delay).await;
                        if !threads.threads.lock().unwrap().contains_key(&id) {
                            break;
                        }
                        interrupter.interrupt();
                        delay = (delay * 2).min(MAX_RETRY_DELAY);
                    }
                });
            }
            Ok(())
        }
    }
}

#[cfg(not(all(unix, feature = "experimental-host-interrupt")))]
mod imp {
    use std::sync::Arc;

    use tokio::runtime::Handle;
    use wasmer::Store;

    use crate::{WasiFunctionEnv, WasiProcessId, WasiThreadId, os::task::thread::WasiThreadError};

    /// Without host interrupts, running threads are not tracked.
    #[derive(Debug, Default)]
    pub(crate) struct RunningThreads;

    pub(crate) struct RunningThreadGuard;

    impl RunningThreads {
        pub(crate) fn register(
            self: &Arc<Self>,
            _ctx: &WasiFunctionEnv,
            _store: &Store,
        ) -> Option<RunningThreadGuard> {
            None
        }

        pub(crate) fn terminate(
            self: &Arc<Self>,
            _runtime: &Handle,
            _pid: WasiProcessId,
            _tid: WasiThreadId,
        ) -> Result<(), WasiThreadError> {
            Err(WasiThreadError::Unsupported)
        }
    }
}
