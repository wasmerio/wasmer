use std::env;
use wasmer_runtime::Backend;

pub fn get_backend() -> Option<Backend> {
    #[cfg(feature = "backend-cranelift")]
    {
        if let Ok(v) = env::var("WASMER_TEST_CLIF") {
            if v == "1" {
                return Some(Backend::Cranelift);
            }
        }
    }
    #[cfg(feature = "backend-llvm")]
    {
        if let Ok(v) = env::var("WASMER_TEST_LLVM") {
            if v == "1" {
                return Some(Backend::LLVM);
            }
        }
    }
    #[cfg(feature = "backend-singlepass")]
    {
        if let Ok(v) = env::var("WASMER_TEST_SINGLEPASS") {
            if v == "1" {
                return Some(Backend::Singlepass);
            }
        }
    }

    None
}

macro_rules! assert_wasi_output {
    ($file:expr, $name:expr, $po_dir_args: expr, $mapdir_args:expr, $envvar_args:expr, $expected:expr) => {{
        use wasmer_dev_utils::stdio::StdioCapturer;
        use wasmer_runtime::Func;
        use wasmer_wasi::{generate_import_object_for_version, get_wasi_version};

        let wasm_bytes = include_bytes!($file);
        let backend = $crate::wasitests::_common::get_backend().expect("Please set one of `WASMER_TEST_CLIF`, `WASMER_TEST_LLVM`, or `WASMER_TEST_SINGELPASS` to `1`.");
        let compiler = wasmer_runtime::compiler_for_backend(backend).expect("The desired compiler was not found!");

        let module = wasmer_runtime::compile_with_config_with(&wasm_bytes[..], Default::default(), &*compiler).expect("WASM can't be compiled");

        let wasi_version = get_wasi_version(&module, true).expect("WASI module");

        let import_object = generate_import_object_for_version(
            wasi_version,
            vec![$name.into()],
            vec![],
            $po_dir_args,
            $mapdir_args,
        );

        let instance = module
            .instantiate(&import_object)
            .map_err(|err| format!("Can't instantiate the WebAssembly module: {:?}", err))
            .unwrap(); // NOTE: Need to figure what the unwrap is for ??

        let capturer = StdioCapturer::new();

        let start: Func<(), ()> = instance
            .exports
            .get("_start")
            .map_err(|e| format!("{:?}", e))
            .expect("start function in wasi module");

        start.call().expect("execute the wasm");

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
