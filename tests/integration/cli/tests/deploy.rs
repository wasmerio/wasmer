use assert_cmd::prelude::OutputAssertExt;
use std::{fs::OpenOptions, path::Path};
use wasmer_integration_tests_cli::get_wasmer_path;

fn project_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
}

#[test]
fn wasmer_deploy_php() -> anyhow::Result<()> {
    let wapm_dev_token = std::env::var("WAPM_DEV_TOKEN").ok();

    let username = "ciuser";
    let app_name = format!("ci-{}", rand::random::<u32>());
    let random3 = format!("{}", rand::random::<u32>());

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
        .arg(format!("--owner={username}"))
        .arg(format!("--dir={}", app_dir.display()))
        .arg("--registry=wasmer.wtf");

    if let Some(token) = wapm_dev_token {
        // Special case: GitHub secrets aren't visible to outside collaborators
        if token.is_empty() {
            return Ok(());
        }
        cmd.arg("--token").arg(token);
    }

    let app_url = format!("https://{app_name}-{username}.wasmer.dev");

    cmd.assert()
        .success()
        .stderr(predicates::str::contains("Deployment complete"));

    let r = reqwest::blocking::Client::new();
    let r = r.get(app_url).query(&[("ci_rand", &random3)]).send()?;
    let r = r.text()?;

    assert!(r.contains(&random3));

    Ok(())
}

#[test]
fn wasmer_deploy_static_website() -> anyhow::Result<()> {
    let wapm_dev_token = std::env::var("WAPM_DEV_TOKEN").ok();

    let username = "ciuser";
    let app_name = format!("ci-{}", rand::random::<u32>());
    let random3 = format!("{}", rand::random::<u32>());

    let src_app_dir = project_root()
        .join("tests")
        .join("integration")
        .join("cli")
        .join("tests")
        .join("packages")
        .join("static_website");

    let tempdir = tempfile::tempdir()?;
    let app_dir = tempdir.path();

    let mut cmd = std::process::Command::new("cp");
    cmd.arg("-r")
        .arg(format!("{}", src_app_dir.display()))
        .arg(format!("{}", app_dir.display()))
        .output()?;

    let app_dir = app_dir.join("static_website");

    let index_file_path = app_dir.join("public").join("index.html");
    let contents = std::fs::read_to_string(&index_file_path)?;
    let new = contents.replace("RANDOM_NUMBER", &random3);

    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(index_file_path)?;

    std::io::Write::write(&mut file, new.as_bytes())?;

    let mut cmd = std::process::Command::new(get_wasmer_path());
    cmd.arg("deploy")
        // .arg("--quiet")
        .arg("--non-interactive")
        .arg("-vvvvvv")
        .arg(format!("--app-name={app_name}"))
        .arg(format!("--owner={username}"))
        .arg(format!("--dir={}", app_dir.display()))
        .arg("--registry=wasmer.wtf");

    if let Some(token) = wapm_dev_token {
        // Special case: GitHub secrets aren't visible to outside collaborators
        if token.is_empty() {
            return Ok(());
        }
        cmd.arg("--token").arg(token);
    }

    let app_url = format!("https://{app_name}-{username}.wasmer.dev");

    cmd.assert()
        .success()
        .stderr(predicates::str::contains("Deployment complete"));

    let r = reqwest::blocking::Client::new();
    let r = r.get(app_url).send()?;
    let r = r.text()?;

    assert!(r.contains(&random3));

    Ok(())
}

#[test]
fn wasmer_deploy_js() -> anyhow::Result<()> {
    let wapm_dev_token = std::env::var("WAPM_DEV_TOKEN").ok();

    let username = "ciuser";
    let app_name = format!("ci-{}", rand::random::<u32>());
    let random3 = format!("{}", rand::random::<u32>());

    let src_app_dir = project_root()
        .join("tests")
        .join("integration")
        .join("cli")
        .join("tests")
        .join("packages")
        .join("js");

    let tempdir = tempfile::tempdir()?;
    let app_dir = tempdir.path();

    let mut cmd = std::process::Command::new("cp");
    cmd.arg("-r")
        .arg(format!("{}", src_app_dir.display()))
        .arg(format!("{}", app_dir.display()))
        .output()?;

    let app_dir = app_dir.join("js");

    let mut cmd = std::process::Command::new(get_wasmer_path());
    cmd.arg("deploy")
        // .arg("--quiet")
        .arg("--non-interactive")
        .arg("-vvvvvv")
        .arg(format!("--app-name={app_name}"))
        .arg(format!("--owner={username}"))
        .arg(format!("--dir={}", app_dir.display()))
        .arg("--registry=wasmer.wtf");

    if let Some(token) = wapm_dev_token {
        // Special case: GitHub secrets aren't visible to outside collaborators
        if token.is_empty() {
            return Ok(());
        }
        cmd.arg("--token").arg(token);
    }

    let app_url = format!("https://{app_name}-{username}.wasmer.dev");

    cmd.assert()
        .success()
        .stderr(predicates::str::contains("Deployment complete"));

    let r = reqwest::blocking::Client::new();
    let r = r.get(app_url).query(&[("ci_rand", &random3)]).send()?;
    let r = r.text()?;

    assert!(r.contains(&random3));

    Ok(())
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

#[test]
fn wasmer_deploy_axum() -> anyhow::Result<()> {
    let wapm_dev_token = std::env::var("WAPM_DEV_TOKEN").ok();

    let username = "ciuser";
    let app_name = format!("ci-{}", rand::random::<u32>());
    let random3 = format!("{}", rand::random::<u32>());

    let src_app_dir = project_root()
        .join("tests")
        .join("integration")
        .join("cli")
        .join("tests")
        .join("packages")
        .join("axum");

    let tempdir = tempfile::tempdir()?;
    let app_dir = tempdir.path();

    let mut cmd = std::process::Command::new("cp");
    cmd.arg("-r")
        .arg(format!("{}", src_app_dir.display()))
        .arg(format!("{}", app_dir.display()))
        .output()?;

    let app_dir = app_dir.join("axum");

    std::env::set_current_dir(&app_dir)?;

    let mut cmd = std::process::Command::new("cargo");
    cmd.arg("wasix").arg("build").output()?;

    let mut cmd = std::process::Command::new(get_wasmer_path());
    cmd.arg("deploy")
        // .arg("--quiet")
        .arg("--non-interactive")
        .arg("-vvvvvv")
        .arg(format!("--app-name={app_name}"))
        .arg(format!("--owner={username}"))
        .arg(format!("--dir={}", app_dir.display()))
        .arg("--registry=wasmer.wtf");

    if let Some(token) = wapm_dev_token {
        // Special case: GitHub secrets aren't visible to outside collaborators
        if token.is_empty() {
            return Ok(());
        }
        cmd.arg("--token").arg(token);
    }

    let app_url = format!("https://{app_name}-{username}.wasmer.dev");

    cmd.assert()
        .success()
        .stderr(predicates::str::contains("Deployment complete"));

    let r = reqwest::blocking::Client::new();
    let r = r.get(app_url).query(&[("ci_rand", &random3)]).send()?;
    let r = r.text()?;

    assert!(r.contains(&random3));

    Ok(())
}
