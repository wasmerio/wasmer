//! Completing [asyncify] stack unwinds that host functions start.
//!
//! A module built with asyncify can unwind its own stack back to the host and
//! later be rewound to where it left off. A host function drives this:
//!
//! 1. It calls the module's `asyncify_start_unwind` export, leaves an
//!    [`OnCalledHandler`] on its function environment and returns.
//! 2. The guest unwinds back to the host call that entered it. When that call
//!    was made through [`AsyncifyFunction`], the handler runs there, with the
//!    stack fully unwound.
//! 3. The handler calls `asyncify_stop_unwind` and decides what happens next
//!    through [`OnCalledAction`]: re-enter the function (after starting a
//!    rewind), finish the call, or trap.
//!
//! The environment type tells the call where the handler is through
//! [`AsyncifyEnv`].
//!
//! [asyncify]: https://emscripten.org/docs/porting/asyncify.html

#[cfg(feature = "experimental-async")]
use std::future::Future;

#[cfg(feature = "experimental-async")]
use crate::AsStoreAsync;
use crate::{AsStoreMut, CallWrapper, Function, FunctionEnv, RuntimeError, StoreMut, Value, WasmCall};

/// What the call does once the guest stack has unwound to the host.
#[derive(Debug)]
pub enum OnCalledAction {
    /// Will call the function again
    InvokeAgain,
    /// Will return the result of the invocation
    Finish,
    /// Traps with an error
    Trap(Box<dyn std::error::Error + Send + Sync>),
}

/// Callback a host function leaves for the host to run once the guest stack
/// has unwound.
pub type OnCalledHandler = Box<
    dyn FnOnce(StoreMut<'_>) -> Result<OnCalledAction, Box<dyn std::error::Error + Send + Sync>>
        + Send
        + Sync,
>;

/// Function environment that holds the [`OnCalledHandler`] its host functions
/// leave when they start an unwind.
pub trait AsyncifyEnv: Send + 'static {
    /// Takes the pending handler, if a host function left one.
    fn take_on_called(&mut self) -> Option<OnCalledHandler>;
}

/// [`CallWrapper`] that completes the asyncify unwinds host functions start
/// during a call.
///
/// Once the call returns, it runs the handler left on the environment and
/// enters the function again when the handler asks for it.
pub struct AsyncifyCallWrapper<T> {
    env: FunctionEnv<T>,
}

impl<T> AsyncifyCallWrapper<T> {
    /// Creates a wrapper for calls whose host functions run against `env`.
    pub fn new(env: FunctionEnv<T>) -> Self {
        Self { env }
    }
}

impl<T: AsyncifyEnv> CallWrapper for AsyncifyCallWrapper<T> {
    fn wrap(&mut self, call: &mut WasmCall<'_>) -> Result<(), RuntimeError> {
        loop {
            let result = call.invoke();
            if !resume_after_unwind(&self.env, call)? {
                return result;
            }
        }
    }
}

/// Runs the handler an unwind left on `env`, if any.
///
/// Returns `true` when the function must be entered again.
fn resume_after_unwind<T: AsyncifyEnv>(
    env: &FunctionEnv<T>,
    store: &mut impl AsStoreMut,
) -> Result<bool, RuntimeError> {
    let Some(on_called) = env.as_mut(store).take_on_called() else {
        return Ok(false);
    };
    match on_called(store.as_store_mut()) {
        Ok(OnCalledAction::InvokeAgain) => Ok(true),
        Ok(OnCalledAction::Finish) => Ok(false),
        Ok(OnCalledAction::Trap(err)) | Err(err) => Err(RuntimeError::user(err)),
    }
}

/// Calls into WebAssembly so that the asyncify unwinds started by host
/// functions complete.
///
/// Typed functions get the same behaviour from
/// [`TypedFunction::call_wrapped`](crate::TypedFunction::call_wrapped) with an
/// [`AsyncifyCallWrapper`].
pub trait AsyncifyFunction {
    /// Calls the function, completing unwinds left on `env`.
    fn call_asyncify<T: AsyncifyEnv>(
        &self,
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<T>,
        params: &[Value],
    ) -> Result<Box<[Value]>, RuntimeError>;

    /// Calls the function asynchronously, completing unwinds left on `env`.
    #[cfg(feature = "experimental-async")]
    fn call_async_asyncify<T: AsyncifyEnv>(
        &self,
        store: &impl AsStoreAsync,
        env: &FunctionEnv<T>,
        params: Vec<Value>,
    ) -> impl Future<Output = Result<Box<[Value]>, RuntimeError>> + 'static;
}

impl AsyncifyFunction for Function {
    fn call_asyncify<T: AsyncifyEnv>(
        &self,
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<T>,
        params: &[Value],
    ) -> Result<Box<[Value]>, RuntimeError> {
        self.call_wrapped(store, &mut AsyncifyCallWrapper::new(env.clone()), params)
    }

    #[cfg(feature = "experimental-async")]
    fn call_async_asyncify<T: AsyncifyEnv>(
        &self,
        store: &impl AsStoreAsync,
        env: &FunctionEnv<T>,
        params: Vec<Value>,
    ) -> impl Future<Output = Result<Box<[Value]>, RuntimeError>> + 'static {
        let function = self.clone();
        let store = store.store();
        let env = env.clone();
        // A wrapper cannot hold the store across an await, so the unwind is
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
