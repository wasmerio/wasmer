use anyhow::bail;
use std::path::PathBuf;
use std::process::Command;
use wasmer_integration_tests_cli::get_wasmer_path;

#[test]
fn config_works() -> anyhow::Result<()> {
    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("get")
        .arg("registry.url")
        .output()?;

    assert!(output.status.success());

    let registry_url = std::str::from_utf8(&output.stdout)
        .expect("stdout is not utf8! need to handle arbitrary bytes");

    println!("registry url {}", registry_url);

    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("set")
        .arg("registry.url")
        .arg("wapm.io")
        .output()?;

    assert!(output.status.success());

    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("set")
        .arg("registry.url")
        .arg(registry_url)
        .output()?;

    assert!(output.status.success());

    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("get")
        .arg("telemetry.enabled")
        .output()?;

    assert!(output.status.success());

    let telemetry_enabled = std::str::from_utf8(&output.stdout)
        .expect("stdout is not utf8! need to handle arbitrary bytes")
        .trim();

    println!("telemetry enabled {}", telemetry_enabled);

    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("set")
        .arg("telemetry.enabled")
        .arg("false")
        .output()?;

    assert!(output.status.success());

    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("set")
        .arg("telemetry.enabled")
        .arg(telemetry_enabled)
        .output()?;

    assert!(output.status.success());

    Ok(())
}
