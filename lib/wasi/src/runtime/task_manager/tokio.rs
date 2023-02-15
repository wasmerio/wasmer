use std::{pin::Pin, time::Duration};

use futures::Future;
use tokio::runtime::Handle;
use wasmer::vm::{VMMemory, VMSharedMemory};

use crate::os::task::thread::WasiThreadError;

use super::{SpawnType, VirtualTaskManager};

/// A task manager that uses tokio to spawn tasks.
#[derive(Clone, Debug)]
pub struct TokioTaskManager(Handle);

impl TokioTaskManager {
    pub fn new(rt: Handle) -> Self {
        Self(rt)
    }

    pub fn runtime_handle(&self) -> tokio::runtime::Handle {
        self.0.clone()
    }

    /// Shared tokio [`Runtime`] that is used by default.
    ///
    /// This exists because a tokio runtime is heavy, and there should not be many
    /// independent ones in a process.
    pub fn shared() -> Self {
        static GLOBAL_RUNTIME: once_cell::sync::Lazy<(Option<tokio::runtime::Runtime>, Handle)> =
            once_cell::sync::Lazy::new(|| {
                if let Ok(handle) = tokio::runtime::Handle::try_current() {
                    (None, handle)
                } else {
                    #[cfg(feature = "sys")]
                    {
                        let rt = tokio::runtime::Runtime::new().unwrap();
                        let handle = rt.handle().clone();
                        (Some(rt), handle)
                    }
                    #[cfg(not(feature = "sys"))]
                    {
                        let rt = tokio::runtime::Runtime::new().unwrap();
                        let handle = rt.handle().clone();
                        (Some(rt), handle)
                    }
                }
            });

        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            Self(handle)
        } else {
            Self(GLOBAL_RUNTIME.1.clone())
        }
    }
}

impl Default for TokioTaskManager {
    fn default() -> Self {
        Self::shared()
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
    fn runtime(&self) -> &Handle {
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
