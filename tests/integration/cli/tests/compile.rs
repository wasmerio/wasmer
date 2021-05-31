//! CLI tests for the compile subcommand.

use anyhow::{bail, Context};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use wasmer_integration_tests_cli::link_code::*;
use wasmer_integration_tests_cli::*;

const STATICLIB_ENGINE_TEST_C_SOURCE: &[u8] = include_bytes!("staticlib_engine_test_c_source.c");

fn staticlib_engine_test_wasm_path() -> String {
    format!("{}/{}", C_ASSET_PATH, "qjs.wasm")
}

/// Data used to run the `wasmer compile` command.
#[derive(Debug)]
struct WasmerCompile {
    /// The directory to operate in.
    current_dir: PathBuf,
    /// Path to wasmer executable used to run the command.
    wasmer_path: PathBuf,
    /// Path to the Wasm file to compile.
    wasm_path: PathBuf,
    /// Path to the static object file produced by compiling the Wasm.
    wasm_object_path: PathBuf,
    /// Path to output the generated header to.
    header_output_path: PathBuf,
    /// Compiler with which to compile the Wasm.
    compiler: Compiler,
    /// Engine with which to use to generate the artifacts.
    engine: Engine,
}

impl Default for WasmerCompile {
    fn default() -> Self {
        #[cfg(not(windows))]
        let wasm_obj_path = "wasm.o";
        #[cfg(windows)]
        let wasm_obj_path = "wasm.obj";
        Self {
            current_dir: std::env::current_dir().unwrap(),
            wasmer_path: get_wasmer_path(),
            wasm_path: PathBuf::from(staticlib_engine_test_wasm_path()),
            wasm_object_path: PathBuf::from(wasm_obj_path),
            header_output_path: PathBuf::from("my_wasm.h"),
            compiler: Compiler::Cranelift,
            engine: Engine::Staticlib,
        }
    }
}

impl WasmerCompile {
    fn run(&self) -> anyhow::Result<()> {
        let output = Command::new(&self.wasmer_path)
            .current_dir(&self.current_dir)
            .arg("compile")
            .arg(&self.wasm_path.canonicalize()?)
            .arg(&self.compiler.to_flag())
            .arg(&self.engine.to_flag())
            .arg("-o")
            .arg(&self.wasm_object_path)
            .arg("--header")
            .arg(&self.header_output_path)
            .output()?;

        if !output.status.success() {
            bail!(
                "wasmer compile failed with: stdout: {}\n\nstderr: {}",
                std::str::from_utf8(&output.stdout)
                    .expect("stdout is not utf8! need to handle arbitrary bytes"),
                std::str::from_utf8(&output.stderr)
                    .expect("stderr is not utf8! need to handle arbitrary bytes")
            );
        }
        Ok(())
    }
}

/// Compile the C code.
fn run_c_compile(
    current_dir: &Path,
    path_to_c_src: &Path,
    output_name: &Path,
) -> anyhow::Result<()> {
    #[cfg(not(windows))]
    let c_compiler = "cc";
    #[cfg(windows)]
    let c_compiler = "clang++";

    let output = Command::new(c_compiler)
        .current_dir(current_dir)
        .arg("-O2")
        .arg("-c")
        .arg(path_to_c_src)
        .arg("-I")
        .arg(WASMER_INCLUDE_PATH)
        .arg("-o")
        .arg(output_name)
        .output()?;

    if !output.status.success() {
        bail!(
            "C code compile failed with: stdout: {}\n\nstderr: {}",
            std::str::from_utf8(&output.stdout)
                .expect("stdout is not utf8! need to handle arbitrary bytes"),
            std::str::from_utf8(&output.stderr)
                .expect("stderr is not utf8! need to handle arbitrary bytes")
        );
    }
    Ok(())
}

#[test]
fn staticlib_engine_works() -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir().context("Making a temp dir")?;
    let operating_dir: PathBuf = temp_dir.path().to_owned();

    let wasm_path = operating_dir.join(staticlib_engine_test_wasm_path());
    #[cfg(not(windows))]
    let wasm_object_path = operating_dir.join("wasm.o");
    #[cfg(windows)]
    let wasm_object_path = operating_dir.join("wasm.obj");
    let header_output_path = operating_dir.join("my_wasm.h");

    WasmerCompile {
        current_dir: operating_dir.clone(),
        wasm_path: wasm_path.clone(),
        wasm_object_path: wasm_object_path.clone(),
        header_output_path,
        compiler: Compiler::Cranelift,
        engine: Engine::Staticlib,
        ..Default::default()
    }
    .run()
    .context("Failed to compile wasm with Wasmer")?;

    let c_src_file_name = operating_dir.join("c_src.c");
    #[cfg(not(windows))]
    let c_object_path = operating_dir.join("c_src.o");
    #[cfg(windows)]
    let c_object_path = operating_dir.join("c_src.obj");
    let executable_path = operating_dir.join("a.out");

    // TODO: adjust C source code based on locations of things
    {
        let mut c_src_file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&c_src_file_name)
            .context("Failed to open C source code file")?;
        c_src_file.write_all(STATICLIB_ENGINE_TEST_C_SOURCE)?;
    }
    run_c_compile(&operating_dir, &c_src_file_name, &c_object_path)
        .context("Failed to compile C source code")?;
    LinkCode {
        current_dir: operating_dir.clone(),
        object_paths: vec![c_object_path, wasm_object_path],
        output_path: executable_path.clone(),
        ..Default::default()
    }
    .run()
    .context("Failed to link objects together")?;

    let result = run_code(&operating_dir, &executable_path, &[])
        .context("Failed to run generated executable")?;
    let result_lines = result.lines().collect::<Vec<&str>>();
    assert_eq!(result_lines, vec!["Initializing...", "\"Hello, World\""],);

    Ok(())
}
