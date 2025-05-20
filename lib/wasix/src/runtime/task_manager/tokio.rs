use std::sync::Mutex;
use std::{num::NonZeroUsize, pin::Pin, sync::Arc, time::Duration};

use futures::{future::BoxFuture, Future};
use tokio::runtime::{Handle, Runtime};
use virtual_mio::InlineWaker;
use wasmer::AsStoreMut;

use crate::runtime::SpawnMemoryType;
use crate::{os::task::thread::WasiThreadError, WasiFunctionEnv};

use super::{SpawnMemoryTypeOrStore, TaskWasm, TaskWasmRunProperties, VirtualTaskManager};

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

#[derive(Clone)]
pub struct ThreadPool {
    inner: rusty_pool::ThreadPool,
}

impl std::ops::Deref for ThreadPool {
    type Target = rusty_pool::ThreadPool;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl std::fmt::Debug for ThreadPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThreadPool")
            .field("name", &self.get_name())
            .field("current_worker_count", &self.get_current_worker_count())
            .field("idle_worker_count", &self.get_idle_worker_count())
            .finish()
    }
}

/// A task manager that uses tokio to spawn tasks.
#[derive(Clone, Debug)]
pub struct TokioTaskManager {
    rt: RuntimeOrHandle,
    pool: Arc<ThreadPool>,
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
            pool: Arc::new(ThreadPool {
                inner: rusty_pool::Builder::new()
                    .name("TokioTaskManager Thread Pool".to_string())
                    .core_size(max_threads)
                    .max_size(max_threads)
                    .build(),
            }),
        }
    }

    pub fn runtime_handle(&self) -> tokio::runtime::Handle {
        self.rt.handle().clone()
    }

    pub fn pool_handle(&self) -> Arc<ThreadPool> {
        self.pool.clone()
    }
}

impl Default for TokioTaskManager {
    fn default() -> Self {
        Self::new(Handle::current())
    }
}

#[allow(dead_code)]
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
        let handle = self.runtime_handle();
        Box::pin(async move {
            SleepNow::default()
                .enter(handle, time)
                .await
                .ok()
                .unwrap_or(())
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
        let run = task.run;
        let recycle = task.recycle;
        let env = task.env;
        let pre_run = task.pre_run;

        let make_memory: SpawnMemoryTypeOrStore = match task.spawn_type {
            SpawnMemoryType::CreateMemory => SpawnMemoryTypeOrStore::New,
            SpawnMemoryType::CreateMemoryOfType(t) => SpawnMemoryTypeOrStore::Type(t),
            SpawnMemoryType::ShareMemory(_, _) | SpawnMemoryType::CopyMemory(_, _) => {
                let mut store = env.runtime().new_store();
                let memory = self.build_memory(&mut store.as_store_mut(), task.spawn_type)?;
                SpawnMemoryTypeOrStore::StoreAndMemory(store, memory)
            }
        };

        if let Some(trigger) = task.trigger {
            tracing::trace!("spawning task_wasm trigger in async pool");
            // In principle, we'd need to create this in the `pool.execute` function below, that is
            //
            // ```
            // 227: pool.execute(move || {
            // ...:      let (ctx, mut store) = WasiFunctionEnv::new_with_store(
            // ...:      ...
            // ```
            //
            // However, in the loop spawned below we need to have a `FunctionEnvMut<WasiEnv>`, which
            // must be created with a mutable reference to the store. We can't, however since
            // ```
            // pool.execute(move || {
            //      let (ctx, mut store) = WasiFunctionEnv::new_with_store(
            //      ...
            //      tx.send(store.as_store_mut())
            // ```
            // or
            // ```
            // pool.execute(move || {
            //      let (ctx, mut store) = WasiFunctionEnv::new_with_store(
            //      ...
            //      tx.send(ctx.env.clone().into_mut(&mut store.as_store_mut()))
            // ```
            // Since the reference would outlive the owned value.
            //
            // So, we create the store (and memory, and instance) outside the execution thread (the
            // pool's one), and let it fail for runtimes that don't support entities created in a
            // thread that's not the one in which execution happens in; this until we can clone
            // stores.
            let (mut ctx, mut store) = WasiFunctionEnv::new_with_store(
                task.module,
                env,
                task.globals,
                make_memory,
                task.update_layout,
            )?;

            let mut trigger = trigger();
            let pool = self.pool.clone();
            self.rt.handle().spawn(async move {
                // We wait for either the trigger or for a snapshot to take place
                let result = loop {
                    let env = ctx.data(&store);
                    break tokio::select! {
                        r = &mut trigger => r,
                        _ = env.thread.wait_for_signal() => {
                            tracing::debug!("wait-for-signal(triggered)");
                            let mut ctx = ctx.env.clone().into_mut(&mut store);
                            if let Err(err) = crate::WasiEnv::process_signals_and_exit(&mut ctx) {
                                match err {
                                    crate::WasiError::Exit(code) => Err(code),
                                    err => {
                                        tracing::error!("failed to process signals - {}", err);
                                        continue;
                                    }
                                }
                            } else {
                                continue;
                            }
                        }
                        _ = crate::wait_for_snapshot(env) => {
                            tracing::debug!("wait-for-snapshot(triggered)");
                            let mut ctx = ctx.env.clone().into_mut(&mut store);
                            crate::os::task::WasiProcessInner::do_checkpoints_from_outside(&mut ctx);
                            continue;
                        }
                    };
                };

                if let Some(pre_run) = pre_run {
                    pre_run(&mut ctx, &mut store).await;
                }

                // Build the task that will go on the callback
                pool.execute(move || {
                    // Invoke the callback
                    run(TaskWasmRunProperties {
                        ctx,
                        store,
                        trigger_result: Some(result),
                        recycle,
                    });
                });
            });
        } else {
            tracing::trace!("spawning task_wasm in blocking thread");

            let (sx, rx) = std::sync::mpsc::channel();

            // Run the callback on a dedicated thread
            self.pool.execute(move || {
                tracing::trace!("task_wasm started in blocking thread");
                let ret = WasiFunctionEnv::new_with_store(
                    task.module,
                    env,
                    task.globals,
                    make_memory,
                    task.update_layout,
                );

                let (mut ctx, mut store) = match ret {
                    Ok(x) => {
                        sx.send(Ok(())).unwrap();
                        x
                    }
                    Err(c) => {
                        sx.send(Err(c)).unwrap();
                        return;
                    }
                };

                if let Some(pre_run) = pre_run {
                    InlineWaker::block_on(pre_run(&mut ctx, &mut store));
                }

                // Invoke the callback
                run(TaskWasmRunProperties {
                    ctx,
                    store,
                    trigger_result: None,
                    recycle,
                });
            });

            rx.recv()
                .map_err(|_| WasiThreadError::InvalidWasmContext)??;
        }
        Ok(())
    }

    /// See [`VirtualTaskManager::task_dedicated`].
    fn task_dedicated(
        &self,
        task: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), WasiThreadError> {
        self.pool.execute(move || {
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

// Used by [`VirtualTaskManager::sleep_now`] to abort a sleep task when drop.
#[derive(Default)]
struct SleepNow {
    abort_handle: Option<tokio::task::AbortHandle>,
}

impl SleepNow {
    async fn enter(
        &mut self,
        handle: tokio::runtime::Handle,
        time: Duration,
    ) -> Result<(), tokio::task::JoinError> {
        let handle = handle.spawn(async move {
            if time == Duration::ZERO {
                tokio::task::yield_now().await;
            } else {
                tokio::time::sleep(time).await;
            }
        });
        self.abort_handle = Some(handle.abort_handle());
        handle.await
    }
}

impl Drop for SleepNow {
    fn drop(&mut self) {
        if let Some(h) = self.abort_handle.as_ref() {
            h.abort()
        }
    }
}
