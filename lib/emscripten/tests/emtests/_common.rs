macro_rules! assert_emscripten_output {
    ($file:expr, $name:expr, $args:expr, $expected:expr) => {{
        // TODO: Cyclic Dep!
        use wasmer::{
            webassembly::{
                compile,
                start_instance,
            },
            common::stdio::StdioCapturer,
        };

        use wasmer_runtime::{
            instance::Instance,
            module::Module,
            table::TableBacking
        };

        use wasmer_clif_backend::CraneliftCompiler;
        use wasmer_emscripten::{
            EmscriptenGlobals,
            generate_emscripten_env,
        };

        use std::sync::Arc;

        let wasm_bytes = include_bytes!($file);

        let module = compile(&wasm_bytes[..])
            .map_err(|err| format!("Can't create the WebAssembly module: {}", err)).unwrap(); // NOTE: Need to figure what the unwrap is for ??

        let emscripten_globals = EmscriptenGlobals::new();
        let mut import_object = generate_emscripten_env(&emscripten_globals);

        let mut instance = module.instantiate(import_object)
            .map_err(|err| format!("Can't instantiate the WebAssembly module: {}", err)).unwrap(); // NOTE: Need to figure what the unwrap is for ??

        start_instance(
            Arc::clone(&module),
            &mut instance,
            $name,
            $args,
        );

        assert!(false, "Emscripten tests are mocked");

        // let capturer = StdioCapturer::new();
        // start_instance(
        //     Arc::clone(&result_object.module),
        //     &mut result_object.instance,
        //     $name,
        //     $args,
        // )
        // .unwrap();
        // let output = capturer.end().unwrap().0;
        // let expected_output = include_str!($expected);
        // assert!(false, "Emscripten tests are mocked");
        // assert!(
        //     output.contains(expected_output),
        //     "Output: `{}` does not contain expected output: `{}`",
        //     output,
        //     expected_output
        // );
    }};
}
