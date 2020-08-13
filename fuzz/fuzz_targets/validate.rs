#![no_main]
#[macro_use]
extern crate libfuzzer_sys;

use wasmer::{imports, Instance, Module, Store};
use wasmer_compiler_cranelift::Cranelift;
use wasmer_engine_jit::JIT;

fuzz_target!(|wasm_bytes: &[u8]| {
    let compiler = Cranelift::default();
    let store = Store::new(&JIT::new(&compiler).engine());
    Module::validate(&store, wasm_bytes);
});
