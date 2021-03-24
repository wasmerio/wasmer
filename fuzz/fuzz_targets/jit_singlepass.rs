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
    fn max_memory_pages(&self) -> u32 {
        // https://github.com/wasmerio/wasmer/issues/2187
        65535
    }
    fn allow_start_export(&self) -> bool {
        false
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
            if error_message.contains("Validation error: invalid result arity: func type returns multiple values") || error_message.contains("Validation error: blocks, loops, and ifs accept no parameters when multi-value is not enabled") || error_message.contains("multi-value returns not yet implemented") {
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
                || error_message
                    .contains("RuntimeError: table out of bounds: elements segment does not fit")
            {
                return;
            }
            panic!("{}", e);
        }
    }
});
