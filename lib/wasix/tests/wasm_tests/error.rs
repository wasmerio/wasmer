use anyhow::Error;
use wasmer_wasix::{WasiError, WasiRuntimeError};

pub(crate) fn exit_code_from_error(err: &Error) -> Option<i32> {
    err.chain()
        .find_map(|cause| {
            cause
                .downcast_ref::<WasiRuntimeError>()
                .and_then(WasiRuntimeError::as_exit_code)
                .or_else(|| match cause.downcast_ref::<WasiError>() {
                    Some(WasiError::Exit(code)) => Some(*code),
                    _ => None,
                })
        })
        .map(|code| code.raw())
}
