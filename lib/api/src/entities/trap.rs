use std::{any::Any, error::Error, fmt::Debug};

use crate::{macros::backend::match_rt, RuntimeError};

/// An enumeration of all the trap kinds supported by the runtimes.
#[derive(Debug, derive_more::From)]
pub enum BackendTrap {
    #[cfg(feature = "sys")]
    /// The trap from the `sys` runtime.
    Sys(crate::backend::sys::vm::Trap),

    #[cfg(feature = "wamr")]
    /// The trap from the `wamr` runtime.
    Wamr(crate::backend::wamr::vm::Trap),

    #[cfg(feature = "wasmi")]
    /// The trap from the `wasmi` runtime.
    Wasmi(crate::backend::wasmi::vm::Trap),

    #[cfg(feature = "v8")]
    /// The trap from the `v8` runtime.
    V8(crate::backend::v8::vm::Trap),

    #[cfg(feature = "js")]
    /// The trap from the `js` runtime.
    Js(crate::backend::js::vm::Trap),

    #[cfg(feature = "jsc")]
    /// The trap from the `jsc` runtime.
    Jsc(crate::backend::jsc::vm::Trap),
}

impl BackendTrap {
    /// Construct a new Error with the given a user error.
    ///
    /// Internally saves a backtrace when constructed.
    pub fn user(err: Box<dyn Error + Send + Sync>) -> RuntimeError {
        #[cfg(feature = "sys")]
        {
            return crate::backend::sys::vm::Trap::user(err).into();
        }
        #[cfg(feature = "wamr")]
        {
            return crate::backend::wamr::vm::Trap::user(err).into();
        }

        #[cfg(feature = "wasmi")]
        {
            return crate::backend::wasmi::vm::Trap::user(err).into();
        }

        #[cfg(feature = "v8")]
        {
            return crate::backend::v8::vm::Trap::user(err).into();
        }
        #[cfg(feature = "js")]
        {
            return crate::backend::js::vm::Trap::user(err).into();
        }
        #[cfg(feature = "jsc")]
        {
            return crate::backend::jsc::vm::Trap::user(err).into();
        }

        panic!("No runtime enabled!")
    }
    /// Attempts to downcast the `Trap` to a concrete type.
    #[inline]
    pub fn downcast<T: Error + 'static>(self) -> Result<T, Self> {
        match_rt!(on self => s {
            s.downcast::<T>().map_err(Into::into)
        })
    }

    /// Attempts to downcast the `Trap` to a concrete type.
    #[inline]
    pub fn downcast_ref<T: Error + 'static>(&self) -> Option<&T> {
        match_rt!(on self => s {
            s.downcast_ref::<T>()
        })
    }

    /// Returns true if the `Trap` is the same as T
    #[inline]
    pub fn is<T: Error + 'static>(&self) -> bool {
        match_rt!(on self => s {
            s.is::<T>()
        })
    }
}

impl std::fmt::Display for BackendTrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match_rt!(on self => s {
            (s as &dyn std::fmt::Display).fmt(f)
        })
    }
}

impl std::error::Error for BackendTrap {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match_rt!(on self => s {
            s.source()
        })
    }
}
