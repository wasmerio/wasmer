//! Trap handling for targets without OS signal infrastructure.
//!
//! On conventional operating systems, Wasmer's trap handler uses OS signal
//! infrastructure (`SIGSEGV`, `SIGFPE`, …) together with coroutine stacks
//! managed by `corosensei`.  That machinery is absent on targets such as ZK
//! virtual machines or embedded RTOSes that provide a Rust `std` environment
//! but no Unix/Windows signal delivery.
//!
//! > **`std` is still required.**  This module uses `std::sync::{Arc, Mutex}`
//! > and `thread_local!`.  It targets environments where the *signal* layer is
//! > missing, not environments where the standard library itself is missing.
//! > True `no_std` support would require a separate effort.
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
use std::cell::Cell;
use std::error::Error;
use std::fmt;
use std::mem;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// Types that mirror the OS traphandlers API
// ---------------------------------------------------------------------------

/// Signal-handler callback type.
///
/// On OS targets this is a platform-specific `dyn Fn` signature that a caller
/// can use to intercept raw signals.  On bare-metal targets there are no
/// signals, so this is an **uninhabited** marker type: it cannot be
/// constructed, and any `Option<*const TrapHandlerFn<'_>>` must be `None`.
/// Attempting to form a real handler value will produce a compile error.
pub enum TrapHandlerFn<'a> {
    #[doc(hidden)]
    _Uninhabited(std::convert::Infallible, std::marker::PhantomData<&'a ()>),
}

/// Runtime VM configuration.
pub struct VMConfig {
    /// Optional stack size hint.  Ignored in baremetal mode (there is no
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
/// baremetal mode there is no Wasm coroutine stack to switch to, so `f` runs
/// in-place.
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
///   unwinder directly and bypass this call frame entirely.
/// * Rust panics propagate as normal Rust panics — they are not caught here
///   and are not converted to `Err`.
/// * Hardware faults (divide-by-zero, misaligned access, …) are not detected
///   and will terminate the process or produce undefined behaviour.
///
/// `trap_handler` and `config` are accepted for API compatibility and are
/// ignored.
///
/// # Safety
///
/// The closure must not rely on its destructor running if a trap is raised
/// inside it; the unwinder's non-local exit will skip all destructors above
/// its landing site.  See the module-level documentation.
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
            // SAFETY: `VMFunctionContext` is a `*mut VMContext` newtype and
            // `*mut u8` / `*mut RawValue` are both opaque byte-array pointers
            // with identical layout and calling-convention treatment on all
            // supported targets.  The transmute changes only the Rust type
            // checker's view of the pointer types, not the generated machine
            // code.  We transmute to `unsafe extern "C" fn` to preserve the
            // unsafety at the call site.
            mem::transmute::<
                unsafe extern "C" fn(
                    *mut VMContext,
                    *const VMFunctionBody,
                    *mut wasmer_types::RawValue,
                ),
                unsafe extern "C" fn(VMFunctionContext, *const VMFunctionBody, *mut u8),
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
///
/// # Structural divergence from the OS backend
///
/// The OS backend's internal `UnwindReason` has a fourth `WasmTrap` variant
/// produced by signal handlers.  That variant cannot exist in baremetal mode
/// (there are no signal handlers), so it is omitted here.  This type is
/// `#[non_exhaustive]` to allow adding variants in future without a breaking
/// change; match arms should include a wildcard to remain forward-compatible.
#[derive(Debug)]
#[non_exhaustive]
pub enum UnwindReason {
    /// A Rust panic propagated out of a host function.
    ///
    /// Note: `Box<dyn Any + Send>` — the bound is `Send` only, not `Send +
    /// Sync`, because [`std::panic::resume_unwind`] requires only `Send`.
    /// Adding `Sync` here would silently prevent resuming panics that lack it.
    Panic(Box<dyn Any + Send>),
    /// A user-defined error raised via [`raise_user_trap`].
    UserTrap(Box<dyn Error + Send + Sync>),
    /// A trap raised by a Wasm libcall via [`raise_lib_trap`].
    LibTrap(Trap),
}

impl fmt::Display for UnwindReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Panic(_) => write!(f, "panic"),
            Self::UserTrap(e) => write!(f, "user trap: {e}"),
            Self::LibTrap(t) => write!(f, "lib trap: {t}"),
        }
    }
}

impl UnwindReason {
    /// Convert this reason into a [`Trap`], or re-raise the panic.
    ///
    /// For the `Panic` variant this calls [`std::panic::resume_unwind`] and
    /// therefore **never returns**.  Callers that need to distinguish the
    /// non-returning arm should match on the variant before calling this.
    pub fn into_trap(self) -> Trap {
        match self {
            Self::UserTrap(data) => Trap::User(data),
            Self::LibTrap(trap) => trap,
            Self::Panic(panic) => std::panic::resume_unwind(panic),
        }
    }
}

// The unwinder is stored behind an Arc so that `unwind_with` can clone the
// reference before releasing the lock, allowing the callback to call
// `install_unwinder` without deadlocking.
static UNWINDER: Mutex<Option<Arc<dyn Fn(UnwindReason) + Send + Sync>>> = Mutex::new(None);

/// Install (or remove) the trap unwinder for baremetal targets.
///
/// The callback receives an [`UnwindReason`] and **must not return** — it
/// should transfer control out of the current call stack via
/// `process::abort`, a `longjmp`, or equivalent.  Returning from the callback
/// is treated as a bug and will trigger `unreachable!`.
///
/// Note: the callback signature is `Fn(UnwindReason)` rather than
/// `Fn(UnwindReason) -> !` because `-> !` is not currently object-safe in
/// Rust stable, preventing its use in a `dyn Fn` trait object.
///
/// If no unwinder is installed, any trap forwards as a Rust `panic!`.
/// Passing `None` removes a previously installed unwinder.
///
/// # Concurrency
///
/// `install_unwinder` may be called from any thread at any time, but the
/// semantic result of swapping the unwinder concurrently with live Wasm
/// execution is non-deterministic: a trap raised during the swap may be
/// handled by either the old or the new unwinder.  Install the unwinder
/// before starting Wasm execution to avoid this.
pub fn install_unwinder(unwinder: Option<Arc<dyn Fn(UnwindReason) + Send + Sync>>) {
    *UNWINDER
        .lock()
        .expect("baremetal unwinder mutex poisoned in install_unwinder") = unwinder;
}

thread_local! {
    static UNWINDING: Cell<bool> = const { Cell::new(false) };
}

struct UnwindingGuard;

impl UnwindingGuard {
    /// Set the flag and return a guard that clears it on drop.
    ///
    /// Returns `None` if the flag was already set (re-entrant call).
    fn acquire() -> Option<Self> {
        UNWINDING.with(|u| {
            if u.replace(true) {
                None
            } else {
                Some(UnwindingGuard)
            }
        })
    }
}

impl Drop for UnwindingGuard {
    fn drop(&mut self) {
        UNWINDING.with(|u| u.set(false));
    }
}

fn unwind_with(reason: UnwindReason) -> ! {
    // Detect re-entrant calls: if the unwinder itself triggers a trap we must
    // not recurse (doing so would corrupt its state or overflow the typically
    // tiny embedded stack).
    //
    // The guard resets the flag in Drop, so if this function panics and the
    // panic is caught by a higher-level catch_unwind, the thread-local is
    // cleared and future traps on this thread are not misclassified.
    let Some(_guard) = UnwindingGuard::acquire() else {
        panic!("wasm trap raised inside the baremetal unwinder (re-entrant unwinding): {reason}");
    };

    // Clone the Arc while holding the lock, then release the lock before
    // invoking the callback so the callback can call `install_unwinder`
    // without deadlocking.
    let unwinder = UNWINDER
        .lock()
        .expect("baremetal unwinder mutex poisoned in unwind_with")
        .clone();

    match unwinder {
        Some(f) => {
            // Capture the display string before moving `reason` into the
            // callback, so the unreachable! message retains diagnostic context
            // even though `f` must not return.
            let display = reason.to_string();
            f(reason);
            unreachable!("baremetal unwinder must not return (trap was: {display})");
        }
        None => panic!("wasm trap with no baremetal unwinder installed: {reason}"),
    }
}

// ---------------------------------------------------------------------------
// Trap-raise entry points (mirror the OS traphandlers signatures)
// ---------------------------------------------------------------------------

/// Raise a trap from a Wasm libcall.
///
/// # Safety
///
/// All locally-owned values in the calling frame must be dropped *before*
/// this function is called.  The installed unwinder performs a non-local exit
/// that skips destructors for every frame above its landing site.  See the
/// module-level documentation for the recommended nested-block pattern.
pub unsafe fn raise_lib_trap(trap: Trap) -> ! {
    unwind_with(UnwindReason::LibTrap(trap))
}

/// Raise a user-defined trap error.
///
/// # Safety
///
/// All locally-owned values in the calling frame must be dropped before this
/// function is called (see [`raise_lib_trap`] and the module-level
/// documentation).
pub unsafe fn raise_user_trap(data: Box<dyn Error + Send + Sync>) -> ! {
    unwind_with(UnwindReason::UserTrap(data))
}

/// Forward a previously caught Rust panic to the installed unwinder.
///
/// # Safety
///
/// `payload` must be the value originally passed to `panic!` and must not
/// have been partially moved or invalidated since it was caught.
pub unsafe fn resume_panic(payload: Box<dyn Any + Send>) -> ! {
    unwind_with(UnwindReason::Panic(payload))
}
