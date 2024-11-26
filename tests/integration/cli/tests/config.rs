use std::{fs::OpenOptions, io::Write, path::Path};

use assert_cmd::Command;
use predicates::str::contains;
use tempfile::TempDir;
use wasmer_integration_tests_cli::get_wasmer_path;

fn setup_wasmer_dir() -> TempDir {
    let temp = TempDir::new().unwrap();

    // The config path and the config contents themselves are manually crafted so that we don't
    // depend on the cli crate.
    //
    // Eventually, this part of the config shall live on a freestanding crate - perhaps added to
    // `wasmer-config`.
    let config_path = temp.path().join("wasmer.toml");

    let contents = r#"
telemetry_enabled = true
update_notifications_enabled = true

[registry]
active_registry = "https://registry.wasmer.io/graphql"

[[registry.tokens]]
registry = "https://registry.wasmer.io/graphql"

[proxy]
        "#;

    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&config_path)
        .unwrap();

    file.write_all(contents.as_bytes()).unwrap();

    temp
}

fn contains_path(path: impl AsRef<Path>) -> predicates::str::ContainsPredicate {
    let expected = path.as_ref().display().to_string();
    contains(expected)
}

fn wasmer_cmd(temp: &TempDir) -> Command {
    let mut cmd = Command::new(get_wasmer_path());
    cmd.env("WASMER_DIR", temp.path());
    cmd
}

#[test]
fn wasmer_config_multiget() {
    let temp = setup_wasmer_dir();
    let wasmer_dir = temp.path();

    let bin_path = wasmer_dir.join("bin");
    let include_path = wasmer_dir.join("include");
    let bin = bin_path.display().to_string();
    let include = format!("-I{}", include_path.display());

    wasmer_cmd(&temp)
        .arg("config")
        .arg("--bindir")
        .arg("--cflags")
        .env("WASMER_DIR", wasmer_dir)
        .assert()
        .success()
        .stdout(contains(bin))
        .stdout(contains(include));
}

#[test]
fn wasmer_config_conflicting_flags() {
    let temp = setup_wasmer_dir();

    let expected_1 = if cfg!(windows) {
        "Usage: wasmer.exe config --bindir --cflags"
    } else {
        "Usage: wasmer config --bindir --cflags"
    };

    wasmer_cmd(&temp)
        .arg("config")
        .arg("--bindir")
        .arg("--cflags")
        .arg("--pkg-config")
        .assert()
        .stderr(contains(
            "error: the argument '--bindir' cannot be used with '--pkg-config'",
        ))
        .stderr(contains(expected_1))
        .stderr(contains("For more information, try '--help'."));
}

#[test]
fn c_flags() {
    let temp = setup_wasmer_dir();
    let wasmer_dir = temp.path();

    wasmer_cmd(&temp)
        .arg("config")
        .arg("--bindir")
        .assert()
        .success()
        .stdout(contains_path(temp.path().join("bin")));

    wasmer_cmd(&temp)
        .arg("config")
        .arg("--cflags")
        .assert()
        .success()
        .stdout(contains(format!(
            "-I{}\n",
            wasmer_dir.join("include").display()
        )));

    wasmer_cmd(&temp)
        .arg("config")
        .arg("--includedir")
        .assert()
        .success()
        .stdout(contains_path(wasmer_dir.join("include")));

    wasmer_cmd(&temp)
        .arg("config")
        .arg("--libdir")
        .assert()
        .success()
        .stdout(contains_path(wasmer_dir.join("lib")));

    wasmer_cmd(&temp)
        .arg("config")
        .arg("--libs")
        .assert()
        .stdout(contains(format!(
            "-L{} -lwasmer\n",
            wasmer_dir.join("lib").display()
        )));

    wasmer_cmd(&temp)
        .arg("config")
        .arg("--prefix")
        .assert()
        .success()
        .stdout(contains_path(wasmer_dir));

    let output = wasmer_cmd(&temp)
        .arg("config")
        .arg("--pkg-config")
        .output()
        .unwrap();

    let pkg_config = vec![
        format!("prefix={}", wasmer_dir.display()),
        format!("exec_prefix={}", wasmer_dir.join("bin").display()),
        format!("includedir={}", wasmer_dir.join("include").display()),
        format!("libdir={}", wasmer_dir.join("lib").display()),
        format!(""),
        format!("Name: wasmer"),
        format!("Description: The Wasmer library for running WebAssembly"),
        format!("Version: {}", env!("CARGO_PKG_VERSION")),
        format!("Cflags: -I{}", wasmer_dir.join("include").display()),
        format!("Libs: -L{} -lwasmer", wasmer_dir.join("lib").display()),
    ]
    .join("\n");

    assert!(output.status.success());
    let stderr = std::str::from_utf8(&output.stdout)
        .unwrap()
        .replace("\r\n", "\n");
    assert_eq!(stderr.trim(), pkg_config.trim());

    wasmer_cmd(&temp)
        .arg("config")
        .arg("--config-path")
        .assert()
        .success()
        .stdout(contains_path(temp.path().join("wasmer.toml")));
}

#[test]
fn get_and_set_config_fields() -> anyhow::Result<()> {
    let temp = setup_wasmer_dir();

    // ---- config get

    let output = wasmer_cmd(&temp)
        .arg("config")
        .arg("get")
        .arg("registry.token")
        .output()?;

    let original_token = String::from_utf8_lossy(&output.stdout);

    let output = wasmer_cmd(&temp)
        .arg("config")
        .arg("set")
        .arg("registry.token")
        .arg("abc123")
        .output()?;

    assert_eq!(String::from_utf8_lossy(&output.stdout), "".to_string());

    let output = wasmer_cmd(&temp)
        .arg("config")
        .arg("get")
        .arg("registry.token")
        .output()?;

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "abc123\n".to_string()
    );

    let output = wasmer_cmd(&temp)
        .arg("config")
        .arg("set")
        .arg("registry.token")
        .arg(original_token.to_string().trim())
        .output()?;

    assert_eq!(String::from_utf8_lossy(&output.stdout), "".to_string());

    let output = wasmer_cmd(&temp)
        .arg("config")
        .arg("get")
        .arg("registry.token")
        .output()?;

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{}\n", original_token.to_string().trim())
    );

    let output = wasmer_cmd(&temp)
        .arg("config")
        .arg("get")
        .arg("registry.url")
        .output()?;

    let original_url = String::from_utf8_lossy(&output.stdout);

    let output = wasmer_cmd(&temp)
        .arg("config")
        .arg("set")
        .arg("registry.url")
        .arg("wasmer.wtf")
        .output()?;

    let output_str = String::from_utf8_lossy(&output.stdout);

    assert_eq!(output_str, "".to_string());

    let output = wasmer_cmd(&temp)
        .arg("config")
        .arg("get")
        .arg("registry.url")
        .output()?;

    let output_str = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        output_str,
        "https://registry.wasmer.wtf/graphql\n".to_string()
    );

    let output = wasmer_cmd(&temp)
        .arg("config")
        .arg("set")
        .arg("registry.url")
        .arg(original_url.to_string().trim())
        .output()?;

    let output_str = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output_str, "".to_string());

    let output = wasmer_cmd(&temp)
        .arg("config")
        .arg("get")
        .arg("registry.url")
        .output()?;

    let output_str = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output_str, original_url.to_string());

    let output = wasmer_cmd(&temp)
        .arg("config")
        .arg("get")
        .arg("telemetry.enabled")
        .output()?;

    let original_output = String::from_utf8_lossy(&output.stdout);

    let output = wasmer_cmd(&temp)
        .arg("config")
        .arg("set")
        .arg("telemetry.enabled")
        .arg("true")
        .output()?;

    assert_eq!(String::from_utf8_lossy(&output.stdout), "".to_string());

    let output = wasmer_cmd(&temp)
        .arg("config")
        .arg("get")
        .arg("telemetry.enabled")
        .output()?;

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "true\n".to_string()
    );

    let output = wasmer_cmd(&temp)
        .arg("config")
        .arg("set")
        .arg("telemetry.enabled")
        .arg(original_output.to_string().trim())
        .output()?;

    assert_eq!(String::from_utf8_lossy(&output.stdout), "".to_string());

    let output = wasmer_cmd(&temp)
        .arg("config")
        .arg("get")
        .arg("telemetry.enabled")
        .output()?;

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        original_output.to_string()
    );

    let output = wasmer_cmd(&temp)
        .arg("config")
        .arg("get")
        .arg("update-notifications.enabled")
        .output()?;

    let original_output = String::from_utf8_lossy(&output.stdout);

    let output = wasmer_cmd(&temp)
        .arg("config")
        .arg("set")
        .arg("update-notifications.enabled")
        .arg("true")
        .output()?;

    assert_eq!(String::from_utf8_lossy(&output.stdout), "".to_string());

    let output = wasmer_cmd(&temp)
        .arg("config")
        .arg("get")
        .arg("update-notifications.enabled")
        .output()?;

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "true\n".to_string()
    );

    let output = wasmer_cmd(&temp)
        .arg("config")
        .arg("set")
        .arg("update-notifications.enabled")
        .arg(original_output.to_string().trim())
        .output()?;

    assert_eq!(String::from_utf8_lossy(&output.stdout), "".to_string());

    let output = wasmer_cmd(&temp)
        .arg("config")
        .arg("get")
        .arg("update-notifications.enabled")
        .output()?;

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        original_output.to_string()
    );

    Ok(())
}
