use crate::c_api::trap::Trap;
use crate::RuntimeError;

impl From<Trap> for RuntimeError {
    fn from(trap: Trap) -> Self {
        if trap.is::<RuntimeError>() {
            return trap.downcast::<RuntimeError>().unwrap();
        }
        let wasm_trace = vec![];
        let trap_code = None;
        RuntimeError::new_from_source(trap, wasm_trace, trap_code)
    }
}
