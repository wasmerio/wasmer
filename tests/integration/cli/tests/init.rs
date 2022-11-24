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

    let read = std::fs::read_to_string(path.join("wapm.toml"))
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

// Test that wasmer init works with cargo wapm
#[cfg(not(target_os = "macos"))]
#[test]
fn wasmer_init_works_3() -> anyhow::Result<()> {
    println!("starting test...");
    // running test locally: should always pass since
    // developers don't have access to WAPM_DEV_TOKEN
    if std::env::var("GITHUB_TOKEN").is_err() {
        return Ok(());
    }
    let wapm_dev_token = std::env::var("WAPM_DEV_TOKEN").expect("WAPM_DEV_TOKEN env var not set");
    println!("wapm dev token ok...");

    let cargo_wapm_stdout = std::process::Command::new("cargo")
        .arg("wapm")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map(|s| String::from_utf8_lossy(&s.stdout).to_string())
        .unwrap_or_default();

    let cargo_wapm_present =
        cargo_wapm_stdout.lines().count() == 1 && cargo_wapm_stdout.contains("cargo wapm");

    if !cargo_wapm_present {
        println!("cargo wapm not present");

        // Install cargo wapm if not installed
        let output = Command::new("cargo")
            .arg("install")
            .arg("cargo-wapm")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()?;

        check_output!(output);
    }

    let tempdir = tempfile::tempdir()?;
    let path = tempdir.path();
    let path = path.join("testfirstproject");
    std::fs::create_dir_all(&path)?;
    std::fs::write(
        path.join("Cargo.toml"),
        include_str!("./fixtures/init5.toml")
            .replace("RANDOMVERSION1", &format!("{}", rand::random::<u32>()))
            .replace("RANDOMVERSION2", &format!("{}", rand::random::<u32>()))
            .replace("RANDOMVERSION3", &format!("{}", rand::random::<u32>())),
    )?;
    std::fs::create_dir_all(path.join("src"))?;
    std::fs::write(path.join("src").join("main.rs"), b"fn main() { }")?;

    println!("project created");

    let output = Command::new(get_wasmer_path())
        .arg("init")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .stdin(Stdio::null())
        .current_dir(&path)
        .output()?;
    check_output!(output);

    println!("wasmer init ok!");

    // login to wapm.dev, prepare for publish
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

    let output = Command::new("cargo")
        .arg("wapm")
        .arg("publish")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .stdin(Stdio::null())
        .current_dir(&path)
        .output()?;

    check_output!(output);

    println!("cargo wapm publish ok! test done.");

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

    let cargo_wapm_present =
        cargo_wapm_stdout.lines().count() == 1 && cargo_wapm_stdout.contains("cargo wapm");

    if cargo_wapm_present {
        assert!(!path.join("wapm.toml").exists());
        let read = std::fs::read_to_string(path.join("Cargo.toml"))
            .unwrap()
            .lines()
            .collect::<Vec<_>>()
            .join("\n");
        let target = include_str!("./fixtures/init3.toml")
            .lines()
            .collect::<Vec<_>>()
            .join("\n");
        pretty_assertions::assert_eq!(read.trim(), target.trim());

        // Install cargo wapm if not installed
        let output = Command::new("cargo")
            .arg("install")
            .arg("cargo-wapm")
            .current_dir(&path)
            .output()?;

        check_output!(output);
    } else {
        pretty_assertions::assert_eq!(
            std::fs::read_to_string(path.join("Cargo.toml")).unwrap(),
            include_str!("./fixtures/init2.toml")
        );
        let read = std::fs::read_to_string(path.join("wapm.toml"))
            .unwrap()
            .lines()
            .collect::<Vec<_>>()
            .join("\n");
        let target = include_str!("./fixtures/init4.toml")
            .lines()
            .collect::<Vec<_>>()
            .join("\n");
        pretty_assertions::assert_eq!(read.trim(), target.trim());
    }

    Ok(())
}
