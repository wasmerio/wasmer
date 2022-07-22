//! Basic tests for the `run` subcommand

use anyhow::bail;
use std::process::Command;
use wasmer_integration_tests_cli::{ASSET_PATH, C_ASSET_PATH, WASMER_PATH};

fn wasi_test_wasm_path() -> String {
    format!("{}/{}", C_ASSET_PATH, "qjs.wasm")
}

fn test_no_imports_wat_path() -> String {
    format!("{}/{}", ASSET_PATH, "fib.wat")
}

fn test_no_start_wat_path() -> String {
    format!("{}/{}", ASSET_PATH, "no_start.wat")
}

#[test]
fn run_wasi_works() -> anyhow::Result<()> {
    let output = Command::new(WASMER_PATH)
        .arg("run")
        .arg(wasi_test_wasm_path())
        .arg("--")
        .arg("-e")
        .arg("print(3 * (4 + 5))")
        .output()?;

    if !output.status.success() {
        bail!(
            "linking failed with: stdout: {}\n\nstderr: {}",
            std::str::from_utf8(&output.stdout)
                .expect("stdout is not utf8! need to handle arbitrary bytes"),
            std::str::from_utf8(&output.stderr)
                .expect("stderr is not utf8! need to handle arbitrary bytes")
        );
    }

    let stdout_output = std::str::from_utf8(&output.stdout).unwrap();
    assert_eq!(stdout_output, "27\n");

    Ok(())
}

#[test]
fn run_no_imports_wasm_works() -> anyhow::Result<()> {
    let output = Command::new(WASMER_PATH)
        .arg("run")
        .arg(test_no_imports_wat_path())
        .output()?;

    if !output.status.success() {
        bail!(
            "linking failed with: stdout: {}\n\nstderr: {}",
            std::str::from_utf8(&output.stdout)
                .expect("stdout is not utf8! need to handle arbitrary bytes"),
            std::str::from_utf8(&output.stderr)
                .expect("stderr is not utf8! need to handle arbitrary bytes")
        );
    }

    Ok(())
}

// This test verifies that "wasmer run --invoke _start module.wat"
// works the same as "wasmer run module.wat" (without --invoke).
#[test]
fn run_invoke_works_with_nomain_wasi() -> anyhow::Result<()> {
    // In this example the function "wasi_unstable.arg_sizes_get"
    // is a function that is imported from the WASI env.
    let wasi_wat = "
    (module
        (import \"wasi_unstable\" \"args_sizes_get\"
          (func $__wasi_args_sizes_get (param i32 i32) (result i32)))
        (func $_start)
        (memory 1)
        (export \"memory\" (memory 0))
        (export \"_start\" (func $_start))
      )
    ";

    let random = rand::random::<u64>();
    let module_file = std::env::temp_dir().join(&format!("{random}.wat"));
    std::fs::write(&module_file, wasi_wat.as_bytes()).unwrap();
    let output = Command::new(WASMER_PATH)
        .arg("run")
        .arg(&module_file)
        .output()?;

    let stderr = std::str::from_utf8(&output.stderr).unwrap().to_string();
    let success = output.status.success();
    if !success {
        println!("ERROR in 'wasmer run [module.wat]':\r\n{stderr}");
        panic!();
    }

    let output = Command::new(WASMER_PATH)
        .arg("run")
        .arg("--invoke")
        .arg("_start")
        .arg(&module_file)
        .output()?;

    let stderr = std::str::from_utf8(&output.stderr).unwrap().to_string();
    let success = output.status.success();
    if !success {
        println!("ERROR in 'wasmer run --invoke _start [module.wat]':\r\n{stderr}");
        panic!();
    }

    std::fs::remove_file(&module_file).unwrap();
    Ok(())
}

#[test]
fn run_no_start_wasm_report_error() -> anyhow::Result<()> {
    let output = Command::new(WASMER_PATH)
        .arg("run")
        .arg(test_no_start_wat_path())
        .output()?;

    assert_eq!(output.status.success(), false);
    let result = std::str::from_utf8(&output.stderr).unwrap().to_string();
    assert_eq!(result.contains("Can not find any export functions."), true);
    Ok(())
}
