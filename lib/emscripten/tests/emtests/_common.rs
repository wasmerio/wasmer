macro_rules! assert_emscripten_output {
    ($file:expr, $name:expr, $args:expr, $expected:expr) => {{
        use wasmer_emscripten::generate_emscripten_env;
        // use wasmer::common::stdio::StdioCapturer;
        use wasmer_runtime::{Import, Imports, FuncRef};
        use wasmer_runtime::table::TableBacking;
        use wasmer_runtime::{Instance, module::Module};
        use wasmer_clif_backend::CraneliftCompiler;

        use std::sync::Arc;

        let wasm_bytes = include_bytes!($file);
        let import_object = generate_emscripten_env();
//         let options = Some(InstanceOptions {
//             mock_missing_imports: true,
//             mock_missing_globals: true,
//             mock_missing_tables: true,
//             abi: InstanceABI::Emscripten,
//             show_progressbar: false,
// //            isa: get_isa(),
//         });
//         let mut result_object = instantiate(&wasm_bytes.to_vec(), &import_object, options)
//             .expect("Not compiled properly");

        let module = wasmer_runtime::compile(&wasm_bytes[..], &CraneliftCompiler::new()).expect("WASM can't be compiled");
        let instance = module.instantiate(&import_object).expect("WASM can't be instantiated");

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
        assert!(false, "Emscripten tests are mocked");
        // assert!(
        //     output.contains(expected_output),
        //     "Output: `{}` does not contain expected output: `{}`",
        //     output,
        //     expected_output
        // );
    }};
}
