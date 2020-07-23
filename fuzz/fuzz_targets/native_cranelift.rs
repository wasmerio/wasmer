#![no_main]
#[macro_use]
extern crate libfuzzer_sys;

use wasmer::{imports, Instance, Module, Store};
use wasmer_compiler_cranelift::Cranelift;
use wasmer_engine_native::Native;

fuzz_target!(|wasm_bytes: &[u8]| {
    let serialized = {
        let mut compiler = Cranelift::default();
        let store = Store::new(&Native::new(&mut compiler).engine());
        match Module::validate(&store, wasm_bytes) {
            Err(_) => return,
            Ok(_) => {}
        };
        let module = Module::new(&store, wasm_bytes).unwrap();
        module.serialize().unwrap()
    };

    let engine = Native::headless().engine();
    let store = Store::new(&engine);
    let module = unsafe { Module::deserialize(&store, serialized.as_slice()) }.unwrap();
    let _instance = Instance::new(&module, &imports! {});
});
