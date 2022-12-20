use anyhow::bail;

use std::process::{Command, Stdio};
use wasmer_integration_tests_cli::get_wasmer_path;

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

    let wapm_dev_token = std::env::var("WAPM_DEV_TOKEN").ok();
    println!("wapm dev token ok...");

    if let Some(token) = wapm_dev_token {
        let output = Command::new(get_wasmer_path())
            .arg("login")
            .arg("--registry")
            .arg("wapm.dev")
            .arg(token)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .stdin(Stdio::null())
            .output()?;
        check_output!(output);
    }

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

    let wapm_dev_token = std::env::var("WAPM_DEV_TOKEN").ok();
    println!("wapm dev token ok...");

    if let Some(token) = wapm_dev_token.as_ref() {
        let mut cmd = Command::new(get_wasmer_path());
        cmd.arg("login");
        cmd.arg("--registry");
        cmd.arg("wapm.dev");
        cmd.arg(token);
        cmd.stdout(Stdio::inherit());
        cmd.stderr(Stdio::inherit());
        cmd.stdin(Stdio::null());
        let output = cmd.output()?;
        check_output!(output);
    }

    println!("wasmer login ok!");

    let output = Command::new(get_wasmer_path())
        .arg("init")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .current_dir(&path)
        .output()?;
    check_output!(output);

    pretty_assertions::assert_eq!(
        std::fs::read_to_string(path.join("Cargo.toml")).unwrap(),
        include_str!("./fixtures/init2.toml")
    );

    println!("ok 1");

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
