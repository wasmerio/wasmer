use std::pin::Pin;

use bytes::Bytes;
use futures::Future;
use wasmer_wasix_types::{
    wasi::Errno,
    wasix::{ThreadStartType, WasiMemoryLayout},
};

use wasmer::{
    AsStoreAsync, AsStoreMut, CallWrapper, Function, FunctionEnv, RuntimeError, StoreMut, Value,
    WasmCall,
};

use crate::{WasiEnv, os::task::thread::RewindResultType};

/// Future that will be polled by asyncify methods
#[doc(hidden)]
pub type AsyncifyFuture = dyn Future<Output = Bytes> + Send + Sync + 'static;

/// Trait that will be invoked after the rewind has finished
/// It is possible that the process will be terminated rather
/// than restored at this point
pub trait RewindPostProcess {
    /// Returns the serialized object that is returned on the rewind
    fn finish(&mut self, res: Result<(), Errno>) -> Bytes;
}

/// The rewind state after a deep sleep
pub struct RewindState {
    /// Memory stack used to restore the stack trace back to where it was
    pub memory_stack: Bytes,
    /// Call stack used to restore the stack trace back to where it was
    pub rewind_stack: Bytes,
    /// All the global data stored in the store
    pub store_data: Bytes,
    /// Describes the type of thread start
    pub start: ThreadStartType,
    /// Layout of the memory,
    pub layout: WasiMemoryLayout,
    /// Flag that indicates if this rewind is 64-bit or 32-bit memory based
    pub is_64bit: bool,
}

pub type RewindStateOption = Option<(RewindState, RewindResultType)>;

/// Represents the work that will be done when a thread goes to deep sleep and
/// includes the things needed to restore it again
pub struct DeepSleepWork {
    /// This is the work that will be performed before the thread is rewoken
    pub trigger: Pin<Box<AsyncifyFuture>>,
    /// State that the thread will be rewound to
    pub rewind: RewindState,
}
impl std::fmt::Debug for DeepSleepWork {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "deep-sleep-work(memory_stack_len={}, rewind_stack_len={}, store_size={})",
            self.rewind.memory_stack.len(),
            self.rewind.rewind_stack.len(),
            self.rewind.store_data.len()
        )
    }
}

/// What the call loop does after a syscall's asyncify unwind reaches the host.
#[derive(Debug)]
pub enum OnCalledAction {
    /// Will call the function again
    InvokeAgain,
    /// Will return the result of the invocation
    Finish,
    /// Traps with an error
    Trap(Box<dyn std::error::Error + Send + Sync>),
}

/// Callback a syscall leaves on the [`WasiEnv`] for the host to run once the
/// stack has unwound.
pub(crate) type OnCalledHandler = Box<
    dyn FnOnce(StoreMut<'_>) -> Result<OnCalledAction, Box<dyn std::error::Error + Send + Sync>>
        + Send
        + Sync,
>;

/// [`CallWrapper`] that completes the asyncify unwinds WASIX syscalls start
/// during a call.
///
/// Syscalls such as `proc_fork` unwind the guest stack and leave a callback on
/// the [`WasiEnv`]. Once the call returns, this wrapper runs that callback and
/// enters the function again when the callback rewinds the stack.
pub struct AsyncifyCallWrapper {
    env: FunctionEnv<WasiEnv>,
}

impl AsyncifyCallWrapper {
    /// Creates a wrapper for calls whose syscalls run against `env`.
    pub fn new(env: FunctionEnv<WasiEnv>) -> Self {
        Self { env }
    }
}

impl CallWrapper for AsyncifyCallWrapper {
    fn wrap(&mut self, call: &mut WasmCall<'_>) -> Result<(), RuntimeError> {
        loop {
            let result = call.invoke();
            if !resume_after_unwind(&self.env, call)? {
                return result;
            }
        }
    }
}

/// Runs the callback an asyncify unwind left on `env`, if any.
///
/// Returns `true` when the function must be entered again.
fn resume_after_unwind(
    env: &FunctionEnv<WasiEnv>,
    store: &mut impl AsStoreMut,
) -> Result<bool, RuntimeError> {
    let Some(on_called) = env.as_mut(store).on_called.take() else {
        return Ok(false);
    };
    match on_called(store.as_store_mut()) {
        Ok(OnCalledAction::InvokeAgain) => Ok(true),
        Ok(OnCalledAction::Finish) => Ok(false),
        Ok(OnCalledAction::Trap(err)) | Err(err) => Err(RuntimeError::user(err)),
    }
}

/// Calls into WebAssembly so that the asyncify unwinds started by WASIX
/// syscalls (fork, exec, `longjmp`, deep sleep) complete.
///
/// Use these instead of [`Function::call`] and [`Function::call_async`]
/// whenever guest code may reach WASIX syscalls. Typed functions get the same
/// behaviour from
/// [`TypedFunction::call_wrapped`](wasmer::TypedFunction::call_wrapped) with an
/// [`AsyncifyCallWrapper`].
pub trait AsyncifyFunction {
    /// Calls the function, completing asyncify unwinds against `env`.
    fn call_asyncify(
        &self,
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<WasiEnv>,
        params: &[Value],
    ) -> Result<Box<[Value]>, RuntimeError>;

    /// Calls the function asynchronously, completing asyncify unwinds against
    /// `env`.
    fn call_async_asyncify(
        &self,
        store: &impl AsStoreAsync,
        env: &FunctionEnv<WasiEnv>,
        params: Vec<Value>,
    ) -> impl Future<Output = Result<Box<[Value]>, RuntimeError>> + 'static;
}

impl AsyncifyFunction for Function {
    fn call_asyncify(
        &self,
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<WasiEnv>,
        params: &[Value],
    ) -> Result<Box<[Value]>, RuntimeError> {
        self.call_wrapped(store, &mut AsyncifyCallWrapper::new(env.clone()), params)
    }

    fn call_async_asyncify(
        &self,
        store: &impl AsStoreAsync,
        env: &FunctionEnv<WasiEnv>,
        params: Vec<Value>,
    ) -> impl Future<Output = Result<Box<[Value]>, RuntimeError>> + 'static {
        let function = self.clone();
        let store = store.store();
        let env = env.clone();
        // The wrapper cannot hold the store across an await, so the unwind is
        // completed between async calls instead.
        async move {
            loop {
                let result = function.call_async(&store, params.clone()).await;
                let mut store = store.write_lock().await;
                if !resume_after_unwind(&env, &mut store)? {
                    return result;
                }
            }
        }
    }
}
