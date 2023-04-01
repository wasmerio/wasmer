use std::pin::Pin;

use futures::Future;
#[cfg(feature = "sys-thread")]
use tokio::runtime::{Builder, Runtime};
use wasmer::{vm::VMMemory, Module, Store};

use crate::{WasiCallingId, WasiThreadError};

use super::{SpawnType, VirtualTaskManager};

#[derive(Debug)]
pub struct ThreadTaskManager {
    /// This is the tokio runtime used for ASYNC operations that is
    /// used for non-javascript environments
    #[cfg(feature = "sys-thread")]
    runtime: std::sync::Arc<Runtime>,
}

impl Default for ThreadTaskManager {
    #[cfg(feature = "sys-thread")]
    fn default() -> Self {
        let runtime: std::sync::Arc<Runtime> =
            std::sync::Arc::new(Builder::new_current_thread().enable_all().build().unwrap());
        Self { runtime }
    }

    #[cfg(not(feature = "sys-thread"))]
    fn default() -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(100);
        Self {
            periodic_wakers: Arc::new(Mutex::new((Vec::new(), tx))),
        }
    }
}

#[allow(unused_variables)]
#[cfg(not(feature = "sys-thread"))]
impl VirtualTaskManager for ThreadTaskManager {
    /// Invokes whenever a WASM thread goes idle. In some runtimes (like singlethreaded
    /// execution environments) they will need to do asynchronous work whenever the main
    /// thread goes idle and this is the place to hook for that.
    fn sleep_now(
        &self,
        id: WasiCallingId,
        ms: u128,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + Sync + 'static>> {
        if ms == 0 {
            std::thread::yield_now();
        } else {
            std::thread::sleep(std::time::Duration::from_millis(ms as u64));
        }
        Box::pin(async move {})
    }

    /// Starts an asynchronous task that will run on a shared worker pool
    /// This task must not block the execution or it could cause a deadlock
    fn task_shared(
        &self,
        task: Box<
            dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> + Send + 'static,
        >,
    ) -> Result<(), WasiThreadError> {
        Err(WasiThreadError::Unsupported)
    }

    /// Starts an asynchronous task on the local thread (by running it in a runtime)
    fn block_on(&self, task: Pin<Box<dyn Future<Output = ()>>>) {
        unimplemented!("asynchronous operations are not supported on this task manager");
    }

    /// Enters the task runtime
    fn enter(&self) -> Box<dyn std::any::Any> {
        unimplemented!("asynchronous operations are not supported on this task manager");
    }

    /// Starts an asynchronous task will will run on a dedicated thread
    /// pulled from the worker pool that has a stateful thread local variable
    /// It is ok for this task to block execution and any async futures within its scope
    fn task_wasm(
        &self,
        task: Box<dyn FnOnce(Store, Module, Option<VMMemory>) + Send + 'static>,
        store: Store,
        module: Module,
        spawn_type: SpawnType,
    ) -> Result<(), WasiThreadError> {
        Err(WasiThreadError::Unsupported)
    }

    /// Starts an asynchronous task will will run on a dedicated thread
    /// pulled from the worker pool. It is ok for this task to block execution
    /// and any async futures within its scope
    fn task_dedicated(
        &self,
        task: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), WasiThreadError> {
        Err(WasiThreadError::Unsupported)
    }

    /// Returns the amount of parallelism that is possible on this platform
    fn thread_parallelism(&self) -> Result<usize, WasiThreadError> {
        Err(WasiThreadError::Unsupported)
    }
}

#[cfg(feature = "sys-thread")]
impl VirtualTaskManager for ThreadTaskManager {
    /// See [`VirtualTaskManager::sleep_now`].
    fn sleep_now(
        &self,
        _id: WasiCallingId,
        ms: u128,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + Sync + 'static>> {
        Box::pin(async move {
            if ms == 0 {
                tokio::task::yield_now().await;
            } else {
                tokio::time::sleep(std::time::Duration::from_millis(ms as u64)).await;
            }
        })
    }

    /// Starts an asynchronous task will will run on a dedicated thread
    /// pulled from the worker pool that has a stateful thread local variable
    /// It is ok for this task to block execution and any async futures within its scope
    fn task_shared(
        &self,
        task: Box<
            dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> + Send + 'static,
        >,
    ) -> Result<(), WasiThreadError> {
        self.runtime.spawn(async move {
            let fut = task();
            fut.await
        });
        Ok(())
    }

    /// See [`VirtualTaskManager::block_on`].
    fn block_on<'a>(&self, task: Pin<Box<dyn Future<Output = ()> + 'a>>) {
        let _guard = self.runtime.enter();
        self.runtime.block_on(async move {
            task.await;
        });
    }

    /// See [`VirtualTaskManager::enter`].
    fn enter<'a>(&'a self) -> Box<dyn std::any::Any + 'a> {
        Box::new(self.runtime.enter())
    }

    /// See [`VirtualTaskManager::enter`].
    fn task_wasm(
        &self,
        task: Box<dyn FnOnce(Store, Module, Option<VMMemory>) + Send + 'static>,
        store: Store,
        module: Module,
        spawn_type: SpawnType,
    ) -> Result<(), WasiThreadError> {
        use wasmer::vm::VMSharedMemory;

        let memory: Option<VMMemory> = match spawn_type {
            SpawnType::CreateWithType(mem) => Some(
                VMSharedMemory::new(&mem.ty, &mem.style)
                    .map_err(|err| {
                        tracing::error!("failed to create memory - {}", err);
                    })
                    .unwrap()
                    .into(),
            ),
            SpawnType::NewThread(mem) => Some(mem),
            SpawnType::Create => None,
        };

        std::thread::spawn(move || {
            // Invoke the callback
            task(store, module, memory);
        });
        Ok(())
    }

    /// See [`VirtualTaskManager::task_dedicated`].
    fn task_dedicated(
        &self,
        task: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), WasiThreadError> {
        std::thread::spawn(move || {
            task();
        });
        Ok(())
    }

    /// See [`VirtualTaskManager::thread_parallelism`].
    fn thread_parallelism(&self) -> Result<usize, WasiThreadError> {
        Ok(std::thread::available_parallelism()
            .map(|a| usize::from(a))
            .unwrap_or(8))
    }
}
