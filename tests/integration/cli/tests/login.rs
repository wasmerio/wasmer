use assert_cmd::prelude::OutputAssertExt;
use predicates::str::contains;
use tempfile::TempDir;

use std::process::Command;
use wasmer_integration_tests_cli::get_wasmer_path;

#[test]
fn login_works() {
    let wasmer_dir = TempDir::new().unwrap();

    // running test locally: should always pass since
    // developers don't have access to WAPM_DEV_TOKEN
    if std::env::var("GITHUB_TOKEN").is_err() {
        return;
    }
    let wapm_dev_token = std::env::var("WAPM_DEV_TOKEN").expect("WAPM_DEV_TOKEN env var not set");
    // Special case: GitHub secrets aren't visible to outside collaborators
    if wapm_dev_token.is_empty() {
        return;
    }
    let assert = Command::new(get_wasmer_path())
        .arg("login")
        .arg("--registry=wasmer.wtf")
        .arg(wapm_dev_token)
        .env("WASMER_DIR", wasmer_dir.path())
        .assert();

    assert
        .success()
        .stdout(contains(r#"Login for Wasmer user "ciuser" saved"#));
}

#[test]
fn run_whoami_works() {
    let wasmer_dir = TempDir::new().unwrap();

    // running test locally: should always pass since
    // developers don't have access to WAPM_DEV_TOKEN
    if std::env::var("GITHUB_TOKEN").is_err() {
        return;
    }

    let ciuser_token = std::env::var("WAPM_DEV_TOKEN").expect("no CIUSER / WAPM_DEV_TOKEN token");
    // Special case: GitHub secrets aren't visible to outside collaborators
    if ciuser_token.is_empty() {
        return;
    }

    let assert = Command::new(get_wasmer_path())
        .arg("whoami")
        .arg("--registry=wasmer.wtf")
        .env("WASMER_TOKEN", &ciuser_token)
        .env("WASMER_DIR", wasmer_dir.path())
        .assert()
        .success();

    assert.stdout(
        "logged into registry \"https://registry.wasmer.wtf/graphql\" as user \"ciuser\"\n",
    );
}
