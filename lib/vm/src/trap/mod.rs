// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/main/docs/ATTRIBUTIONS.md

//! This is the module that facilitates the usage of Traps
//! in Wasmer Runtime

#[allow(clippy::module_inception)]
mod trap;
mod traphandlers;

pub use trap::Trap;
pub use traphandlers::{
    TrapHandlerFn, VMConfig, catch_traps, catch_traps_resume, on_host_stack, raise_lib_trap, raise_user_trap,
    set_stack_size, wasmer_call_trampoline, wasmer_call_trampoline_resume,
};
pub use traphandlers::{init_traps, resume_panic};
pub use wasmer_types::TrapCode;
