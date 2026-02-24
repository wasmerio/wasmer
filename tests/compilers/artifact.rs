use std::{fs, path::PathBuf, str::FromStr};

use anyhow::Result;
use sha2::{Digest, Sha256};

use wasmer::Module;
use wasmer_types::Features;

// Instead of preserving the .wasmu files in the repository,
// we can just compare the expected SHA 256 of the files.
const ARTIFACT_FILES: [(&str, &str); 3] = [
    (
        "bash.wasm",
        "7a1a3ad384f8c43f43e35e459a2636f0e294bc72f6a0358d14edae4df775b6aa",
    ),
    (
        "cowsay.wasm",
        "403ecdabf98629fb59fef1e91b310e40ecc17b125a374275562d1ac0d61b25bc",
    ),
    (
        "python-3.11.3.wasm",
        "a98547c968710d9777dc8a9be690d5377f446b4c3b95784648ae121dd1377096",
    ),
];

#[compiler_test(artifact)]
fn artifact_serialization_roundtrip(config: crate::Config) -> Result<()> {
    for (file_name, _) in ARTIFACT_FILES {
        let path = PathBuf::from("tests/integration/cli/tests/wasm").join(file_name);
        let wasm_module = fs::read(path).unwrap();
        let store = config.store();
        let module = Module::new(&store, wasm_module).unwrap();
        let serialized_bytes = module.serialize().unwrap();
        let deserialized_module =
            unsafe { Module::deserialize(&store, serialized_bytes.clone()) }.unwrap();
        let reserialized_bytes = deserialized_module.serialize().unwrap();
        assert_eq!(serialized_bytes, reserialized_bytes);
    }
    Ok(())
}

#[test]
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn artifact_deserialization_roundtrip() {
    use wasmer::sys::NativeEngineExt;

    let triple = wasmer::sys::Triple::from_str("x86_64-linux").unwrap();
    let mut cpu_feature = wasmer::sys::CpuFeature::set();
    cpu_feature.insert(wasmer::sys::CpuFeature::from_str("sse2").unwrap());
    let target = wasmer::sys::Target::new(triple, cpu_feature);
    let config = wasmer::sys::engine::get_default_compiler_config().unwrap();
    let engine = wasmer::Engine::new(config, target, Features::default());

    // This test is included to make sure we don't break the serialized format
    // by mistake. Otherwise, everything in this test is already tested in
    // `artifact_serialization_roundtrip`.
    for (file_name, expected_sha256) in ARTIFACT_FILES {
        let path = PathBuf::from("tests/integration/cli/tests/wasm").join(file_name);
        let wasm_module_bytes = fs::read(path).unwrap();

        let module = Module::new(&engine, wasm_module_bytes.clone()).unwrap();
        let serialized_bytes = module.serialize().unwrap();

        let digest_hex = hex::encode(Sha256::digest(&serialized_bytes));
        assert_eq!(digest_hex, expected_sha256);

        let deserialized_module =
            unsafe { Module::deserialize(&engine, serialized_bytes.clone()) }.unwrap();
        assert_eq!(deserialized_module.name(), module.name());
        assert_eq!(
            deserialized_module.exports().collect::<Vec<_>>(),
            module.exports().collect::<Vec<_>>()
        );
        assert_eq!(
            deserialized_module.imports().collect::<Vec<_>>(),
            module.imports().collect::<Vec<_>>()
        );
    }
}
