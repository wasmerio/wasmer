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
fn wasmer_create_package() -> anyhow::Result<()> {
    // Only run this test in the CI
    if std::env::var("GITHUB_TOKEN").is_err() {
        return Ok(());
    }

    let wapm_dev_token = std::env::var("WAPM_DEV_TOKEN").ok();

    let username = "ciuser";
    let app_name = format!("ci-create-replica-{}", rand::random::<u32>());

    let tempdir = tempfile::tempdir()?;
    let app_dir = tempdir.path();

    let mut cmd = std::process::Command::new(get_wasmer_path());
    cmd.arg("app")
        .arg("create")
        .arg("--quiet")
        .arg(format!("--name={app_name}"))
        .arg(format!("--owner={username}"))
        .arg(format!("--package=wasmer/hello"))
        .arg(format!("--dir={}", app_dir.display()))
        .arg(format!("--non-interactive"))
        .arg("--registry=https://registry.wasmer.wtf/graphql");

    if let Some(token) = wapm_dev_token {
        // Special case: GitHub secrets aren't visible to outside collaborators
        if token.is_empty() {
            return Ok(());
        }
        cmd.arg("--token").arg(token);
    }

    cmd.assert().success();

    let want = format!(
        r#"kind: wasmer.io/App.v0
name: {app_name}
owner: {username}
package: wasmer/hello
"#
    );
    let got = std::fs::read_to_string(app_dir.join("app.yaml"))?;
    assert_eq!(got, want);

    Ok(())
}

#[test]
fn wasmer_create_template() -> anyhow::Result<()> {
    // Only run this test in the CI
    if std::env::var("GITHUB_TOKEN").is_err() {
        return Ok(());
    }

    let wapm_dev_token = std::env::var("WAPM_DEV_TOKEN").ok();

    let username = "ciuser";
    let app_name = format!("ci-create-replica-{}", rand::random::<u32>());

    let tempdir = tempfile::tempdir()?;
    let app_dir = tempdir.path();

    let mut cmd = std::process::Command::new(get_wasmer_path());
    cmd.arg("app")
        .arg("create")
        .arg("--quiet")
        .arg(format!("--name={app_name}"))
        .arg(format!("--owner={username}"))
        .arg(format!("--template=static-website"))
        .arg(format!("--dir={}", app_dir.display()))
        .arg(format!("--non-interactive"))
        .arg("--registry=https://registry.wasmer.wtf/graphql");

    if let Some(token) = wapm_dev_token {
        // Special case: GitHub secrets aren't visible to outside collaborators
        if token.is_empty() {
            return Ok(());
        }
        cmd.arg("--token").arg(token);
    }

    cmd.assert().success();

    let want = format!(
        r#"kind: wasmer.io/App.v0
package: .
name: {app_name}
owner: {username}
"#
    );
    let got = std::fs::read_to_string(app_dir.clone().join("app.yaml"))?;
    assert_eq!(got, want);

    let want = format!(
        r#"[dependencies]
"wasmer/static-web-server" = "^1"

[fs]
"/public" = "public"
"/settings" = "settings"

[[command]]
name = "script"
module = "wasmer/static-web-server:webserver"
runner = "https://webc.org/runner/wasi"

[command.annotations.wasi]
main-args = ["-w", "/settings/config.toml"]
"#
    );
    let got = std::fs::read_to_string(app_dir.join("wasmer.toml"))?;
    assert_eq!(got, want);

    Ok(())
}
