//! Process images: `execve` replaces the instance with a new one of the image the kernel
//! loaded, vfork switches an instance between the parent and child tasks, and a fork child
//! starts from a copy of its parent's instance.
//!
//! A task switch is one field, [`ExecEnv::task`], which every import reads on entry.

use std::{future::Future, sync::Arc};

use wasixv2_abi::Signal;
use wasixv2_host::Host;
use wasixv2_syscalls::{Control, GuestAction, Image, Syscalls, TaskId};
use wasmer::{AsyncFunctionEnvMut, Engine, Function, Module, RuntimeError, Store, Value};

use crate::{Ended, ExecEnv, ExecError, abi::Abis, asyncify};

/// Raised from an import to unwind the old image after `execve`, carrying the new one.
#[derive(Debug, thiserror::Error)]
#[error("execve replaced the image")]
pub(crate) struct ExecNew(pub(crate) Image);

/// Starts the processes an instance creates on instances of their own: the image of a vfork
/// child that called `execve`, and a fork child; the type-erased part of the execution layer
/// that the return-to-guest path can reach.
pub(crate) trait Spawn: Send + Sync {
    fn spawn(&self, task: TaskId, image: Image);
    fn fork(&self, task: TaskId, image: asyncify::ForkImage);
}

pub(crate) struct Spawner<S> {
    engine: Engine,
    syscalls: Arc<S>,
    host: Arc<dyn Host>,
    /// The ABIs the images of the processes it starts select from.
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
    fn spawn(&self, task: TaskId, image: Image) {
        let (engine, syscalls, host, abis) = (
            self.engine.clone(),
            self.syscalls.clone(),
            self.host.clone(),
            self.abis.clone(),
        );
        self.start(task, move || {
            run_new_image(engine, image, syscalls, task, host, abis)
        });
    }

    fn fork(&self, task: TaskId, image: asyncify::ForkImage) {
        let (engine, syscalls, host, abis) = (
            self.engine.clone(),
            self.syscalls.clone(),
            self.host.clone(),
            self.abis.clone(),
        );
        self.start(task, move || {
            let run = run_forked(engine, image, syscalls.clone(), task, host, abis);
            async move {
                crate::catch_panic(run)
                    .await
                    .unwrap_or_else(|panic| Err(crate::host_panicked(&*syscalls, task, panic)))
            }
        });
    }
}

impl<S> Spawner<S>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    /// Runs the process `task` on a host thread of its own. The run ends the process itself when
    /// its image fails; when no thread can start, the process dies of SIGSEGV here.
    fn start<F, R>(&self, task: TaskId, run: R)
    where
        R: FnOnce() -> F + Send + 'static,
        F: Future<Output = Result<i32, ExecError>> + 'static,
    {
        let start = Box::new(move || {
            Box::pin(async move {
                if let Err(err) = run().await {
                    tracing::warn!(tid = task.0, %err, "process image failed");
                }
            }) as wasixv2_host::LocalFuture<'static, ()>
        });
        if let Err(err) = self.host.executor().spawn_thread(start) {
            tracing::warn!(%err, "cannot start a process on a thread of its own");
            fault(&*self.syscalls, task, Signal::SIGSEGV.0);
            self.syscalls.release(task);
        }
    }
}

/// Runs `module` as `task`, then every image its `execve` calls load, until the process exits;
/// releases the task then.
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
    let first = crate::run_image(
        store,
        module,
        syscalls.clone(),
        task,
        host.clone(),
        abis.clone(),
    )
    .await;
    finish(engine, first, syscalls, task, host, abis).await
}

/// Runs a fork child from the copy of its parent's instance, then every image its `execve`
/// calls load, until the process exits; releases the task then.
async fn run_forked<S>(
    engine: Engine,
    image: asyncify::ForkImage,
    syscalls: Arc<S>,
    task: TaskId,
    host: Arc<dyn Host>,
    abis: Arc<Abis<S>>,
) -> Result<i32, ExecError>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    let store = Store::new(engine.clone());
    let first = crate::run_forked(
        store,
        image,
        syscalls.clone(),
        task,
        host.clone(),
        abis.clone(),
    )
    .await;
    finish(engine, first, syscalls, task, host, abis).await
}

/// Runs `image`, which `kernel_execve` made `task`'s, then every image its `execve` calls load,
/// until the process exits; releases the task then. `engine` is the engine the process would
/// have run on, which the loader may compile with.
pub(crate) async fn run_exec<S>(
    engine: Engine,
    image: Image,
    syscalls: Arc<S>,
    task: TaskId,
    host: Arc<dyn Host>,
    abis: Arc<Abis<S>>,
) -> Result<i32, ExecError>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    finish(engine, Ok(Ended::Exec(image)), syscalls, task, host, abis).await
}

/// Follows `task`'s process from the end of its first image's run through the images its
/// `execve` calls load, until it exits; releases the task then. An image that cannot start kills
/// the process with SIGSEGV.
async fn finish<S>(
    mut engine: Engine,
    first: Result<Ended, ExecError>,
    syscalls: Arc<S>,
    task: TaskId,
    host: Arc<dyn Host>,
    abis: Arc<Abis<S>>,
) -> Result<i32, ExecError>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    let mut ended = match first {
        Ok(ended) => ended,
        Err(err) => {
            fault(&*syscalls, task, Signal::SIGSEGV.0);
            syscalls.release(task);
            return Err(err);
        }
    };
    let status = loop {
        let image = match ended {
            Ended::Exited(status) => break status,
            Ended::Fault {
                task: at,
                signal,
                report,
            } => break fault_in(&*syscalls, &*host, task, at, signal, &report),
            Ended::Exec(image) => image,
        };
        // `de_thread`: the old image is gone from every thread before the new one starts.
        if let Err(status) = syscalls.exec_wait(task).await {
            break status;
        }
        let loaded = abis.loader().load(&engine, &image).await;
        drop(image);
        ended = match loaded {
            Ok((image_engine, module)) => {
                engine = image_engine;
                let store = Store::new(engine.clone());
                let run = crate::run_image(
                    store,
                    &module,
                    syscalls.clone(),
                    task,
                    host.clone(),
                    abis.clone(),
                );
                run.await.unwrap_or_else(|err| Ended::Fault {
                    task,
                    signal: Signal::SIGSEGV.0,
                    report: format!(
                        "wasixv2: cannot start the image process {} loaded with execve, \
                         killing it with SIGSEGV: {err}",
                        task.0
                    ),
                })
            }
            // Past the point of no return, a binary that does not load is a SIGSEGV.
            Err(err) => Ended::Fault {
                task,
                signal: Signal::SIGSEGV.0,
                report: format!(
                    "wasixv2: process {} loaded a module with execve that does not compile, \
                     killing it with SIGSEGV: {err}",
                    task.0
                ),
            },
        };
    };
    // A zombie leader stays until its group is empty.
    syscalls.group_wait(task).await;
    syscalls.release(task);
    Ok(status)
}

/// The image of a vfork child's `execve`.
async fn run_new_image<S>(
    engine: Engine,
    image: Image,
    syscalls: Arc<S>,
    task: TaskId,
    host: Arc<dyn Host>,
    abis: Arc<Abis<S>>,
) -> Result<i32, ExecError>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    // `de_thread`: the old image is gone from every thread before the new one starts.
    if let Err(status) = syscalls.exec_wait(task).await {
        syscalls.group_wait(task).await;
        syscalls.release(task);
        return Ok(status);
    }
    let loaded = abis.loader().load(&engine, &image).await;
    drop(image);
    let (engine, module) = match loaded {
        Ok(loaded) => loaded,
        Err(err) => {
            tracing::warn!(tid = task.0, %err, "execve loaded a module that does not compile");
            let status = fault(&*syscalls, task, Signal::SIGSEGV.0);
            syscalls.release(task);
            return Ok(status);
        }
    };
    crate::run_with_abis(Store::new(engine), &module, syscalls, task, host, abis).await
}

/// Kills `task`'s thread group with the fault `signal` and returns its exit status.
pub(crate) fn fault<S: Control>(syscalls: &S, task: TaskId, signal: i32) -> i32 {
    syscalls.fault(task, signal);
    exit_status(syscalls, task)
}

/// A trap of the instance of `task` while it ran as `at`: `task` itself, or a vfork child
/// running in its instance. The child dies of the fault; the parent's stack went with the
/// instance, so it cannot resume either and dies of SIGSEGV. `report` goes to `host` when the
/// fault kills a process: a trap while its group is exiting anyway is part of tearing it down.
/// Returns `task`'s exit status.
pub(crate) fn fault_in<S: Control>(
    syscalls: &S,
    host: &dyn Host,
    task: TaskId,
    at: TaskId,
    signal: i32,
    report: &str,
) -> i32 {
    if syscalls.fault(at, signal) {
        host.report(report);
    }
    if at == task {
        return exit_status(syscalls, task);
    }
    exit_status(syscalls, at);
    syscalls.release(at);
    fault(syscalls, task, Signal::SIGSEGV.0)
}

/// The status of the exit pending on `task`, taking everything else that is.
pub(crate) fn exit_status<S: Control>(syscalls: &S, task: TaskId) -> i32 {
    let mut status = 128 + Signal::SIGKILL.0;
    while let Some(action) = syscalls.take_pending(task) {
        if let GuestAction::Exit(exit) = action {
            status = exit;
        }
    }
    status
}

/// Makes the instance behind `env` run as `task` from now on.
pub(crate) async fn switch<S: Control + 'static>(
    env: &AsyncFunctionEnvMut<ExecEnv<S>>,
    task: TaskId,
) {
    env.write().await.data_mut().switch(task);
}

/// Starts the image `task` loaded with `execve` while it was a vfork child.
pub(crate) async fn spawn_image<S: 'static>(
    env: &AsyncFunctionEnvMut<ExecEnv<S>>,
    task: TaskId,
    image: Image,
) {
    let spawner = env.read().await.data().proc.spawner.clone();
    spawner.spawn(task, image);
}

/// What the execution layer needs of an instance for processes.
pub(crate) struct ProcExports {
    pub(crate) spawner: Arc<dyn Spawn>,
    /// The instance's module, which a fork child instantiates.
    pub(crate) module: Option<Module>,
}

impl ProcExports {
    pub(crate) fn new(spawner: Arc<dyn Spawn>) -> Self {
        Self {
            spawner,
            module: None,
        }
    }

    pub(crate) fn set_module(&mut self, module: &Module) {
        self.module = Some(module.clone());
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
    let args = [Value::I32(child.0 as i32)];
    match asyncify::call(env, None, function, &args).await {
        Err(err) => err,
        Ok(_) => RuntimeError::new(format!("`{VFORK_RETURN}` returned")),
    }
}
