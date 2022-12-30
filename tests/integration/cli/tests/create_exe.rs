//! Tests of the `wasmer create-exe` command.

use anyhow::{bail, Context};
use std::fs;
use std::io::prelude::*;
use std::path::PathBuf;
use std::process::Command;
use wasmer_integration_tests_cli::*;

fn create_exe_wabt_path() -> String {
    format!("{}/{}", C_ASSET_PATH, "wabt-1.0.37.wasmer")
}

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
    extra_cli_flags: Vec<String>,
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
        let mut output = Command::new(&self.wasmer_path);
        output.current_dir(&self.current_dir);
        output.arg("create-exe");
        output.arg(&self.wasm_path.canonicalize()?);
        output.arg(&self.compiler.to_flag());
        output.args(self.extra_cli_flags.iter());
        output.arg("-o");
        output.arg(&self.native_executable_path);

        let cmd = format!("{:?}", output);

        let output = output.output()?;

        if !output.status.success() {
            bail!(
                "{cmd}\r\n failed with: stdout: {}\n\nstderr: {}",
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
    extra_cli_flags: Vec<String>,
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
        let mut output = Command::new(&self.wasmer_path);
        output.current_dir(&self.current_dir);
        output.arg("create-obj");
        output.arg(&self.wasm_path.canonicalize()?);
        output.arg(&self.compiler.to_flag());
        output.args(self.extra_cli_flags.iter());
        output.arg("-o");
        output.arg(&self.output_object_path);

        let cmd = format!("{:?}", output);

        let output = output.output()?;

        if !output.status.success() {
            bail!(
                "{cmd}\r\n failed with: stdout: {}\n\nstderr: {}",
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
fn create_exe_works_multi_command() -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let operating_dir: PathBuf = temp_dir.path().to_owned();

    let wasm_path = operating_dir.join(create_exe_wabt_path());
    #[cfg(not(windows))]
    let executable_path = operating_dir.join("multicommand.out");
    #[cfg(windows)]
    let executable_path = operating_dir.join("multicommand.exe");

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
        &[
            "--command".to_string(),
            "wasm2wat".to_string(),
            "--version".to_string(),
        ],
    )
    .context("Failed to run generated executable")?;

    let result_lines = result.lines().collect::<Vec<&str>>();
    assert_eq!(result_lines, vec!["1.0.37 (git~v1.0.37)"]);

    let result = run_code(
        &operating_dir,
        &executable_path,
        &[
            "-c".to_string(),
            "wasm-validate".to_string(),
            "--version".to_string(),
        ],
    )
    .context("Failed to run generated executable")?;

    let result_lines = result.lines().collect::<Vec<&str>>();
    assert_eq!(result_lines, vec!["1.0.37 (git~v1.0.37)"]);

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
        current_dir: std::env::current_dir().unwrap(),
        wasm_path,
        native_executable_path: executable_path.clone(),
        compiler: Compiler::Cranelift,
        extra_cli_flags: vec!["--object-format".to_string(), "serialized".to_string()],
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

fn create_obj(args: Vec<String>, keyword_needle: &str, keyword: &str) -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let operating_dir: PathBuf = temp_dir.path().to_owned();

    let wasm_path = operating_dir.as_path().join(create_exe_test_wasm_path());

    let object_path = operating_dir.as_path().join("wasm");
    let output: Vec<u8> = WasmerCreateObj {
        current_dir: operating_dir,
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
    create_obj(
        vec!["--object-format".to_string(), "symbols".to_string()],
        "Symbols",
        "symbols",
    )
}

#[test]
fn create_obj_serialized() -> anyhow::Result<()> {
    create_obj(
        vec!["--object-format".to_string(), "serialized".to_string()],
        "Serialized",
        "serialized",
    )
}

fn create_exe_with_object_input(mut args: Vec<String>) -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let operating_dir: PathBuf = temp_dir.path().to_owned();

    let wasm_path = operating_dir.join(create_exe_test_wasm_path());

    #[cfg(not(windows))]
    let object_path = operating_dir.join("wasm.o");
    #[cfg(windows)]
    let object_path = operating_dir.join("wasm.obj");

    args.push("--prefix".to_string());
    args.push("abc123".to_string());

    WasmerCreateObj {
        current_dir: operating_dir.clone(),
        wasm_path: wasm_path.clone(),
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

    #[cfg(not(windows))]
    let executable_path = operating_dir.join("wasm.out");
    #[cfg(windows)]
    let executable_path = operating_dir.join("wasm.exe");

    let create_exe_stdout = WasmerCreateExe {
        current_dir: std::env::current_dir().unwrap(),
        wasm_path,
        native_executable_path: executable_path.clone(),
        compiler: Compiler::Cranelift,
        extra_cli_flags: vec![
            "--precompiled-atom".to_string(),
            format!("qjs:abc123:{}", object_path.display()),
        ],
        ..Default::default()
    }
    .run()
    .context("Failed to create-exe wasm with Wasmer")?;

    let create_exe_stdout = std::str::from_utf8(&create_exe_stdout).unwrap();
    assert!(
        create_exe_stdout.contains("cache hit for atom \"qjs\""),
        "missed cache hit"
    );

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
    create_exe_with_object_input(vec!["--object-format".to_string(), "symbols".to_string()])
}

#[test]
fn create_exe_with_object_input_serialized() -> anyhow::Result<()> {
    create_exe_with_object_input(vec![
        "--object-format".to_string(),
        "serialized".to_string(),
    ])
}
