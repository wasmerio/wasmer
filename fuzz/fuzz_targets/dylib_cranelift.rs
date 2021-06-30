#![no_main]

use libfuzzer_sys::{arbitrary, arbitrary::Arbitrary, fuzz_target};
use wasm_smith::{Config, ConfiguredModule};
use wasmer::{imports, Instance, Module, Store};
use wasmer_compiler_cranelift::Cranelift;
use wasmer_engine_dylib::Dylib;

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
#[derive(Arbitrary)]
struct WasmSmithModule(ConfiguredModule<NoImportsConfig>);
impl std::fmt::Debug for WasmSmithModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&wasmprinter::print_bytes(self.0.to_bytes()).unwrap())
    }
}

fuzz_target!(|module: WasmSmithModule| {
    let serialized = {
        let wasm_bytes = module.0.to_bytes();

        if let Ok(path) = std::env::var("DUMP_TESTCASE") {
            use std::fs::File;
            use std::io::Write;
            let mut file = File::create(path).unwrap();
            file.write_all(&wasm_bytes).unwrap();
            return;
        }

        let compiler = Cranelift::default();
        let store = Store::new(&Dylib::new(compiler).engine());
        let module = Module::new(&store, &wasm_bytes).unwrap();
        module.serialize().unwrap()
    };

    let engine = Dylib::headless().engine();
    let store = Store::new(&engine);
    let module = unsafe { Module::deserialize(&store, serialized.as_slice()) }.unwrap();
    match Instance::new(&module, &imports! {}) {
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
