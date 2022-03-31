// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md

//! WebAssembly trap handling, which is built on top of the lower-level
//! signalhandling mechanisms.

use crate::vmcontext::{VMFunctionBody, VMFunctionEnvironment, VMTrampoline};
use backtrace::Backtrace;
use std::any::Any;
use std::error::Error;
use std::sync::Once;
use wasmer_types::{Trap, TrapCode};

/// Not implemented on FakeVM
pub type TrapHandlerFn = dyn Fn() -> bool;

/// A package of functionality needed by `catch_traps` to figure out what to do
/// when handling a trap.
///
/// Note that this is an `unsafe` trait at least because it's being run in the
/// context of a synchronous signal handler, so it needs to be careful to not
/// access too much state in answering these queries.
pub unsafe trait TrapHandler {
    /// Uses `call` to call a custom signal handler, if one is specified.
    ///
    /// Returns `true` if `call` returns true, otherwise returns `false`.
    fn custom_trap_handler(&self, call: &dyn Fn(&TrapHandlerFn) -> bool) -> bool;
}

/// Fake plateform functions
fn platform_init() {}
//fn trap_handler() {}

/// This function is required to be called before any WebAssembly is entered.
/// This will configure global state such as signal handlers to prepare the
/// process to receive wasm traps.
///
/// This function must not only be called globally once before entering
/// WebAssembly but it must also be called once-per-thread that enters
/// WebAssembly. Currently in wasmer's integration this function is called on
/// creation of a `Store`.
pub fn init_traps() {
    static INIT: Once = Once::new();
    INIT.call_once(|| platform_init());
}

/// Raises a user-defined trap immediately.
///
/// This function performs as-if a wasm trap was just executed, only the trap
/// has a dynamic payload associated with it which is user-provided. This trap
/// payload is then returned from `catch_traps` below.
///
/// # Safety
///
/// Only safe to call when wasm code is on the stack, aka `catch_traps` must
/// have been previous called and not yet returned.
/// Additionally no Rust destructors may be on the stack.
/// They will be skipped and not executed.
pub unsafe fn raise_user_trap(data: Box<dyn Error + Send + Sync>) -> ! {
    unwind_with(UnwindReason::UserTrap(data))
}

/// Raises a trap from inside library code immediately.
///
/// This function performs as-if a wasm trap was just executed. This trap
/// payload is then returned from `catch_traps` below.
///
/// # Safety
///
/// Only safe to call when wasm code is on the stack, aka `catch_traps` must
/// have been previous called and not yet returned.
/// Additionally no Rust destructors may be on the stack.
/// They will be skipped and not executed.
pub unsafe fn raise_lib_trap(trap: Trap) -> ! {
    unwind_with(UnwindReason::LibTrap(trap))
}

/// Carries a Rust panic across wasm code and resumes the panic on the other
/// side.
///
/// # Safety
///
/// Only safe to call when wasm code is on the stack, aka `catch_traps` must
/// have been previously called and not returned. Additionally no Rust destructors may be on the
/// stack. They will be skipped and not executed.
pub unsafe fn resume_panic(payload: Box<dyn Any + Send>) -> ! {
    unwind_with(UnwindReason::Panic(payload))
}

/// Not implemented on FakeVM
pub unsafe fn wasmer_call_trampoline(
    _trap_handler: &(impl TrapHandler + 'static),
    _vmctx: VMFunctionEnvironment,
    _trampoline: VMTrampoline,
    _callee: *const VMFunctionBody,
    _values_vec: *mut u8,
) -> Result<(), Trap> {
    panic!("Not implemented!")
}

/// Not implemented on FakeVM
pub unsafe fn catch_traps<F, R>(
    _trap_handler: &(dyn TrapHandler + 'static),
    _closure: F,
) -> Result<R, Trap>
where
    F: FnOnce() -> R,
{
    panic!("Not implemented!")
}

/// Not implemented on FakeVM
pub fn on_host_stack<F: FnOnce() -> T, T>(_f: F) -> T {
    panic!("Not implemented!")
}

#[allow(dead_code)]
enum UnwindReason {
    /// A panic caused by the host
    Panic(Box<dyn Any + Send>),
    /// A custom error triggered by the user
    UserTrap(Box<dyn Error + Send + Sync>),
    /// A Trap triggered by a wasm libcall
    LibTrap(Trap),
    /// A trap caused by the Wasm generated code
    WasmTrap {
        backtrace: Backtrace,
        pc: usize,
        signal_trap: Option<TrapCode>,
    },
}

#[allow(dead_code)]
impl UnwindReason {
    fn to_trap(self) -> Trap {
        match self {
            UnwindReason::UserTrap(data) => Trap::User(data),
            UnwindReason::LibTrap(trap) => trap,
            UnwindReason::WasmTrap {
                backtrace,
                pc,
                signal_trap,
            } => Trap::wasm(pc, backtrace, signal_trap),
            UnwindReason::Panic(panic) => std::panic::resume_unwind(panic),
        }
    }
}

unsafe fn unwind_with(_reason: UnwindReason) -> ! {
    panic!("Not implemented!")
}
