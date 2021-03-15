#![no_main]

use libfuzzer_sys::{arbitrary, arbitrary::Arbitrary, fuzz_target};
use wasm_smith::{Config, ConfiguredModule};
use wasmer::{imports, Instance, Module, Store};
use wasmer_compiler_cranelift::Cranelift;
use wasmer_engine_native::Native;

#[derive(Arbitrary, Debug, Default, Copy, Clone)]
struct NoImportsConfig;
impl Config for NoImportsConfig {
    fn max_imports(&self) -> usize {
        0
    }
}

fuzz_target!(|module: ConfiguredModule<NoImportsConfig>| {
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
