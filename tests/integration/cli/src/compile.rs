//! CLI tests for the compile subcommand.

use anyhow::{bail, Context};
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

const CLI_INTEGRATION_TESTS_ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");

const OBJECT_FILE_ENGINE_TEST_C_SOURCE: &[u8] =
    include_bytes!("object_file_engine_test_c_source.c");
// TODO:
const OBJECT_FILE_ENGINE_TEST_WASM_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/assets/qjs.wasm");

const WASMER_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../target/release/wasmer"
);

const LIBWASMER_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../target/release/libwasmer_c_api.a"
);

/// Get the path to the `wasmer` executable to be used in this test.
fn get_wasmer_path() -> PathBuf {
    PathBuf::from(env::var("WASMER_TEST_WASMER_PATH").unwrap_or_else(|_| WASMER_PATH.to_string()))
}

/// Get the path to the `libwasmer.a` static library.
fn get_libwasmer_path() -> PathBuf {
    PathBuf::from(
        env::var("WASMER_TEST_LIBWASMER_PATH").unwrap_or_else(|_| LIBWASMER_PATH.to_string()),
    )
}

#[derive(Debug, Copy, Clone)]
pub enum Engine {
    Jit,
    Native,
    ObjectFile,
}

impl Engine {
    // TODO: make this `const fn` when Wasmer moves to Rust 1.46.0+
    pub fn to_flag(self) -> &'static str {
        match self {
            Engine::Jit => "--jit",
            Engine::Native => "--native",
            Engine::ObjectFile => "--object-file",
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum Compiler {
    Cranelift,
    LLVM,
    Singlepass,
}

impl Compiler {
    // TODO: make this `const fn` when Wasmer moves to Rust 1.46.0+
    pub fn to_flag(self) -> &'static str {
        match self {
            Compiler::Cranelift => "--cranelift",
            Compiler::LLVM => "--llvm",
            Compiler::Singlepass => "--singlepass",
        }
    }
}

/// Data used to run the `wasmer compile` command.
#[derive(Debug)]
struct WasmerCompile {
    /// Path to wasmer executable used to run the command.
    wasmer_path: PathBuf,
    /// Path to the Wasm file to compile.
    wasm_path: PathBuf,
    /// Path to the object file produced by compiling the Wasm.
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
        Self {
            wasmer_path: get_wasmer_path(),
            wasm_path: PathBuf::from(OBJECT_FILE_ENGINE_TEST_WASM_PATH),
            wasm_object_path: PathBuf::from("wasm.o"),
            header_output_path: PathBuf::from("my_wasm.h"),
            compiler: Compiler::Cranelift,
            engine: Engine::ObjectFile,
        }
    }
}

impl WasmerCompile {
    fn run(&self) -> anyhow::Result<()> {
        let output = Command::new(&self.wasmer_path)
            .arg("compile")
            .arg(&self.wasm_path)
            .arg(&self.compiler.to_flag())
            .arg(&self.engine.to_flag())
            .arg("-o")
            .arg(&self.wasm_object_path)
            .arg("--header")
            .arg(&self.header_output_path)
            .output()?;

        if !output.status.success() {
            bail!(
                "wasmer compile failed with: {}",
                std::str::from_utf8(&output.stderr)
                    .expect("stderr is not utf8! need to handle arbitrary bytes")
            );
        }
        Ok(())
    }
}

fn run_c_compile(path_to_c_src: &Path, output_name: &Path) -> anyhow::Result<()> {
    let output = Command::new("cc")
        .arg("-O2")
        .arg("-c")
        .arg(path_to_c_src)
        .arg("-I")
        .arg(CLI_INTEGRATION_TESTS_ASSETS)
        .arg("-o")
        .arg(output_name)
        .output()?;

    if !output.status.success() {
        bail!(
            "C code compile failed with: {}",
            std::str::from_utf8(&output.stderr)
                .expect("stderr is not utf8! need to handle arbitrary bytes")
        );
    }
    Ok(())
}

/// Data used to run a linking command for generated artifacts.
#[derive(Debug)]
struct LinkCode {
    /// Path to the linker used to run the linking command.
    linker_path: PathBuf,
    /// String used as an optimization flag.
    optimization_flag: String,
    /// Paths of objects to link.
    object_paths: Vec<PathBuf>,
    /// Path to the output target.
    output_path: PathBuf,
    /// Path to the static libwasmer library.
    libwasmer_path: PathBuf,
}

impl Default for LinkCode {
    fn default() -> Self {
        Self {
            linker_path: PathBuf::from("g++"),
            optimization_flag: String::from("-O2"),
            object_paths: vec![],
            output_path: PathBuf::from("a.out"),
            libwasmer_path: get_libwasmer_path(),
        }
    }
}

impl LinkCode {
    fn run(&self) -> anyhow::Result<()> {
        let output = Command::new(&self.linker_path)
            .arg(&self.optimization_flag)
            .args(&self.object_paths)
            .arg(&self.libwasmer_path)
            .arg("-o")
            .arg(&self.output_path)
            .output()?;

        if !output.status.success() {
            bail!(
                "linking failed with: {}",
                std::str::from_utf8(&output.stderr)
                    .expect("stderr is not utf8! need to handle arbitrary bytes")
            );
        }
        Ok(())
    }
}

fn run_code(executable_path: &Path) -> anyhow::Result<String> {
    let output = Command::new(executable_path).output()?;

    if !output.status.success() {
        bail!(
            "running executable failed: {}",
            std::str::from_utf8(&output.stderr)
                .expect("stderr is not utf8! need to handle arbitrary bytes")
        );
    }
    let output =
        std::str::from_utf8(&output.stdout).expect("output from running executable is not utf-8");

    Ok(output.to_owned())
}

#[test]
fn object_file_engine_works() -> anyhow::Result<()> {
    let operating_dir = tempfile::tempdir()?;

    std::env::set_current_dir(&operating_dir)?;

    let wasm_path = PathBuf::from(OBJECT_FILE_ENGINE_TEST_WASM_PATH);
    let wasm_object_path = PathBuf::from("wasm.o");
    let header_output_path = PathBuf::from("my_wasm.h");
    let libwasmer_path = get_libwasmer_path();

    WasmerCompile {
        wasm_path: wasm_path.clone(),
        wasm_object_path: wasm_object_path.clone(),
        header_output_path,
        compiler: Compiler::Cranelift,
        engine: Engine::ObjectFile,
        ..Default::default()
    }
    .run()
    .context("Failed to compile wasm with Wasmer")?;

    let c_src_file_name = Path::new("c_src.c");
    let c_object_path = PathBuf::from("c_src.o");
    let executable_path = PathBuf::from("a.out");

    // TODO: adjust C source code based on locations of things
    {
        let mut c_src_file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&c_src_file_name)
            .context("Failed to open C source code file")?;
        c_src_file.write_all(OBJECT_FILE_ENGINE_TEST_C_SOURCE)?;
    }
    run_c_compile(&c_src_file_name, &c_object_path).context("Failed to compile C source code")?;
    LinkCode {
        object_paths: vec![c_object_path, wasm_object_path],
        output_path: executable_path.clone(),
        ..Default::default()
    }
    .run()
    .context("Failed to link objects together")?;

    let result = run_code(&executable_path).context("Failed to run generated executable")?;
    assert_eq!(
        &result,
        r#"Initializing...
Buffer size: 1801380
"Hello, World"
"#
    );

    Ok(())
}
