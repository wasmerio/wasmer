#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use wabt::wat2wasm;
    use wasmer_emscripten::is_emscripten_module;
    use wasmer_runtime::compile;

    #[test]
    fn should_detect_emscripten_files() {
        const WAST_BYTES: &[u8] = include_bytes!("tests/is_emscripten_true.wast");
        let wasm_binary = wat2wasm(WAST_BYTES.to_vec()).expect("Can't convert to wasm");
        let module = compile(&wasm_binary[..]).expect("WASM can't be compiled");
        let module = Arc::new(module);
        assert!(is_emscripten_module(&module));
    }

    #[test]
    fn should_detect_non_emscripten_files() {
        const WAST_BYTES: &[u8] = include_bytes!("tests/is_emscripten_false.wast");
        let wasm_binary = wat2wasm(WAST_BYTES.to_vec()).expect("Can't convert to wasm");
        let module = compile(&wasm_binary[..]).expect("WASM can't be compiled");
        let module = Arc::new(module);
        assert!(!is_emscripten_module(&module));
    }
}
