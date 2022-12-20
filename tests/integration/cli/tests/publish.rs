use std::process::Stdio;
use wasmer_integration_tests_cli::{get_wasmer_path, C_ASSET_PATH};

fn create_exe_test_wasm_path() -> String {
    format!("{}/{}", C_ASSET_PATH, "qjs.wasm")
}

#[test]
fn wasmer_publish() -> anyhow::Result<()> {
    // Only run this test in the CI
    if std::env::var("GITHUB_TOKEN").is_err() {
        return Ok(());
    }

    let wapm_dev_token = std::env::var("WAPM_DEV_TOKEN").ok();
    let tempdir = tempfile::tempdir()?;
    let path = tempdir.path();
    let username = "ciuser";

    let random1 = format!("{}", rand::random::<u32>());
    let random2 = format!("{}", rand::random::<u32>());
    let random3 = format!("{}", rand::random::<u32>());

    std::fs::copy(create_exe_test_wasm_path(), path.join("largewasmfile.wasm")).unwrap();
    std::fs::write(
        path.join("wasmer.toml"),
        include_str!("./fixtures/init6.toml")
            .replace("WAPMUSERNAME", username) // <-- TODO!
            .replace("RANDOMVERSION1", &random1)
            .replace("RANDOMVERSION2", &random2)
            .replace("RANDOMVERSION3", &random3),
    )?;

    let mut cmd = std::process::Command::new(get_wasmer_path());
    cmd.arg("publish");
    cmd.arg("--quiet");
    cmd.arg("--registry");
    cmd.arg("wapm.dev");
    cmd.arg(path);

    if let Some(token) = wapm_dev_token {
        cmd.arg("--token");
        cmd.arg(token);
    }

    let output = cmd.stdin(Stdio::null()).output().unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(stdout, format!("Successfully published package `{username}/largewasmfile@{random1}.{random2}.{random3}`\n"), "failed to publish: {cmd:?}: {stderr}");

    println!("wasmer publish ok! test done.");

    Ok(())
}

// Runs a full integration test to test that the flow wasmer init - cargo build -
// wasmer publish is working
#[test]
fn wasmer_init_publish() -> anyhow::Result<()> {
    // Only run this test in the CI
    if std::env::var("GITHUB_TOKEN").is_err() {
        return Ok(());
    }

    let wapm_dev_token = std::env::var("WAPM_DEV_TOKEN").ok();
    let tempdir = tempfile::tempdir()?;
    let path = tempdir.path();
    let username = "ciuser";

    let random1 = format!("{}", rand::random::<u32>());
    let random2 = format!("{}", rand::random::<u32>());
    let random3 = format!("{}", rand::random::<u32>());

    let mut cmd = std::process::Command::new("cargo");
    cmd.arg("init");
    cmd.arg("--bin");
    cmd.arg(path.join("randomversion"));

    let _ = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output()
        .unwrap();

    let mut cmd = std::process::Command::new("cargo");
    cmd.arg("build");
    cmd.arg("--release");
    cmd.arg("--target");
    cmd.arg("wasm32-wasi");
    cmd.arg("--manifest-path");
    cmd.arg(path.join("randomversion").join("Cargo.toml"));

    let _ = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output()
        .unwrap();

    // generate the wasmer.toml
    let mut cmd = std::process::Command::new(get_wasmer_path());
    cmd.arg("init");
    cmd.arg("--namespace");
    cmd.arg(username);
    cmd.arg("--version");
    cmd.arg(format!("{random1}.{random2}.{random3}"));
    cmd.arg(path.join("randomversion"));

    let _ = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output()
        .unwrap();

    let s = std::fs::read_to_string(path.join("randomversion").join("wasmer.toml")).unwrap();

    println!("{s}");

    // publish
    let mut cmd = std::process::Command::new(get_wasmer_path());
    cmd.arg("publish");
    cmd.arg("--quiet");
    cmd.arg("--registry");
    cmd.arg("wapm.dev");
    cmd.arg(path.join("randomversion"));

    if let Some(token) = wapm_dev_token {
        cmd.arg("--token");
        cmd.arg(token);
    }

    let output = cmd.stdin(Stdio::null()).output().unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(stdout, format!("Successfully published package `{username}/randomversion@{random1}.{random2}.{random3}`\n"), "failed to publish: {cmd:?}: {stderr}");

    println!("wasmer init publish ok! test done.");

    Ok(())
}
