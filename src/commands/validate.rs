use crate::common::PrestandardFeatures;
use crate::utils::{is_wasm_binary, read_file_contents};
use std::path::PathBuf;
use std::process::exit;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
pub struct Validate {
    /// Input file
    #[structopt(parse(from_os_str))]
    path: PathBuf,

    #[structopt(flatten)]
    features: PrestandardFeatures,
}

impl Validate {
    /// Runs logic for the `validate` subcommand
    pub fn execute(self) {
        match validate_wasm(self) {
            Err(message) => {
                eprintln!("Error: {}", message);
                exit(-1);
            }
            _ => (),
        }
    }
}

fn validate_wasm(validate: Validate) -> Result<(), String> {
    let wasm_path = validate.path;
    let wasm_path_as_str = wasm_path.to_str().unwrap();

    let wasm_binary: Vec<u8> = read_file_contents(&wasm_path).map_err(|err| {
        format!(
            "Can't read the file {}: {}",
            wasm_path.as_os_str().to_string_lossy(),
            err
        )
    })?;

    if !is_wasm_binary(&wasm_binary) {
        return Err(format!(
            "Cannot recognize \"{}\" as a WASM binary",
            wasm_path_as_str,
        ));
    }

    wasmer_runtime_core::validate_and_report_errors_with_features(
        &wasm_binary,
        validate.features.into_backend_features(),
    )
    .map_err(|err| format!("Validation failed: {}", err))?;

    Ok(())
}
