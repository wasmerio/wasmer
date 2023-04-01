use std::{pin::Pin, sync::Arc, time::Duration};

use futures::Future;
use tokio::runtime::Handle;
use wasmer::{
    vm::{VMMemory, VMSharedMemory},
    Module, Store,
};

use crate::os::task::thread::WasiThreadError;

use super::{SpawnType, VirtualTaskManager};

/// A task manager that uses tokio to spawn tasks.
#[derive(Clone, Debug)]
pub struct TokioTaskManager {
    /// Reference to the handle that provides access to the runtime
    handle: Handle,
    /// When this task manager owns the runtime this field is set
    /// otherwise only the handle is set
    _runtime: Option<Arc<tokio::runtime::Runtime>>,
}

impl TokioTaskManager {
    pub fn new() -> Self {
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.into()
        } else {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.into()
        }
    }

    pub fn runtime_handle(&self) -> tokio::runtime::Handle {
        self.handle.clone()
    }
}

impl From<Handle> for TokioTaskManager {
    fn from(handle: Handle) -> Self {
        Self {
            handle,
            _runtime: None,
        }
    }
}

impl From<&Arc<tokio::runtime::Runtime>> for TokioTaskManager {
    fn from(rt: &Arc<tokio::runtime::Runtime>) -> Self {
        Self {
            handle: rt.handle().clone(),
            _runtime: Some(rt.clone()),
        }
    }
}

impl From<tokio::runtime::Runtime> for TokioTaskManager {
    fn from(rt: tokio::runtime::Runtime) -> Self {
        let rt = Arc::new(rt);
        Self {
            handle: rt.handle().clone(),
            _runtime: Some(rt),
        }
    }
}

struct TokioRuntimeGuard<'g> {
    #[allow(unused)]
    inner: tokio::runtime::EnterGuard<'g>,
}
impl<'g> Drop for TokioRuntimeGuard<'g> {
    fn drop(&mut self) {}
}

#[async_trait::async_trait]
impl VirtualTaskManager for TokioTaskManager {
    fn build_memory(&self, spawn_type: SpawnType) -> Result<Option<VMMemory>, WasiThreadError> {
        match spawn_type {
            SpawnType::CreateWithType(mem) => VMSharedMemory::new(&mem.ty, &mem.style)
                .map_err(|err| {
                    tracing::error!("could not create memory: {err}");
                    WasiThreadError::MemoryCreateFailed
                })
                .map(|m| Some(m.into())),
            SpawnType::NewThread(mem) => Ok(Some(mem)),
            SpawnType::Create => Ok(None),
        }
    }

    /// See [`VirtualTaskManager::sleep_now`].
    async fn sleep_now(&self, time: Duration) {
        if time == Duration::ZERO {
            tokio::task::yield_now().await;
        } else {
            tokio::time::sleep(time).await;
        }
    }

    /// See [`VirtualTaskManager::task_shared`].
    fn task_shared(
        &self,
        task: Box<
            dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> + Send + 'static,
        >,
    ) -> Result<(), WasiThreadError> {
        self.handle.spawn(async move {
            let fut = task();
            fut.await
        });
        Ok(())
    }

    /// See [`VirtualTaskManager::runtime`].
    fn runtime(&self) -> &Handle {
        &self.handle
    }

    /// See [`VirtualTaskManager::block_on`].
    #[allow(dyn_drop)]
    fn runtime_enter<'g>(&'g self) -> Box<dyn std::ops::Drop + 'g> {
        Box::new(TokioRuntimeGuard {
            inner: self.handle.enter(),
        })
    }

    /// See [`VirtualTaskManager::enter`].
    fn task_wasm(
        &self,
        task: Box<dyn FnOnce(Store, Module, Option<VMMemory>) + Send + 'static>,
        store: Store,
        module: Module,
        spawn_type: SpawnType,
    ) -> Result<(), WasiThreadError> {
        let memory = self.build_memory(spawn_type)?;
        self.handle.spawn_blocking(move || {
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
        self.handle.spawn_blocking(move || {
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
