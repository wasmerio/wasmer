#![no_main]

use libfuzzer_sys::{arbitrary::Arbitrary, fuzz_target};
mod misc;
use misc::{ignore_compilation_error, ignore_runtime_error, save_wasm_file};
use wasmer::{Instance, Module, Store, imports};
use wasmer_compiler::{CompilerConfig, EngineBuilder};
use wasmer_compiler_cranelift::Cranelift;

struct CraneliftPassFuzzModule(wasm_smith::Module);

impl Arbitrary<'_> for CraneliftPassFuzzModule {
    fn arbitrary(
        u: &mut libfuzzer_sys::arbitrary::Unstructured,
    ) -> libfuzzer_sys::arbitrary::Result<Self> {
        let mut config = wasm_smith::Config::arbitrary(u)?;
        config.min_imports = 0;
        config.max_imports = 0;
        config.max_memory32_bytes = 65535 * 4096;
        config.min_funcs = 1;
        config.max_funcs = std::cmp::max(config.min_funcs, config.max_funcs);
        config.min_exports = 1;
        config.max_exports = std::cmp::max(config.min_exports, config.max_exports);
        config.gc_enabled = false;
        config.memory64_enabled = false;
        config.max_memories = 1;
        config.tail_call_enabled = false;
        config.simd_enabled = false;
        config.relaxed_simd_enabled = false;
        config.extended_const_enabled = true;
        Ok(Self(wasm_smith::Module::new(config, u)?))
    }
}

impl std::fmt::Debug for CraneliftPassFuzzModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&wasmprinter::print_bytes(self.0.to_bytes()).unwrap())
    }
}

fuzz_target!(|module: CraneliftPassFuzzModule| {
    let wasm_bytes = module.0.to_bytes();

    let mut compiler = Cranelift::default();
    compiler.canonicalize_nans(true);
    compiler.enable_verifier();
    let mut store = Store::new(EngineBuilder::new(compiler));
    // Save early (and always) as we might hit a crash or a validation error.
    save_wasm_file(&wasm_bytes);
    let module = Module::new(&store, &wasm_bytes);
    let module = match module {
        Ok(m) => m,
        Err(e) => {
            if ignore_compilation_error(&e.to_string()) {
                return;
            }
            panic!("{}", e);
        }
    };

    match Instance::new(&mut store, &module, &imports! {}) {
        Ok(_) => {}
        Err(e) => {
            if ignore_runtime_error(&e.to_string()) {
                return;
            }
            panic!("{}", e);
        }
    }
});
