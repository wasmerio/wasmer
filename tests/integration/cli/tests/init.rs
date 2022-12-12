use anyhow::bail;
use std::path::PathBuf;
use std::process::{Command, Stdio};
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

    if std::env::var("GITHUB_TOKEN").is_err() {
        return Ok(());
    }
    let wapm_dev_token = std::env::var("WAPM_DEV_TOKEN").expect("WAPM_DEV_TOKEN env var not set");
    println!("wapm dev token ok...");

    let output = Command::new(get_wasmer_path())
        .arg("login")
        .arg("--registry")
        .arg("wapm.dev")
        .arg(wapm_dev_token)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .stdin(Stdio::null())
        .output()?;

    println!("wasmer login ok!");

    let output = Command::new(get_wasmer_path())
        .arg("init")
        .current_dir(&path)
        .output()?;
    check_output!(output);

    let read = std::fs::read_to_string(path.join("wasmer.toml"))
        .unwrap()
        .lines()
        .collect::<Vec<_>>()
        .join("\n");
    let target = include_str!("./fixtures/init1.toml")
        .lines()
        .collect::<Vec<_>>()
        .join("\n");
    pretty_assertions::assert_eq!(read.trim(), target.trim());
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
    std::fs::write(
        path.join("Cargo.toml"),
        include_bytes!("./fixtures/init2.toml"),
    )?;
    std::fs::create_dir_all(path.join("src"))?;
    std::fs::write(path.join("src").join("main.rs"), b"fn main() { }")?;

    if std::env::var("GITHUB_TOKEN").is_err() {
        return Ok(());
    }
    let wapm_dev_token = std::env::var("WAPM_DEV_TOKEN").expect("WAPM_DEV_TOKEN env var not set");
    println!("wapm dev token ok...");

    let output = Command::new(get_wasmer_path())
        .arg("login")
        .arg("--registry")
        .arg("wapm.dev")
        .arg(wapm_dev_token)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .stdin(Stdio::null())
        .output()?;

    println!("wasmer login ok!");

    let output = Command::new(get_wasmer_path())
        .arg("init")
        .current_dir(&path)
        .output()?;
    check_output!(output);

    let cargo_wapm_stdout = std::process::Command::new("cargo")
        .arg("wapm")
        .arg("--version")
        .output()
        .map(|s| String::from_utf8_lossy(&s.stdout).to_string())
        .unwrap_or_default();

    let cargo_wapm_present = cargo_wapm_stdout.lines().count() == 1
        && (cargo_wapm_stdout.contains("cargo wapm") || cargo_wapm_stdout.contains("cargo-wapm"));

    pretty_assertions::assert_eq!(
        std::fs::read_to_string(path.join("Cargo.toml")).unwrap(),
        include_str!("./fixtures/init2.toml")
    );
    let read = std::fs::read_to_string(path.join("wasmer.toml"))
        .unwrap()
        .lines()
        .collect::<Vec<_>>()
        .join("\n");
    let target = include_str!("./fixtures/init4.toml")
        .lines()
        .collect::<Vec<_>>()
        .join("\n");
    pretty_assertions::assert_eq!(read.trim(), target.trim());

    Ok(())
}
