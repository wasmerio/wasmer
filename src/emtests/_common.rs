
macro_rules! assert_emscripten_output {
    ($file:expr, $name:expr, $args:expr, $expected:expr) => {{
        use crate::apis::generate_emscripten_env;
        use crate::webassembly::{instantiate, start_instance, InstanceOptions, get_isa};
        use crate::common::stdio::StdioCapturer;

        let wasm_bytes = include_bytes!($file);
        let import_object = generate_emscripten_env();
        let options = Some(InstanceOptions {
            mock_missing_imports: true,
            mock_missing_globals: true,
            mock_missing_tables: true,
            use_emscripten: true,
            show_progressbar: false,
            isa: get_isa(),
        });
        let mut result_object = instantiate(wasm_bytes.to_vec(), import_object, options).expect("Not compiled properly");
        // We use this as a way of hacking the stdout and remove the data that should be flushed
        (StdioCapturer::new()).end();

        let capturer = StdioCapturer::new();
        start_instance(&result_object.module, &mut result_object.instance, $name, $args).unwrap();
        let output = capturer.end().0;
        let expected_output = include_str!($expected);
        assert_eq!(output, expected_output);
    }};
}
