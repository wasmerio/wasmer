use crate::signal::HandlerData;
use wasmer_runtime_core::error::RuntimeResult;

pub fn call_protected<T>(handler_data: &HandlerData, f: impl FnOnce() -> T) -> RuntimeResult<T> {
    unimplemented!("TODO");
}
