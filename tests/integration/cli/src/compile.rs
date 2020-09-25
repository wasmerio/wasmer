//! CLI tests for the compile subcommand.

use anyhow::bail;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Copy, Clone)]
pub enum Engine {
    Jit,
    Native,
    ObjectFile,
}

impl Engine {
    pub const fn to_flag(self) -> &'static str {
        match self {
            Engine::Jit => "--jit",
            Engine::Native => "--native",
            Engine::ObjectFile => "--object-file",
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum Backend {
    Cranelift,
    LLVM,
    Singlepass,
}

impl Backend {
    pub const fn to_flag(self) -> &'static str {
        match self {
            Backend::Cranelift => "--cranelift",
            Backend::LLVM => "--llvm",
            Backend::Singlepass => "--singlepass",
        }
    }
}

fn run_wasmer_compile(
    path_to_wasm: &Path,
    wasm_output_path: &Path,
    header_output_path: &Path,
    backend: Backend,
    engine: Engine,
) -> anyhow::Result<()> {
    let output = Command::new("wasmer")
        .arg("compile")
        .arg(path_to_wasm)
        .arg(backend.to_flag())
        .arg(engine.to_flag())
        .arg("-o")
        .arg(wasm_output_path)
        .arg("--header")
        .arg(header_output_path)
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

fn run_c_compile(path_to_c_src: &Path, output_name: &Path) -> anyhow::Result<()> {
    let output = Command::new("clang")
        .arg("-O2")
        .arg("-c")
        .arg(path_to_c_src)
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

fn link_code(c_src_object: &Path, wasm_object: &Path, lib_wasmer: &Path) -> anyhow::Result<()> {
    // TODO: linker selection
    let output = Command::new("g++")
        .arg("-O2")
        .arg(c_src_object)
        .arg(wasm_object)
        .arg(lib_wasmer)
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

fn run_code(executable_path: &Path) -> anyhow::Result<()> {
    let output = Command::new(executable_path).output()?;

    if !output.status.success() {
        bail!(
            "running executable failed: {}",
            std::str::from_utf8(&output.stderr)
                .expect("stderr is not utf8! need to handle arbitrary bytes")
        );
    }
    Ok(())
}

const OBJECT_FILE_ENGINE_TEST_C_SOURCE: &[u8] =
    include_bytes!("object_file_engine_test_c_source.c");
// TODO:
const OBJECT_FILE_ENGINE_TEST_WASM_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/assets/jq.wasm");

#[test]
fn object_file_engine_works() -> anyhow::Result<()> {
    let operating_dir = tempfile::tempdir()?;

    std::env::set_current_dir(&operating_dir);

    let wasm_path = Path::new(OBJECT_FILE_ENGINE_TEST_WASM_PATH);
    let wasm_object_path = Path::new("wasm.o");
    let header_output_path = Path::new("my_wasm.h");
    // TODO: figure out how to get access to libwasmer here
    let libwasmer_path = Path::new("libwasmer.a");

    run_wasmer_compile(
        &wasm_path,
        &wasm_object_path,
        &header_output_path,
        Backend::Cranelift,
        Engine::ObjectFile,
    )?;

    let c_src_file_name = Path::new("c_src.c");
    let c_object_name = Path::new("c_src.o");
    let executable_name = Path::new("a.out");

    // TODO: adjust C source code based on locations of things
    {
        let mut c_src_file = fs::File::open(&c_src_file_name)?;
        c_src_file.write_all(OBJECT_FILE_ENGINE_TEST_C_SOURCE)?;
    }
    run_c_compile(&c_src_file_name, &c_object_name)?;
    link_code(&c_object_name, wasm_object_path, &libwasmer_path)?;

    run_code(&executable_name)?;

    Ok(())
}
