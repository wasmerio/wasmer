macro_rules! assert_emscripten_output {
    ($file:expr, $name:expr, $args:expr, $expected:expr) => {{
        use wasmer_clif_backend::CraneliftCompiler;
        use wasmer_emscripten::{generate_emscripten_env, stdio::StdioCapturer, EmscriptenGlobals};

        use std::sync::Arc;

        let wasm_bytes = include_bytes!($file);

        let module = wasmer_runtime_core::compile_with(&wasm_bytes[..], &CraneliftCompiler::new())
            .map_err(|err| format!("Can't create the WebAssembly module: {:?}", err))
            .unwrap();

        let module = Arc::new(module);

        let emscripten_globals = EmscriptenGlobals::new();
        let import_object = generate_emscripten_env(&emscripten_globals);

        let mut instance = module
            .instantiate(import_object)
            .map_err(|err| format!("Can't instantiate the WebAssembly module: {:?}", err))
            .unwrap();

        let capturer = StdioCapturer::new();

        instance.call("_main", &[]).map(|_o| ()).unwrap();

        let output = capturer.end().unwrap().0;
        let expected_output = include_str!($expected);
        assert!(
            output.contains(expected_output),
            "Output: `{}` does not contain expected output: `{}`",
            output,
            expected_output
        );
    }};
}
