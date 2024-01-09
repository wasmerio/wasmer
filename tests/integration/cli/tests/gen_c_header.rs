use std::{path::PathBuf, process::Command};

use wasmer_integration_tests_cli::{fixtures, get_wasmer_path};

#[test]
fn gen_c_header_works() -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let operating_dir: PathBuf = temp_dir.path().to_owned();

    let wasm_path = operating_dir.join(fixtures::qjs());
    let out_path = temp_dir.path().join("header.h");

    let _ = Command::new(get_wasmer_path())
        .arg("gen-c-header")
        .arg(&wasm_path)
        .arg("-o")
        .arg(&out_path)
        .output()
        .unwrap();

    let file = std::fs::read_to_string(&out_path).expect("no header.h file");
    assert!(file.contains("wasmer_function_6f62a6bc5c8f8e3e12a54e2ecbc5674ccfe1c75f91d8e4dd6ebb3fec422a4d6c_0"), "no wasmer_function_6f62a6bc5c8f8e3e12a54e2ecbc5674ccfe1c75f91d8e4dd6ebb3fec422a4d6c_0 in file");

    let _ = Command::new(get_wasmer_path())
        .arg("gen-c-header")
        .arg(&wasm_path)
        .arg("-o")
        .arg(&out_path)
        .arg("--prefix")
        .arg("abc123")
        .output()
        .unwrap();

    let file = std::fs::read_to_string(&out_path).expect("no header.h file");
    assert!(
        file.contains("wasmer_function_abc123_0"),
        "no wasmer_function_abc123_0 in file"
    );

    Ok(())
}

#[test]
fn gen_c_header_works_pirita() -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let operating_dir: PathBuf = temp_dir.path().to_owned();

    let wasm_path = operating_dir.join(fixtures::wabt());
    let out_path = temp_dir.path().join("header.h");

    let _ = Command::new(get_wasmer_path())
        .arg("gen-c-header")
        .arg(&wasm_path)
        .arg("-o")
        .arg(&out_path)
        .arg("--atom")
        .arg("wasm-validate")
        .output()
        .unwrap();

    let file = std::fs::read_to_string(&out_path).expect("no header.h file");
    assert!(file.contains("wasmer_function_0f41d38dcfb5abc1fadb5e9acbc5c645e53fe4d0dd86270b72a09bfeee04d055_0"), "no wasmer_function_6f62a6bc5c8f8e3e12a54e2ecbc5674ccfe1c75f91d8e4dd6ebb3fec422a4d6c_0 in file");

    let cmd = Command::new(get_wasmer_path())
        .arg("gen-c-header")
        .arg(&wasm_path)
        .arg("-o")
        .arg(&out_path)
        .output()
        .unwrap();

    assert!(!cmd.status.success());

    Ok(())
}
