use backtrace::Backtrace;
use std::any::Any;
use std::error::Error;
use std::fmt;
use wasmer_types::TrapCode;

use crate::{StoreObjects, VMExceptionRef};

#[cfg(all(unix, feature = "experimental-host-interrupt"))]
fn without_host_interrupt<F: FnOnce() -> T, T>(f: F) -> T {
    struct RestoreSignalMask(libc::sigset_t);

    impl Drop for RestoreSignalMask {
        fn drop(&mut self) {
            let result =
                unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &self.0, std::ptr::null_mut()) };
            debug_assert_eq!(result, 0);
        }
    }

    let restore = unsafe {
        let mut blocked = std::mem::zeroed();
        let mut previous = std::mem::zeroed();
        libc::sigemptyset(&mut blocked);
        libc::sigaddset(&mut blocked, libc::SIGUSR1);
        let result = libc::pthread_sigmask(libc::SIG_BLOCK, &blocked, &mut previous);
        assert_eq!(result, 0, "failed to defer the host-interrupt signal");
        RestoreSignalMask(previous)
    };
    let result = f();
    drop(restore);
    result
}

#[cfg(not(all(unix, feature = "experimental-host-interrupt")))]
fn without_host_interrupt<F: FnOnce() -> T, T>(f: F) -> T {
    f()
}

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
        // Trap construction can be reached from a libcall while the Wasm
        // coroutine is active. Capturing a backtrace takes process-global
        // locks, so defer SIGUSR1 until those lock guards have been dropped.
        // Staying on the Wasm stack preserves the guest frames in the trace.
        let backtrace = without_host_interrupt(Backtrace::new_unresolved);
        Self::Lib {
            trap_code,
            backtrace,
        }
    }

    /// Construct a synthetic host-interrupt trap.
    ///
    /// An interrupt is control-plane state rather than a guest fault. Capturing
    /// a native backtrace here is both unnecessary and unsafe: this constructor
    /// can race the signal-based interrupt path while another backtrace owns
    /// process-global unwinder locks.
    pub fn host_interrupt() -> Self {
        Self::Lib {
            trap_code: TrapCode::HostInterrupt,
            backtrace: Backtrace::from(Vec::new()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_interrupt_does_not_capture_a_native_backtrace() {
        let Trap::Lib {
            trap_code,
            backtrace,
        } = Trap::host_interrupt()
        else {
            unreachable!()
        };
        assert_eq!(trap_code, TrapCode::HostInterrupt);
        assert!(backtrace.frames().is_empty());
    }
}

/// The reason a Wasm execution is being unwound.
///
/// Shared by both trap-handler backends. `WasmTrap` is only produced by the OS
/// backend (signal handler); it is never constructed in baremetal mode.
#[derive(Debug)]
#[non_exhaustive]
pub enum UnwindReason {
    /// A panic caused by the host
    Panic(Box<dyn Any + Send>),
    /// A custom error triggered by the user
    UserTrap(Box<dyn Error + Send + Sync>),
    /// A Trap triggered by a wasm libcall
    LibTrap(Trap),
    /// A trap caused by the Wasm generated code
    WasmTrap {
        /// Native stack backtrace at the time the trap occurred
        backtrace: Backtrace,
        /// Program counter in generated code where the trap occurred
        pc: usize,
        /// Optional trap code associated with the faulting signal
        signal_trap: Option<TrapCode>,
    },
}

impl UnwindReason {
    /// Convert to a [`Trap`], or resume unwinding for the `Panic` variant (never returns).
    pub fn into_trap(self) -> Trap {
        match self {
            Self::UserTrap(data) => Trap::User(data),
            Self::LibTrap(trap) => trap,
            Self::WasmTrap {
                backtrace,
                pc,
                signal_trap,
            } => Trap::wasm(pc, backtrace, signal_trap),
            Self::Panic(panic) => std::panic::resume_unwind(panic),
        }
    }
}
