#![no_main]

use libfuzzer_sys::{arbitrary, arbitrary::Arbitrary, fuzz_target};
use wasm_smith::{Config, ConfiguredModule};
use wasmer::{imports, Instance, Module, Store};
use wasmer_compiler_singlepass::Singlepass;
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
    match Instance::new(&module, &imports! {}) {
        Ok(_) => {}
        Err(e) => {
            let error_message = format!("{}", e);
            if error_message
                .contains("RuntimeError: memory out of bounds: data segment does not fit")
            {
                return;
            }
            panic!("{}", e);
        }
    }
});
