use assert_cmd::prelude::OutputAssertExt;
use std::{
    fs::OpenOptions,
    path::{Path, PathBuf},
};
use wasmer_integration_tests_cli::get_wasmer_path;

fn project_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
}

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    std::fs::create_dir_all(&dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            std::fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}

#[test]
fn wasmer_deploy_php() -> anyhow::Result<()> {
    // Only run this test in the CI
    // if std::env::var("GITHUB_TOKEN").is_err() {
    //     return Ok(());
    // }

    let wapm_dev_token = std::env::var("WAPM_DEV_TOKEN").ok();

    let username = "ciuser";
    let app_name = format!("ci-php-replica-{}", rand::random::<u32>());
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
        .arg("--quiet")
        .arg(format!("--app-name={app_name}"))
        .arg(format!("--owner={username}"))
        .arg(format!("--path={}", app_dir.display()))
        .arg("--registry=https://registry.wasmer.wtf/graphql");

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
        .stderr(predicates::boolean::AndPredicate::new(
            predicates::str::contains("Deployment complete"),
            predicates::str::contains(&app_url),
        ));

    let r = reqwest::blocking::Client::new();
    let r = r.get(app_url).query(&[("ci_rand", &random3)]).send()?;
    let r = r.text()?;

    assert!(r.contains(&random3));

    Ok(())
}

#[test]
fn wasmer_deploy_static_website() -> anyhow::Result<()> {
    // Only run this test in the CI
    //if std::env::var("GITHUB_TOKEN").is_err() {
    //    return Ok(());
    //}

    let wapm_dev_token = std::env::var("WAPM_DEV_TOKEN").ok();

    let username = "ciuser";
    let app_name = format!("ci-static-website-replica-{}", rand::random::<u32>());
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

    let mut cmd = std::process::Command::new("sed");
    cmd.arg("-r")
        .arg(format!("{}", src_app_dir.display()))
        .arg(format!("{}", app_dir.display()))
        .output()?;

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
        .arg("-vvvvv")
        .arg(format!("--app-name={app_name}"))
        .arg(format!("--owner={username}"))
        .arg(format!("--path={}", app_dir.display()))
        .arg("--registry=https://registry.wasmer.wtf/graphql");

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
        .stderr(predicates::boolean::AndPredicate::new(
            predicates::str::contains("Deployment complete"),
            predicates::str::contains(&app_url),
        ));

    let r = reqwest::blocking::Client::new();
    let r = r.get(app_url).send()?;
    let r = r.text()?;

    assert!(r.contains(&random3));

    Ok(())
}
