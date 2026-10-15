#[path = "wasm_tests/error.rs"]
mod error;

use anyhow::{Result, anyhow};
use wasmer_wasix::{WasiError, WasiRuntimeError, wasmer_wasix_types::wasi::ExitCode};

#[test]
fn extracts_exit_code_from_wasi_runtime_error_chain() -> Result<()> {
    let err = anyhow!(WasiRuntimeError::Wasi(WasiError::Exit(ExitCode::from(42))))
        .context("outer context");

    assert_eq!(error::exit_code_from_error(&err), Some(42));
    Ok(())
}

#[test]
fn extracts_exit_code_from_direct_wasi_error_chain() -> Result<()> {
    let err = ErrorWrapper::wrap(anyhow!(WasiError::Exit(ExitCode::from(17))));

    assert_eq!(error::exit_code_from_error(&err), Some(17));
    Ok(())
}

#[test]
fn does_not_treat_free_form_messages_as_exit_codes() -> Result<()> {
    let err = anyhow!("Spawn failed: ExitCode::1").context("outer context");

    assert_eq!(error::exit_code_from_error(&err), None);
    Ok(())
}

struct ErrorWrapper;

impl ErrorWrapper {
    fn wrap(err: anyhow::Error) -> anyhow::Error {
        err.context("wrapped")
    }
}
