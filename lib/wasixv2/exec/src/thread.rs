//! Guest threads. A thread created with a shared address space is another instance of the
//! process's module on the same shared memory; the host's executor decides where it runs.
//!
//! The kernel creates the task in `thread-spawn` and asks for the start on the return-to-guest
//! path ([`GuestAction::StartThread`]). That path only knows the syscalls object and the calling
//! task, so every running instance registers its process image here under that pair.

use std::{
    collections::HashMap,
    future::Future,
    io,
    pin::pin,
    sync::{Arc, LazyLock, Mutex},
    task::{Context, Poll, Waker},
};

use wasixv2_host::Host;
use wasixv2_syscalls::{Caller, Control, Fault, LocalFuture, Syscalls, TaskId, UserMemory};
use wasmer::{Engine, FunctionEnv, Instance, Module, SharedMemory, Store, Value};

use crate::{ExecEnv, ExecError, Exited, generated};

/// The export a new thread starts at, as `wasixv2_thread_start(tid, start_arg)`.
pub const THREAD_ENTRY: &str = "wasixv2_thread_start";

/// The status a thread group exits with when one of its threads traps or returns from its
/// entry point.
// TODO: kill the group with SIGSEGV once signals exist.
const FAULT_STATUS: i32 = 128 + 11;

/// What every thread of one process instantiates.
pub(crate) struct Image<S> {
    engine: Engine,
    module: Module,
    memory: SharedMemory,
    syscalls: Arc<S>,
    host: Arc<dyn Host>,
}

impl<S> Image<S> {
    pub(crate) fn new(
        store: &Store,
        module: &Module,
        memory: SharedMemory,
        syscalls: Arc<S>,
        host: Arc<dyn Host>,
    ) -> Arc<Self> {
        Arc::new(Self {
            engine: store.engine().clone(),
            module: module.clone(),
            memory,
            syscalls,
            host,
        })
    }
}

/// Starts thread tasks of one process; `Image` without its syscalls type.
trait Start: Send + Sync {
    fn start(self: Arc<Self>, task: TaskId, start_arg: u32) -> io::Result<()>;
}

impl<S> Start for Image<S>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    fn start(self: Arc<Self>, task: TaskId, start_arg: u32) -> io::Result<()> {
        let host = self.host.clone();
        host.executor()
            .spawn_thread(Box::new(move || Box::pin(self.run(task, start_arg))))
    }
}

/// Running instances: (syscalls object, task) to the image their threads instantiate.
type Key = (usize, TaskId);

static IMAGES: LazyLock<Mutex<HashMap<Key, Arc<dyn Start>>>> = LazyLock::new(Default::default);

fn key<S>(syscalls: &S, task: TaskId) -> Key {
    (syscalls as *const S as usize, task)
}

/// Keeps `task`'s instance able to start threads until dropped.
pub(crate) struct Registration(Key);

impl Drop for Registration {
    fn drop(&mut self) {
        IMAGES.lock().unwrap().remove(&self.0);
    }
}

pub(crate) fn register<S>(image: &Arc<Image<S>>, task: TaskId) -> Registration
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    let key = key(&*image.syscalls, task);
    IMAGES.lock().unwrap().insert(key, image.clone());
    Registration(key)
}

/// Starts the thread `child` that `parent` created. On failure the child is gone again and the
/// caller reports `EAGAIN`, as `clone` does when the host is out of threads.
pub(crate) fn start<S: Syscalls + Control>(
    syscalls: &S,
    parent: TaskId,
    child: TaskId,
    start_arg: u32,
) -> bool {
    let image = IMAGES.lock().unwrap().get(&key(syscalls, parent)).cloned();
    let started = match image {
        Some(image) => image.start(child, start_arg),
        // The process has no shared memory to start threads on.
        None => Err(io::ErrorKind::Unsupported.into()),
    };
    match started {
        Ok(()) => true,
        Err(err) => {
            tracing::warn!(%err, "cannot start a guest thread");
            // The child never ran: it exits at once, which only touches kernel state.
            let caller = Caller::new(child, &NoMemory);
            let exit = syscalls.exit(&caller, 0);
            match pin!(exit).poll(&mut Context::from_waker(Waker::noop())) {
                Poll::Ready(_) => {}
                Poll::Pending => unreachable!("a thread's exit only waits for its leader"),
            }
            syscalls.take_pending(child);
            false
        }
    }
}

impl<S> Image<S>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    /// Runs the thread `task` to its end, on the host thread it was started on.
    async fn run(self: Arc<Self>, task: TaskId, start_arg: u32) {
        let fault = match self.clone().call_entry(task, start_arg).await {
            Ok(()) => "returned from its entry point".to_string(),
            Err(ExecError::Trap(err)) => match err.downcast::<Exited>() {
                Ok(Exited(_)) => return,
                Err(err) => err.to_string(),
            },
            Err(err) => err.to_string(),
        };
        tracing::warn!(
            tid = task.0,
            "guest thread {fault}; ending its thread group"
        );
        let _ = self
            .syscalls
            .exit_group(&Caller::new(task, &NoMemory), FAULT_STATUS)
            .await;
    }

    async fn call_entry(self: Arc<Self>, task: TaskId, start_arg: u32) -> Result<(), ExecError> {
        let mut store = Store::new(self.engine.clone());
        let env = FunctionEnv::new(
            &mut store,
            ExecEnv {
                syscalls: self.syscalls.clone(),
                task,
                memory: None,
                signal: crate::signal::SignalExports::default(),
                proc: crate::process::ProcExports::new(crate::process::spawner(
                    self.engine.clone(),
                    self.syscalls.clone(),
                    self.host.clone(),
                )),
                asyncify: None,
            },
        );
        let mut imports = generated::imports(&mut store, &env);
        if crate::asyncify::instrumented(&self.module) {
            crate::fork::imports(&mut store, &env, &mut imports);
        }
        let memory = self.memory.clone().try_attach(&mut store)?;
        if let Some(import) = self.module.imports().memories().next() {
            imports.define(import.module(), import.name(), memory.clone());
        }
        let instance = Instance::new(&mut store, &self.module, &imports).map_err(Box::new)?;
        let asyncify = crate::asyncify::Asyncify::new(&instance, &mut store)?;
        let data = env.as_mut(&mut store);
        data.memory = Some(memory);
        data.signal = crate::signal::SignalExports::new(&instance);
        data.proc.set_instance(&instance, &self.module);
        data.asyncify = asyncify;
        let entry = instance.exports.get_function(THREAD_ENTRY)?.clone();
        let _registration = register(&self, task);

        let store = store.into_async();
        let cx = crate::asyncify::Outside {
            env: &env,
            store: &store,
        };
        let args = [Value::I32(task.0 as i32), Value::I32(start_arg as i32)];
        crate::asyncify::call(&cx, Some(THREAD_ENTRY), &entry, &args)
            .await
            .map(drop)
            .map_err(ExecError::Trap)
    }
}

/// Memory for kernel calls the execution layer makes itself, for a task whose instance is not
/// there: every access faults.
pub(crate) struct NoMemory;

impl UserMemory for NoMemory {
    fn read<'a>(&'a self, _: u32, _: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        Box::pin(async { Err(Fault) })
    }

    fn write<'a>(&'a self, _: u32, _: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        Box::pin(async { Err(Fault) })
    }

    fn size(&self) -> LocalFuture<'_, u64> {
        Box::pin(async { 0 })
    }

    fn grow(&self, _: u32) -> LocalFuture<'_, Option<u64>> {
        Box::pin(async { None })
    }
}
