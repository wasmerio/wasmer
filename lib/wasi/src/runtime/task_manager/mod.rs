// TODO: should be behind a different , tokio specific feature flag.
#[cfg(feature = "sys-thread")]
pub mod tokio;

use std::{ops::Deref, pin::Pin, time::Duration};

use ::tokio::runtime::Handle;
use futures::Future;
use wasmer::{Memory, MemoryType, Module, Store, StoreMut};

use crate::os::task::thread::WasiThreadError;

#[derive(Debug)]
pub struct SpawnedMemory {
    pub ty: MemoryType,
}

#[derive(Debug)]
pub enum SpawnType {
    Create,
    CreateWithType(SpawnedMemory),
    NewThread(Memory),
}

/// An implementation of task management
#[async_trait::async_trait]
#[allow(unused_variables)]
pub trait VirtualTaskManager: std::fmt::Debug + Send + Sync + 'static {
    /// Build a new Webassembly memory.
    ///
    /// May return `None` if the memory can just be auto-constructed.
    fn build_memory(
        &self,
        store: &mut StoreMut,
        spawn_type: SpawnType,
    ) -> Result<Option<Memory>, WasiThreadError>;

    /// Invokes whenever a WASM thread goes idle. In some runtimes (like singlethreaded
    /// execution environments) they will need to do asynchronous work whenever the main
    /// thread goes idle and this is the place to hook for that.
    async fn sleep_now(&self, time: Duration);

    /// Starts an asynchronous task that will run on a shared worker pool
    /// This task must not block the execution or it could cause a deadlock
    fn task_shared(
        &self,
        task: Box<
            dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> + Send + 'static,
        >,
    ) -> Result<(), WasiThreadError>;

    /// Returns a runtime that can be used for asynchronous tasks
    fn runtime(&self) -> &Handle;

    /// Enters a runtime context
    #[allow(dyn_drop)]
    fn runtime_enter<'g>(&'g self) -> Box<dyn std::ops::Drop + 'g>;

    /// Starts an asynchronous task will will run on a dedicated thread
    /// pulled from the worker pool that has a stateful thread local variable
    /// It is ok for this task to block execution and any async futures within its scope
    fn task_wasm(
        &self,
        task: Box<dyn FnOnce(Store, Module, Option<Memory>) + Send + 'static>,
        store: Store,
        module: Module,
        spawn_type: SpawnType,
    ) -> Result<(), WasiThreadError>;

    /// Starts an asynchronous task will will run on a dedicated thread
    /// pulled from the worker pool. It is ok for this task to block execution
    /// and any async futures within its scope
    fn task_dedicated(
        &self,
        task: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), WasiThreadError>;

    /// Returns the amount of parallelism that is possible on this platform
    fn thread_parallelism(&self) -> Result<usize, WasiThreadError>;
}

#[async_trait::async_trait]
impl<D, T> VirtualTaskManager for D
where
    D: Deref<Target = T> + std::fmt::Debug + Send + Sync + 'static,
    T: VirtualTaskManager + ?Sized,
{
    fn build_memory(
        &self,
        store: &mut StoreMut,
        spawn_type: SpawnType,
    ) -> Result<Option<Memory>, WasiThreadError> {
        (**self).build_memory(store, spawn_type)
    }

    async fn sleep_now(&self, time: Duration) {
        (**self).sleep_now(time).await
    }

    fn task_shared(
        &self,
        task: Box<
            dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> + Send + 'static,
        >,
    ) -> Result<(), WasiThreadError> {
        (**self).task_shared(task)
    }

    fn runtime(&self) -> &Handle {
        (**self).runtime()
    }

    #[allow(dyn_drop)]
    fn runtime_enter<'g>(&'g self) -> Box<dyn std::ops::Drop + 'g> {
        (**self).runtime_enter()
    }

    fn task_wasm(
        &self,
        task: Box<dyn FnOnce(Store, Module, Option<Memory>) + Send + 'static>,
        store: Store,
        module: Module,
        spawn_type: SpawnType,
    ) -> Result<(), WasiThreadError> {
        (**self).task_wasm(task, store, module, spawn_type)
    }

    fn task_dedicated(
        &self,
        task: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), WasiThreadError> {
        (**self).task_dedicated(task)
    }

    fn thread_parallelism(&self) -> Result<usize, WasiThreadError> {
        (**self).thread_parallelism()
    }
}

impl dyn VirtualTaskManager {
    /// Execute a future and return the output.
    /// This method blocks until the future is complete.
    // This needs to be a generic impl on `dyn T` because it is generic, and hence not object-safe.
    pub fn block_on<'a, A>(&self, task: impl Future<Output = A> + 'a) -> A {
        self.runtime().block_on(task)
    }
}

/// Generic utility methods for VirtualTaskManager
pub trait VirtualTaskManagerExt {
    fn block_on<'a, A>(&self, task: impl Future<Output = A> + 'a) -> A;
}

impl<D, T> VirtualTaskManagerExt for D
where
    D: Deref<Target = T>,
    T: VirtualTaskManager + ?Sized,
{
    fn block_on<'a, A>(&self, task: impl Future<Output = A> + 'a) -> A {
        self.runtime().block_on(task)
    }
}
