// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md

//! This is the module that facilitates the usage of Traps
//! in Wasmer Runtime

mod code;
mod handlers;

pub use code::TrapCode;
pub use handlers::{
    catch_traps, catch_traps_with_result, init_traps, raise_lib_trap, raise_user_trap,
    resume_panic, wasmer_call_trampoline, SignalHandler, Trap, TrapInfo,
};
