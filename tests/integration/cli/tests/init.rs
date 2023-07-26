#[macro_use]
extern crate pretty_assertions;

use assert_cmd::prelude::OutputAssertExt;
use tempfile::TempDir;

use std::process::Command;
use wasmer_integration_tests_cli::get_wasmer_path;

// Test that wasmer init without arguments works
#[test]
fn wasmer_init_works_1() -> anyhow::Result<()> {
    let wasmer_dir = TempDir::new()?;
    let tempdir = tempfile::tempdir()?;
    let path = tempdir.path().join("testfirstproject");
    std::fs::create_dir_all(&path)?;

    if std::env::var("GITHUB_TOKEN").is_err() {
        return Ok(());
    }

    let wapm_dev_token = std::env::var("WAPM_DEV_TOKEN").ok();
    println!("wapm dev token ok...");

    if let Some(token) = wapm_dev_token {
        // Special case: GitHub secrets aren't visible to outside collaborators
        if token.is_empty() {
            return Ok(());
        }
        Command::new(get_wasmer_path())
            .arg("login")
            .arg("--registry=wapm.dev")
            .arg(token)
            .env("WASMER_DIR", wasmer_dir.path())
            .assert()
            .success();
    }

    println!("wasmer login ok!");

    Command::new(get_wasmer_path())
        .arg("init")
        .current_dir(&path)
        .env("WASMER_DIR", wasmer_dir.path())
        .assert()
        .success();

    assert_eq!(
        std::fs::read_to_string(path.join("wasmer.toml")).unwrap(),
        include_str!("./fixtures/init1.toml"),
    );

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
        // Special case: GitHub secrets aren't visible to outside collaborators
        if token.is_empty() {
            return Ok(());
        }
        Command::new(get_wasmer_path())
            .arg("login")
            .arg("--registry=wapm.dev")
            .arg(token)
            .assert()
            .success();
    }

    println!("wasmer login ok!");

    Command::new(get_wasmer_path())
        .arg("init")
        .current_dir(&path)
        .assert()
        .success();

    assert_eq!(
        std::fs::read_to_string(path.join("Cargo.toml")).unwrap(),
        include_str!("./fixtures/init2.toml")
    );

    println!("ok 1");

    assert_eq!(
        std::fs::read_to_string(path.join("wasmer.toml")).unwrap(),
        include_str!("./fixtures/init4.toml")
    );

    Ok(())
}
