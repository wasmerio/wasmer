use rusty_jsc::JSValue;

use crate::RuntimeError;
use std::error::Error;
use std::fmt;

#[derive(Debug)]
enum InnerTrap {
    User(Box<dyn Error + Send + Sync>),
    JSC(JSValue),
}

/// A struct representing a Trap
#[derive(Debug)]
pub struct Trap {
    inner: InnerTrap,
}

unsafe impl Send for Trap {}
unsafe impl Sync for Trap {}

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
            InnerTrap::User(e) => write!(f, "user: {}", e),
            InnerTrap::JSC(value) => write!(f, "jsc: obscure"),
        }
    }
}

impl From<JSValue> for RuntimeError {
    fn from(original: JSValue) -> Self {
        let trap = Trap {
            inner: InnerTrap::JSC(original),
        };
        trap.into()
        // unimplemented!("TODO: implement Trap::from(JSValue) for RuntimeError");
        // // We try to downcast the error and see if it's
        // // an instance of RuntimeError instead, so we don't need
        // // to re-wrap it.
        // let trap = Trap::downcast_js(original).unwrap_or_else(|o| Trap {
        //     inner: InnerTrap::JSC(o),
        // });
        // trap.into()
    }
}
