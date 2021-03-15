#![no_main]

use libfuzzer_sys::fuzz_target;
use wasm_smith::Module as FuzzModule;
use wasmer::{imports, Instance, Module, Store};
use wasmer_compiler_singlepass::Singlepass;
use wasmer_engine_jit::JIT;

fuzz_target!(|module: FuzzModule| {
    let wasm_bytes = module.to_bytes();
    let compiler = Singlepass::default();
    let store = Store::new(&JIT::new(compiler).engine());
    let module = Module::new(&store, &wasm_bytes);
    let module = match module {
        Ok(m) => m,
        Err(e) => {
            let error_message = format!("{}", e);
            if error_message.contains("Validation error: invalid result arity: func type returns multiple values") || error_message.contains("Validation error: blocks, loops, and ifs accept no parameters when multi-value is not enabled") {
                return;
            }
            panic!("{}", e);
        }
    };
    let _ignored = Instance::new(&module, &imports! {});
});
