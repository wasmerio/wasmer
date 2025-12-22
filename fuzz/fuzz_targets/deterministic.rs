#![no_main]

use libfuzzer_sys::fuzz_target;
use wasmer::{Module, Store};
use wasmer_compiler::Engine;
use wasmer_compiler::{CompilerConfig, EngineBuilder};
use wasmer_compiler_cranelift::Cranelift;
use wasmer_compiler_llvm::LLVM;
use wasmer_compiler_singlepass::Singlepass;

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

fuzz_target!(|module: wasm_smith::Module| {
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
