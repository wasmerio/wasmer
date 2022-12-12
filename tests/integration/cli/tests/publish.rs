use anyhow::bail;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use wasmer_integration_tests_cli::{get_repo_root_path, get_wasmer_path, ASSET_PATH, C_ASSET_PATH};

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
    cmd.arg("--dir");
    cmd.arg(path);
    cmd.arg("--quiet");
    cmd.arg("--registry");
    cmd.arg("wapm.dev");

    if let Some(token) = wapm_dev_token {
        cmd.arg("--token");
        cmd.arg(token);
    }

    let output = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(stdout, format!("Successfully published package `{username}/largewasmfile@{random1}.{random2}.{random3}`\n"), "failed to publish: {cmd:?}: {stderr}");

    println!("wasmer publish ok! test done.");

    Ok(())
}
