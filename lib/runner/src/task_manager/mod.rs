// TODO: should be behind a different , tokio specific feature flag.
#[cfg(feature = "sys-thread")]
pub mod tokio;

use std::ops::Deref;
use std::task::{Context, Poll};
use std::{pin::Pin, time::Duration};

use bytes::Bytes;
use futures::future::BoxFuture;
use futures::Future;
use wasmer::{AsStoreMut, AsStoreRef, Memory, MemoryType, Module, Store, StoreMut, StoreRef};
use wasmer_wasix_types::wasi::{Errno, ExitCode};

use crate::os::task::thread::WasiThreadError;
use crate::{WasiEnv, WasiFunctionEnv, WasiThread};
use crate::snapshot::{InstanceSnapshot, capture_snapshot};

/// Future that will be polled by asyncify methods
#[doc(hidden)]
pub type AsyncifyFuture = dyn Future<Output = Bytes> + Send + Sync + 'static;

pub use virtual_mio::waker::*;

#[derive(Debug)]
pub enum SpawnMemoryType<'a> {
    CreateMemory,
    CreateMemoryOfType(MemoryType),
    // TODO: is there a way to get rid of the memory reference
    ShareMemory(Memory, StoreRef<'a>),
    // TODO: is there a way to get rid of the memory reference
    // Note: The message sender is triggered once the memory
    // has been copied, this makes sure its not modified until
    // its been properly copied
    CopyMemory(Memory, StoreRef<'a>),
}

pub type WasmResumeTask = dyn FnOnce(WasiFunctionEnv, Store, Bytes) + Send + 'static;

pub type WasmResumeTrigger = dyn FnOnce() -> Pin<Box<dyn Future<Output = Result<Bytes, ExitCode>> + Send + 'static>>
    + Send
    + Sync;

/// The properties passed to the task
#[derive(Debug)]
pub struct TaskWasmRunProperties {
    pub ctx: WasiFunctionEnv,
    pub store: Store,
    /// The result of the asynchronous trigger serialized into bytes using the bincode serializer
    /// When no trigger is associated with the run operation (i.e. spawning threads) then this will be None.
    /// (if the trigger returns an ExitCode then the WASM process will be terminated without resuming)
    pub trigger_result: Option<Result<Bytes, ExitCode>>,
}

/// Callback that will be invoked
pub type TaskWasmRun = dyn FnOnce(TaskWasmRunProperties) + Send + 'static;

/// Callback that will be invoked
pub type TaskExecModule = dyn FnOnce(Module) + Send + 'static;

/// Represents a WASM task that will be executed on a dedicated thread
pub struct TaskWasm<'a, 'b> {
    pub run: Box<TaskWasmRun>,
    pub env: WasiEnv,
    pub module: Module,
    pub snapshot: Option<&'b InstanceSnapshot>,
    pub spawn_type: SpawnMemoryType<'a>,
    pub trigger: Option<Box<WasmResumeTrigger>>,
    pub update_layout: bool,
}
impl<'a, 'b> TaskWasm<'a, 'b> {
    pub fn new(run: Box<TaskWasmRun>, env: WasiEnv, module: Module, update_layout: bool) -> Self {
        Self {
            run,
            env,
            module,
            snapshot: None,
            spawn_type: SpawnMemoryType::CreateMemory,
            trigger: None,
            update_layout,
        }
    }

    pub fn with_memory(mut self, spawn_type: SpawnMemoryType<'a>) -> Self {
        self.spawn_type = spawn_type;
        self
    }

    pub fn with_snapshot(mut self, snapshot: &'b InstanceSnapshot) -> Self {
        self.snapshot.replace(snapshot);
        self
    }

    pub fn with_trigger(mut self, trigger: Box<WasmResumeTrigger>) -> Self {
        self.trigger.replace(trigger);
        self
    }
}

/// A task executor backed by a thread pool.
///
/// ## Thread Safety
///
/// Due to [#4158], it is possible to pass non-thread safe objects across
/// threads by capturing them in the task passed to
/// [`VirtualTaskManager::task_shared()`] or
/// [`VirtualTaskManager::task_dedicated()`].
///
/// If your task needs access to a [`wasmer::Module`], [`wasmer::Memory`], or
/// [`wasmer::Instance`], it should explicitly transfer the objects using
/// either [`VirtualTaskManager::task_wasm()`] when in syscall context or
/// [`VirtualTaskManager::spawn_with_module()`] for higher level code.
///
/// [#4158]: https://github.com/wasmerio/wasmer/issues/4158
#[allow(unused_variables)]
pub trait VirtualTaskManager: std::fmt::Debug + Send + Sync + 'static {
    /// Build a new Webassembly memory.
    ///
    /// May return `None` if the memory can just be auto-constructed.
    fn build_memory(
        &self,
        mut store: &mut StoreMut,
        spawn_type: SpawnMemoryType,
    ) -> Result<Option<Memory>, WasiThreadError> {
        match spawn_type {
            SpawnMemoryType::CreateMemoryOfType(mut ty) => {
                ty.shared = true;

                // Note: If memory is shared, maximum needs to be set in the
                // browser otherwise creation will fail.
                let _ = ty.maximum.get_or_insert(wasmer_types::Pages::max_value());

                let mem = Memory::new(&mut store, ty).map_err(|err| {
                    tracing::error!(
                        error = &err as &dyn std::error::Error,
                        memory_type=?ty,
                        "could not create memory",
                    );
                    WasiThreadError::MemoryCreateFailed(err)
                })?;
                Ok(Some(mem))
            }
            SpawnMemoryType::ShareMemory(mem, old_store) => {
                let mem = mem.share_in_store(&old_store, store).map_err(|err| {
                    tracing::warn!(
                        error = &err as &dyn std::error::Error,
                        "could not clone memory",
                    );
                    WasiThreadError::MemoryCreateFailed(err)
                })?;
                Ok(Some(mem))
            }
            SpawnMemoryType::CopyMemory(mem, old_store) => {
                let mem = mem.copy_to_store(&old_store, store).map_err(|err| {
                    tracing::warn!(
                        error = &err as &dyn std::error::Error,
                        "could not copy memory",
                    );
                    WasiThreadError::MemoryCreateFailed(err)
                })?;
                Ok(Some(mem))
            }
            SpawnMemoryType::CreateMemory => Ok(None),
        }
    }

    /// Pause the current thread of execution.
    ///
    /// This is typically invoked whenever a WASM thread goes idle. Besides
    /// acting as a platform-agnostic [`std::thread::sleep()`], this also gives
    /// the runtime a chance to do asynchronous work like pumping an event
    /// loop.
    fn sleep_now(
        &self,
        time: Duration,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + Sync + 'static>>;

    /// Run an asynchronous operation on the thread pool.
    ///
    /// This task must not block execution or it could cause deadlocks.
    ///
    /// See the "Thread Safety" documentation on [`VirtualTaskManager`] for
    /// limitations on what a `task` can and can't contain.
    fn task_shared(
        &self,
        task: Box<dyn FnOnce() -> BoxFuture<'static, ()> + Send + 'static>,
    ) -> Result<(), WasiThreadError>;

    /// Run a blocking WebAssembly operation on the thread pool.
    ///
    /// This is primarily used inside the context of a syscall and allows
    /// the transfer of things like [`wasmer::Module`] across threads.
    fn task_wasm(&self, task: TaskWasm) -> Result<(), WasiThreadError>;

    /// Run a blocking operation on the thread pool.
    ///
    /// It is okay for this task to block execution and any async futures within
    /// its scope.
    fn task_dedicated(
        &self,
        task: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), WasiThreadError>;

    /// Returns the amount of parallelism that is possible on this platform.
    fn thread_parallelism(&self) -> Result<usize, WasiThreadError>;

    /// Schedule a blocking task to run on the threadpool, explicitly
    /// transferring a [`Module`] to the task.
    ///
    /// This should be preferred over [`VirtualTaskManager::task_dedicated()`]
    /// where possible because [`wasmer::Module`] is actually `!Send` in the
    /// browser and can only be transferred to background threads via
    /// an explicit `postMessage()`. See [#4158] for more details.
    ///
    /// This is very similar to [`VirtualTaskManager::task_wasm()`], but
    /// intended for use outside of a syscall context. For example, when you are
    /// running in the browser and want to run a WebAssembly module in the
    /// background.
    ///
    /// [#4158]: https://github.com/wasmerio/wasmer/issues/4158
    fn spawn_with_module(
        &self,
        module: Module,
        task: Box<dyn FnOnce(Module) + Send + 'static>,
    ) -> Result<(), WasiThreadError> {
        // Note: Ideally, this function and task_wasm() would be superseded by
        // a more general mechanism for transferring non-thread safe values
        // to the thread pool.
        self.task_dedicated(Box::new(move || task(module)))
    }
}

impl<D, T> VirtualTaskManager for D
where
    D: Deref<Target = T> + std::fmt::Debug + Send + Sync + 'static,
    T: VirtualTaskManager + ?Sized,
{
    fn build_memory(
        &self,
        store: &mut StoreMut,
        spawn_type: SpawnMemoryType,
    ) -> Result<Option<Memory>, WasiThreadError> {
        (**self).build_memory(store, spawn_type)
    }

    fn sleep_now(
        &self,
        time: Duration,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + Sync + 'static>> {
        (**self).sleep_now(time)
    }

    fn task_shared(
        &self,
        task: Box<dyn FnOnce() -> BoxFuture<'static, ()> + Send + 'static>,
    ) -> Result<(), WasiThreadError> {
        (**self).task_shared(task)
    }

    fn task_wasm(&self, task: TaskWasm) -> Result<(), WasiThreadError> {
        (**self).task_wasm(task)
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

    fn spawn_with_module(
        &self,
        module: Module,
        task: Box<dyn FnOnce(Module) + Send + 'static>,
    ) -> Result<(), WasiThreadError> {
        (**self).spawn_with_module(module, task)
    }
}

impl dyn VirtualTaskManager {
    /// Starts an WebAssembly task will will run on a dedicated thread
    /// pulled from the worker pool that has a stateful thread local variable
    /// After the poller has succeeded
    #[doc(hidden)]
    pub unsafe fn resume_wasm_after_poller(
        &self,
        task: Box<WasmResumeTask>,
        ctx: WasiFunctionEnv,
        mut store: Store,
        trigger: Pin<Box<AsyncifyFuture>>,
    ) -> Result<(), WasiThreadError> {
        // This poller will process any signals when the main working function is idle
        struct AsyncifyPollerOwned {
            thread: WasiThread,
            trigger: Pin<Box<AsyncifyFuture>>,
        }
        impl Future for AsyncifyPollerOwned {
            type Output = Result<Bytes, ExitCode>;
            fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                let work = self.trigger.as_mut();
                Poll::Ready(if let Poll::Ready(res) = work.poll(cx) {
                    Ok(res)
                } else if let Some(forced_exit) = self.thread.try_join() {
                    return Poll::Ready(Err(forced_exit.unwrap_or_else(|err| {
                        tracing::debug!("exit runtime error - {}", err);
                        Errno::Child.into()
                    })));
                } else {
                    return Poll::Pending;
                })
            }
        }

        let snapshot = capture_snapshot(&mut store.as_store_mut());
        let env = ctx.data(&store);
        let module = env.inner().module_clone();
        let memory = env.inner().memory_clone();
        let thread = env.thread.clone();
        let env = env.clone();

        let thread_inner = thread.clone();
        self.task_wasm(
            TaskWasm::new(
                Box::new(move |props| {
                    let result = props
                        .trigger_result
                        .expect("If there is no result then its likely the trigger did not run");
                    let result = match result {
                        Ok(r) => r,
                        Err(exit_code) => {
                            thread.set_status_finished(Ok(exit_code));
                            return;
                        }
                    };
                    task(props.ctx, props.store, result)
                }),
                env.clone(),
                module,
                false,
            )
            .with_memory(SpawnMemoryType::ShareMemory(memory, store.as_store_ref()))
            .with_snapshot(&snapshot)
            .with_trigger(Box::new(move || {
                Box::pin(async move {
                    let mut poller = AsyncifyPollerOwned {
                        thread: thread_inner,
                        trigger,
                    };
                    let res = Pin::new(&mut poller).await;
                    let res = match res {
                        Ok(res) => res,
                        Err(exit_code) => {
                            env.thread.set_status_finished(Ok(exit_code));
                            return Err(exit_code);
                        }
                    };

                    tracing::trace!("deep sleep woken - {:?}", res);
                    Ok(res)
                })
            })),
        )
    }
}

/// Generic utility methods for VirtualTaskManager
pub trait VirtualTaskManagerExt {
    /// Runs the work in the background via the task managers shared background
    /// threads while blocking the current execution until it finishs
    fn spawn_and_block_on<A>(&self, task: impl Future<Output = A> + Send + 'static) -> A
    where
        A: Send + 'static;
}

impl<D, T> VirtualTaskManagerExt for D
where
    D: Deref<Target = T>,
    T: VirtualTaskManager + ?Sized,
{
    /// Runs the work in the background via the task managers shared background
    /// threads while blocking the current execution until it finishs
    fn spawn_and_block_on<A>(&self, task: impl Future<Output = A> + Send + 'static) -> A
    where
        A: Send + 'static,
    {
        let (work_tx, mut work_rx) = ::tokio::sync::mpsc::unbounded_channel();
        let work = Box::pin(async move {
            let ret = task.await;
            work_tx.send(ret).ok();
        });
        self.task_shared(Box::new(move || work)).unwrap();
        InlineWaker::block_on(work_rx.recv()).unwrap()
    }
}
