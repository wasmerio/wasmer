// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/main/docs/ATTRIBUTIONS.md

//! This is the module that facilitates the usage of Traps
//! in Wasmer Runtime

use crate::vmcontext::{VMFunctionContext, VMTrampoline};
use crate::{VMContext, VMFunctionBody};

#[allow(clippy::module_inception)]
mod trap;
#[cfg(not(feature = "baremetal"))]
mod traphandlers;
#[cfg(feature = "baremetal")]
#[path = "traphandlers_baremetal.rs"]
mod traphandlers;

pub use trap::{Trap, UnwindReason};
#[cfg(feature = "baremetal")]
pub use traphandlers::install_unwinder;
pub use traphandlers::{
    ActivationState, CurrentThread, MAX_STACK_SIZE, TrapHandlerFn, VMConfig, catch_traps,
    drain_stack_pool, get_stack_size, on_host_stack, raise_lib_trap, raise_user_trap,
    set_stack_size,
};
pub use traphandlers::{init_traps, resume_panic};
pub use wasmer_types::TrapCode;

/// Call a Wasm trampoline.
///
/// # Safety
///
/// All pointer arguments must be valid for the duration of the call.
#[inline]
pub unsafe fn wasmer_call_trampoline(
    trap_handler: Option<*const TrapHandlerFn<'static>>,
    config: &VMConfig,
    vmctx: VMFunctionContext,
    trampoline: VMTrampoline,
    callee: *const VMFunctionBody,
    values_vec: *mut u8,
) -> Result<(), Trap> {
    unsafe {
        wasmer_call_trampoline_on(
            CurrentThread::get(),
            trap_handler,
            config,
            vmctx,
            trampoline,
            callee,
            values_vec,
        )
    }
}

/// [`wasmer_call_trampoline`] on `thread`, which must be the calling thread's.
///
/// # Safety
///
/// All pointer arguments must be valid for the duration of the call.
pub unsafe fn wasmer_call_trampoline_on(
    thread: CurrentThread,
    trap_handler: Option<*const TrapHandlerFn<'static>>,
    config: &VMConfig,
    vmctx: VMFunctionContext,
    trampoline: VMTrampoline,
    callee: *const VMFunctionBody,
    values_vec: *mut u8,
) -> Result<(), Trap> {
    unsafe {
        traphandlers::catch_traps_on(thread, trap_handler, config, move || {
            // pointer types are ABI-compatible; transmute to unsafe fn to preserve unsafety
            std::mem::transmute::<
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
