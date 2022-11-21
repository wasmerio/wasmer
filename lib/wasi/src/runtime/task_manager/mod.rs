// TODO: should be behind a different , tokio specific feature flag.
#[cfg(feature = "sys-thread")]
pub mod tokio;

use std::pin::Pin;

use futures::Future;
use wasmer::{vm::VMMemory, MemoryType, Module, Store};
#[cfg(feature = "sys")]
use wasmer_types::MemoryStyle;

use crate::{WasiCallingId, WasiThreadError};

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
#[allow(unused_variables)]
pub trait VirtualTaskManager: std::fmt::Debug + Send + Sync + 'static {
    /// Invokes whenever a WASM thread goes idle. In some runtimes (like singlethreaded
    /// execution environments) they will need to do asynchronous work whenever the main
    /// thread goes idle and this is the place to hook for that.
    fn sleep_now(
        &self,
        _id: WasiCallingId,
        ms: u128,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + Sync + 'static>>;

    /// Starts an asynchronous task that will run on a shared worker pool
    /// This task must not block the execution or it could cause a deadlock
    fn task_shared(
        &self,
        task: Box<
            dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> + Send + 'static,
        >,
    ) -> Result<(), WasiThreadError>;

    /// Starts an asynchronous task on the local thread (by running it in a runtime)
    // TODO: return output future?
    // TODO: should be fallible
    fn block_on_generic<'a>(&self, task: Pin<Box<dyn Future<Output = ()> + 'a>>);

    /// Starts an asynchronous task on the local thread (by running it in a runtime)
    // TODO: should be fallible
    fn enter<'a>(&'a self) -> Box<dyn std::any::Any + 'a>;

    /// Starts an asynchronous task will will run on a dedicated thread
    /// pulled from the worker pool that has a stateful thread local variable
    /// It is ok for this task to block execution and any async futures within its scope
    fn task_wasm(
        &self,
        task: Box<dyn FnOnce(Store, Module, Option<VMMemory>) + Send + 'static>,
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

    /// Starts an asynchronous task will will run on a dedicated thread
    /// pulled from the worker pool. It is ok for this task to block execution
    /// and any async futures within its scope
    fn task_dedicated_async(
        &self,
        task: Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + 'static>> + Send + 'static>,
    ) -> Result<(), WasiThreadError>;

    /// Returns the amount of parallelism that is possible on this platform
    fn thread_parallelism(&self) -> Result<usize, WasiThreadError>;
}

/// A no-op taskmanager that does not support any spawning operations.
#[derive(Clone, Debug)]
pub struct StubTaskManager;

impl VirtualTaskManager for StubTaskManager {
    #[allow(unused_variables)]
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

    #[allow(unused_variables)]
    fn task_shared(
        &self,
        task: Box<
            dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> + Send + 'static,
        >,
    ) -> Result<(), WasiThreadError> {
        Err(WasiThreadError::Unsupported)
    }

    #[allow(unused_variables)]
    fn block_on_generic<'a>(&self, task: Pin<Box<dyn Future<Output = ()> + 'a>>) {
        unimplemented!("asynchronous operations are not supported on this task manager");
    }

    #[allow(unused_variables)]
    fn enter(&self) -> Box<dyn std::any::Any> {
        unimplemented!("asynchronous operations are not supported on this task manager");
    }

    #[allow(unused_variables)]
    fn task_wasm(
        &self,
        task: Box<dyn FnOnce(Store, Module, Option<VMMemory>) + Send + 'static>,
        store: Store,
        module: Module,
        spawn_type: SpawnType,
    ) -> Result<(), WasiThreadError> {
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
    fn task_dedicated_async(
        &self,
        task: Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + 'static>> + Send + 'static>,
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
        // TODO: evaluate if this is a reasonable default impl.
        let (tx, rx) = std::sync::mpsc::channel();
        self.block_on_generic(Box::pin(async move {
            let ret = task.await;
            tx.send(ret).unwrap();
        }));
        rx.recv().unwrap()
    }
}

/// Generic utility methods for VirtualTaskManager
pub trait VirtualTaskManagerExt {
    fn block_on<'a, A>(&self, task: impl Future<Output = A> + 'a) -> A;
}

impl<'a, T: VirtualTaskManager> VirtualTaskManagerExt for &'a T {
    fn block_on<'x, A>(&self, task: impl Future<Output = A> + 'x) -> A {
        let (tx, rx) = std::sync::mpsc::channel();
        self.block_on_generic(Box::pin(async move {
            let ret = task.await;
            tx.send(ret).unwrap();
        }));
        rx.recv().unwrap()
    }
}

impl<T: VirtualTaskManager + ?Sized> VirtualTaskManagerExt for std::sync::Arc<T> {
    fn block_on<'x, A>(&self, task: impl Future<Output = A> + 'x) -> A {
        let (tx, rx) = std::sync::mpsc::channel();
        self.block_on_generic(Box::pin(async move {
            let ret = task.await;
            tx.send(ret).unwrap();
        }));
        rx.recv().unwrap()
    }
}
