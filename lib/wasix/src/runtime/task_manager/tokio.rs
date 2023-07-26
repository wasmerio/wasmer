use std::sync::Mutex;
use std::{num::NonZeroUsize, pin::Pin, sync::Arc, time::Duration};

use futures::{future::BoxFuture, Future};
use tokio::runtime::{Handle, Runtime};

use crate::{os::task::thread::WasiThreadError, WasiFunctionEnv};

use super::{TaskWasm, TaskWasmRunProperties, VirtualTaskManager};

#[derive(Debug, Clone)]
pub enum RuntimeOrHandle {
    Handle(Handle),
    Runtime(Handle, Arc<Mutex<Option<Runtime>>>),
}
impl From<Handle> for RuntimeOrHandle {
    fn from(value: Handle) -> Self {
        Self::Handle(value)
    }
}
impl From<Runtime> for RuntimeOrHandle {
    fn from(value: Runtime) -> Self {
        Self::Runtime(value.handle().clone(), Arc::new(Mutex::new(Some(value))))
    }
}

impl Drop for RuntimeOrHandle {
    fn drop(&mut self) {
        if let Self::Runtime(_, runtime) = self {
            if let Some(h) = runtime.lock().unwrap().take() {
                h.shutdown_timeout(Duration::from_secs(0))
            }
        }
    }
}

impl RuntimeOrHandle {
    pub fn handle(&self) -> &Handle {
        match self {
            Self::Handle(h) => h,
            Self::Runtime(h, _) => h,
        }
    }
}

/// A task manager that uses tokio to spawn tasks.
#[derive(Clone, Debug)]
pub struct TokioTaskManager {
    rt: RuntimeOrHandle,
    pool: Arc<rayon::ThreadPool>,
}

impl TokioTaskManager {
    pub fn new<I>(rt: I) -> Self
    where
        I: Into<RuntimeOrHandle>,
    {
        let concurrency = std::thread::available_parallelism()
            .unwrap_or(NonZeroUsize::new(1).unwrap())
            .get();
        let max_threads = 200usize.max(concurrency * 100);

        Self {
            rt: rt.into(),
            pool: Arc::new(
                rayon::ThreadPoolBuilder::new()
                    .num_threads(max_threads)
                    .build()
                    .unwrap(),
            ),
        }
    }

    pub fn runtime_handle(&self) -> tokio::runtime::Handle {
        self.rt.handle().clone()
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
        self.rt.handle().spawn(async move {
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
        self.rt.handle().spawn(async move {
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
            self.rt.handle().spawn(async move {
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
