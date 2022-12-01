// use super::frame_info::{FrameInfo, GlobalFrameInfo, FRAME_INFO};
use std::error::Error;
use std::fmt;
use std::sync::Arc;
use wasm_bindgen::convert::FromWasmAbi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsValue;
use wasm_bindgen_downcast::DowncastJS;

pub trait CoreError: fmt::Debug + fmt::Display {
    fn source(&self) -> Option<&(dyn CoreError + 'static)> {
        None
    }

    fn type_id(&self) -> core::any::TypeId
    where
        Self: 'static,
    {
        core::any::TypeId::of::<Self>()
    }

    fn description(&self) -> &str {
        "description() is deprecated; use Display"
    }
    fn cause(&self) -> Option<&dyn CoreError> {
        self.source()
    }
}

impl<T: fmt::Debug + fmt::Display> CoreError for T {}

impl dyn CoreError + 'static {
    /// Returns `true` if the inner type is the same as `T`.
    pub fn core_is_equal<T: CoreError + 'static>(&self) -> bool {
        let t = core::any::TypeId::of::<T>();
        let concrete = self.type_id();
        t == concrete
    }
}

impl dyn CoreError + Send + Sync + 'static {
    /// Returns `true` if the inner type is the same as `T`.
    pub fn core_is_equal<T: CoreError + 'static>(&self) -> bool {
        let t = core::any::TypeId::of::<T>();
        let concrete = self.type_id();
        t == concrete
    }
}

impl dyn CoreError + Send {
    #[inline]
    /// Attempts to downcast the box to a concrete type.
    pub fn downcast_core<T: CoreError + 'static>(
        self: Box<Self>,
    ) -> Result<Box<T>, Box<dyn CoreError + Send>> {
        let err: Box<dyn CoreError> = self;
        <dyn CoreError>::downcast_core(err).map_err(|s| unsafe {
            // Reapply the `Send` marker.
            core::mem::transmute::<Box<dyn CoreError>, Box<dyn CoreError + Send>>(s)
        })
    }
}

impl dyn CoreError + Send + Sync {
    #[inline]
    /// Attempts to downcast the box to a concrete type.
    pub fn downcast_core<T: CoreError + 'static>(self: Box<Self>) -> Result<Box<T>, Box<Self>> {
        let err: Box<dyn CoreError> = self;
        <dyn CoreError>::downcast_core(err).map_err(|s| unsafe {
            // Reapply the `Send + Sync` marker.
            core::mem::transmute::<Box<dyn CoreError>, Box<dyn CoreError + Send + Sync>>(s)
        })
    }
}

impl dyn CoreError {
    #[inline]
    /// Attempts to downcast the box to a concrete type.
    pub fn downcast_core<T: CoreError + 'static>(
        self: Box<Self>,
    ) -> Result<Box<T>, Box<dyn CoreError>> {
        if self.core_is_equal::<T>() {
            unsafe {
                let raw: *mut dyn CoreError = Box::into_raw(self);
                Ok(Box::from_raw(raw as *mut T))
            }
        } else {
            Err(self)
        }
    }
}

/// A struct representing an aborted instruction execution, with a message
/// indicating the cause.
#[wasm_bindgen]
#[derive(Clone, DowncastJS)]
pub struct WasmerRuntimeError {
    inner: Arc<RuntimeErrorSource>,
}

/// This type is the same as `WasmerRuntimeError`.
///
/// We use the `WasmerRuntimeError` name to not collide with the
/// `RuntimeError` in JS.
pub type RuntimeError = WasmerRuntimeError;

impl PartialEq for RuntimeError {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

/// The source of the `RuntimeError`.
#[derive(Debug)]
enum RuntimeErrorSource {
    Generic(String),
    #[cfg(feature = "std")]
    User(Box<dyn Error + Send + Sync>),
    #[cfg(feature = "core")]
    User(Box<dyn CoreError + Send + Sync>),
    Js(JsValue),
}

/// This is a hack to ensure the error type is Send+Sync
unsafe impl Send for RuntimeErrorSource {}
unsafe impl Sync for RuntimeErrorSource {}

impl fmt::Display for RuntimeErrorSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Generic(s) => write!(f, "{}", s),
            Self::User(s) => write!(f, "{}", s),
            Self::Js(s) => write!(f, "{:?}", s),
        }
    }
}

impl RuntimeError {
    /// Creates a new generic `RuntimeError` with the given `message`.
    ///
    /// # Example
    /// ```
    /// let trap = wasmer_compiler::RuntimeError::new("unexpected error");
    /// assert_eq!("unexpected error", trap.message());
    /// ```
    pub fn new<I: Into<String>>(message: I) -> Self {
        RuntimeError {
            inner: Arc::new(RuntimeErrorSource::Generic(message.into())),
        }
    }

    /// Raises a custom user Error
    #[deprecated(since = "2.1.1", note = "return a Result from host functions instead")]
    #[cfg(feature = "std")]
    pub(crate) fn raise(error: Box<dyn Error + Send + Sync>) -> ! {
        let error = Self::user(error);
        let js_error: JsValue = error.into();
        wasm_bindgen::throw_val(js_error)
    }

    /// Raises a custom user Error
    #[deprecated(since = "2.1.1", note = "return a Result from host functions instead")]
    #[cfg(feature = "core")]
    pub(crate) fn raise(error: Box<dyn CoreError + Send + Sync>) -> ! {
        let error = Self::user(error);
        let js_error: JsValue = error.into();
        wasm_bindgen::throw_val(js_error)
    }

    /// Creates a custom user Error.
    ///
    /// This error object can be passed through Wasm frames and later retrieved
    /// using the `downcast` method.
    #[cfg(feature = "std")]
    pub fn user(error: Box<dyn Error + Send + Sync>) -> Self {
        match error.downcast::<Self>() {
            // The error is already a RuntimeError, we return it directly
            Ok(runtime_error) => *runtime_error,
            Err(error) => RuntimeError {
                inner: Arc::new(RuntimeErrorSource::User(error)),
            },
        }
    }

    #[cfg(feature = "core")]
    pub fn user(error: Box<dyn CoreError + Send + Sync>) -> Self {
        match error.downcast_core::<Self>() {
            // The error is already a RuntimeError, we return it directly
            Ok(runtime_error) => *runtime_error,
            Err(error) => RuntimeError {
                inner: Arc::new(RuntimeErrorSource::User(error)),
            },
        }
    }

    /// Returns a reference the `message` stored in `Trap`.
    pub fn message(&self) -> String {
        format!("{}", self.inner)
    }

    /// Attempts to downcast the `RuntimeError` to a concrete type.
    pub fn downcast<T: Error + 'static>(self) -> Result<T, Self> {
        match Arc::try_unwrap(self.inner) {
            // We only try to downcast user errors
            #[cfg(feature = "std")]
            Ok(RuntimeErrorSource::User(err)) if err.is::<T>() => Ok(*err.downcast::<T>().unwrap()),
            #[cfg(feature = "core")]
            Ok(RuntimeErrorSource::User(err)) if (*err).core_is_equal::<T>() => {
                Ok(*err.downcast_core::<T>().unwrap())
            }
            Ok(inner) => Err(Self {
                inner: Arc::new(inner),
            }),
            Err(inner) => Err(Self { inner }),
        }
    }

    /// Returns true if the `RuntimeError` is the same as T
    pub fn is<T: Error + 'static>(&self) -> bool {
        match self.inner.as_ref() {
            #[cfg(feature = "std")]
            RuntimeErrorSource::User(err) => err.is::<T>(),
            #[cfg(feature = "core")]
            RuntimeErrorSource::User(err) => (*err).core_is_equal::<T>(),
            _ => false,
        }
    }
}

impl fmt::Debug for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeError")
            .field("source", &self.inner)
            .finish()
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RuntimeError: {}", self.message())?;
        Ok(())
    }
}

#[cfg(feature = "std")]
impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self.inner.as_ref() {
            RuntimeErrorSource::User(err) => Some(&**err),
            _ => None,
        }
    }
}

impl From<JsValue> for RuntimeError {
    fn from(original: JsValue) -> Self {
        // We try to downcast the error and see if it's
        // an instance of RuntimeError instead, so we don't need
        // to re-wrap it.
        WasmerRuntimeError::downcast_js(original).unwrap_or_else(|js| RuntimeError {
            inner: Arc::new(RuntimeErrorSource::Js(js)),
        })
    }
}
