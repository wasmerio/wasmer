use std::{fs, path::PathBuf};

use cfg_if::cfg_if;
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

#[test]
fn artifact_deserialization_roundtrip() {
    // This test is included to make sure we don't break the serialized format
    // by mistake. Otherwise, everything in this test is already tested in
    // `artifact_serialization_roundtrip`.
    let file_names = ["bash.wasmu", "cowsay.wasmu", "python-3.11.3.wasmu"];

    cfg_if!(
        if #[cfg(target_os = "windows")] {
            let base_path = "tests/compilers/wasmu/windows";
        } else {
            let base_path = "tests/compilers/wasmu/linux";
        }
    );

    for file_name in file_names {
        let path = PathBuf::from(base_path).join(file_name);
        let wasm_module_bytes = fs::read(path).unwrap();
        let engine = Engine::default();
        let module = unsafe { Module::deserialize(&engine, wasm_module_bytes.clone()) }.unwrap();
        let reserialized_bytes = module.serialize().unwrap();
        assert_eq!(wasm_module_bytes.to_vec(), reserialized_bytes);
    }
}
