#![no_main]

use libfuzzer_sys::fuzz_target;
mod misc;
use misc::{SinglePassFuzzModule, ignore_compilation_error, ignore_runtime_error, save_wasm_file};
use wasmer::{Instance, Module, Store, imports};
use wasmer_compiler::EngineBuilder;
use wasmer_compiler_singlepass::Singlepass;

fuzz_target!(|module: SinglePassFuzzModule| {
    let wasm_bytes = module.0.to_bytes();

    let compiler = Singlepass::default();
    let mut store = Store::new(EngineBuilder::new(compiler));
    // Save early (and always) as we might hit a crash or a validation error.
    save_wasm_file(&wasm_bytes);
    let module = Module::new(&store, &wasm_bytes);
    let module = match module {
        Ok(m) => m,
        Err(e) => {
            if ignore_compilation_error(&e.to_string()) {
                return;
            }
            panic!("{}", e);
        }
    };

    match Instance::new(&mut store, &module, &imports! {}) {
        Ok(_) => {}
        Err(e) => {
            if ignore_runtime_error(&e.to_string()) {
                return;
            }
            panic!("{}", e);
        }
    }
});
