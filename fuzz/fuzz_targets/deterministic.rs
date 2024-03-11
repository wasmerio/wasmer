#![no_main]

use libfuzzer_sys::{arbitrary, arbitrary::Arbitrary, fuzz_target};
use wasm_smith::{Config, ConfiguredModule};
use wasmer::{CompilerConfig, EngineBuilder, Module, Store};
use wasmer_compiler::Engine;
use wasmer_compiler_cranelift::Cranelift;
use wasmer_compiler_llvm::LLVM;
use wasmer_compiler_singlepass::Singlepass;

#[derive(Arbitrary, Debug, Default, Copy, Clone)]
struct NoImportsConfig;
impl Config for NoImportsConfig {
    fn max_imports(&self) -> usize {
        0
    }
    fn max_memory_pages(&self) -> u32 {
        // https://github.com/wasmerio/wasmer/issues/2187
        65535
    }
    fn allow_start_export(&self) -> bool {
        false
    }
}

fn compile_and_compare(name: &str, engine: Engine, wasm: &[u8]) {
    let store = Store::new(engine);

    // compile for first time
    let module = Module::new(&store, wasm).unwrap();
    let first = module.serialize().unwrap();

    // compile for second time
    let module = Module::new(&store, wasm).unwrap();
    let second = module.serialize().unwrap();

    if first != second {
        panic!("non-deterministic compilation from {}", name);
    }
}

fuzz_target!(|module: ConfiguredModule<NoImportsConfig>| {
    let wasm_bytes = module.to_bytes();

    let mut compiler = Cranelift::default();
    compiler.canonicalize_nans(true);
    compiler.enable_verifier();
    compile_and_compare(
        "universal-cranelift",
        EngineBuilder::new(compiler.clone()).engine(),
        &wasm_bytes,
    );

    let mut compiler = LLVM::default();
    compiler.canonicalize_nans(true);
    compiler.enable_verifier();
    compile_and_compare(
        "universal-llvm",
        EngineBuilder::new(compiler.clone()).engine(),
        &wasm_bytes,
    );

    let compiler = Singlepass::default();
    compile_and_compare(
        "universal-singlepass",
        EngineBuilder::new(compiler.clone()).engine(),
        &wasm_bytes,
    );
});
