macro_rules! assert_emscripten_output {
    ($file:expr, $name:expr, $args:expr, $expected:expr) => {{

        use wasmer_clif_backend::CraneliftCompiler;
        use wasmer_emscripten::{
            EmscriptenGlobals,
            generate_emscripten_env,
            stdio::StdioCapturer
        };

        let wasm_bytes = include_bytes!($file);

        let module = wasmer_runtime_core::compile_with(&wasm_bytes[..], &CraneliftCompiler::new())
            .expect("WASM can't be compiled");

//        let module = compile(&wasm_bytes[..])
//            .map_err(|err| format!("Can't create the WebAssembly module: {}", err)).unwrap(); // NOTE: Need to figure what the unwrap is for ??
        let mut emscripten_globals = EmscriptenGlobals::new(&module);
        let import_object = generate_emscripten_env(&mut emscripten_globals);

        let mut instance = module.instantiate(&import_object)
            .map_err(|err| format!("Can't instantiate the WebAssembly module: {:?}", err)).unwrap(); // NOTE: Need to figure what the unwrap is for ??

        let capturer = StdioCapturer::new();

        wasmer_emscripten::run_emscripten_instance(
            &module,
            &mut instance,
            $name,
            $args,
        ).expect("run_emscripten_instance finishes");

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
