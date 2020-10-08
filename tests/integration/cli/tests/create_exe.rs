//! Tests of the `wasmer create-exe` command.

use anyhow::{bail, Context};
use std::path::PathBuf;
use std::process::Command;
use wasmer_integration_tests_cli::*;

fn create_exe_test_wasm_path() -> String {
    format!("{}/{}", ASSET_PATH, "qjs.wasm")
}

/// Data used to run the `wasmer compile` command.
#[derive(Debug)]
struct WasmerCreateExe {
    /// Path to wasmer executable used to run the command.
    wasmer_path: PathBuf,
    /// Path to the Wasm file to compile.
    wasm_path: PathBuf,
    /// Path to the native executable produced by compiling the Wasm.
    native_executable_path: PathBuf,
    /// Compiler with which to compile the Wasm.
    compiler: Compiler,
}

impl Default for WasmerCreateExe {
    fn default() -> Self {
        #[cfg(not(windows))]
        let native_executable_path = PathBuf::from("wasm.out");
        #[cfg(windows)]
        let native_executable_path = PathBuf::from("wasm.exe");
        Self {
            wasmer_path: get_wasmer_path(),
            wasm_path: PathBuf::from(create_exe_test_wasm_path()),
            native_executable_path,
            compiler: Compiler::Cranelift,
        }
    }
}

impl WasmerCreateExe {
    fn run(&self) -> anyhow::Result<()> {
        let output = Command::new(&self.wasmer_path)
            .arg("create-exe")
            .arg(&self.wasm_path.canonicalize()?)
            .arg(&self.compiler.to_flag())
            // TODO: remove before shipping
            .arg("-lffi")
            .arg("-o")
            .arg(&self.native_executable_path)
            .output()?;

        if !output.status.success() {
            bail!(
                "wasmer create-exe failed with: stdout: {}\n\nstderr: {}",
                std::str::from_utf8(&output.stdout)
                    .expect("stdout is not utf8! need to handle arbitrary bytes"),
                std::str::from_utf8(&output.stderr)
                    .expect("stderr is not utf8! need to handle arbitrary bytes")
            );
        }
        Ok(())
    }
}

#[test]
fn create_exe_works() -> anyhow::Result<()> {
    let operating_dir = tempfile::tempdir()?;

    std::env::set_current_dir(&operating_dir)?;

    let wasm_path = PathBuf::from(create_exe_test_wasm_path());
    #[cfg(not(windows))]
    let executable_path = PathBuf::from("wasm.out");
    #[cfg(windows)]
    let executable_path = PathBuf::from("wasm.exe");

    WasmerCreateExe {
        wasm_path: wasm_path.clone(),
        native_executable_path: executable_path.clone(),
        compiler: Compiler::Cranelift,
        ..Default::default()
    }
    .run()
    .context("Failed to create-exe wasm with Wasmer")?;

    let result = run_code(&executable_path).context("Failed to run generated executable")?;
    let result_lines = result.lines().collect::<Vec<&str>>();
    assert_eq!(result_lines, vec!["Initializing...", "\"Hello, World\""],);

    Ok(())
}
