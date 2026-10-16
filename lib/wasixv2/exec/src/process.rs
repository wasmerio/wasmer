//! Process images: `execve` replaces the instance with a new one of the image the kernel
//! loaded, vfork switches an instance between the parent and child tasks, and a fork child
//! starts from a copy of its parent's instance.
//!
//! A task switch is one field, [`ExecEnv::task`], which every import reads on entry.

use std::{
    future::Future,
    sync::Arc,
    task::{Context, Waker},
};

use wasixv2_host::Host;
use wasixv2_syscalls::{Caller, Control, Syscalls, TaskId};
use wasmer::{AsyncFunctionEnvMut, Engine, Function, Instance, Module, RuntimeError, Store, Value};

use crate::{ExecEnv, ExecError, asyncify, thread::NoMemory};

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

/// Starts the processes an instance creates on instances of their own: the image of a vfork
/// child that called `execve`, and a fork child; the type-erased part of the execution layer
/// that the return-to-guest path can reach.
pub(crate) trait Spawn: Send + Sync {
    fn spawn(&self, task: TaskId, image: Vec<u8>);
    fn fork(&self, task: TaskId, image: asyncify::ForkImage);
}

pub(crate) struct Spawner<S> {
    engine: Engine,
    syscalls: Arc<S>,
    host: Arc<dyn Host>,
}

pub(crate) fn spawner<S>(engine: Engine, syscalls: Arc<S>, host: Arc<dyn Host>) -> Arc<dyn Spawn>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    Arc::new(Spawner {
        engine,
        syscalls,
        host,
    })
}

impl<S> Spawn for Spawner<S>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    fn spawn(&self, task: TaskId, image: Vec<u8>) {
        let (engine, syscalls, host) = (
            self.engine.clone(),
            self.syscalls.clone(),
            self.host.clone(),
        );
        self.start(task, move || async move {
            let store = Store::new(engine);
            run_bytes(store, image, syscalls, task, host).await
        });
    }

    fn fork(&self, task: TaskId, image: asyncify::ForkImage) {
        let (engine, syscalls, host) = (
            self.engine.clone(),
            self.syscalls.clone(),
            self.host.clone(),
        );
        self.start(task, move || async move {
            let store = Store::new(engine);
            run_forked(store, image, syscalls, task, host).await
        });
    }
}

impl<S> Spawner<S>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    /// Runs the process `task` on a host thread of its own, ending it as a fault would if its
    /// image fails or no thread can start.
    fn start<F, R>(&self, task: TaskId, run: R)
    where
        R: FnOnce() -> F + Send + 'static,
        F: Future<Output = Result<i32, ExecError>> + 'static,
    {
        let syscalls = self.syscalls.clone();
        let start = Box::new(move || {
            Box::pin(async move {
                if let Err(err) = run().await {
                    tracing::warn!(tid = task.0, %err, "process image failed");
                    fault(&*syscalls, task).await;
                }
            }) as wasixv2_host::LocalFuture<'static, ()>
        });
        if let Err(err) = self.host.executor().spawn_thread(start) {
            tracing::warn!(%err, "cannot start a process on a thread of its own");
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
) -> Result<i32, ExecError>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    let engine = store.engine().clone();
    let ended = crate::run_image(store, module, syscalls.clone(), task, host.clone()).await?;
    finish(engine, ended, syscalls, task, host).await
}

/// Runs a fork child from the copy of its parent's instance, then every image its `execve`
/// calls load, until the process exits.
async fn run_forked<S>(
    store: Store,
    image: asyncify::ForkImage,
    syscalls: Arc<S>,
    task: TaskId,
    host: Arc<dyn Host>,
) -> Result<i32, ExecError>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    let engine = store.engine().clone();
    let module = image.module.clone();
    let ready = crate::instantiate(
        store,
        &module,
        syscalls.clone(),
        task,
        host.clone(),
        Some(&image),
    )?;
    let (entry, args, buffer) = (image.entry, image.args.clone(), image.buffer());
    drop(image);
    let ended = ready.run(entry, args, Some(buffer)).await?;
    finish(engine, ended, syscalls, task, host).await
}

/// Follows `task`'s process from the end of one image's run through the images its `execve`
/// calls load, until it exits.
async fn finish<S>(
    engine: Engine,
    mut ended: Ended,
    syscalls: Arc<S>,
    task: TaskId,
    host: Arc<dyn Host>,
) -> Result<i32, ExecError>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
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
                ended =
                    crate::run_image(store, &module, syscalls.clone(), task, host.clone()).await?;
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
    run(store, &module, syscalls, task, host).await
}

/// Ends `task`'s thread group as a fault would, and returns its status.
pub(crate) async fn fault<S: Syscalls + Control>(syscalls: &S, task: TaskId) -> i32 {
    let _ = syscalls
        .exit_group(&Caller::new(task, &NoMemory), FAULT_STATUS)
        .await;
    while syscalls.take_pending(task).is_some() {}
    FAULT_STATUS
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
    pub(crate) spawner: Arc<dyn Spawn>,
    /// The instance's module, which a fork child instantiates.
    pub(crate) module: Option<Module>,
    /// `wasixv2_vfork_return(child)`, which libc exports to unwind to the vfork point.
    vfork_return: Option<Function>,
}

impl ProcExports {
    pub(crate) fn new(spawner: Arc<dyn Spawn>) -> Self {
        Self {
            spawner,
            module: None,
            vfork_return: None,
        }
    }

    pub(crate) fn set_instance(&mut self, instance: &Instance, module: &Module) {
        self.module = Some(module.clone());
        self.vfork_return = instance.exports.get_function(VFORK_RETURN).ok().cloned();
    }
}

/// The export a vfork parent resumes through, as `wasixv2_vfork_return(child_tid)`.
pub const VFORK_RETURN: &str = "wasixv2_vfork_return";

/// Switches the instance back to the vfork parent and calls the guest's vfork return, which
/// unwinds to the vfork point with an exception; returns that error for the import to raise.
pub(crate) async fn vfork_return<S: 'static>(
    env: &AsyncFunctionEnvMut<ExecEnv<S>>,
    parent: TaskId,
    child: TaskId,
) -> RuntimeError {
    switch(env, parent).await;
    let function = env.read().await.data().proc.vfork_return.clone();
    let Some(function) = function else {
        return RuntimeError::new(format!(
            "a vfork child ended, but the guest has no `{VFORK_RETURN}`"
        ));
    };
    let args = [Value::I32(child.0 as i32)];
    match asyncify::call(env, None, &function, &args).await {
        Err(err) => err,
        Ok(_) => RuntimeError::new(format!("`{VFORK_RETURN}` returned")),
    }
}
