use libfuzzer_sys::arbitrary::Arbitrary;

#[allow(dead_code)]
pub(crate) struct SinglePassFuzzModule(pub(crate) wasm_smith::Module);

impl Arbitrary<'_> for SinglePassFuzzModule {
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
        config.exceptions_enabled = false;
        config.memory64_enabled = false;
        config.max_memories = 1;
        config.tail_call_enabled = false;
        config.simd_enabled = false;
        Ok(Self(wasm_smith::Module::new(config, u)?))
    }
}

impl std::fmt::Debug for SinglePassFuzzModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&wasmprinter::print_bytes(self.0.to_bytes()).unwrap())
    }
}

#[allow(dead_code)]
pub(crate) fn save_wasm_file(data: &[u8]) {
    if let Ok(path) = std::env::var("DUMP_TESTCASE") {
        use std::fs::File;
        use std::io::Write;
        let mut file = File::create(&path).unwrap();
        file.write_all(data).unwrap();
    }
}

#[allow(dead_code)]
pub(crate) fn ignore_compilation_error(error_message: &str) -> bool {
    error_message.starts_with("Compilation error: singlepass init_local unimplemented type: V128")
        || error_message.starts_with("Validation error: constant expression required")
        || error_message.starts_with("Compilation error: not yet implemented: V128Const")
        || error_message.starts_with("WebAssembly translation error: Unsupported feature: `ref.null T` that is not a `funcref` or an `externref`: Exn")
        || error_message.starts_with("WebAssembly translation error: Unsupported feature: unsupported element type in element section: exnref")
}

#[allow(dead_code)]
pub(crate) fn ignore_runtime_error(error_message: &str) -> bool {
    error_message.starts_with("RuntimeError: out of bounds")
        || error_message.starts_with("RuntimeError: call stack exhausted")
        || error_message.starts_with("RuntimeError: undefined element: out of bounds")
        || error_message.starts_with("RuntimeError: unreachable")
        || error_message.starts_with("Insufficient resources: tables of types other than funcref or externref (ExceptionRef)")
}
