mod utils;
use crate::utils::StdioCapturer;
use serde::{Deserialize, Serialize};
use wasmer::compiler::{compile_with, compiler_for_backend};
use wasmer::Func;
use wasmer_wasi::state::WasiState;
use wasmer_wasi::{generate_import_object_from_state, get_wasi_version};

use lazy_static::lazy_static;
use std::sync::Mutex;
lazy_static! {
    // We want to run wasi tests one by one
    // Based from: https://stackoverflow.com/questions/51694017/how-can-i-avoid-running-some-tests-in-parallel
    static ref WASI_LOCK: Mutex<()> = Mutex::new(());
}

/// This is the structure of the `.out` file
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct WasiTest {
    /// The program expected output
    pub output: String,
    /// The program options
    pub options: WasiOptions,
}

/// The options provied when executed a WASI Wasm program
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct WasiOptions {
    /// Mapped pre-opened dirs
    pub mapdir: Vec<(String, String)>,
    /// Environment vars
    pub env: Vec<(String, String)>,
    /// Program arguments
    pub args: Vec<String>,
    /// Pre-opened directories
    pub dir: Vec<String>,
}

// The generated tests (from build.rs) look like:
// #[cfg(test)]
// mod singlepass {
//     mod wasi {
//         #[test]
//         fn hello() -> anyhow::Result<()> {
//             crate::run_wasi(
//                 "tests/wasi_test_resources/snapshot1/hello.wasm",
//                 "tests/wasi_test_resources/snapshot1/hello.out",
//                 "singlepass"
//             )
//         }
//     }
// }
include!(concat!(env!("OUT_DIR"), "/generated_wasitests.rs"));

fn run_wasi(
    wasm_program_path: &str,
    wasi_json_test_path: &str,
    backend: &str,
) -> anyhow::Result<()> {
    let _shared = WASI_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let backend = utils::get_backend_from_str(backend)?;

    let wasi_json_test = std::fs::read(wasi_json_test_path)?;
    let wasitest: WasiTest = serde_json::from_slice(&wasi_json_test)?;

    let wasm_binary = std::fs::read(wasm_program_path)?;
    let compiler = compiler_for_backend(backend).expect("Backend not recognized");
    let module = compile_with(&wasm_binary, &*compiler).unwrap();

    let wasi_version = get_wasi_version(&module, true).expect("WASI module");
    // println!("Executing WASI: {:?}", wasitest);
    let mut state_builder = WasiState::new("");
    let wasi_state = state_builder
        .envs(wasitest.options.env)
        .args(wasitest.options.args)
        .preopen_dirs(wasitest.options.dir)?
        .map_dirs(wasitest.options.mapdir)?
        .build()?;

    let import_object = generate_import_object_from_state(wasi_state, wasi_version);

    let instance = module
        .instantiate(&import_object)
        .map_err(|err| format!("Can't instantiate the WebAssembly module: {:?}", err))
        .unwrap(); // NOTE: Need to figure what the unwrap is for ??

    let capturer = StdioCapturer::new();

    let start: Func<(), ()> = instance
        .exports
        .get("_start")
        .map_err(|e| format!("{:?}", e))
        .expect("start function in wasi module");

    start.call().expect("execute the wasm");

    let output = capturer.end().unwrap().0;
    let expected_output = wasitest.output;

    assert!(
        output.contains(&expected_output),
        "Output: `{}` does not contain expected output: `{}`",
        output,
        expected_output
    );

    Ok(())
}
