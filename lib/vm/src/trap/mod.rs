// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/main/docs/ATTRIBUTIONS.md

//! This is the module that facilitates the usage of Traps
//! in Wasmer Runtime

use crate::vmcontext::{VMFunctionContext, VMTrampoline};
use crate::{VMContext, VMFunctionBody};
#[cfg(all(unix, not(feature = "baremetal")))]
use std::num::NonZeroUsize;

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
    MAX_STACK_SIZE, TrapHandlerFn, VMConfig, catch_traps, drain_stack_pool, get_stack_size,
    on_host_stack, raise_lib_trap, raise_user_trap, set_stack_size,
};
#[cfg(all(unix, not(feature = "baremetal")))]
#[doc(hidden)]
pub use traphandlers::{current_signal_token, try_raise_signal_trap};
pub use traphandlers::{init_traps, resume_panic};
pub use wasmer_types::TrapCode;

/// Call a Wasm trampoline.
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
    let invoke = trampoline_invocation(vmctx, trampoline, callee, values_vec);
    unsafe { catch_traps(trap_handler, config, invoke) }
}

/// Call a Wasm trampoline with a token for an external signal handler.
///
/// # Safety
/// All pointer arguments must be valid for the duration of the call.
#[cfg(all(unix, not(feature = "baremetal")))]
#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub unsafe fn wasmer_call_trampoline_with_signal_token(
    trap_handler: Option<*const TrapHandlerFn<'static>>,
    config: &VMConfig,
    vmctx: VMFunctionContext,
    trampoline: VMTrampoline,
    callee: *const VMFunctionBody,
    values_vec: *mut u8,
    signal_token: NonZeroUsize,
) -> Result<(), Trap> {
    let invoke = trampoline_invocation(vmctx, trampoline, callee, values_vec);
    unsafe { traphandlers::catch_traps_inner(trap_handler, config, Some(signal_token), invoke) }
}

fn trampoline_invocation(
    vmctx: VMFunctionContext,
    trampoline: VMTrampoline,
    callee: *const VMFunctionBody,
    values_vec: *mut u8,
) -> impl FnOnce() {
    move || unsafe {
        // pointer types are ABI-compatible; transmute to unsafe fn to preserve unsafety
        std::mem::transmute::<
            unsafe extern "C" fn(
                *mut VMContext,
                *const VMFunctionBody,
                *mut wasmer_types::RawValue,
            ),
            unsafe extern "C" fn(VMFunctionContext, *const VMFunctionBody, *mut u8),
        >(trampoline)(vmctx, callee, values_vec);
    }
}
