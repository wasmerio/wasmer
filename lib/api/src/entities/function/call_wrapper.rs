//! Host code that runs around a call into WebAssembly.

use crate::{AsStoreMut, AsStoreRef, RuntimeError, StoreMut, StoreObjects, StoreRef};

/// Runs host code around a call into WebAssembly.
///
/// Pass one to [`Function::call_wrapped`](crate::Function::call_wrapped) or
/// [`TypedFunction::call_wrapped`](crate::TypedFunction::call_wrapped). The
/// wrapper decides when, and how many times, the function is entered through
/// [`WasmCall::invoke`], and can use the store in between through the
/// [`WasmCall`] itself.
///
/// Wrappers compose by delegation: a wrapper holding another one calls the
/// inner [`CallWrapper::wrap`] with the same [`WasmCall`].
///
/// Closures taking `&mut WasmCall<'_>` are wrappers too.
pub trait CallWrapper {
    /// Performs the wrapped call.
    ///
    /// Returning `Ok` is only accepted when the last [`WasmCall::invoke`]
    /// succeeded, since the results are read from that invocation.
    fn wrap(&mut self, call: &mut WasmCall<'_>) -> Result<(), RuntimeError>;
}

impl<F> CallWrapper for F
where
    F: FnMut(&mut WasmCall<'_>) -> Result<(), RuntimeError>,
{
    fn wrap(&mut self, call: &mut WasmCall<'_>) -> Result<(), RuntimeError> {
        self(call)
    }
}

/// The call into WebAssembly that a [`CallWrapper`] wraps.
///
/// It is also the store the call runs in, so the wrapper never holds a store
/// borrow across an invocation.
pub struct WasmCall<'a> {
    call: &'a mut dyn ErasedCall,
    interruptible: bool,
}

impl WasmCall<'_> {
    /// Enters the wrapped function.
    ///
    /// Invoking again enters the function again, with the same arguments.
    pub fn invoke(&mut self) -> Result<(), RuntimeError> {
        self.call.invoke(self.interruptible)
    }

    #[cfg(all(
        unix,
        feature = "sys",
        feature = "experimental-host-interrupt",
        not(feature = "baremetal")
    ))]
    pub(crate) fn interruptible(&mut self) -> WasmCall<'_> {
        WasmCall {
            call: &mut *self.call,
            interruptible: true,
        }
    }
}

impl AsStoreRef for WasmCall<'_> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        self.call.store_ref()
    }
}

impl AsStoreMut for WasmCall<'_> {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        self.call.store_mut()
    }

    fn objects_mut(&mut self) -> &mut StoreObjects {
        self.call.objects_mut()
    }
}

// `AsStoreMut` is not dyn-compatible, so the caller's store and its invoke
// closure are erased together behind this trait.
trait ErasedCall {
    fn invoke(&mut self, interruptible: bool) -> Result<(), RuntimeError>;
    fn store_ref(&self) -> StoreRef<'_>;
    fn store_mut(&mut self) -> StoreMut<'_>;
    fn objects_mut(&mut self) -> &mut StoreObjects;
}

struct CallState<'a, S, F> {
    store: &'a mut S,
    invoke: F,
    completed: bool,
}

impl<S, F> ErasedCall for CallState<'_, S, F>
where
    S: AsStoreMut,
    F: FnMut(&mut S, bool) -> Result<(), RuntimeError>,
{
    fn invoke(&mut self, interruptible: bool) -> Result<(), RuntimeError> {
        // Cleared first, so an entry that unwinds never counts as completed.
        self.completed = false;
        let result = (self.invoke)(self.store, interruptible);
        self.completed = result.is_ok();
        result
    }

    fn store_ref(&self) -> StoreRef<'_> {
        self.store.as_store_ref()
    }

    fn store_mut(&mut self) -> StoreMut<'_> {
        self.store.as_store_mut()
    }

    fn objects_mut(&mut self) -> &mut StoreObjects {
        self.store.objects_mut()
    }
}

/// Enters WebAssembly through `invoke`, directly or through `wrapper`.
///
/// Every backend call path goes through here, so a plain call stays a direct
/// call and wrappers apply the same way on every backend.
#[cfg(any(feature = "js", feature = "v8"))]
pub(crate) fn call_wrapped<S: AsStoreMut>(
    store: &mut S,
    wrapper: Option<&mut dyn CallWrapper>,
    mut invoke: impl FnMut(&mut S) -> Result<(), RuntimeError>,
) -> Result<(), RuntimeError> {
    call_wrapped_with_interrupt(store, wrapper, |store, _| invoke(store))
}

pub(crate) fn call_wrapped_with_interrupt<S: AsStoreMut>(
    store: &mut S,
    wrapper: Option<&mut dyn CallWrapper>,
    mut invoke: impl FnMut(&mut S, bool) -> Result<(), RuntimeError>,
) -> Result<(), RuntimeError> {
    let Some(wrapper) = wrapper else {
        return invoke(store, false);
    };
    let mut state = CallState {
        store,
        invoke,
        completed: false,
    };
    wrapper.wrap(&mut WasmCall {
        call: &mut state,
        interruptible: false,
    })?;
    if state.completed {
        Ok(())
    } else {
        Err(RuntimeError::new(
            "call wrapper returned Ok without completing the wrapped call",
        ))
    }
}
