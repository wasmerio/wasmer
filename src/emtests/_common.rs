macro_rules! assert_emscripten_output {
    ($file:expr, $name:expr, $args:expr, $expected:expr) => {{
        use crate::apis::generate_emscripten_env;
        use crate::common::stdio::StdioCapturer;
        use crate::runtime::types::{ElementType, FuncSig, Table, Type, Value};
        use crate::runtime::{Import, Imports};
        use crate::webassembly::{
            get_isa, instantiate, start_instance, InstanceABI, InstanceOptions,
        };
        use std::sync::Arc;

        let wasm_bytes = include_bytes!($file);
        let import_object = generate_emscripten_env();
        let options = Some(InstanceOptions {
            mock_missing_imports: true,
            mock_missing_globals: true,
            mock_missing_tables: true,
            abi: InstanceABI::Emscripten,
            show_progressbar: false,
            isa: get_isa(),
        });
        let mut result_object = instantiate(&wasm_bytes.to_vec(), &import_object, options)
            .expect("Not compiled properly");
        let capturer = StdioCapturer::new();
        start_instance(
            Arc::clone(&result_object.module),
            &mut result_object.instance,
            $name,
            $args,
        )
        .unwrap();
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
