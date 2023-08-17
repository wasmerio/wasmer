use std::{
    error::Error,
    fmt::{self, Display},
};

use wasm_bindgen::{prelude::*, JsValue};
use wasm_bindgen_downcast::DowncastJS;

use crate::RuntimeError;

#[derive(Debug)]
enum InnerTrap {
    User(Box<dyn Error + Send + Sync>),
    Js(JsTrap),
}

/// A struct representing a Trap
#[wasm_bindgen]
#[derive(Debug, DowncastJS)]
pub struct Trap {
    inner: InnerTrap,
}

impl Trap {
    pub fn user(error: Box<dyn Error + Send + Sync>) -> Self {
        Self {
            inner: InnerTrap::User(error),
        }
    }

    /// Attempts to downcast the `Trap` to a concrete type.
    pub fn downcast<T: Error + 'static>(self) -> Result<T, Self> {
        match self.inner {
            // We only try to downcast user errors
            InnerTrap::User(err) if err.is::<T>() => Ok(*err.downcast::<T>().unwrap()),
            _ => Err(self),
        }
    }

    /// Attempts to downcast the `Trap` to a concrete type.
    pub fn downcast_ref<T: Error + 'static>(&self) -> Option<&T> {
        match &self.inner {
            // We only try to downcast user errors
            InnerTrap::User(err) if err.is::<T>() => err.downcast_ref::<T>(),
            _ => None,
        }
    }

    /// Returns true if the `Trap` is the same as T
    pub fn is<T: Error + 'static>(&self) -> bool {
        match &self.inner {
            InnerTrap::User(err) => err.is::<T>(),
            _ => false,
        }
    }
}

impl std::error::Error for Trap {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.inner {
            InnerTrap::User(err) => Some(&**err),
            _ => None,
        }
    }
}

impl fmt::Display for Trap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.inner {
            InnerTrap::User(e) => write!(f, "user: {e}"),
            InnerTrap::Js(value) => write!(f, "js: {value}"),
        }
    }
}

impl From<JsValue> for RuntimeError {
    fn from(original: JsValue) -> Self {
        // We try to downcast the error and see if it's
        // an instance of RuntimeError instead, so we don't need
        // to re-wrap it.
        let trap: Trap = match Trap::downcast_js(original) {
            Ok(trap) => trap,
            Err(other) => Trap {
                inner: InnerTrap::Js(JsTrap::from(other)),
            },
        };

        trap.into()
    }
}

/// A `Send+Sync` version of a JavaScript error.
#[derive(Debug)]
enum JsTrap {
    /// An error message.
    Message(String),
    /// Unable to determine the underlying error.
    Unknown,
}

impl From<JsValue> for JsTrap {
    fn from(value: JsValue) -> Self {
        // Let's try some easy special cases first
        if let Some(error) = value.dyn_ref::<js_sys::Error>() {
            return JsTrap::Message(error.message().into());
        }

        if let Some(s) = value.as_string() {
            return JsTrap::Message(s);
        }

        // Otherwise, we'll try to stringify the error and hope for the best
        if let Some(obj) = value.dyn_ref::<js_sys::Object>() {
            return JsTrap::Message(obj.to_string().into());
        }

        JsTrap::Unknown
    }
}

impl Display for JsTrap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsTrap::Message(m) => write!(f, "{m}"),
            JsTrap::Unknown => write!(f, "unknown"),
        }
    }
}
