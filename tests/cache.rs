#[macro_use]
mod utils;

wasmer_backends! {
    use std::env;
    use wasmer::{compiler::compile_with, imports, Func};
    use wasmer_runtime::Backend as BaseBackend;
    use wasmer_runtime::cache::{FileSystemCache, Cache, WasmHash};
    use wabt::wat2wasm;
    use std::str::FromStr;

    #[test]
    fn test_file_system_cache_run() {
        static WAT: &'static str = r#"
            (module
              (type $t0 (func (param i32) (result i32)))
              (func $add_one (export "add_one") (type $t0) (param $p0 i32) (result i32)
                get_local $p0
                i32.const 1
                i32.add))
        "#;

        let wasm = wat2wasm(WAT).unwrap();

        let module = compile_with(&wasm, &*get_compiler()).unwrap();

        let cache_dir = env::temp_dir();
        println!("test temp_dir {:?}", cache_dir);

        let mut fs_cache = unsafe {
            FileSystemCache::new(cache_dir)
                .map_err(|e| format!("Cache error: {:?}", e))
                .unwrap()
        };
        // store module
        let key = WasmHash::generate(&wasm);
        fs_cache.store(key, module.clone()).unwrap();

        // We need to transform the `wasmer::Backend` into
        // `wasmer_runtime::Backend` as that's something that the
        // cache loader can understand.
        let backend = BaseBackend::from_str(get_backend().to_string()).expect("Can't transform wasmer backend into wasmer_runtime backend");

        // load module
        let cached_module = fs_cache.load_with_backend(key, backend).unwrap();

        let import_object = imports! {};
        let instance = cached_module.instantiate(&import_object).unwrap();
        let add_one: Func<i32, i32> = instance.exports.get("add_one").unwrap();

        let value = add_one.call(42).unwrap();

        // verify it works
        assert_eq!(value, 43);
    }
}
