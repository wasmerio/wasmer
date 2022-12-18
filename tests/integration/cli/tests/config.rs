use anyhow::bail;
use std::path::{Path, PathBuf};
use std::process::Command;
use wasmer_integration_tests_cli::get_wasmer_path;

#[test]
fn config_works() -> anyhow::Result<()> {
    let bindir = Command::new(get_wasmer_path())
        .arg("config")
        .arg("--bindir")
        .output()?;

    let bin_path = Path::new(env!("WASMER_DIR")).join("bin");
    assert_eq!(
        String::from_utf8(bindir.stdout).unwrap(),
        format!("{}\n", bin_path.display())
    );

    let bindir = Command::new(get_wasmer_path())
        .arg("config")
        .arg("--cflags")
        .output()?;

    let include_path = Path::new(env!("WASMER_DIR")).join("include");
    assert_eq!(
        String::from_utf8(bindir.stdout).unwrap(),
        format!("-I{}\n", include_path.display())
    );

    let bindir = Command::new(get_wasmer_path())
        .arg("config")
        .arg("--includedir")
        .output()?;

    let include_path = Path::new(env!("WASMER_DIR")).join("include");
    assert_eq!(
        String::from_utf8(bindir.stdout).unwrap(),
        format!("{}\n", include_path.display())
    );

    let bindir = Command::new(get_wasmer_path())
        .arg("config")
        .arg("--libdir")
        .output()?;

    let lib_path = Path::new(env!("WASMER_DIR")).join("lib");
    assert_eq!(
        String::from_utf8(bindir.stdout).unwrap(),
        format!("{}\n", lib_path.display())
    );

    let bindir = Command::new(get_wasmer_path())
        .arg("config")
        .arg("--libs")
        .output()?;

    let lib_path = Path::new(env!("WASMER_DIR")).join("lib");
    assert_eq!(
        String::from_utf8(bindir.stdout).unwrap(),
        format!("-L{} -lwasmer\n", lib_path.display())
    );

    let bindir = Command::new(get_wasmer_path())
        .arg("config")
        .arg("--prefix")
        .output()?;

    let wasmer_dir = Path::new(env!("WASMER_DIR"));
    assert_eq!(
        String::from_utf8(bindir.stdout).unwrap(),
        format!("{}\n", wasmer_dir.display())
    );

    let bindir = Command::new(get_wasmer_path())
        .arg("config")
        .arg("--pkg-config")
        .output()?;

    let bin_path = format!("{}", bin_path.display());
    let include_path = format!("{}", include_path.display());
    let lib_path = format!("{}", lib_path.display());
    let wasmer_dir = format!("{}", wasmer_dir.display());

    let args = vec![
        format!("prefix={wasmer_dir}"),
        format!("exec_prefix={bin_path}"),
        format!("includedir={include_path}"),
        format!("libdir={lib_path}"),
        format!(""),
        format!("Name: wasmer"),
        format!("Description: The Wasmer library for running WebAssembly"),
        format!("Version: {}", env!("CARGO_PKG_VERSION")),
        format!("Cflags: -I{include_path}"),
        format!("Libs: -L{lib_path} -lwasmer"),
    ];

    let wasmer_dir = Path::new(env!("WASMER_DIR"));
    let lines = String::from_utf8(bindir.stdout)
        .unwrap()
        .lines()
        .map(|s| s.trim().to_string())
        .collect::<Vec<_>>();

    assert_eq!(lines, args);

    // ---- config get

    /*
        config get

        config.path                     Print the path to the configuration file
        proxy.url                       Print the proxy URL
        registry.token                  Print the token for the currently active registry or nothing
                                            if not logged in
        registry.url                    Print the registry URL of the currently active registry
        telemetry.enabled               Print whether telemetry is currently enabled
        update-notifications.enabled    Print whether update notifications are enabled
    */

    // ---- config set

    /*
        config set

        proxy.url                       `proxy.url`
        registry.token                  `registry.token`
        registry.url                    `registry.url`
        telemetry.enabled               `telemetry.enabled`
        update-notifications.enabled    `update-notifications.url`
    */

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
