use assert_cmd::prelude::OutputAssertExt;
use std::path::Path;
use wasmer_integration_tests_cli::get_wasmer_path;

fn project_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
}

#[test]
fn wasmer_deploy_fails_no_app_name() -> anyhow::Result<()> {
    let username = "ciuser";

    let php_app_dir = project_root()
        .join("tests")
        .join("integration")
        .join("cli")
        .join("tests")
        .join("packages")
        .join("php");

    let tempdir = tempfile::tempdir()?;
    let app_dir = tempdir.path();

    let mut cmd = std::process::Command::new("cp");
    cmd.arg("-r")
        .arg(format!("{}", php_app_dir.display()))
        .arg(format!("{}", app_dir.display()))
        .output()?;

    let app_dir = app_dir.join("php");

    let mut cmd = std::process::Command::new(get_wasmer_path());
    cmd.arg("deploy")
        .arg("--non-interactive")
        .arg("-vvvvvv")
        .arg(format!("--owner={username}"))
        .arg(format!("--dir={}", app_dir.display()))
        .arg("--registry=wasmer.wtf");

    cmd.assert().failure().stderr(predicates::str::contains(
        "The app.yaml does not specify any app name.",
    ));

    Ok(())
}

#[test]
fn wasmer_deploy_fails_no_owner() -> anyhow::Result<()> {
    let app_name = format!("ci-{}", rand::random::<u32>());

    let php_app_dir = project_root()
        .join("tests")
        .join("integration")
        .join("cli")
        .join("tests")
        .join("packages")
        .join("php");

    let tempdir = tempfile::tempdir()?;
    let app_dir = tempdir.path();

    let mut cmd = std::process::Command::new("cp");
    cmd.arg("-r")
        .arg(format!("{}", php_app_dir.display()))
        .arg(format!("{}", app_dir.display()))
        .output()?;

    let app_dir = app_dir.join("php");

    let mut cmd = std::process::Command::new(get_wasmer_path());
    cmd.arg("deploy")
        .arg("--non-interactive")
        .arg("-vvvvvv")
        .arg(format!("--app-name={app_name}"))
        .arg(format!("--dir={}", app_dir.display()))
        .arg("--registry=wasmer.wtf");

    cmd.assert()
        .failure()
        .stderr(predicates::str::contains("No owner specified"));

    Ok(())
}
