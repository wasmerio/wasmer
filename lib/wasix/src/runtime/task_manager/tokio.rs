use std::{pin::Pin, sync::Arc, time::Duration};

use futures::{future::BoxFuture, Future};
use tokio::runtime::Handle;

use crate::{os::task::thread::WasiThreadError, WasiFunctionEnv};

use super::{TaskWasm, TaskWasmRunProperties, VirtualTaskManager};

/// A task manager that uses tokio to spawn tasks.
#[derive(Clone, Debug)]
pub struct TokioTaskManager {
    handle: Handle,
    pool: Arc<rayon::ThreadPool>,
}

impl TokioTaskManager {
    pub fn new(rt: Handle) -> Self {
        Self {
            handle: rt,
            pool: Arc::new(rayon::ThreadPoolBuilder::new().build().unwrap()),
        }
    }

    pub fn runtime_handle(&self) -> tokio::runtime::Handle {
        self.handle.clone()
    }
}

impl Default for TokioTaskManager {
    fn default() -> Self {
        Self::new(Handle::current())
    }
}

struct TokioRuntimeGuard<'g> {
    #[allow(unused)]
    inner: tokio::runtime::EnterGuard<'g>,
}
impl<'g> Drop for TokioRuntimeGuard<'g> {
    fn drop(&mut self) {}
}

impl VirtualTaskManager for TokioTaskManager {
    /// See [`VirtualTaskManager::sleep_now`].
    fn sleep_now(&self, time: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + Sync>> {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        self.handle.spawn(async move {
            if time == Duration::ZERO {
                tokio::task::yield_now().await;
            } else {
                tokio::time::sleep(time).await;
            }
            tx.send(()).ok();
        });
        Box::pin(async move {
            rx.recv().await;
        })
    }

    /// See [`VirtualTaskManager::task_shared`].
    fn task_shared(
        &self,
        task: Box<dyn FnOnce() -> BoxFuture<'static, ()> + Send + 'static>,
    ) -> Result<(), WasiThreadError> {
        self.handle.spawn(async move {
            let fut = task();
            fut.await
        });
        Ok(())
    }

    /// See [`VirtualTaskManager::task_wasm`].
    fn task_wasm(&self, task: TaskWasm) -> Result<(), WasiThreadError> {
        // Create the context on a new store
        let run = task.run;
        let (ctx, store) = WasiFunctionEnv::new_with_store(
            task.module,
            task.env,
            task.snapshot,
            task.spawn_type,
            task.update_layout,
        )?;

        // If we have a trigger then we first need to run
        // the poller to completion
        if let Some(trigger) = task.trigger {
            tracing::trace!("spawning task_wasm trigger in async pool");

            let trigger = trigger();
            let pool = self.pool.clone();
            self.handle.spawn(async move {
                let result = trigger.await;
                // Build the task that will go on the callback
                pool.spawn(move || {
                    // Invoke the callback
                    run(TaskWasmRunProperties {
                        ctx,
                        store,
                        trigger_result: Some(result),
                    });
                });
            });
        } else {
            tracing::trace!("spawning task_wasm in blocking thread");

            // Run the callback on a dedicated thread
            self.pool.spawn(move || {
                tracing::trace!("task_wasm started in blocking thread");

                // Invoke the callback
                run(TaskWasmRunProperties {
                    ctx,
                    store,
                    trigger_result: None,
                });
            });
        }
        Ok(())
    }

    /// See [`VirtualTaskManager::task_dedicated`].
    fn task_dedicated(
        &self,
        task: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), WasiThreadError> {
        self.pool.spawn(move || {
            task();
        });
        Ok(())
    }

    /// See [`VirtualTaskManager::thread_parallelism`].
    fn thread_parallelism(&self) -> Result<usize, WasiThreadError> {
        Ok(std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(8))
    }
}
