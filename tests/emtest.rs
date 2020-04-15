mod utils;

use crate::utils::stdio::StdioCapturer;
use anyhow::{anyhow, bail};
use wasmer::compiler::{compile_with, compiler_for_backend, Backend};
use wasmer_emscripten::{generate_emscripten_env, run_emscripten_instance, EmscriptenGlobals};

use lazy_static::lazy_static;
use std::sync::Mutex;
lazy_static! {
    // We want to run emscripten tests one by one
    // Based from: https://stackoverflow.com/questions/51694017/how-can-i-avoid-running-some-tests-in-parallel
    static ref EMSCRIPTEN_LOCK: Mutex<()> = Mutex::new(());
}

// The generated tests (from build.rs) look like:
// #[cfg(test)]
// mod singlepass {
//     mod wasi {
//         #[test]
//         fn test_hello_world() -> anyhow::Result<()> {
//             crate::run_wasi(
//                 "tests/emscripten_resources/emtests/test_hello_world.wasm",
//                 "tests/emscripten_resources/emtests/test_hello_world.out",
//                 "singlepass"
//             )
//         }
//     }
// }
include!(concat!(env!("OUT_DIR"), "/generated_emtests.rs"));

fn run_emscripten(wasm_program_path: &str, output_path: &str, backend: &str) -> anyhow::Result<()> {
    let _shared = EMSCRIPTEN_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let backend = utils::get_backend_from_str(backend)?;
    let program_name = "name";

    let wasm_binary = std::fs::read(wasm_program_path)?;
    let compiler = compiler_for_backend(backend).expect("Backend not recognized");
    let module = compile_with(&wasm_binary, &*compiler).unwrap();

    let mut emscripten_globals = EmscriptenGlobals::new(&module).expect("globals are valid");
    let import_object = generate_emscripten_env(&mut emscripten_globals);

    let mut instance = module
        .instantiate(&import_object)
        .map_err(|err| anyhow!("Can't instantiate the WebAssembly module: {:?}", err))?;

    let capturer = StdioCapturer::new();

    run_emscripten_instance(
        &module,
        &mut instance,
        &mut emscripten_globals,
        program_name,
        vec![],
        None,
        vec![],
    )
    .expect("run_emscripten_instance finishes");

    let output = capturer.end().unwrap().0;

    let expected_output = String::from_utf8(std::fs::read(output_path)?)?;

    assert!(
        output.contains(&expected_output),
        "Output: `{}` does not contain expected output: `{}`",
        output,
        expected_output
    );
    Ok(())
}
