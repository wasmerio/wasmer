use std::sync::Mutex;
use std::{num::NonZeroUsize, pin::Pin, sync::Arc, time::Duration};

use futures::{Future, future::BoxFuture};
use tokio::runtime::{Handle, Runtime};
use virtual_mio::block_on;

use crate::runtime::{SpawnType, task_manager::TaskWasmCallbacks};
use crate::{WasiFunctionEnv, os::task::thread::WasiThreadError};

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
        if let Self::Runtime(_, runtime) = self
            && let Some(h) = runtime.lock().unwrap().take()
        {
            h.shutdown_timeout(Duration::from_secs(0))
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

// Guest threads can block or synchronously spawn another guest thread.
// A bounded pool can leave the child's initialization queued forever.
// The control plane's max_threads capability limits guest threads; without it,
// host thread creation is limited only by the OS. Use the default stack size.
fn spawn_wasm_task(task: impl FnOnce() + Send + 'static) -> Result<(), WasiThreadError> {
    std::thread::Builder::new()
        .name("wasmer-wasm-task".to_string())
        .spawn(task)
        .map(|_| ())
        .map_err(|err| WasiThreadError::ThreadCreateFailed(Arc::new(err)))
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
        fn env_and_store(
            task: TaskWasm,
        ) -> Result<(WasiFunctionEnv, wasmer::Store, TaskWasmCallbacks), WasiThreadError> {
            let (make_memory, instance_group_data) = match task.spawn_type {
                SpawnType::CreateMemory => (SpawnMemoryTypeOrStore::New, None),
                SpawnType::NewLinkerInstanceGroup(instance_group_data) => {
                    (SpawnMemoryTypeOrStore::New, Some(instance_group_data))
                }
                SpawnType::CreateMemoryOfType(t) => (SpawnMemoryTypeOrStore::Type(t), None),
                SpawnType::AttachMemory(mem) => {
                    let mut store = task.env.runtime().new_store();
                    let memory = mem
                        .try_attach(&mut store)
                        .map_err(WasiThreadError::MemoryCreateFailed)?;
                    (SpawnMemoryTypeOrStore::StoreAndMemory(store, memory), None)
                }
            };

            let (env, store) = WasiFunctionEnv::new_with_store(
                task.module,
                task.env,
                task.globals,
                make_memory,
                task.update_layout,
                task.call_initialize,
                instance_group_data,
            )?;
            Ok((env, store, task.callbacks))
        }

        let (sx, rx) = std::sync::mpsc::channel();

        if task.callbacks.trigger.is_some() {
            tracing::trace!("spawning task_wasm trigger in dedicated thread");
            spawn_wasm_task(move || {
                let (mut ctx, mut store, callbacks) = match env_and_store(task) {
                    Ok(x) => {
                        sx.send(Ok(())).unwrap();
                        x
                    }
                    Err(c) => {
                        tracing::error!("failed to prepare environment for task_wasm trigger: {c}");
                        sx.send(Err(c)).unwrap();
                        return;
                    }
                };

                let result = {
                    let mut trigger = (callbacks.trigger.unwrap())();
                    let pre_run = callbacks.pre_run;
                    let ctx = &mut ctx;
                    let store = &mut store;
                    block_on(async move {
                        // We wait for either the trigger or for a snapshot to take place
                        let result = loop {
                            let env = ctx.data(store);
                            break tokio::select! {
                                r = &mut trigger => r,
                                _ = env.thread.wait_for_signal() => {
                                    tracing::debug!("wait-for-signal(triggered)");
                                    let mut ctx = ctx.env.clone().into_mut(store);
                                    if let Err(err) =
                                        crate::WasiEnv::do_pending_link_operations(
                                            &mut ctx,
                                            false
                                        ).and_then(|()|
                                            crate::WasiEnv::process_signals_and_exit(&mut ctx)
                                        )
                                    {
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
                                    let mut ctx = ctx.env.clone().into_mut(store);
                                    crate::os::task::WasiProcessInner::do_checkpoints_from_outside(&mut ctx);
                                    continue;
                                }
                            };
                        };

                        if let Some(pre_run) = pre_run {
                            pre_run(ctx, store).await;
                        }

                        result
                    })
                };

                // Invoke the callback
                (callbacks.run)(TaskWasmRunProperties {
                    ctx,
                    store,
                    trigger_result: Some(result),
                    recycle: callbacks.recycle,
                });
            })?;
        } else {
            tracing::trace!("spawning task_wasm in dedicated thread");

            // Run the callback on a dedicated thread
            spawn_wasm_task(move || {
                tracing::trace!("task_wasm started in dedicated thread");
                let (mut ctx, mut store, callbacks) = match env_and_store(task) {
                    Ok(x) => {
                        sx.send(Ok(())).unwrap();
                        x
                    }
                    Err(c) => {
                        sx.send(Err(c)).unwrap();
                        return;
                    }
                };

                if let Some(pre_run) = callbacks.pre_run {
                    block_on(pre_run(&mut ctx, &mut store));
                }

                // Invoke the callback
                (callbacks.run)(TaskWasmRunProperties {
                    ctx,
                    store,
                    trigger_result: None,
                    recycle: callbacks.recycle,
                });
            })?;
        }

        rx.recv()
            .map_err(|_| WasiThreadError::InvalidWasmContext)??;

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
pub struct SleepNow {
    abort_handle: Option<tokio::task::AbortHandle>,
}

impl SleepNow {
    pub async fn enter(
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

#[cfg(all(test, feature = "sys"))]
mod tests {
    use super::*;
    use crate::{WasiEnv, runtime::PluggableRuntime};
    use std::sync::mpsc;
    use wasmer::{Module, Store};

    #[test]
    fn host_thread_creation_failure_is_retryable() {
        let error = WasiThreadError::ThreadCreateFailed(Arc::new(std::io::Error::from(
            std::io::ErrorKind::WouldBlock,
        )));
        assert_eq!(
            wasmer_wasix_types::wasi::Errno::from(error),
            wasmer_wasix_types::wasi::Errno::Again
        );
    }

    #[test]
    fn wasm_tasks_start_when_the_blocking_pool_is_full() {
        for with_trigger in [false, true] {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let _entered = rt.enter();
            let manager = Arc::new(TokioTaskManager {
                rt: rt.handle().clone().into(),
                pool: Arc::new(ThreadPool {
                    inner: rusty_pool::Builder::new().core_size(1).max_size(1).build(),
                }),
            });
            let (occupied_tx, occupied_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            manager.pool.execute(move || {
                occupied_tx.send(()).unwrap();
                let _ = release_rx.recv();
            });
            occupied_rx.recv_timeout(Duration::from_secs(10)).unwrap();

            let store = Store::default();
            let module = Module::new(
                &store,
                r#"(module (memory (export "memory") 1) (func (export "_start")))"#,
            )
            .unwrap();
            let mut runtime = PluggableRuntime::new(manager.clone());
            runtime.set_engine(store.engine().clone());
            let env = WasiEnv::builder("task-pool-regression")
                .runtime(Arc::new(runtime))
                .build()
                .unwrap();
            let (ran_tx, ran_rx) = mpsc::channel();
            let mut task = TaskWasm::new(
                Box::new(move |props| {
                    assert_eq!(props.trigger_result.is_some(), with_trigger);
                    ran_tx.send(()).unwrap();
                }),
                env,
                module,
                false,
                false,
            );
            if with_trigger {
                task = task.with_trigger(Box::new(|| Box::pin(async { Ok(bytes::Bytes::new()) })));
            }
            let (started_tx, started_rx) = mpsc::channel();
            let caller = std::thread::spawn(move || {
                started_tx.send(manager.task_wasm(task)).unwrap();
            });
            let started = started_rx.recv_timeout(Duration::from_secs(10));
            let ran = ran_rx.recv_timeout(Duration::from_secs(10));
            // Release the worker even on failure, so the old implementation can exit.
            release_tx.send(()).unwrap();
            caller.join().unwrap();
            started
                .expect("Wasm task initialization waited for a pool worker")
                .unwrap();
            ran.expect("Wasm callback waited for a pool worker");
        }
    }
}
