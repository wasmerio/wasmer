use std::path::PathBuf;
use std::process::Command;
use wasmer_integration_tests_cli::get_wasmer_path;
use wasmer_integration_tests_cli::C_ASSET_PATH;

fn create_exe_wabt_path() -> String {
    format!("{}/{}", C_ASSET_PATH, "wabt-1.0.37.wasmer")
}

fn create_exe_python_wasmer() -> String {
    format!("{}/{}", C_ASSET_PATH, "python-0.1.0.wasmer")
}

fn create_exe_test_wasm_path() -> String {
    format!("{}/{}", C_ASSET_PATH, "qjs.wasm")
}

#[test]
fn gen_c_header_works() -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let operating_dir: PathBuf = temp_dir.path().to_owned();

    let wasm_path = operating_dir.join(create_exe_test_wasm_path());
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

    let cmd = Command::new(get_wasmer_path())
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
