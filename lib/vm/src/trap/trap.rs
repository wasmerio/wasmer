use backtrace::Backtrace;
use std::error::Error;
use std::fmt;
use wasmer_types::TrapCode;

use crate::{StoreObjects, VMExceptionRef};

/// Stores trace message with backtrace.
#[derive(Debug)]
pub enum Trap {
    /// A user-raised trap through `raise_user_trap`.
    User(Box<dyn Error + Send + Sync>),

    /// A trap raised from the Wasm generated code
    ///
    /// Note: this trap is deterministic (assuming a deterministic host implementation)
    Wasm {
        /// The program counter in generated code where this trap happened.
        pc: usize,
        /// Native stack backtrace at the time the trap occurred
        backtrace: Backtrace,
        /// Optional trapcode associated to the signal that caused the trap
        signal_trap: Option<TrapCode>,
    },

    /// A trap raised from a wasm libcall
    ///
    /// Note: this trap is deterministic (assuming a deterministic host implementation)
    Lib {
        /// Code of the trap.
        trap_code: TrapCode,
        /// Native stack backtrace at the time the trap occurred
        backtrace: Backtrace,
    },

    /// A trap indicating that the runtime was unable to allocate sufficient memory.
    ///
    /// Note: this trap is nondeterministic, since it depends on the host system.
    OOM {
        /// Native stack backtrace at the time the OOM occurred
        backtrace: Backtrace,
    },

    /// A WASM exception was thrown but not caught.
    UncaughtException {
        /// The exception reference of the uncaught exception.
        exnref: VMExceptionRef,
        /// Native stack backtrace at the time the exception was thrown.
        /// This is a clone of the backtrace stored in the exception itself.
        backtrace: Backtrace,
    },
}

fn _assert_trap_is_sync_and_send(t: &Trap) -> (&dyn Sync, &dyn Send) {
    (t, t)
}

impl Trap {
    /// Construct a new Error with the given a user error.
    ///
    /// Internally saves a backtrace when constructed.
    pub fn user(err: Box<dyn Error + Send + Sync>) -> Self {
        Self::User(err)
    }

    /// Construct a new Wasm trap with the given source location and backtrace.
    ///
    /// Internally saves a backtrace when constructed.
    pub fn wasm(pc: usize, backtrace: Backtrace, signal_trap: Option<TrapCode>) -> Self {
        Self::Wasm {
            pc,
            backtrace,
            signal_trap,
        }
    }

    /// Returns trap code, if it's a Trap
    pub fn to_trap(self) -> Option<TrapCode> {
        unimplemented!()
    }

    /// Construct a new Wasm trap with the given trap code.
    ///
    /// Internally saves a backtrace when constructed.
    pub fn lib(trap_code: TrapCode) -> Self {
        let backtrace = Backtrace::new_unresolved();
        Self::Lib {
            trap_code,
            backtrace,
        }
    }

    /// Construct a new OOM trap with the given source location and trap code.
    ///
    /// Internally saves a backtrace when constructed.
    pub fn oom() -> Self {
        let backtrace = Backtrace::new_unresolved();
        Self::OOM { backtrace }
    }

    /// Construct a new UncaughtException trap with the given exception reference.
    pub fn uncaught_exception(exnref: VMExceptionRef, ctx: &StoreObjects) -> Self {
        Self::UncaughtException {
            backtrace: exnref.0.get(ctx).backtrace().clone(),
            exnref,
        }
    }

    /// Attempts to downcast the `Trap` to a concrete type.
    pub fn downcast<T: Error + 'static>(self) -> Result<T, Self> {
        match self {
            // We only try to downcast user errors
            Self::User(err) if err.is::<T>() => Ok(*err.downcast::<T>().unwrap()),
            _ => Err(self),
        }
    }

    /// Attempts to downcast the `Trap` to a concrete type.
    pub fn downcast_ref<T: Error + 'static>(&self) -> Option<&T> {
        match &self {
            // We only try to downcast user errors
            Self::User(err) if err.is::<T>() => err.downcast_ref::<T>(),
            _ => None,
        }
    }

    /// Returns true if the `Trap` is the same as T
    pub fn is<T: Error + 'static>(&self) -> bool {
        match self {
            Self::User(err) => err.is::<T>(),
            _ => false,
        }
    }

    /// Returns true if the trap is an exception
    pub fn is_exception(&self) -> bool {
        matches!(self, Self::UncaughtException { .. })
    }

    /// If the `Trap` is an uncaught exception, returns it.
    pub fn to_exception_ref(&self) -> Option<VMExceptionRef> {
        match self {
            // Self::UncaughtException { exnref, .. } => Some(Exception::from_vm_exceptionref(
            //     crate::vm::VMExceptionRef::Sys(exnref.clone()),
            // )),
            Self::UncaughtException { exnref, .. } => Some(exnref.clone()),
            _ => None,
        }
    }
}

impl std::error::Error for Trap {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self {
            Self::User(err) => Some(&**err),
            _ => None,
        }
    }
}

impl fmt::Display for Trap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::User(e) => write!(f, "{e}"),
            Self::Lib { .. } => write!(f, "lib"),
            Self::Wasm { .. } => write!(f, "wasm"),
            Self::OOM { .. } => write!(f, "Wasmer VM out of memory"),
            Self::UncaughtException { .. } => write!(f, "Uncaught wasm exception"),
        }
    }
}
