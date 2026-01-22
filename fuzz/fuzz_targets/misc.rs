pub fn save_wasm_file(data: &[u8]) {
    if let Ok(path) = std::env::var("DUMP_TESTCASE") {
        use std::fs::File;
        use std::io::Write;
        let mut file = File::create(&path).unwrap();
        eprintln!("Saving fuzzed WASM file to: {path:?}");
        file.write_all(data).unwrap();
    }
}

pub fn ignore_compilation_error(error_message: &str) -> bool {
    error_message.starts_with("Compilation error: singlepass init_local unimplemented type: V128")
        || error_message.starts_with("Validation error: constant expression required")
        || error_message.starts_with("Compilation error: not yet implemented: V128Const")
        || error_message.starts_with("WebAssembly translation error: Unsupported feature: `ref.null T` that is not a `funcref` or an `externref`: Exn")
        || error_message.starts_with("WebAssembly translation error: Unsupported feature: unsupported element type in element section: exnref")
}

pub fn ignore_runtime_error(error_message: &str) -> bool {
    error_message.starts_with("RuntimeError: out of bounds")
        || error_message.starts_with("RuntimeError: call stack exhausted")
        || error_message.starts_with("RuntimeError: undefined element: out of bounds")
        || error_message.starts_with("RuntimeError: unreachable")
        || error_message.starts_with("Insufficient resources: tables of types other than funcref or externref (ExceptionRef)")
}
