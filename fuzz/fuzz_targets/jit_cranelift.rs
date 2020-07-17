#![no_main]
#[macro_use]
extern crate libfuzzer_sys;

use wasmer::{imports, CompilerConfig, Instance, Module, Store};
use wasmer_compiler_cranelift::Cranelift;
use wasmer_engine_jit::JIT;

fuzz_target!(|wasm_bytes: &[u8]| {
    let mut compiler = Cranelift::default();
    compiler.canonicalize_nans(true);
    compiler.enable_verifier();
    let store = Store::new(&JIT::new(&compiler).engine());
    match Module::validate(&store, wasm_bytes) {
        Ok(_) => {
            let module = Module::new(&store, wasm_bytes).unwrap();
            let _instance = Instance::new(&module, &imports! {});
        }
        Err(_) => {}
    };
});
