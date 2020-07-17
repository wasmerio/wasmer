#![no_main]
#[macro_use]
extern crate libfuzzer_sys;

use wasmer::{imports, Instance, Module, Store};
use wasmer_compiler_singlepass::Singlepass;
use wasmer_engine_jit::JIT;

fuzz_target!(|wasm_bytes: &[u8]| {
    let compiler = Singlepass::default();
    let store = Store::new(&JIT::new(&compiler).engine());
    match Module::validate(&store, wasm_bytes) {
        Ok(_) => {
            let module = Module::new(&store, wasm_bytes).unwrap();
            let _instance = Instance::new(&module, &imports! {});
        }
        Err(_) => {}
    };
});
