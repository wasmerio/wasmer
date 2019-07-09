#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use wabt::wat2wasm;
    use wasmer_emscripten::is_emscripten_module;
    use wasmer_runtime_core::backend::Compiler;
    use wasmer_runtime_core::compile_with;

    #[cfg(feature = "clif")]
    fn get_compiler() -> impl Compiler {
        use wasmer_clif_backend::CraneliftCompiler;
        CraneliftCompiler::new()
    }

    #[cfg(feature = "llvm")]
    fn get_compiler() -> impl Compiler {
        use wasmer_llvm_backend::LLVMCompiler;
        LLVMCompiler::new()
    }

    #[cfg(feature = "singlepass")]
    fn get_compiler() -> impl Compiler {
        use wasmer_singlepass_backend::SinglePassCompiler;
        SinglePassCompiler::new()
    }

    #[cfg(not(any(feature = "llvm", feature = "clif", feature = "singlepass")))]
    fn get_compiler() -> impl Compiler {
        panic!("compiler not specified, activate a compiler via features");
        use wasmer_clif_backend::CraneliftCompiler;
        CraneliftCompiler::new()
    }

    #[test]
    fn should_detect_emscripten_files() {
        const WAST_BYTES: &[u8] = include_bytes!("tests/is_emscripten_true.wast");
        let wasm_binary = wat2wasm(WAST_BYTES.to_vec()).expect("Can't convert to wasm");
        let module =
            compile_with(&wasm_binary[..], &get_compiler()).expect("WASM can't be compiled");
        let module = Arc::new(module);
        assert!(is_emscripten_module(&module));
    }

    #[test]
    fn should_detect_non_emscripten_files() {
        const WAST_BYTES: &[u8] = include_bytes!("tests/is_emscripten_false.wast");
        let wasm_binary = wat2wasm(WAST_BYTES.to_vec()).expect("Can't convert to wasm");
        let module =
            compile_with(&wasm_binary[..], &get_compiler()).expect("WASM can't be compiled");
        let module = Arc::new(module);
        assert!(!is_emscripten_module(&module));
    }
}
