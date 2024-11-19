use std::{any::Any, error::Error};

use crate::RuntimeError;

/// An enumeration of all the trap kinds supported by the runtimes.
#[derive(Debug, derive_more::From)]
pub enum RuntimeTrap {
    #[cfg(feature = "sys")]
    /// The trap from the `sys` runtime.
    Sys(crate::rt::sys::vm::Trap),
    #[cfg(feature = "wamr")]
    /// The trap from the `wamr` runtime.
    Wamr(crate::rt::wamr::vm::Trap),

    #[cfg(feature = "v8")]
    /// The trap from the `v8` runtime.
    V8(crate::rt::v8::vm::Trap),
}

impl RuntimeTrap {
    /// Construct a new Error with the given a user error.
    ///
    /// Internally saves a backtrace when constructed.
    pub fn user(err: Box<dyn Error + Send + Sync>) -> RuntimeError {
        #[cfg(feature = "sys")]
        {
            return crate::rt::sys::vm::Trap::user(err).into();
        }
        #[cfg(feature = "wamr")]
        {
            return crate::rt::wamr::vm::Trap::user(err).into();
        }
        #[cfg(feature = "v8")]
        {
            return crate::rt::v8::vm::Trap::user(err).into();
        }

        panic!("No runtime enabled!")
    }
    /// Attempts to downcast the `Trap` to a concrete type.
    pub fn downcast<T: Error + 'static>(self) -> Result<T, Self> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.downcast::<T>().map_err(Into::into),
            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s.downcast::<T>().map_err(Into::into),
            #[cfg(feature = "v8")]
            Self::V8(s) => s.downcast::<T>().map_err(Into::into),
            _ => panic!("No runtime enabled!"),
        }
    }

    /// Attempts to downcast the `Trap` to a concrete type.
    pub fn downcast_ref<T: Error + 'static>(&self) -> Option<&T> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.downcast_ref::<T>(),
            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s.downcast_ref::<T>(),
            #[cfg(feature = "v8")]
            Self::V8(s) => s.downcast_ref::<T>(),
            _ => panic!("No runtime enabled!"),
        }
    }

    /// Returns true if the `Trap` is the same as T
    pub fn is<T: Error + 'static>(&self) -> bool {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.is::<T>(),
            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s.is::<T>(),
            #[cfg(feature = "v8")]
            Self::V8(s) => s.is::<T>(),
            _ => panic!("No runtime enabled!"),
        }
    }
}

impl std::fmt::Display for RuntimeTrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(t) => write!(f, "{t}"),
            #[cfg(feature = "wamr")]
            Self::Wamr(t) => write!(f, "{t}"),
            #[cfg(feature = "v8")]
            Self::V8(t) => write!(f, "{t}"),
            _ => panic!("No runtime enabled!"),
        }
    }
}

impl std::error::Error for RuntimeTrap {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(t) => t.source(),
            #[cfg(feature = "wamr")]
            Self::Wamr(t) => t.source(),
            #[cfg(feature = "v8")]
            Self::V8(t) => t.source(),
            _ => panic!("No runtime enabled!"),
        }
    }
}
