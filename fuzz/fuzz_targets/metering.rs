#![no_main]

use libfuzzer_sys::fuzz_target;
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

fuzz_target!(|module: wasm_smith::Module| {
    let wasm_bytes = module.to_bytes();

    if let Ok(path) = std::env::var("DUMP_TESTCASE") {
        use std::fs::File;
        use std::io::Write;
        let mut file = File::create(path).unwrap();
        file.write_all(&wasm_bytes).unwrap();
        return;
    }

    let mut compiler = Cranelift::default();
    compiler.canonicalize_nans(true);
    compiler.enable_verifier();
    let metering = Arc::new(Metering::new(10, cost));
    compiler.push_middleware(metering);
    let mut store = Store::new(compiler);
    let module = Module::new(&store, &wasm_bytes).unwrap();
    match Instance::new(&mut store, &module, &imports! {}) {
        Ok(_) => {}
        Err(e) => {
            let error_message = format!("{}", e);
            if error_message.starts_with("RuntimeError: ")
                && error_message.contains("out of bounds")
            {
                return;
            }
            panic!("{}", e);
        }
    }
});
