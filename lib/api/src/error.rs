use std::sync::Arc;
use thiserror::Error;
use wasmer_types::{FrameInfo, ImportError, TrapCode};

use crate::{AsStoreMut, AsStoreRef, BackendTrap as Trap, Exception, Value};

/// The WebAssembly.LinkError object indicates an error during
/// module instantiation (besides traps from the start function).
///
/// This is based on the [link error][link-error] API.
///
/// [link-error]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/WebAssembly/LinkError
#[derive(Debug, Clone)]
#[cfg_attr(feature = "std", derive(Error))]
#[cfg_attr(feature = "std", error("Link error: {0}"))]
pub enum LinkError {
    /// An error occurred when checking the import types.
    #[cfg_attr(feature = "std", error("Error while importing {0:?}.{1:?}: {2}"))]
    Import(String, String, ImportError),

    /// A trap occurred during linking.
    #[cfg_attr(feature = "std", error("RuntimeError occurred during linking: {0}"))]
    Trap(#[cfg_attr(feature = "std", source)] RuntimeError),
    /// Insufficient resources available for linking.
    #[cfg_attr(feature = "std", error("Insufficient resources: {0}"))]
    Resource(String),
}

/// An error while instantiating a module.
///
/// This is not a common WebAssembly error, however
/// we need to differentiate from a `LinkError` (an error
/// that happens while linking, on instantiation), a
/// Trap that occurs when calling the WebAssembly module
/// start function, and an error when initializing the user's
/// host environments.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "std", derive(Error))]
pub enum InstantiationError {
    /// A linking occurred during instantiation.
    #[cfg_attr(feature = "std", error(transparent))]
    Link(LinkError),

    /// A runtime error occured while invoking the start function
    #[cfg_attr(feature = "std", error(transparent))]
    Start(RuntimeError),

    /// The module was compiled with a CPU feature that is not available on
    /// the current host.
    #[cfg_attr(feature = "std", error("missing required CPU features: {0:?}"))]
    CpuFeature(String),

    /// Import from a different [`Store`][super::Store].
    /// This error occurs when an import from a different store is used.
    #[cfg_attr(feature = "std", error("cannot mix imports from different stores"))]
    DifferentStores,

    /// Import from a different Store.
    /// This error occurs when an import from a different store is used.
    #[cfg_attr(feature = "std", error("incorrect OS or architecture"))]
    DifferentArchOS,
}

/// A struct representing an aborted instruction execution, with a message
/// indicating the cause.
#[derive(Clone)]
pub struct RuntimeError {
    pub(crate) inner: Arc<RuntimeErrorInner>,
}

#[derive(Debug)]
struct RuntimeStringError {
    details: String,
}

impl RuntimeStringError {
    fn new(msg: String) -> Self {
        Self { details: msg }
    }
}

impl std::fmt::Display for RuntimeStringError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.details)
    }
}

impl std::error::Error for RuntimeStringError {
    fn description(&self) -> &str {
        &self.details
    }
}

pub(crate) struct RuntimeErrorInner {
    /// The source error
    pub(crate) source: Trap,
    /// The trap code (if any)
    trap_code: Option<TrapCode>,
    /// The reconstructed Wasm trace (from the native trace and the `GlobalFrameInfo`).
    wasm_trace: Vec<FrameInfo>,
}

impl RuntimeError {
    /// Creates a new generic `RuntimeError` with the given `message`.
    ///
    /// # Example
    /// ```
    /// let trap = wasmer::RuntimeError::new("unexpected error");
    /// assert_eq!("unexpected error", trap.message());
    /// ```
    pub fn new<I: Into<String>>(message: I) -> Self {
        let msg = message.into();
        let source = RuntimeStringError::new(msg);
        Self::user(Box::new(source))
    }

    /// Creates `RuntimeError` from an error and a WasmTrace
    ///
    /// # Example
    /// ```ignore
    /// let wasm_trace = vec![wasmer_types::FrameInfo::new(
    ///   "my_module".to_string(),
    ///   0,
    ///   Some("my_function".to_string()),
    ///   0.into(),
    ///   2.into()
    /// )];
    /// let trap = wasmer::RuntimeError::new_from_source(my_error, wasm_trace, None);
    /// assert_eq!("unexpected error", trap.message());
    /// ```
    pub fn new_from_source(
        source: Trap,
        wasm_trace: Vec<FrameInfo>,
        trap_code: Option<TrapCode>,
    ) -> Self {
        Self {
            inner: Arc::new(RuntimeErrorInner {
                source,
                wasm_trace,
                trap_code,
            }),
        }
    }

    /// Creates a custom user Error.
    ///
    /// This error object can be passed through Wasm frames and later retrieved
    /// using the `downcast` method.
    pub fn user(error: Box<dyn std::error::Error + Send + Sync>) -> Self {
        match error.downcast::<Self>() {
            Ok(err) => *err,
            Err(error) => error.into(),
        }
    }

    /// Creates a `RuntimeError` containing an exception.
    ///
    /// If this error is returned from an imported function, the exception
    /// will be thrown in the WebAssembly code instead of the usual trapping.
    pub fn exception(ctx: &impl AsStoreRef, exception: Exception) -> Self {
        let exnref = exception.vm_exceptionref();
        let store = ctx.as_store_ref();
        match store.inner.objects {
            #[cfg(feature = "sys")]
            crate::StoreObjects::Sys(ref store_objects) => {
                crate::backend::sys::vm::Trap::uncaught_exception(
                    exnref.as_sys().clone(),
                    store_objects,
                )
                .into()
            }
            _ => panic!("exceptions are only supported in the `sys` backend"),
        }
    }

    /// Returns a reference the `message` stored in `Trap`.
    pub fn message(&self) -> String {
        if let Some(trap_code) = self.inner.trap_code {
            trap_code.message().to_string()
        } else {
            self.inner.source.to_string()
        }
    }

    /// Returns a list of function frames in WebAssembly code that led to this
    /// trap happening.
    pub fn trace(&self) -> &[FrameInfo] {
        &self.inner.wasm_trace
    }

    /// Returns trap code, if it's a Trap
    pub fn to_trap(self) -> Option<TrapCode> {
        self.inner.trap_code
    }

    // /// Returns trap code, if it's a Trap
    // pub fn to_source(self) -> &'static Trap {
    //     &self.inner.as_ref().source
    // }

    /// Attempts to downcast the `RuntimeError` to a concrete type.
    pub fn downcast<T: std::error::Error + 'static>(self) -> Result<T, Self> {
        match Arc::try_unwrap(self.inner) {
            Ok(inner) if inner.source.is::<T>() => Ok(inner.source.downcast::<T>().unwrap()),
            Ok(inner) => Err(Self {
                inner: Arc::new(inner),
            }),
            Err(inner) => Err(Self { inner }),
        }
    }

    /// Attempts to downcast the `RuntimeError` to a concrete type.
    pub fn downcast_ref<T: std::error::Error + 'static>(&self) -> Option<&T> {
        self.inner.as_ref().source.downcast_ref::<T>()
    }

    /// Returns true if the `RuntimeError` is the same as T
    pub fn is<T: std::error::Error + 'static>(&self) -> bool {
        self.inner.source.is::<T>()
    }

    /// Returns true if the `RuntimeError` is an uncaught exception.
    pub fn is_exception(&self) -> bool {
        self.inner.source.is_exception()
    }

    /// If the `RuntimeError` is an uncaught exception, returns it.
    pub fn to_exception(&self) -> Option<Exception> {
        self.inner.source.to_exception()
    }

    /// Returns a displayable version of the `RuntimeError` that also shows exception payloads.
    pub fn display<'a>(&'a self, store: &'a mut impl AsStoreMut) -> RuntimeErrorDisplay<'a> {
        if let Some(exception) = self.to_exception() {
            RuntimeErrorDisplay::Exception(exception.payload(store), self.trace())
        } else {
            RuntimeErrorDisplay::Other(self)
        }
    }

    /// Write the WASM trace to the given formatter, if we have one.
    pub fn write_trace(trace: &[FrameInfo], f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if trace.is_empty() {
            return Ok(());
        }
        for frame in trace.iter() {
            let name = frame.module_name();
            let func_index = frame.func_index();
            writeln!(f)?;
            write!(f, "    at ")?;
            match frame.function_name() {
                Some(name) => match rustc_demangle::try_demangle(name) {
                    Ok(name) => write!(f, "{name}")?,
                    Err(_) => write!(f, "{name}")?,
                },
                None => write!(f, "<unnamed>")?,
            }
            write!(
                f,
                " ({}[{}]:0x{:x})",
                name,
                func_index,
                frame.module_offset()
            )?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeError")
            .field("source", &self.inner.source)
            .field("wasm_trace", &self.inner.wasm_trace)
            .finish()
    }
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RuntimeError: {}", self.message())?;
        Self::write_trace(self.trace(), f)
    }
}

/// A displayable version of the `RuntimeError` that also shows exception payloads.
pub enum RuntimeErrorDisplay<'a> {
    /// The error is an uncaught exception, with its payload and trace.
    Exception(Vec<Value>, &'a [FrameInfo]),
    /// The error is not an exception, just display it.
    Other(&'a RuntimeError),
}

impl std::fmt::Display for RuntimeErrorDisplay<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeErrorDisplay::Exception(payload, trace) => {
                write!(f, "Uncaught exception")?;
                if !payload.is_empty() {
                    write!(f, " with payload: {payload:?}")?;
                }
                RuntimeError::write_trace(trace, f)
            }
            RuntimeErrorDisplay::Other(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.inner.source.source()
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for RuntimeError {
    fn from(error: Box<dyn std::error::Error + Send + Sync>) -> Self {
        match error.downcast::<Self>() {
            // The error is already a RuntimeError, we return it directly
            Ok(runtime_error) => *runtime_error,
            Err(error) => Trap::user(error),
        }
    }
}

/// Error that can occur during atomic operations. (notify/wait)
// Non-exhaustive to allow for future variants without breaking changes!
#[derive(PartialEq, Eq, Debug, Error)]
#[non_exhaustive]
pub enum AtomicsError {
    /// Atomic operations are not supported by this memory.
    Unimplemented,
    /// To many waiter for address.
    TooManyWaiters,
    /// Atomic operations are disabled.
    AtomicsDisabled,
}

impl std::fmt::Display for AtomicsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unimplemented => write!(f, "Atomic operations are not supported"),
            Self::TooManyWaiters => write!(f, "Too many waiters for address"),
            Self::AtomicsDisabled => write!(f, "Atomic operations are disabled"),
        }
    }
}
