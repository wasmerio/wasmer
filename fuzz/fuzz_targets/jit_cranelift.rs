#![no_main]

use libfuzzer_sys::{arbitrary, arbitrary::Arbitrary, fuzz_target};
use wasm_smith::{Config, ConfiguredModule};
use wasmer::{imports, CompilerConfig, Instance, Module, Store};
use wasmer_compiler_cranelift::Cranelift;
use wasmer_engine_jit::JIT;

#[derive(Arbitrary, Debug, Default, Copy, Clone)]
struct NoImportsConfig;
impl Config for NoImportsConfig {
    fn max_imports(&self) -> usize {
        0
    }
}

fuzz_target!(|module: ConfiguredModule<NoImportsConfig>| {
    let wasm_bytes = module.to_bytes();
    let mut compiler = Cranelift::default();
    compiler.canonicalize_nans(true);
    compiler.enable_verifier();
    let store = Store::new(&JIT::new(compiler).engine());
    let module = Module::new(&store, &wasm_bytes).unwrap();
    Instance::new(&module, &imports! {}).unwrap();
});
