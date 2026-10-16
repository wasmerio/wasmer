//! Guest ABIs: which imports an instance gets, and how the engine enters it, ends it and
//! interrupts it. The `wasixv2` ABI ([`crate::Wasixv2`]) is the native one; others (such as WASIX
//! v1) are served by clients of [`Syscalls`] that register here.

use std::sync::Arc;

use wasixv2_syscalls::{Control, LocalFuture, SignalFrame, Syscalls, TaskId};
use wasmer::{
    AsyncFunctionEnvMut, FunctionEnv, Imports, Instance, Module, RuntimeError, Store, StoreAsync,
};

use crate::{ExecEnv, ExecError};

/// One guest ABI.
pub trait Abi<S>: Send + Sync + 'static
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    /// A short name for logs, such as `wasixv2`.
    fn name(&self) -> &'static str;

    /// Whether `module` targets this ABI, decided from its imports.
    fn matches(&self, module: &Module) -> bool;

    /// Every function import the ABI provides. The execution layer adds the memory import.
    fn imports(&self, store: &mut Store, env: &FunctionEnv<ExecEnv<S>>) -> Imports;

    /// Binds what the execution layer needs of a new instance of a module this ABI matched.
    /// Called once per instance, before any of its code runs, with the task it starts as.
    #[allow(clippy::result_large_err)] // `ExecError` is the crate's error, returned once per instance
    fn bind(
        &self,
        instance: &Instance,
        store: &mut Store,
        task: TaskId,
        role: Role,
    ) -> Result<Arc<dyn AbiInstance<S>>, ExecError>;
}

/// What an instance is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// A process image: the first one of a process, or one `execve` loaded.
    Process,
    /// A thread on the shared memory of a process image.
    Thread,
}

/// The per-instance half of an [`Abi`]: entry points and the guest-visible interruptions.
pub trait AbiInstance<S: 'static>: Send + Sync {
    /// Runs the process image from its entry point until the entry returns or the guest unwinds.
    fn start(&self, store: &StoreAsync) -> LocalFuture<'static, Result<(), RuntimeError>>;

    /// What it means when [`AbiInstance::start`] returns normally.
    fn on_entry_return(&self) -> EntryReturn;

    /// Runs the thread entry of `task` with the `start_arg` its creator passed.
    fn start_thread(
        &self,
        store: &StoreAsync,
        task: TaskId,
        start_arg: u32,
    ) -> LocalFuture<'static, Result<(), RuntimeError>>;

    /// What it means when [`AbiInstance::start_thread`] returns normally.
    fn on_thread_return(&self) -> EntryReturn;

    /// Runs the guest's handling of one signal. Returns the mask to restore (`rt_sigreturn`), or
    /// `None` to leave it as delivery set it. An error, such as the exception of a `siglongjmp`
    /// or an exit, passes through untouched; the execution layer then reports that the handler
    /// did not return.
    fn deliver_signal<'a>(
        &'a self,
        env: &'a AsyncFunctionEnvMut<ExecEnv<S>>,
        frame: &'a SignalFrame,
    ) -> LocalFuture<'a, Result<Option<u64>, RuntimeError>>;

    /// Resumes a vfork parent after its child `child` ended (exited or loaded a new image). The
    /// instance already runs as the parent again.
    fn vfork_resume<'a>(
        &'a self,
        env: &'a AsyncFunctionEnvMut<ExecEnv<S>>,
        child: TaskId,
    ) -> LocalFuture<'a, VforkResume>;

    /// The ABI's own state, for its import shims to downcast.
    fn as_any(&self) -> &dyn std::any::Any;
}

/// The meaning of an entry point returning normally.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryReturn {
    /// The task exits with this status: the whole process for a process entry, the thread
    /// for a thread entry (WASI's `_start` and `wasi_thread_start`).
    Exit(i32),
    /// The guest broke the ABI's contract; the process ends as after a fault.
    Fault,
}

/// How a vfork parent resumes.
pub enum VforkResume {
    /// The guest unwound to the vfork point; the current import raises this error, which carries
    /// the guest's exception.
    Unwind(RuntimeError),
    /// The current import returns 0 to the parent.
    ReturnSuccess,
}

/// The registered ABIs, in order: the first one whose [`Abi::matches`] accepts a module serves
/// it. Process images loaded by `execve` and the images of vfork children select through it
/// too, so a program of one ABI may run a program of another.
pub struct Abis<S> {
    abis: Vec<Arc<dyn Abi<S>>>,
}

impl<S> Abis<S>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    /// No ABIs; nothing runs until one is pushed.
    pub fn new() -> Self {
        Self { abis: Vec::new() }
    }

    /// Only the native `wasixv2` ABI.
    pub fn wasixv2() -> Self {
        let mut abis = Self::new();
        abis.push(Arc::new(crate::Wasixv2));
        abis
    }

    pub fn push(&mut self, abi: Arc<dyn Abi<S>>) {
        self.abis.push(abi);
    }

    /// The ABI that serves `module`, if any.
    pub fn select(&self, module: &Module) -> Option<Arc<dyn Abi<S>>> {
        self.abis.iter().find(|abi| abi.matches(module)).cloned()
    }

    /// Like [`Abis::select`], failing with [`ExecError::NoAbi`].
    #[allow(clippy::result_large_err)]
    pub(crate) fn require(&self, module: &Module) -> Result<Arc<dyn Abi<S>>, ExecError> {
        self.select(module).ok_or(ExecError::NoAbi)
    }
}

impl<S> Default for Abis<S>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::wasixv2()
    }
}
