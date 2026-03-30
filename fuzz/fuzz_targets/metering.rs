#![no_main]

use libfuzzer_sys::fuzz_target;
mod misc;
use misc::{SinglePassFuzzModule, ignore_compilation_error, ignore_runtime_error, save_wasm_file};
use std::sync::Arc;
use wasmer::wasmparser::Operator;
use wasmer::{Instance, Module, Store, imports};
use wasmer_compiler::CompilerConfig;
use wasmer_compiler_cranelift::Cranelift;
use wasmer_middlewares::Metering;

fn cost(operator: &Operator) -> u64 {
    match operator {
        Operator::LocalGet { .. } | Operator::I32Const { .. } => 1,
        Operator::I32Add { .. } => 2,
        _ => 0,
    }
}

fuzz_target!(|module: SinglePassFuzzModule| {
    let wasm_bytes = module.0.to_bytes();

    let mut compiler = Cranelift::default();
    compiler.canonicalize_nans(true);
    compiler.enable_verifier();
    let metering = Arc::new(Metering::new(10, cost));
    compiler.push_middleware(metering);
    let mut store = Store::new(compiler);

    let module = Module::new(&store, &wasm_bytes);
    let module = match module {
        Ok(m) => m,
        Err(e) => {
            if ignore_compilation_error(&e.to_string()) {
                return;
            }
            save_wasm_file(&wasm_bytes);
            panic!("{}", e);
        }
    };

    match Instance::new(&mut store, &module, &imports! {}) {
        Ok(_) => {}
        Err(e) => {
            if ignore_runtime_error(&e.to_string()) {
                return;
            }
            save_wasm_file(&wasm_bytes);
            panic!("{}", e);
        }
    }
});
