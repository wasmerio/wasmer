#![no_main]

use libfuzzer_sys::fuzz_target;
use wasmer::{imports, Instance, Module, Store};
use wasmer_compiler_cranelift::Cranelift;
use wasmer_engine_native::Native;
use wasm_smith::Module as FuzzModule;

fuzz_target!(|module: FuzzModule| {
    let serialized = {
        let wasm_bytes = module.to_bytes();
        let compiler = Cranelift::default();
        let store = Store::new(&Native::new(compiler).engine());
        let module = Module::new(&store, &wasm_bytes).unwrap();
        module.serialize().unwrap()
    };

    let engine = Native::headless().engine();
    let store = Store::new(&engine);
    let module = unsafe { Module::deserialize(&store, serialized.as_slice()) }.unwrap();
    Instance::new(&module, &imports! {}).unwrap();
});
