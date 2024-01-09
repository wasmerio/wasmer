//! Tests of the `wasmer create-exe` command.

use std::{
    fs,
    io::prelude::*,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{bail, Context};
use assert_cmd::prelude::OutputAssertExt;
use tempfile::TempDir;
use wasmer_integration_tests_cli::*;

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
            wasm_path: PathBuf::from(fixtures::qjs()),
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
        output.arg(self.compiler.to_flag());
        output.args(self.extra_cli_flags.iter());
        output.arg("-o");
        output.arg(&self.native_executable_path);
        if !self.extra_cli_flags.contains(&"--target".to_string()) {
            let tarball_path = get_repo_root_path().unwrap().join("link.tar.gz");
            assert!(tarball_path.exists(), "link.tar.gz does not exist");
            output.arg("--tarball");
            output.arg(&tarball_path);
        }
        let cmd = format!("{:?}", output);

        println!("(integration-test) running create-exe: {cmd}");

        let output = output.output()?;

        let stdout = std::str::from_utf8(&output.stdout)
            .expect("stdout is not utf8! need to handle arbitrary bytes");

        assert!(
            stdout.contains("headless."),
            "create-exe stdout should link with libwasmer-headless"
        );

        if !output.status.success() {
            bail!(
                "{cmd}\r\n failed with: stdout: {stdout}\n\nstderr: {}",
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
            wasm_path: PathBuf::from(fixtures::qjs()),
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
        output.arg(self.compiler.to_flag());
        output.args(self.extra_cli_flags.iter());
        output.arg("-o");
        output.arg(&self.output_object_path);

        let cmd = format!("{:?}", output);

        println!("(integration-test) running create-obj: {cmd}");

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
#[ignore = "See https://github.com/wasmerio/wasmer/issues/4285"]
fn test_create_exe_with_pirita_works_1() {
    let tempdir = TempDir::new().unwrap();
    let path = tempdir.path();
    let wasm_out = path.join("out.obj");
    let cmd = Command::new(get_wasmer_path())
        .arg("create-obj")
        .arg(fixtures::wabt())
        .arg("-o")
        .arg(&wasm_out)
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&cmd.stderr);

    assert_eq!(stderr.lines().map(|s| s.trim().to_string()).collect::<Vec<_>>(), vec![
        format!("error: cannot compile more than one atom at a time"),
        format!("│   1: note: use --atom <ATOM> to specify which atom to compile"),
        format!("╰─▶ 2: where <ATOM> is one of: wabt, wasm-interp, wasm-strip, wasm-validate, wasm2wat, wast2json, wat2wasm"),
    ]);

    assert!(!cmd.status.success());

    let cmd = Command::new(get_wasmer_path())
        .arg("create-obj")
        .arg(fixtures::wabt())
        .arg("--atom")
        .arg("wasm2wat")
        .arg("-o")
        .arg(&wasm_out)
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&cmd.stderr);

    let real_out = wasm_out.canonicalize().unwrap().display().to_string();
    let real_out = real_out
        .strip_prefix(r"\\?\")
        .unwrap_or(&real_out)
        .to_string();
    assert_eq!(
        stderr
            .lines()
            .map(|s| s.trim().to_string())
            .collect::<Vec<_>>(),
        vec![format!("✔ Object compiled successfully to `{real_out}`"),]
    );

    assert!(cmd.status.success());
}

#[test]
#[ignore = "See https://github.com/wasmerio/wasmer/issues/4285"]
fn test_create_exe_with_precompiled_works_1() {
    use object::{Object, ObjectSymbol};

    let tempdir = TempDir::new().unwrap();
    let path = tempdir.path();
    let wasm_out = path.join("out.obj");
    let _ = Command::new(get_wasmer_path())
        .arg("create-obj")
        .arg(fixtures::qjs())
        .arg("--prefix")
        .arg("sha123123")
        .arg("-o")
        .arg(&wasm_out)
        .output()
        .unwrap();

    let file = std::fs::read(&wasm_out).unwrap();
    let obj_file = object::File::parse(&*file).unwrap();
    let names = obj_file
        .symbols()
        .filter_map(|s| Some(s.name().ok()?.to_string()))
        .collect::<Vec<_>>();

    assert!(
        names.contains(&"_wasmer_function_sha123123_1".to_string())
            || names.contains(&"wasmer_function_sha123123_1".to_string())
    );

    let _ = Command::new(get_wasmer_path())
        .arg("create-obj")
        .arg(fixtures::qjs())
        .arg("-o")
        .arg(&wasm_out)
        .output()
        .unwrap();

    let file = std::fs::read(&wasm_out).unwrap();
    let obj_file = object::File::parse(&*file).unwrap();
    let names = obj_file
        .symbols()
        .filter_map(|s| Some(s.name().ok()?.to_string()))
        .collect::<Vec<_>>();

    assert!(
        names.contains(
            &"_wasmer_function_6f62a6bc5c8f8e3e12a54e2ecbc5674ccfe1c75f91d8e4dd6ebb3fec422a4d6c_1"
                .to_string()
        ) || names.contains(
            &"wasmer_function_6f62a6bc5c8f8e3e12a54e2ecbc5674ccfe1c75f91d8e4dd6ebb3fec422a4d6c_1"
                .to_string()
        )
    );
}

// Ignored because of -lunwind linker issue on Windows
// see https://github.com/wasmerio/wasmer/issues/3459
// Also ignored on macOS because it's flaky
#[cfg_attr(any(target_os = "windows", target_os = "macos"), ignore)]
#[test]
#[ignore = "See https://github.com/wasmerio/wasmer/issues/4285"]
fn create_exe_works() -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let operating_dir: PathBuf = temp_dir.path().to_owned();

    let wasm_path = operating_dir.join(fixtures::qjs());
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
        false,
    )
    .context("Failed to run generated executable")?;
    let result_lines = result.lines().collect::<Vec<&str>>();
    assert_eq!(result_lines, vec!["\"Hello, World\""],);

    Ok(())
}

/// Tests that "-c" and "-- -c" are treated differently
// Ignored because of -lunwind linker issue on Windows
// see https://github.com/wasmerio/wasmer/issues/3459
#[cfg_attr(target_os = "windows", ignore)]
#[test]
// FIXME: Fix an re-enable test
// See https://github.com/wasmerio/wasmer/issues/3615
#[allow(dead_code)]
#[ignore = "See https://github.com/wasmerio/wasmer/issues/4285"]
fn create_exe_works_multi_command_args_handling() -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let operating_dir: PathBuf = temp_dir.path().to_owned();

    let wasm_path = operating_dir.join(fixtures::wabt());
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
            "wasm-strip".to_string(),
            "--".to_string(),
            "-c".to_string(),
        ],
        true,
    )
    .context("Failed to run generated executable")?;
    let result_lines = result.lines().collect::<Vec<&str>>();
    assert_eq!(
        result_lines,
        vec![
            "wasm-strip: unknown option '-c'",
            "Try '--help' for more information.",
            "WASI exited with code: 1"
        ]
    );

    let result = run_code(
        &operating_dir,
        &executable_path,
        &["-c".to_string(), "wasm-strip".to_string()],
        true,
    )
    .context("Failed to run generated executable")?;
    let result_lines = result.lines().collect::<Vec<&str>>();
    assert_eq!(
        result_lines,
        vec![
            "wasm-strip: expected filename argument.",
            "Try '--help' for more information.",
            "WASI exited with code: 1"
        ]
    );

    Ok(())
}

/// Tests that create-exe works with underscores and dashes in command names
// Ignored because of -lunwind linker issue on Windows
// see https://github.com/wasmerio/wasmer/issues/3459
#[cfg_attr(target_os = "windows", ignore)]
#[test]
#[ignore = "See https://github.com/wasmerio/wasmer/issues/4285"]
fn create_exe_works_underscore_module_name() -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let operating_dir: PathBuf = temp_dir.path().to_owned();
    let wasm_path = operating_dir.join(fixtures::wabt());

    let atoms = &[
        "wabt",
        "wasm-interp",
        "wasm-strip",
        "wasm-validate",
        "wasm2wat",
        "wast2json",
        "wat2wasm",
    ];

    let mut create_exe_flags = Vec::new();

    for a in atoms.iter() {
        let object_path = operating_dir.as_path().join(&format!("{a}.o"));
        let _output: Vec<u8> = WasmerCreateObj {
            current_dir: operating_dir.clone(),
            wasm_path: wasm_path.clone(),
            output_object_path: object_path.clone(),
            compiler: Compiler::Cranelift,
            extra_cli_flags: vec!["--atom".to_string(), a.to_string()],
            ..Default::default()
        }
        .run()
        .context("Failed to create-obj wasm with Wasmer")?;

        assert!(
            object_path.exists(),
            "create-obj successfully completed but object output file `{}` missing",
            object_path.display()
        );

        create_exe_flags.push("--precompiled-atom".to_string());
        create_exe_flags.push(format!(
            "{a}:{}",
            object_path.canonicalize().unwrap().display()
        ));
    }

    #[cfg(not(windows))]
    let executable_path = operating_dir.join("multicommand.out");
    #[cfg(windows)]
    let executable_path = operating_dir.join("multicommand.exe");

    WasmerCreateExe {
        current_dir: operating_dir,
        wasm_path,
        native_executable_path: executable_path,
        compiler: Compiler::Cranelift,
        extra_cli_flags: create_exe_flags,
        ..Default::default()
    }
    .run()
    .context("Failed to create-exe wasm with Wasmer")?;

    Ok(())
}

// Ignored because of -lunwind linker issue on Windows
// see https://github.com/wasmerio/wasmer/issues/3459
#[cfg_attr(target_os = "windows", ignore)]
#[test]
#[ignore = "See https://github.com/wasmerio/wasmer/issues/4285"]
fn create_exe_works_multi_command() -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let operating_dir: PathBuf = temp_dir.path().to_owned();

    let wasm_path = operating_dir.join(fixtures::wabt());
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
        false,
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
        false,
    )
    .context("Failed to run generated executable")?;

    let result_lines = result.lines().collect::<Vec<&str>>();
    assert_eq!(result_lines, vec!["1.0.37 (git~v1.0.37)"]);

    Ok(())
}

// Ignored because of -lunwind linker issue on Windows
// see https://github.com/wasmerio/wasmer/issues/3459
#[cfg_attr(target_os = "windows", ignore)]
#[test]
#[ignore = "See https://github.com/wasmerio/wasmer/issues/4285"]
fn create_exe_works_with_file() -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let operating_dir: PathBuf = temp_dir.path().to_owned();

    let wasm_path = operating_dir.join(fixtures::qjs());
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
        false,
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
        false,
    )
    .context("Failed to run generated executable")?;
    let result_lines = result.lines().collect::<Vec<&str>>();
    assert_eq!(result_lines, vec!["\"Hello, World\""],);

    Ok(())
}

fn create_obj(args: Vec<String>) -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let operating_dir: PathBuf = temp_dir.path().to_owned();

    let wasm_path = operating_dir.as_path().join(fixtures::qjs());

    let object_path = operating_dir.as_path().join("wasm");
    let _output: Vec<u8> = WasmerCreateObj {
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

    Ok(())
}

#[test]
#[ignore = "See https://github.com/wasmerio/wasmer/issues/4285"]
fn create_obj_default() -> anyhow::Result<()> {
    create_obj(vec![])
}

fn create_exe_with_object_input(args: Vec<String>) -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let operating_dir: PathBuf = temp_dir.path().to_owned();

    let wasm_path = operating_dir.join(fixtures::qjs());

    #[cfg(not(windows))]
    let object_path = operating_dir.join("wasm.o");
    #[cfg(windows)]
    let object_path = operating_dir.join("wasm.obj");

    let mut create_obj_args = args.clone();
    create_obj_args.push("--prefix".to_string());
    create_obj_args.push("abc123".to_string());
    create_obj_args.push("--debug-dir".to_string());
    create_obj_args.push(format!(
        "{}",
        operating_dir.join("compile-create-obj").display()
    ));

    WasmerCreateObj {
        current_dir: operating_dir.clone(),
        wasm_path: wasm_path.clone(),
        output_object_path: object_path.clone(),
        compiler: Compiler::Cranelift,
        extra_cli_flags: create_obj_args,
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

    let mut create_exe_args = args;
    create_exe_args.push("--precompiled-atom".to_string());
    create_exe_args.push(format!("qjs:abc123:{}", object_path.display()));
    create_exe_args.push("--debug-dir".to_string());
    create_exe_args.push(format!(
        "{}",
        operating_dir.join("compile-create-exe").display()
    ));

    let create_exe_stdout = WasmerCreateExe {
        current_dir: std::env::current_dir().unwrap(),
        wasm_path,
        native_executable_path: executable_path.clone(),
        compiler: Compiler::Cranelift,
        extra_cli_flags: create_exe_args,
        ..Default::default()
    }
    .run()
    .context("Failed to create-exe wasm with Wasmer")?;

    let create_exe_stdout = std::str::from_utf8(&create_exe_stdout).unwrap();
    assert!(
        create_exe_stdout.contains("Using cached object file for atom \"qjs\"."),
        "missed cache hit"
    );

    let result = run_code(
        &operating_dir,
        &executable_path,
        &["--eval".to_string(), "function greet(name) { return JSON.stringify('Hello, ' + name); }; print(greet('World'));".to_string()],
        false,
    )
    .context("Failed to run generated executable")?;
    let result_lines = result.lines().collect::<Vec<&str>>();
    assert_eq!(result_lines, vec!["\"Hello, World\""],);

    Ok(())
}

// Ignored because of -lunwind linker issue on Windows
// see https://github.com/wasmerio/wasmer/issues/3459
#[cfg_attr(target_os = "windows", ignore)]
#[test]
#[ignore = "See https://github.com/wasmerio/wasmer/issues/4285"]
fn create_exe_with_object_input_default() -> anyhow::Result<()> {
    create_exe_with_object_input(vec![])
}

/// TODO: on linux-musl, the packaging of libwasmer.a doesn't work properly
/// Tracked in https://github.com/wasmerio/wasmer/issues/3271
#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
#[ignore = "See https://github.com/wasmerio/wasmer/issues/4285"]
fn test_wasmer_create_exe_pirita_works() {
    // let temp_dir = Path::new("debug");
    // std::fs::create_dir_all(&temp_dir);

    use wasmer_integration_tests_cli::get_repo_root_path;
    let temp_dir = TempDir::new().unwrap();
    let temp_dir = temp_dir.path().to_path_buf();
    let python_wasmer_path = temp_dir.join("python.wasmer");
    std::fs::copy(fixtures::python(), &python_wasmer_path).unwrap();
    let python_exe_output_path = temp_dir.join("python");

    let native_target = target_lexicon::HOST;
    let tmp_targz_path = get_repo_root_path().unwrap().join("link.tar.gz");

    println!("compiling to target {native_target}");

    let mut cmd = Command::new(get_wasmer_path());
    cmd.arg("create-exe");
    cmd.arg(&python_wasmer_path);
    cmd.arg("--tarball");
    cmd.arg(&tmp_targz_path);
    cmd.arg("--target");
    cmd.arg(format!("{native_target}"));
    cmd.arg("-o");
    cmd.arg(&python_exe_output_path);
    // change temp_dir to a local path and run this test again
    // to output the compilation files into a debug folder
    //
    // cmd.arg("--debug-dir");
    // cmd.arg(&temp_dir);

    cmd.assert().success();

    println!("compilation ok!");

    if !python_exe_output_path.exists() {
        panic!(
            "python_exe_output_path {} does not exist",
            python_exe_output_path.display()
        );
    }

    println!("invoking command...");

    let mut command = Command::new(&python_exe_output_path);
    command.arg("-c");
    command.arg("print(\"hello\")");

    command.assert().success().stdout("hello\n");
}

// FIXME: Fix and re-enable this test
// See https://github.com/wasmerio/wasmer/issues/3615
#[test]
#[ignore]
fn test_cross_compile_python_windows() {
    let temp_dir = TempDir::new().unwrap();

    let targets: &[&str] = if cfg!(windows) {
        &[
            "aarch64-darwin",
            "x86_64-darwin",
            "x86_64-linux-gnu",
            "aarch64-linux-gnu",
        ]
    } else {
        &[
            "aarch64-darwin",
            "x86_64-darwin",
            "x86_64-linux-gnu",
            "aarch64-linux-gnu",
            "x86_64-windows-gnu",
        ]
    };

    let compilers: &[&str] = if cfg!(target_env = "musl") {
        // MUSL has no support for LLVM in C-API
        &["cranelift", "singlepass"]
    } else {
        &["cranelift", "singlepass", "llvm"]
    };

    // llvm-objdump  --disassemble-all --demangle ./objects/wasmer_vm-50cb118b098c15db.wasmer_vm.60425a0a-cgu.12.rcgu.o
    // llvm-objdump --macho --exports-trie ~/.wasmer/cache/wasmer-darwin-arm64/lib/libwasmer.dylib
    let excluded_combinations = &[
        ("aarch64-darwin", "llvm"), // LLVM: aarch64 not supported relocation Arm64MovwG0 not supported
        ("aarch64-linux-gnu", "llvm"), // LLVM: aarch64 not supported relocation Arm64MovwG0 not supported
        // https://github.com/ziglang/zig/issues/13729
        ("x86_64-darwin", "llvm"), // undefined reference to symbol 'wasmer_vm_raise_trap' kind Unknown
        ("x86_64-windows-gnu", "llvm"), // unimplemented symbol `wasmer_vm_raise_trap` kind Unknown
    ];

    for t in targets {
        for c in compilers {
            if excluded_combinations.contains(&(t, c)) {
                continue;
            }
            println!("{t} target {c}");
            let python_wasmer_path = temp_dir.path().join(format!("{t}-python"));

            let tarball = match std::env::var("GITHUB_TOKEN") {
                Ok(_) => Some(assert_tarball_is_present_local(t).unwrap()),
                Err(_) => None,
            };
            let mut cmd = Command::new(get_wasmer_path());

            cmd.arg("create-exe");
            cmd.arg(fixtures::python());
            cmd.arg("--target");
            cmd.arg(t);
            cmd.arg("-o");
            cmd.arg(python_wasmer_path.clone());
            cmd.arg(format!("--{c}"));
            if std::env::var("GITHUB_TOKEN").is_ok() {
                cmd.arg("--debug-dir");
                cmd.arg(format!("{t}-{c}"));
            }

            if t.contains("x86_64") && *c == "singlepass" {
                cmd.arg("-m");
                cmd.arg("avx");
            }

            if let Some(t) = tarball {
                cmd.arg("--tarball");
                cmd.arg(t);
            }

            let assert = cmd.assert().success();

            if !python_wasmer_path.exists() {
                let p = std::fs::read_dir(temp_dir.path())
                    .unwrap()
                    .filter_map(|e| Some(e.ok()?.path()))
                    .collect::<Vec<_>>();
                let output = assert.get_output();
                panic!("target {t} was not compiled correctly tempdir: {p:#?}, {output:?}",);
            }
        }
    }
}

fn assert_tarball_is_present_local(target: &str) -> Result<PathBuf, anyhow::Error> {
    let wasmer_dir = std::env::var("WASMER_DIR").expect("no WASMER_DIR set");
    let directory = match target {
        "aarch64-darwin" => "wasmer-darwin-arm64.tar.gz",
        "x86_64-darwin" => "wasmer-darwin-amd64.tar.gz",
        "x86_64-linux-gnu" => "wasmer-linux-amd64.tar.gz",
        "aarch64-linux-gnu" => "wasmer-linux-aarch64.tar.gz",
        "x86_64-windows-gnu" => "wasmer-windows-gnu64.tar.gz",
        _ => return Err(anyhow::anyhow!("unknown target {target}")),
    };
    let libwasmer_cache_path = Path::new(&wasmer_dir).join("cache").join(directory);
    if !libwasmer_cache_path.exists() {
        return Err(anyhow::anyhow!(
            "targz {} does not exist",
            libwasmer_cache_path.display()
        ));
    }
    println!("using targz {}", libwasmer_cache_path.display());
    Ok(libwasmer_cache_path)
}
