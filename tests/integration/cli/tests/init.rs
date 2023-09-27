#[macro_use]
extern crate pretty_assertions;

use assert_cmd::prelude::OutputAssertExt;
use tempfile::TempDir;

use std::process::Command;
use wasmer_integration_tests_cli::get_wasmer_path;

// Test that wasmer init without arguments works
#[test]
fn wasmer_init_works_1() {
    let wasmer_dir = TempDir::new().unwrap();
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join("testfirstproject");
    std::fs::create_dir_all(&path).unwrap();

    Command::new(get_wasmer_path())
        .arg("init")
        .arg("--namespace=ciuser")
        .current_dir(&path)
        .env("WASMER_DIR", wasmer_dir.path())
        .assert()
        .success();

    assert_eq!(
        std::fs::read_to_string(path.join("wasmer.toml")).unwrap(),
        include_str!("./fixtures/init1.toml"),
    );
}

#[test]
fn wasmer_init_works_2() {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path();
    let path = path.join("testfirstproject");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(
        path.join("Cargo.toml"),
        include_bytes!("./fixtures/init2.toml"),
    )
    .unwrap();
    std::fs::create_dir_all(path.join("src")).unwrap();
    std::fs::write(path.join("src").join("main.rs"), b"fn main() { }").unwrap();

    Command::new(get_wasmer_path())
        .arg("init")
        .arg("--namespace=ciuser")
        .current_dir(&path)
        .assert()
        .success();

    assert_eq!(
        std::fs::read_to_string(path.join("Cargo.toml")).unwrap(),
        include_str!("./fixtures/init2.toml")
    );
    assert_eq!(
        std::fs::read_to_string(path.join("wasmer.toml")).unwrap(),
        include_str!("./fixtures/init4.toml")
    );
}
