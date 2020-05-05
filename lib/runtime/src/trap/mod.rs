//! This is the module that facilitates the usage of Traps
//! in Wasmer Runtime
mod trapcode;
mod traphandlers;

pub use trapcode::TrapCode;
pub use traphandlers::{
    catch_traps, raise_lib_trap, raise_user_trap, wasmer_call_trampoline, Trap,
};
pub use traphandlers::{init_traps, resume_panic};
