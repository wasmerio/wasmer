// TODO: should be behind a different , tokio specific feature flag.
#[cfg(feature = "sys-thread")]
pub mod tokio;

use std::{pin::Pin, time::Duration};

use ::tokio::runtime::Handle;
use futures::Future;
use wasmer::MemoryType;

#[cfg(feature = "sys")]
use wasmer_types::MemoryStyle;
use wasmer_vm::VMMemory;

use crate::os::task::thread::WasiThreadError;

#[derive(Debug)]
pub struct SpawnedMemory {
    pub ty: MemoryType,
    // TODO: don't put behind a feature (Option<MemoryStyle>?)
    #[cfg(feature = "sys")]
    pub style: MemoryStyle,
}

#[derive(Debug)]
pub enum SpawnType {
    Create,
    CreateWithType(SpawnedMemory),
    NewThread(VMMemory),
}

/// An implementation of task management
#[async_trait::async_trait]
#[allow(unused_variables)]
pub trait VirtualTaskManager: std::fmt::Debug + Send + Sync + 'static {
    /// Build a new Webassembly memory.
    ///
    /// May return `None` if the memory can just be auto-constructed.
    fn build_memory(&self, spawn_type: SpawnType) -> Result<Option<VMMemory>, WasiThreadError>;

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
    fn task_wasm(&self, task: Box<dyn FnOnce() + Send + 'static>) -> Result<(), WasiThreadError>;

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

/// A no-op taskmanager that does not support any spawning operations.
#[derive(Clone, Debug)]
pub struct StubTaskManager;

#[async_trait::async_trait]
impl VirtualTaskManager for StubTaskManager {
    fn build_memory(&self, _spawn_type: SpawnType) -> Result<Option<VMMemory>, WasiThreadError> {
        Err(WasiThreadError::Unsupported)
    }

    #[allow(unused_variables)]
    async fn sleep_now(&self, time: Duration) {
        if time == Duration::ZERO {
            std::thread::yield_now();
        } else {
            std::thread::sleep(time);
        }
    }

    #[allow(unused_variables)]
    fn task_shared(
        &self,
        task: Box<
            dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> + Send + 'static,
        >,
    ) -> Result<(), WasiThreadError> {
        Err(WasiThreadError::Unsupported)
    }

    fn runtime(&self) -> &Handle {
        unimplemented!("asynchronous operations are not supported on this task manager");
    }

    #[allow(dyn_drop)]
    #[allow(unused_variables)]
    fn runtime_enter<'g>(&'g self) -> Box<dyn std::ops::Drop + 'g> {
        unimplemented!("asynchronous operations are not supported on this task manager");
    }

    #[allow(unused_variables)]
    fn task_wasm(&self, task: Box<dyn FnOnce() + Send + 'static>) -> Result<(), WasiThreadError> {
        Err(WasiThreadError::Unsupported)
    }

    #[allow(unused_variables)]
    fn task_dedicated(
        &self,
        task: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), WasiThreadError> {
        Err(WasiThreadError::Unsupported)
    }

    #[allow(unused_variables)]
    fn thread_parallelism(&self) -> Result<usize, WasiThreadError> {
        Err(WasiThreadError::Unsupported)
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

impl<'a, T: VirtualTaskManager> VirtualTaskManagerExt for &'a T {
    fn block_on<'x, A>(&self, task: impl Future<Output = A> + 'x) -> A {
        self.runtime().block_on(task)
    }
}

impl<T: VirtualTaskManager + ?Sized> VirtualTaskManagerExt for std::sync::Arc<T> {
    fn block_on<'x, A>(&self, task: impl Future<Output = A> + 'x) -> A {
        self.runtime().block_on(task)
    }
}
