//! The engine side: loading images, mapping tasks to instances, suspending the guest while a call is
//! pending, the return-to-guest path, fork and preemption.
//!
//! Drives any `Syscalls` implementation and never reaches into kernel internals.

mod generated;
mod memory;

use std::sync::Arc;

use wasixv2_syscalls::{Control, GuestAction, SysErr, SysResult, Syscalls, TaskId};
use wasmer::{FunctionEnv, Instance, Memory, Module, RuntimeError, Store};

/// Name of the import module that holds every syscall and extension.
pub const IMPORT_MODULE: &str = "wasixv2";

#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error("instantiation failed: {0}")]
    Instantiate(#[from] wasmer::InstantiationError),
    #[error("missing export: {0}")]
    Export(#[from] wasmer::ExportError),
    #[error("guest trapped: {0}")]
    Trap(RuntimeError),
    // TODO: once signals exist, kill the process with SIGSEGV instead, as
    // Linux does when the entry point returns, and drop this variant.
    #[error("`_start` returned without calling `exit-group`")]
    ReturnedFromStart,
}

/// Host state behind every import of one instance.
pub(crate) struct ExecEnv<S> {
    syscalls: Arc<S>,
    task: TaskId,
    memory: Option<Memory>,
}

/// Raised from an import to unwind the guest after its thread group exited.
#[derive(Debug, thiserror::Error)]
#[error("thread group exited with status {0}")]
struct Exited(i32);

/// Runs `module` as `task` until its thread group exits, and returns the exit status.
pub async fn run<S>(
    mut store: Store,
    module: &Module,
    syscalls: Arc<S>,
    task: TaskId,
) -> Result<i32, ExecError>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    let env = FunctionEnv::new(
        &mut store,
        ExecEnv {
            syscalls,
            task,
            memory: None,
        },
    );
    let imports = generated::imports(&mut store, &env);
    let instance = Instance::new(&mut store, module, &imports)?;
    env.as_mut(&mut store).memory = Some(instance.exports.get_memory("memory")?.clone());
    let start = instance.exports.get_function("_start")?.clone();

    let store = store.into_async();
    match start.call_async(&store, Vec::new()).await {
        Ok(_) => Err(ExecError::ReturnedFromStart),
        Err(err) => match err.downcast::<Exited>() {
            Ok(Exited(status)) => Ok(status),
            Err(err) => Err(ExecError::Trap(err)),
        },
    }
}

/// The return-to-guest path: runs the work the kernel left for `task`, then turns the syscall
/// result into the guest's return value.
fn return_to_guest<S: Control>(
    syscalls: &S,
    task: TaskId,
    ret: SysResult<i32>,
) -> Result<i32, RuntimeError> {
    if let Some(action) = syscalls.take_pending(task) {
        match action {
            GuestAction::Exit(status) => return Err(RuntimeError::user(Box::new(Exited(status)))),
        }
    }
    Ok(match ret {
        Ok(value) => value,
        Err(SysErr::Errno(errno)) => -errno.0,
        // Nothing can interrupt a call until signals exist, so there is nothing to restart.
        Err(SysErr::Restart) => -wasixv2_syscalls::Errno::EINTR.0,
    })
}
