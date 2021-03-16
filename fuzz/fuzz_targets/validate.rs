#![no_main]

use libfuzzer_sys::fuzz_target;
use wasmer::{Module, Store};
use wasmer_compiler_cranelift::Cranelift;
use wasmer_engine_jit::JIT;

fuzz_target!(|wasm_bytes: &[u8]| {
    let compiler = Cranelift::default();
    let store = Store::new(&JIT::new(compiler).engine());
    let _ignored = Module::validate(&store, wasm_bytes);
});
