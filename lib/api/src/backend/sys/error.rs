use crate::LinkError;
use wasmer_vm::{Trap, VMExceptionRef};

impl From<wasmer_compiler::LinkError> for LinkError {
    fn from(other: wasmer_compiler::LinkError) -> Self {
        match other {
            wasmer_compiler::LinkError::Import(namespace, name, error) => {
                Self::Import(namespace, name, error)
            }
            wasmer_compiler::LinkError::Trap(e) => Self::Trap(e.into()),
            wasmer_compiler::LinkError::Resource(e) => Self::Resource(e),
        }
    }
}

impl From<Trap> for crate::RuntimeError {
    fn from(trap: Trap) -> Self {
        if trap.is::<Self>() {
            return trap.downcast::<Self>().unwrap();
        }
        let (wasm_trace, trap_code) = wasmer_compiler::get_trace_and_trapcode(&trap);
        Self::new_from_source(crate::BackendTrap::Sys(trap), wasm_trace, trap_code)
    }
}
