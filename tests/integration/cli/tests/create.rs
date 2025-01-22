use assert_cmd::prelude::OutputAssertExt;
use wasmer_integration_tests_cli::get_wasmer_path;

#[test]
fn wasmer_create_package() -> anyhow::Result<()> {
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
        .arg("--package=wasmer/hello")
        .arg(format!("--dir={}", app_dir.display()))
        .arg("--non-interactive")
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
        .arg("--template=static-website")
        .arg(format!("--dir={}", app_dir.display()))
        .arg("--non-interactive")
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
    let got = std::fs::read_to_string(app_dir.join("app.yaml"))?;
    assert_eq!(got, want);

    let want = r#"[dependencies]
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
"#;
    let got = std::fs::read_to_string(app_dir.join("wasmer.toml"))?;
    assert_eq!(got, want);

    Ok(())
}
