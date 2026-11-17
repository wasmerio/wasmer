// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/main/docs/ATTRIBUTIONS.md

//! This is the module that facilitates the usage of Traps
//! in Wasmer Runtime

use crate::VMFunctionBody;
use crate::vmcontext::{VMFunctionContext, VMTrampoline};

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
    ActivationState, MAX_STACK_SIZE, TrapHandlerFn, VMConfig, catch_traps, drain_stack_pool,
    get_stack_size, on_host_stack, raise_lib_trap, raise_user_trap, set_stack_size,
};
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
    let call = TrampolineCall {
        vmctx,
        trampoline,
        callee,
        values_vec,
    };
    #[cfg(feature = "baremetal")]
    unsafe {
        catch_traps(trap_handler, config, move || call.invoke())
    }
    #[cfg(not(feature = "baremetal"))]
    unsafe {
        traphandlers::call_trampoline(trap_handler, config, call)
    }
}

struct TrampolineCall {
    vmctx: VMFunctionContext,
    trampoline: VMTrampoline,
    callee: *const VMFunctionBody,
    values_vec: *mut u8,
}

impl TrampolineCall {
    /// The caller owns the callee, its context and its ABI slots until this returns.
    unsafe fn invoke(&self) {
        unsafe { (self.trampoline)(self.vmctx.vmctx, self.callee, self.values_vec.cast()) }
    }
}
