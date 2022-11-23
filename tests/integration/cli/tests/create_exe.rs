//! Tests of the `wasmer create-exe` command.

use anyhow::{bail, Context};
use std::fs;
use std::io::prelude::*;
use std::path::PathBuf;
use std::process::Command;
use wasmer_integration_tests_cli::*;

fn create_exe_test_wasm_path() -> String {
    format!("{}/{}", C_ASSET_PATH, "qjs.wasm")
}
const JS_TEST_SRC_CODE: &[u8] =
    b"function greet(name) { return JSON.stringify('Hello, ' + name); }; print(greet('World'));\n";

/// Data used to run the `wasmer compile` command.
#[derive(Debug)]
struct WasmerCreateExe {
    /// The directory to operate in.
    current_dir: PathBuf,
    /// Path to wasmer executable used to run the command.
    wasmer_path: PathBuf,
    /// Path to the Wasm file to compile.
    wasm_path: PathBuf,
    /// Path to the native executable produced by compiling the Wasm.
    native_executable_path: PathBuf,
    /// Compiler with which to compile the Wasm.
    compiler: Compiler,
    /// Extra CLI flags
    extra_cli_flags: Vec<&'static str>,
}

impl Default for WasmerCreateExe {
    fn default() -> Self {
        #[cfg(not(windows))]
        let native_executable_path = PathBuf::from("wasm.out");
        #[cfg(windows)]
        let native_executable_path = PathBuf::from("wasm.exe");
        Self {
            current_dir: std::env::current_dir().unwrap(),
            wasmer_path: get_wasmer_path(),
            wasm_path: PathBuf::from(create_exe_test_wasm_path()),
            native_executable_path,
            compiler: Compiler::Cranelift,
            extra_cli_flags: vec![],
        }
    }
}

impl WasmerCreateExe {
    fn run(&self) -> anyhow::Result<Vec<u8>> {
        let output = Command::new(&self.wasmer_path)
            .current_dir(&self.current_dir)
            .arg("create-exe")
            .arg(&self.wasm_path.canonicalize()?)
            .arg(&self.compiler.to_flag())
            .args(self.extra_cli_flags.iter())
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
        Ok(output.stdout)
    }
}

/// Data used to run the `wasmer compile` command.
#[derive(Debug)]
struct WasmerCreateObj {
    /// The directory to operate in.
    current_dir: PathBuf,
    /// Path to wasmer executable used to run the command.
    wasmer_path: PathBuf,
    /// Path to the Wasm file to compile.
    wasm_path: PathBuf,
    /// Path to the object file produced by compiling the Wasm.
    output_object_path: PathBuf,
    /// Compiler with which to compile the Wasm.
    compiler: Compiler,
    /// Extra CLI flags
    extra_cli_flags: Vec<&'static str>,
}

impl Default for WasmerCreateObj {
    fn default() -> Self {
        #[cfg(not(windows))]
        let output_object_path = PathBuf::from("wasm.o");
        #[cfg(windows)]
        let output_object_path = PathBuf::from("wasm.obj");
        Self {
            current_dir: std::env::current_dir().unwrap(),
            wasmer_path: get_wasmer_path(),
            wasm_path: PathBuf::from(create_exe_test_wasm_path()),
            output_object_path,
            compiler: Compiler::Cranelift,
            extra_cli_flags: vec![],
        }
    }
}

impl WasmerCreateObj {
    fn run(&self) -> anyhow::Result<Vec<u8>> {
        let output = Command::new(&self.wasmer_path)
            .current_dir(&self.current_dir)
            .arg("create-obj")
            .arg(&self.wasm_path.canonicalize()?)
            .arg(&self.compiler.to_flag())
            .args(self.extra_cli_flags.iter())
            .arg("-o")
            .arg(&self.output_object_path)
            .output()?;

        if !output.status.success() {
            bail!(
                "wasmer create-obj failed with: stdout: {}\n\nstderr: {}",
                std::str::from_utf8(&output.stdout)
                    .expect("stdout is not utf8! need to handle arbitrary bytes"),
                std::str::from_utf8(&output.stderr)
                    .expect("stderr is not utf8! need to handle arbitrary bytes")
            );
        }
        Ok(output.stdout)
    }
}

#[test]
fn create_exe_works() -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let operating_dir: PathBuf = temp_dir.path().to_owned();

    let wasm_path = operating_dir.join(create_exe_test_wasm_path());
    #[cfg(not(windows))]
    let executable_path = operating_dir.join("wasm.out");
    #[cfg(windows)]
    let executable_path = operating_dir.join("wasm.exe");

    WasmerCreateExe {
        current_dir: operating_dir.clone(),
        wasm_path,
        native_executable_path: executable_path.clone(),
        compiler: Compiler::Cranelift,
        ..Default::default()
    }
    .run()
    .context("Failed to create-exe wasm with Wasmer")?;

    let result = run_code(
        &operating_dir,
        &executable_path,
        &["--eval".to_string(), "function greet(name) { return JSON.stringify('Hello, ' + name); }; print(greet('World'));".to_string()],
    )
    .context("Failed to run generated executable")?;
    let result_lines = result.lines().collect::<Vec<&str>>();
    assert_eq!(result_lines, vec!["\"Hello, World\""],);

    Ok(())
}

#[test]
fn create_exe_works_with_file() -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let operating_dir: PathBuf = temp_dir.path().to_owned();

    let wasm_path = operating_dir.join(create_exe_test_wasm_path());
    #[cfg(not(windows))]
    let executable_path = operating_dir.join("wasm.out");
    #[cfg(windows)]
    let executable_path = operating_dir.join("wasm.exe");

    WasmerCreateExe {
        current_dir: operating_dir.clone(),
        wasm_path,
        native_executable_path: executable_path.clone(),
        compiler: Compiler::Cranelift,
        ..Default::default()
    }
    .run()
    .context("Failed to create-exe wasm with Wasmer")?;

    {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(operating_dir.join("test.js"))?;
        f.write_all(JS_TEST_SRC_CODE)?;
    }

    // test with `--dir`
    let result = run_code(
        &operating_dir,
        &executable_path,
        &[
            "--dir=.".to_string(),
            "--script".to_string(),
            "test.js".to_string(),
        ],
    )
    .context("Failed to run generated executable")?;
    let result_lines = result.lines().collect::<Vec<&str>>();
    assert_eq!(result_lines, vec!["\"Hello, World\""],);

    // test with `--mapdir`
    let result = run_code(
        &operating_dir,
        &executable_path,
        &[
            "--mapdir=abc:.".to_string(),
            "--script".to_string(),
            "abc/test.js".to_string(),
        ],
    )
    .context("Failed to run generated executable")?;
    let result_lines = result.lines().collect::<Vec<&str>>();
    assert_eq!(result_lines, vec!["\"Hello, World\""],);

    Ok(())
}

#[test]
fn create_exe_serialized_works() -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let operating_dir: PathBuf = temp_dir.path().to_owned();

    let wasm_path = operating_dir.join(create_exe_test_wasm_path());
    #[cfg(not(windows))]
    let executable_path = operating_dir.join("wasm.out");
    #[cfg(windows)]
    let executable_path = operating_dir.join("wasm.exe");

    let output: Vec<u8> = WasmerCreateExe {
        current_dir: operating_dir.clone(),
        wasm_path,
        native_executable_path: executable_path.clone(),
        compiler: Compiler::Cranelift,
        extra_cli_flags: vec!["--object-format", "serialized"],
        ..Default::default()
    }
    .run()
    .context("Failed to create-exe wasm with Wasmer")?;

    let result = run_code(
        &operating_dir,
        &executable_path,
        &["--eval".to_string(), "function greet(name) { return JSON.stringify('Hello, ' + name); }; print(greet('World'));".to_string()],
    )
    .context("Failed to run generated executable")?;
    let result_lines = result.lines().collect::<Vec<&str>>();
    assert_eq!(result_lines, vec!["\"Hello, World\""],);

    let output_str = String::from_utf8_lossy(&output);
    assert!(
        output_str.contains("Serialized"),
        "create-exe output doesn't mention `serialized` format keyword:\n{}",
        output_str
    );

    Ok(())
}

fn create_obj(args: Vec<&'static str>, keyword_needle: &str, keyword: &str) -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let operating_dir: PathBuf = temp_dir.path().to_owned();

    let wasm_path = operating_dir.join(create_exe_test_wasm_path());

    #[cfg(not(windows))]
    let object_path = operating_dir.join("wasm.o");
    #[cfg(windows)]
    let object_path = operating_dir.join("wasm.obj");

    let output: Vec<u8> = WasmerCreateObj {
        current_dir: operating_dir.clone(),
        wasm_path,
        output_object_path: object_path.clone(),
        compiler: Compiler::Cranelift,
        extra_cli_flags: args,
        ..Default::default()
    }
    .run()
    .context("Failed to create-obj wasm with Wasmer")?;

    assert!(
        object_path.exists(),
        "create-obj successfully completed but object output file `{}` missing",
        object_path.display()
    );
    let mut object_header_path = object_path.clone();
    object_header_path.set_extension("h");
    assert!(
        object_header_path.exists(),
        "create-obj successfully completed but object output header file `{}` missing",
        object_header_path.display()
    );

    let output_str = String::from_utf8_lossy(&output);
    assert!(
        output_str.contains(keyword_needle),
        "create-obj output doesn't mention `{}` format keyword:\n{}",
        keyword,
        output_str
    );

    Ok(())
}

#[test]
fn create_obj_default() -> anyhow::Result<()> {
    create_obj(vec![], "Symbols", "symbols")
}

#[test]
fn create_obj_symbols() -> anyhow::Result<()> {
    create_obj(vec!["--object-format", "symbols"], "Symbols", "symbols")
}

#[test]
fn create_obj_serialized() -> anyhow::Result<()> {
    create_obj(
        vec!["--object-format", "serialized"],
        "Serialized",
        "serialized",
    )
}

fn create_exe_with_object_input(args: Vec<&'static str>) -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let operating_dir: PathBuf = temp_dir.path().to_owned();

    let wasm_path = operating_dir.join(create_exe_test_wasm_path());

    #[cfg(not(windows))]
    let object_path = operating_dir.join("wasm.o");
    #[cfg(windows)]
    let object_path = operating_dir.join("wasm.obj");

    WasmerCreateObj {
        current_dir: operating_dir.clone(),
        wasm_path,
        output_object_path: object_path.clone(),
        compiler: Compiler::Cranelift,
        extra_cli_flags: args,
        ..Default::default()
    }
    .run()
    .context("Failed to create-obj wasm with Wasmer")?;

    assert!(
        object_path.exists(),
        "create-obj successfully completed but object output file `{}` missing",
        object_path.display()
    );
    let mut object_header_path = object_path.clone();
    object_header_path.set_extension("h");
    assert!(
        object_header_path.exists(),
        "create-obj successfully completed but object output header file `{}` missing",
        object_header_path.display()
    );

    #[cfg(not(windows))]
    let executable_path = operating_dir.join("wasm.out");
    #[cfg(windows)]
    let executable_path = operating_dir.join("wasm.exe");

    WasmerCreateExe {
        current_dir: operating_dir.clone(),
        wasm_path: object_path,
        native_executable_path: executable_path.clone(),
        compiler: Compiler::Cranelift,
        extra_cli_flags: vec!["--header", "wasm.h"],
        ..Default::default()
    }
    .run()
    .context("Failed to create-exe wasm with Wasmer")?;

    let result = run_code(
        &operating_dir,
        &executable_path,
        &["--eval".to_string(), "function greet(name) { return JSON.stringify('Hello, ' + name); }; print(greet('World'));".to_string()],
    )
    .context("Failed to run generated executable")?;
    let result_lines = result.lines().collect::<Vec<&str>>();
    assert_eq!(result_lines, vec!["\"Hello, World\""],);

    Ok(())
}

#[test]
fn create_exe_with_object_input_default() -> anyhow::Result<()> {
    create_exe_with_object_input(vec![])
}

#[test]
fn create_exe_with_object_input_symbols() -> anyhow::Result<()> {
    create_exe_with_object_input(vec!["--object-format", "symbols"])
}

#[test]
fn create_exe_with_object_input_serialized() -> anyhow::Result<()> {
    create_exe_with_object_input(vec!["--object-format", "serialized"])
}
