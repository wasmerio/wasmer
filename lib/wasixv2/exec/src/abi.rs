//! Guest ABIs: which imports an instance gets, and how the engine enters it, ends it and
//! interrupts it. The `wasixv2` ABI ([`crate::Wasixv2`]) is the native one; others (such as WASIX
//! v1) are served by clients of [`Syscalls`] that register here.

use std::sync::Arc;

use wasixv2_syscalls::{Binfmt, Control, Errno, LocalFuture, SignalFrame, Syscalls, TaskId};
use wasmer::{
    AsyncFunctionEnvMut, Function, FunctionEnv, Imports, Instance, Module, RuntimeError, Store,
    StoreAsync, Value,
};

use crate::{ExecEnv, ExecError, LoadError, Loader, asyncify};

/// One guest ABI.
pub trait Abi<S>: Send + Sync + 'static
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    /// A short name for logs, such as `wasixv2`.
    fn name(&self) -> &'static str;

    /// Whether `module` targets this ABI, decided from its imports.
    fn matches(&self, module: &Module) -> bool;

    /// [`Abi::matches`] for `image`, a WebAssembly binary not yet compiled, from its import
    /// section alone; a malformed binary matches no ABI. `execve` asks before the point of no
    /// return, so an image no ABI serves fails with `ENOEXEC` (see [`Abis`]'s [`Binfmt`]).
    fn matches_image(&self, image: &[u8]) -> bool;

    /// Every function import the ABI provides for an instance of `module`. The execution layer
    /// adds the memory import.
    fn imports(&self, store: &mut Store, env: &FunctionEnv<ExecEnv<S>>, module: &Module)
    -> Imports;

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
    fn start<'a>(&'a self, entry: &'a Entry<'a, S>) -> LocalFuture<'a, Result<(), RuntimeError>>;

    /// What it means when [`AbiInstance::start`] returns normally.
    fn on_entry_return(&self) -> EntryReturn;

    /// Runs the thread entry of `task` with the `start_arg` its creator passed.
    fn start_thread<'a>(
        &'a self,
        entry: &'a Entry<'a, S>,
        task: TaskId,
        start_arg: u32,
    ) -> LocalFuture<'a, Result<(), RuntimeError>>;

    /// What it means when [`AbiInstance::start_thread`] returns normally.
    fn on_thread_return(&self) -> EntryReturn;

    /// Runs the guest's handling of one signal, calling into the guest with
    /// [`crate::call_guest`]. `restart` says whether the syscall the signal interrupted runs
    /// again once the handler returns; the [`Sigreturn`] says what the handler left. An error,
    /// such as the exception of a `siglongjmp` or an exit, passes through untouched; the
    /// execution layer then reports that the handler did not return.
    fn deliver_signal<'a>(
        &'a self,
        env: &'a AsyncFunctionEnvMut<ExecEnv<S>>,
        frame: &'a SignalFrame,
        restart: bool,
    ) -> LocalFuture<'a, Result<Sigreturn, RuntimeError>>;

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

/// How an [`AbiInstance`] calls its entry points: each call is the outermost level of guest
/// calls ([`asyncify`]), so that the imports below it may unwind the whole stack, and a fork
/// child can rewind into the same export.
pub struct Entry<'a, S: 'static> {
    cx: asyncify::Outside<'a, S>,
}

impl<'a, S: Send + Sync + 'static> Entry<'a, S> {
    pub(crate) fn new(env: &'a FunctionEnv<ExecEnv<S>>, store: &'a StoreAsync) -> Self {
        Self {
            cx: asyncify::Outside { env, store },
        }
    }

    pub(crate) fn cx(&self) -> &asyncify::Outside<'a, S> {
        &self.cx
    }

    /// Calls `function`, the export named `export`, with `args`. A fork child of a call made
    /// here calls the export again, with the same arguments, to rewind.
    pub async fn call(
        &self,
        export: &'static str,
        function: &Function,
        args: &[Value],
    ) -> Result<Box<[Value]>, RuntimeError> {
        asyncify::call(&self.cx, Some(export), function, args).await
    }

    /// Calls `function` as an entry no fork can come back to, such as a reactor's
    /// `_initialize`: a fork in it fails.
    pub async fn call_once(
        &self,
        function: &Function,
        args: &[Value],
    ) -> Result<Box<[Value]>, RuntimeError> {
        asyncify::call(&self.cx, None, function, args).await
    }
}

/// What a signal handler left when it returned (`rt_sigreturn`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sigreturn {
    /// The mask to restore, or `None` to leave it as delivery set it.
    pub mask: Option<u64>,
    /// Whether the interrupted syscall still runs again: the `restart` delivery was given, unless
    /// the handler cancelled it.
    pub restart: bool,
}

/// The meaning of an entry point returning normally.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryReturn {
    /// The task exits with this status: the whole process for a process entry, the thread
    /// for a thread entry (WASI's `_start` and `wasi_thread_start`).
    Exit(i32),
    /// The guest broke the ABI's contract: the process dies of SIGSEGV, as a Linux process
    /// whose entry point returns runs into whatever follows it.
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
/// too, so a program of one ABI may run a program of another; its [`Loader`] compiles them.
pub struct Abis<S> {
    abis: Vec<Arc<dyn Abi<S>>>,
    loader: Arc<dyn Loader>,
}

impl<S> Abis<S>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    /// No ABIs; nothing runs until one is pushed. Images compile with [`crate::Compile`].
    pub fn new() -> Self {
        Self {
            abis: Vec::new(),
            loader: Arc::new(crate::Compile),
        }
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

    /// Makes `loader` compile the images `execve` loads.
    pub fn set_loader(&mut self, loader: Arc<dyn Loader>) {
        self.loader = loader;
    }

    /// What compiles the images `execve` loads.
    pub fn loader(&self) -> &dyn Loader {
        &*self.loader
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

/// The formats `execve` starts: images the loader takes ([`Loader::probe`]) that one of the ABIs
/// serves ([`Abi::matches_image`]). A loader error other than [`LoadError::Format`] is `EIO`.
impl<S> Binfmt for Abis<S>
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    fn probe(&self, image: &[u8]) -> Result<(), Errno> {
        match self.loader.probe(image) {
            Ok(()) => {}
            Err(LoadError::Format(errno)) => return Err(errno),
            Err(LoadError::Other(err)) => {
                tracing::warn!(%err, "the loader could not probe an image");
                return Err(Errno::EIO);
            }
        }
        match self.abis.iter().any(|abi| abi.matches_image(image)) {
            true => Ok(()),
            false => Err(Errno::ENOEXEC),
        }
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
