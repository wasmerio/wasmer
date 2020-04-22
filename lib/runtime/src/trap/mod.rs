//! This is the module that facilitates the usage of Traps
//! in Wasmer Runtime
mod trap_registry;
mod trapcode;
mod traphandlers;

pub use trap_registry::{
    register_traps, TrapDescription, TrapInformation, TrapRegistration, TrapRegistry, Traps,
};
pub use trapcode::TrapCode;
pub use traphandlers::{
    catch_traps, raise_lib_trap, raise_user_trap, wasmer_call_trampoline, Trap,
};
pub use traphandlers::{init as init_traphandlers, resume_panic};
