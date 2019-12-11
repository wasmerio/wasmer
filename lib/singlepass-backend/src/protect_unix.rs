//! Installing signal handlers allows us to handle traps and out-of-bounds memory
//! accesses that occur when runniing WebAssembly.
//!
//! This code is inspired by: https://github.com/pepyakin/wasmtime/commit/625a2b6c0815b21996e111da51b9664feb174622
//!
//! When a WebAssembly module triggers any traps, we perform recovery here.
//!
//! This module uses TLS (thread-local storage) to track recovery information. Since the four signals we're handling
//! are very special, the async signal unsafety of Rust's TLS implementation generally does not affect the correctness here
//! unless you have memory unsafety elsewhere in your code.
//!
use std::any::Any;
use std::cell::Cell;
use wasmer_runtime_core::codegen::BreakpointMap;
use wasmer_runtime_core::fault::{begin_unsafe_unwind, catch_unsafe_unwind, ensure_sighandler};
use wasmer_runtime_core::typed_func::WasmTrapInfo;

thread_local! {
    pub static TRAP_EARLY_DATA: Cell<Option<Box<dyn Any + Send>>> = Cell::new(None);
}

pub unsafe fn trigger_trap() -> ! {
    begin_unsafe_unwind(Box::new(()));
}

pub enum CallProtError {
    Trap(WasmTrapInfo),
    Error(Box<dyn Any + Send>),
}

pub fn call_protected<T>(
    f: impl FnOnce() -> T,
    breakpoints: Option<BreakpointMap>,
) -> Result<T, CallProtError> {
    ensure_sighandler();
    unsafe {
        let ret = catch_unsafe_unwind(|| f(), breakpoints);
        match ret {
            Ok(x) => Ok(x),
            Err(e) => {
                if let Some(data) = TRAP_EARLY_DATA.with(|cell| cell.replace(None)) {
                    Err(CallProtError::Error(data))
                } else {
                    Err(CallProtError::Error(e))
                }
            }
        }
    }
}

pub unsafe fn throw(payload: Box<dyn Any + Send>) -> ! {
    begin_unsafe_unwind(payload);
}
