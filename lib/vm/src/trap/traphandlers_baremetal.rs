//! Trap handling for bare-metal targets.
//!
//! On conventional operating systems, Wasmer's trap handler uses OS signal
//! infrastructure (`SIGSEGV`, `SIGFPE`, …) together with coroutine stacks
//! managed by `corosensei`.  That machinery does not exist on bare-metal
//! targets (e.g. embedded systems or ZK virtual machines).
//!
//! This module provides the same public API as `traphandlers.rs` but with
//! all OS-specific parts replaced by no-ops or panics.  The one meaningful
//! addition is [`install_unwinder`]: it lets the host register a callback
//! that is invoked whenever Wasm execution would normally raise a trap.
//! Without an unwinder the trap is forwarded as a Rust panic.
//!
//! # Drop / destructor behaviour
//!
//! The OS backend catches traps at the coroutine boundary inside
//! [`catch_traps`]; destructors are skipped only for frames between the raise
//! site and that boundary.  `libcalls.rs` documents the pattern (nested block
//! before `raise_lib_trap`) that ensures libcall-owned values are dropped
//! before the raise.
//!
//! In baremetal mode there is no coroutine boundary.  When the installed
//! unwinder exits — whether by `process::abort`, `longjmp`, or any other
//! non-local transfer — **destructors are skipped for every Rust frame above
//! the unwinder's landing site**, not just those inside a Wasm coroutine.
//! The coding pattern from `libcalls.rs` is therefore even more important
//! here: all libcall-owned values must be dropped in a nested block *before*
//! any call to [`raise_lib_trap`] or [`raise_user_trap`].

use crate::vmcontext::{VMFunctionContext, VMTrampoline};
use crate::{Trap, VMContext, VMFunctionBody};
use std::any::Any;
use std::error::Error;
use std::mem;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// Types that mirror the OS traphandlers API
// ---------------------------------------------------------------------------

/// Dummy trap-handler callback type.
///
/// On OS targets this is a platform-specific signal-handler signature; on
/// bare-metal targets there are no signals, so the type degrades to `()`.
pub type TrapHandlerFn<'a> = ();

/// Runtime VM configuration.
pub struct VMConfig {
    /// Optional stack size hint. Ignored in baremetal mode (there is no
    /// separate Wasm coroutine stack), but kept for API compatibility.
    pub wasm_stack_size: Option<usize>,
}

/// Upper bound on the configurable stack size.
///
/// Bare-metal mode does not manage stacks, so this is 0.  The constant is
/// preserved solely for API compatibility.
pub const MAX_STACK_SIZE: usize = 0;

/// No-op in baremetal mode — there is no stack pool to drain.
pub fn drain_stack_pool() {}

/// Panics: stack sizing is not meaningful in baremetal mode.
pub fn set_stack_size(_size: usize) {
    panic!("set_stack_size is not supported in baremetal mode");
}

/// Panics: stack sizing is not meaningful in baremetal mode.
pub fn get_stack_size() -> usize {
    panic!("get_stack_size is not supported in baremetal mode");
}

/// No-op in baremetal mode — there are no signal handlers to install.
pub fn init_traps() {}

/// Run `f` directly on the current stack.
///
/// On OS targets this switches to a dedicated Wasm coroutine stack.  In
/// baremetal mode there is only one stack, so `f` runs in-place.
pub fn on_host_stack<F: FnOnce() -> T, T>(f: F) -> T {
    f()
}

/// Run `closure` on the current stack.
///
/// **This function never returns `Err`.**  In baremetal mode there is no
/// signal-based trap detection, so the `Result` return type is kept only for
/// API compatibility with the OS trap-handler backend.
///
/// Trap recovery is fully delegated to the unwinder installed via
/// [`install_unwinder`]:
/// * Explicit traps ([`raise_lib_trap`], [`raise_user_trap`]) invoke the
///   unwinder directly and bypass this call frame.
/// * Rust panics propagate as normal Rust panics — they are not caught here
///   and are not converted to `Err`.
/// * Hardware faults (divide-by-zero, misaligned access, …) are not detected
///   and will terminate the process or produce undefined behaviour.
///
/// `trap_handler` and `config` are accepted for API compatibility and are
/// ignored.
pub unsafe fn catch_traps<F, R: 'static>(
    _trap_handler: Option<*const TrapHandlerFn<'static>>,
    _config: &VMConfig,
    closure: F,
) -> Result<R, Trap>
where
    F: FnOnce() -> R + 'static,
{
    Ok(closure())
}

/// Call a Wasm trampoline, returning any trap as an `Err`.
///
/// # Safety
///
/// All pointer arguments must be valid for the duration of the call.
pub unsafe fn wasmer_call_trampoline(
    trap_handler: Option<*const TrapHandlerFn<'static>>,
    config: &VMConfig,
    vmctx: VMFunctionContext,
    trampoline: VMTrampoline,
    callee: *const VMFunctionBody,
    values_vec: *mut u8,
) -> Result<(), Trap> {
    unsafe {
        catch_traps(trap_handler, config, move || {
            mem::transmute::<
                unsafe extern "C" fn(
                    *mut VMContext,
                    *const VMFunctionBody,
                    *mut wasmer_types::RawValue,
                ),
                extern "C" fn(VMFunctionContext, *const VMFunctionBody, *mut u8),
            >(trampoline)(vmctx, callee, values_vec);
        })
    }
}

// ---------------------------------------------------------------------------
// Custom unwinder
// ---------------------------------------------------------------------------

/// The reason a Wasm execution is being unwound.
///
/// Passed to the callback registered with [`install_unwinder`].
#[derive(Debug)]
pub enum UnwindReason {
    /// A Rust panic propagated out of a host function.
    Panic(Box<dyn Any + Send>),
    /// A user-defined error raised via [`raise_user_trap`].
    UserTrap(Box<dyn Error + Send + Sync>),
    /// A trap raised by a Wasm libcall via [`raise_lib_trap`].
    LibTrap(Trap),
}

impl UnwindReason {
    /// Convert this reason into a [`Trap`].
    ///
    /// For `Panic` variants the panic is re-raised rather than converted.
    pub fn into_trap(self) -> Trap {
        match self {
            Self::UserTrap(data) => Trap::User(data),
            Self::LibTrap(trap) => trap,
            Self::Panic(panic) => std::panic::resume_unwind(panic),
        }
    }
}

// The unwinder is accessed from trap-raise sites, which may run concurrently
// on separate threads even in embedded contexts.  We store it behind an Arc so
// that `unwind_with` can clone the reference before releasing the lock,
// preventing a potential deadlock if the callback itself calls
// `install_unwinder`.
static UNWINDER: Mutex<Option<Arc<dyn Fn(UnwindReason) + Send + Sync>>> = Mutex::new(None);

/// Install (or remove) the trap unwinder for baremetal targets.
///
/// The callback is invoked — and must not return — whenever a trap is raised
/// in Wasm code running in baremetal mode.  If no unwinder is installed the
/// trap is forwarded as a Rust `panic!`.
///
/// Passing `None` removes a previously installed unwinder.
pub fn install_unwinder(unwinder: Option<Arc<dyn Fn(UnwindReason) + Send + Sync>>) {
    *UNWINDER.lock().unwrap() = unwinder;
}

fn unwind_with(reason: UnwindReason) -> ! {
    // Clone the Arc while holding the lock so we can release the lock before
    // invoking the callback (avoids deadlock if the callback calls
    // install_unwinder itself).
    let unwinder = UNWINDER.lock().unwrap().clone();
    match unwinder {
        Some(f) => {
            f(reason);
            unreachable!("baremetal unwinder must not return");
        }
        None => panic!("wasm trap with no baremetal unwinder installed: {reason:?}"),
    }
}

// ---------------------------------------------------------------------------
// Trap-raise entry points (mirror the OS traphandlers signatures)
// ---------------------------------------------------------------------------

/// Raise a trap from a Wasm libcall.
///
/// # Safety
///
/// Must only be called from Wasm-generated code running inside
/// [`wasmer_call_trampoline`] / [`catch_traps`].
///
/// All locally-owned values in the calling frame must be dropped *before*
/// this function is called.  The installed unwinder will perform a non-local
/// exit that skips destructors for every frame above its landing site.  See
/// the module-level documentation for the recommended nested-block pattern.
pub unsafe fn raise_lib_trap(trap: Trap) -> ! {
    unwind_with(UnwindReason::LibTrap(trap))
}

/// Raise a user-defined trap error.
///
/// # Safety
///
/// Must only be called from Wasm-generated code running inside
/// [`wasmer_call_trampoline`] / [`catch_traps`].
pub unsafe fn raise_user_trap(data: Box<dyn Error + Send + Sync>) -> ! {
    unwind_with(UnwindReason::UserTrap(data))
}

/// Resume a Rust panic that was caught by the Wasm runtime.
///
/// # Safety
///
/// `payload` must be the value that was originally passed to `panic!`.
pub unsafe fn resume_panic(payload: Box<dyn Any + Send>) -> ! {
    unwind_with(UnwindReason::Panic(payload))
}
