//! Process images: `execve` replaces the instance with a new one of the image the kernel
//! loaded, and vfork switches an instance between the parent and child tasks.
//!
//! A task switch is one field, [`ExecEnv::task`], which every import reads on entry.

use std::{
    future::Future,
    sync::Arc,
    task::{Context, Waker},
};

use wasixv2_host::Host;
use wasixv2_syscalls::{Caller, Control, Syscalls, TaskId};
use wasmer::{AsyncFunctionEnvMut, Engine, Function, Module, RuntimeError, Store, Value};

use crate::{ExecEnv, ExecError, abi::Abis, thread::NoMemory};

/// The status a process exits with when its new image does not compile or traps.
// TODO: kill it with SIGSEGV through the kernel.
const FAULT_STATUS: i32 = 128 + 11;

/// Raised from an import to unwind the old image after `execve`, carrying the new one.
#[derive(Debug, thiserror::Error)]
#[error("execve replaced the image")]
pub(crate) struct ExecNew(pub(crate) Vec<u8>);

/// How one image's run ended.
pub(crate) enum Ended {
    Exited(i32),
    Exec(Vec<u8>),
}

/// Starts the image of a vfork child that called `execve`, on its own instance; the type-erased
/// part of the execution layer that the return-to-guest path can reach.
pub(crate) trait Spawn: Send + Sync {
    fn spawn(&self, task: TaskId, image: Vec<u8>);
}

pub(crate) struct Spawner<S> {
    engine: Engine,
    syscalls: Arc<S>,
    host: Arc<dyn Host>,
    abis: Arc<Abis<S>>,
}

pub(crate) fn spawner<S>(
    engine: Engine,
    syscalls: Arc<S>,
    host: Arc<dyn Host>,
    abis: Arc<Abis<S>>,
) -> Arc<dyn Spawn>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    Arc::new(Spawner {
        engine,
        syscalls,
        host,
        abis,
    })
}

impl<S> Spawn for Spawner<S>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    fn spawn(&self, task: TaskId, image: Vec<u8>) {
        let (engine, syscalls, host, abis) = (
            self.engine.clone(),
            self.syscalls.clone(),
            self.host.clone(),
            self.abis.clone(),
        );
        let start = Box::new(move || {
            Box::pin(async move {
                let store = Store::new(engine);
                if let Err(err) = run_bytes(store, image, syscalls.clone(), task, host, abis).await
                {
                    tracing::warn!(tid = task.0, %err, "process image failed");
                    fault(&*syscalls, task).await;
                }
            }) as wasixv2_host::LocalFuture<'static, ()>
        });
        if let Err(err) = self.host.executor().spawn_thread(start) {
            tracing::warn!(%err, "cannot start the process image of a vfork child");
            // The kernel's exit_group never waits.
            let exit = fault(&*self.syscalls, task);
            let _ = std::pin::pin!(exit).poll(&mut Context::from_waker(Waker::noop()));
        }
    }
}

/// Runs `module` as `task`, then every image its `execve` calls load, until the process exits.
pub(crate) async fn run<S>(
    store: Store,
    module: &Module,
    syscalls: Arc<S>,
    task: TaskId,
    host: Arc<dyn Host>,
    abis: Arc<Abis<S>>,
) -> Result<i32, ExecError>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    let engine = store.engine().clone();
    let mut ended = crate::run_image(
        store,
        module,
        syscalls.clone(),
        task,
        host.clone(),
        abis.clone(),
    )
    .await?;
    loop {
        match ended {
            Ended::Exited(status) => return Ok(status),
            Ended::Exec(image) => {
                let module = match Module::new(&engine, image) {
                    Ok(module) => module,
                    Err(err) => {
                        tracing::warn!(%err, "execve loaded a module that does not compile");
                        return Ok(fault(&*syscalls, task).await);
                    }
                };
                let store = Store::new(engine.clone());
                ended = crate::run_image(
                    store,
                    &module,
                    syscalls.clone(),
                    task,
                    host.clone(),
                    abis.clone(),
                )
                .await?;
            }
        }
    }
}

async fn run_bytes<S>(
    store: Store,
    image: Vec<u8>,
    syscalls: Arc<S>,
    task: TaskId,
    host: Arc<dyn Host>,
    abis: Arc<Abis<S>>,
) -> Result<i32, ExecError>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    let module = match Module::new(&store, image) {
        Ok(module) => module,
        Err(err) => {
            tracing::warn!(%err, "execve loaded a module that does not compile");
            return Ok(fault(&*syscalls, task).await);
        }
    };
    run(store, &module, syscalls, task, host, abis).await
}

/// Ends `task`'s thread group as a fault would, and returns its status.
pub(crate) async fn fault<S: Syscalls + Control>(syscalls: &S, task: TaskId) -> i32 {
    exit_group(syscalls, task, FAULT_STATUS).await
}

/// Ends `task`'s thread group with `status` on the guest's behalf, and returns the status.
pub(crate) async fn exit_group<S: Syscalls + Control>(
    syscalls: &S,
    task: TaskId,
    status: i32,
) -> i32 {
    let _ = syscalls
        .exit_group(&Caller::new(task, &NoMemory), status)
        .await;
    while syscalls.take_pending(task).is_some() {}
    status
}

/// Makes the instance behind `env` run as `task` from now on.
pub(crate) async fn switch<S: 'static>(env: &AsyncFunctionEnvMut<ExecEnv<S>>, task: TaskId) {
    env.write().await.data_mut().task = task;
}

/// Starts the image `task` loaded with `execve` while it was a vfork child.
pub(crate) async fn spawn_image<S: 'static>(
    env: &AsyncFunctionEnvMut<ExecEnv<S>>,
    task: TaskId,
    image: Vec<u8>,
) {
    let spawner = env.read().await.data().proc.spawner.clone();
    spawner.spawn(task, image);
}

/// What the execution layer needs of an instance for processes.
pub(crate) struct ProcExports {
    spawner: Arc<dyn Spawn>,
}

impl ProcExports {
    pub(crate) fn new(spawner: Arc<dyn Spawn>) -> Self {
        Self { spawner }
    }
}

/// The export a vfork parent resumes through, as `wasixv2_vfork_return(child_tid)`.
pub const VFORK_RETURN: &str = "wasixv2_vfork_return";

/// Calls the guest's vfork return, `function`, in the vfork parent, which unwinds to the vfork
/// point with an exception; returns that error for the import to raise.
pub(crate) async fn vfork_return<S: 'static>(
    env: &AsyncFunctionEnvMut<ExecEnv<S>>,
    function: Option<&Function>,
    child: TaskId,
) -> RuntimeError {
    let Some(function) = function else {
        return RuntimeError::new(format!(
            "a vfork child ended, but the guest has no `{VFORK_RETURN}`"
        ));
    };
    let args = vec![Value::I32(child.0 as i32)];
    match function.call_async(&env.as_store_async(), args).await {
        Err(err) => err,
        Ok(_) => RuntimeError::new(format!("`{VFORK_RETURN}` returned")),
    }
}
