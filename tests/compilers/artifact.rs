use std::{fs, path::PathBuf};

use wasmer::{Engine, Module};
use wasmer_types::Features;

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

// This test is just here to update the compiled objects to their
// latest version, so we can commit them to the repo.
#[test]
#[ignore = "Please enable it when tests fail, so we can generate new versions of the .wasmu files"]
fn artifact_serialization_build() {
    use std::str::FromStr;
    use wasmer::{
        sys::{
            engine::{get_default_compiler_config, NativeEngineExt},
            CpuFeature, Target, Triple,
        },
        Engine, Module,
    };

    let file_names = ["bash.wasm", "cowsay.wasm", "python-3.11.3.wasm"];
    let operating_systems = ["linux", "windows"];
    let chipset = "x86_64";

    for os in operating_systems {
        let triple = Triple::from_str(&format!("{chipset}-{os}")).unwrap();
        let mut cpu_feature = CpuFeature::set();
        cpu_feature.insert(CpuFeature::from_str("sse2").unwrap());
        let target = Target::new(triple, cpu_feature);
        for file_name in file_names {
            let path = PathBuf::from("tests/integration/cli/tests/wasm").join(file_name);
            let wasm_module = fs::read(path).unwrap();
            let config = get_default_compiler_config().unwrap();
            let mut engine = Engine::new(config, target.clone(), Features::default());

            engine.set_hash_algorithm(Some(wasmer_types::HashAlgorithm::Sha256));

            let module = Module::new(&engine, wasm_module).unwrap();
            let serialized_bytes = module.serialize().unwrap();
            let path = PathBuf::from(&format!("tests/compilers/wasmu/{os}/{file_name}u"));
            std::fs::write(path, serialized_bytes).unwrap();
        }
    }
}

#[test]
#[cfg(target_arch = "x86_64")]
fn artifact_deserialization_roundtrip() {
    use cfg_if::cfg_if;
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
