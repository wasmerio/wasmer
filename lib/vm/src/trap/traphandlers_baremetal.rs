//! Signal-free trap handling (`baremetal` feature).
//!
//! Mirrors the `traphandlers.rs` API without OS signals or coroutine stacks.
//! Traps are routed through a host-installed callback (`install_unwinder`).
//! Without one, they become panics. std is still required (`Mutex`, `thread_local`).
//!
//! Unlike the OS backend, the unwinder does a naked non-local exit, skipping
//! destructors for everything above its landing site — not just Wasm frames.
//! Drop all locals before calling the raise functions.

use super::trap::UnwindReason;
use crate::Trap;
use std::{
    any::Any, cell::Cell, convert::Infallible, error::Error, marker::PhantomData, sync::Mutex,
};

/// Uninhabited type - no OS signals exist, so no handler can be constructed.
pub enum TrapHandlerFn<'a> {
    #[doc(hidden)]
    /// Uninhabited variant; carries the lifetime. Cannot be constructed.
    _Uninhabited(Infallible, PhantomData<&'a ()>),
}

/// Configuration for the runtime VM
pub struct VMConfig {
    /// Ignored in baremetal mode. Kept for API compatibility.
    pub wasm_stack_size: Option<usize>,
}

/// Always 0 in baremetal mode — no coroutine stacks to bound.
pub const MAX_STACK_SIZE: usize = 0;

/// No-op.
pub fn drain_stack_pool() {}

/// Not supported in baremetal mode.
pub fn set_stack_size(_size: usize) {
    panic!("set_stack_size is not supported in baremetal mode");
}

/// Not supported in baremetal mode.
pub fn get_stack_size() -> usize {
    panic!("get_stack_size is not supported in baremetal mode");
}

/// No-op — no signal handlers to install.
pub fn init_traps() {}

/// Runs `f` in-place (there is only one stack).
pub fn on_host_stack<F: FnOnce() -> T, T>(f: F) -> T {
    f()
}

/// Just run the `closure` and return its result wrapped in `Ok()`. Nothing is
/// caught in `baremetal` mode.
///
/// # Safety
///
/// The closure must not rely on its destructor running if a trap is raised;
/// the unwinder's non-local exit skips all destructors above its landing site.
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

/// Registered unwinder, if any.
static UNWINDER: Mutex<Option<Box<dyn Fn(UnwindReason) + Send>>> = Mutex::new(None);

/// Register (or clear) the trap unwinder.
///
/// The callback **must not return** — it should perform a non-local exit
/// (abort, longjmp, …). Returning is treated as `unreachable!`.
///
/// If no unwinder is installed, traps forward as Rust panics.
/// Install before starting Wasm execution — swapping the unwinder concurrently
/// with a live trap is non-deterministic. The callback must not call
/// `install_unwinder` (the mutex is held for the duration of the call).
pub fn install_unwinder(unwinder: Option<Box<dyn Fn(UnwindReason) + Send>>) {
    *UNWINDER
        .lock()
        .expect("baremetal unwinder mutex poisoned in install_unwinder") = unwinder;
}

thread_local! {
    static UNWINDING: Cell<bool> = const { Cell::new(false) };
}

struct UnwindingGuard;

impl UnwindingGuard {
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
    // Re-entrance guard: if the unwinder itself triggers a trap, panic instead
    // of recursing. RAII resets the flag even if this function panics.
    let Some(_guard) = UnwindingGuard::acquire() else {
        panic!("wasm trap raised inside the baremetal unwinder (re-entrant unwinding): {reason:?}");
    };

    let guard = UNWINDER
        .lock()
        .expect("baremetal unwinder mutex poisoned in unwind_with");

    match guard.as_ref() {
        Some(f) => {
            let display = format!("{reason:?}");
            f(reason);
            unreachable!("baremetal unwinder must not return (trap was: {display})");
        }
        None => panic!("wasm trap with no baremetal unwinder installed: {reason:?}"),
    }
}

/// Raise a trap from a Wasm libcall.
///
/// # Safety
///
/// Drop all locally-owned values before calling — the unwinder skips destructors
/// for every frame above its landing site (see module docs).
pub unsafe fn raise_lib_trap(trap: Trap) -> ! {
    unwind_with(UnwindReason::LibTrap(trap))
}

/// Raise a user-defined trap.
///
/// # Safety
///
/// Drop all locally-owned values before calling (see [`raise_lib_trap`]).
pub unsafe fn raise_user_trap(data: Box<dyn Error + Send + Sync>) -> ! {
    unwind_with(UnwindReason::UserTrap(data))
}

/// Forward a previously caught Rust panic to the unwinder.
///
/// # Safety
///
/// `payload` must be the original panic payload and must not have been
/// partially moved since it was caught.
pub unsafe fn resume_panic(payload: Box<dyn Any + Send>) -> ! {
    unwind_with(UnwindReason::Panic(payload))
}
