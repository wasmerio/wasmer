use anyhow::bail;
use std::path::PathBuf;
use std::process::Command;
use wasmer_integration_tests_cli::{get_repo_root_path, get_wasmer_path, ASSET_PATH, C_ASSET_PATH};

macro_rules! check_output {
    ($output:expr) => {
        let stdout_output = std::str::from_utf8(&$output.stdout).unwrap();
        let stderr_output = std::str::from_utf8(&$output.stdout).unwrap();
        if !$output.status.success() {
            bail!("wasmer init failed with: stdout: {stdout_output}\n\nstderr: {stderr_output}");
        }
    };
}

// Test that wasmer init without arguments works
#[test]
fn wasmer_init_works_1() -> anyhow::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let path = tempdir.path();
    let path = path.join("testfirstproject");
    std::fs::create_dir_all(&path)?;
    let output = Command::new(get_wasmer_path())
        .arg("init")
        .current_dir(&path)
        .output()?;
    check_output!(output);

    let read = std::fs::read_to_string(path.join("wapm.toml")).unwrap().lines().collect::<Vec<_>>().join("\n");
    let target = include_str!("./fixtures/init1.toml").lines().collect::<Vec<_>>().join("\n");
    if read.trim() != target.trim() {
        println!("expected:");
        println!("{target}");
        println!("got:");
        println!("{read}");
        panic!("read != target");
    }
    Ok(())
}

// Test that wasmer init adds to a Cargo.toml 
// instead of creating a new wapm.toml
#[test]
fn wasmer_init_works_2() -> anyhow::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let path = tempdir.path();
    let path = path.join("testfirstproject");
    std::fs::create_dir_all(&path)?;
    std::fs::write(path.join("Cargo.toml"), include_bytes!("./fixtures/init2.toml"));
    let output = Command::new(get_wasmer_path())
        .arg("init")
        .current_dir(&path)
        .output()?;
    check_output!(output);

    assert!(!path.join("wapm.toml").exists());
    let read = std::fs::read_to_string(path.join("Cargo.toml")).unwrap().lines().collect::<Vec<_>>().join("\n");
    let target = include_str!("./fixtures/init3.toml").lines().collect::<Vec<_>>().join("\n");
    if read.trim() != target.trim() {
        println!("expected:");
        println!("{target}");
        println!("got:");
        println!("{read}");
        panic!("read != target");
    }
    Ok(())
}
