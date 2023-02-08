use std::{pin::Pin, time::Duration};

use futures::Future;
#[cfg(feature = "sys-thread")]
use tokio::runtime::{Builder, Runtime};
use wasmer::vm::VMMemory;
use wasmer_vm::VMSharedMemory;

use crate::os::task::thread::WasiThreadError;

use super::{SpawnType, VirtualTaskManager};

/// A task manager that uses tokio to spawn tasks.
#[derive(Clone, Debug)]
pub struct TokioTaskManager(std::sync::Arc<Runtime>);

impl TokioTaskManager {
    pub fn new(rt: std::sync::Arc<Runtime>) -> Self {
        Self(rt)
    }

    pub fn runtime_handle(&self) -> tokio::runtime::Handle {
        self.0.handle().clone()
    }
}

impl Default for TokioTaskManager {
    fn default() -> Self {
        let runtime: std::sync::Arc<Runtime> =
            std::sync::Arc::new(Builder::new_current_thread().enable_all().build().unwrap());
        Self(runtime)
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
        self.0.spawn(async move {
            let fut = task();
            fut.await
        });
        Ok(())
    }

    /// See [`VirtualTaskManager::runtime`].
    fn runtime(&self) -> &Runtime {
        &self.0
    }

    /// See [`VirtualTaskManager::block_on`].
    #[allow(dyn_drop)]
    fn runtime_enter<'g>(&'g self) -> Box<dyn std::ops::Drop + 'g> {
        Box::new(TokioRuntimeGuard {
            inner: self.0.enter(),
        })
    }

    /// See [`VirtualTaskManager::enter`].
    fn task_wasm(&self, task: Box<dyn FnOnce() + Send + 'static>) -> Result<(), WasiThreadError> {
        self.0.spawn_blocking(move || {
            // Invoke the callback
            task();
        });
        Ok(())
    }

    /// See [`VirtualTaskManager::task_dedicated`].
    fn task_dedicated(
        &self,
        task: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), WasiThreadError> {
        self.0.spawn_blocking(move || {
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
