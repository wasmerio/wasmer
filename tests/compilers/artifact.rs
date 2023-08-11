use std::{fs, path::PathBuf};

use wasmer::{Engine, Module};

#[test]
fn artifact_serialization_roundtrip() {
    let file_names = ["bash.wasm", "cowsay.wasm", "python-3.11.3.wasm"];

    for file_name in file_names {
        let path = PathBuf::from("tests/integration/cli/tests/wasm").join(file_name);
        let wasm_module = fs::read(path).unwrap();
        let engine = Engine::default();
        let module = Module::new(&engine, wasm_module).unwrap();
        let serialized_bytes = module.serialize().unwrap();
        let deserialized_module =
            unsafe { Module::deserialize(&engine, serialized_bytes.clone()) }.unwrap();
        let reserialized_bytes = deserialized_module.serialize().unwrap();
        assert_eq!(serialized_bytes, reserialized_bytes);
    }
}
