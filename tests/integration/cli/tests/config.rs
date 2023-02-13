use std::path::{Path, PathBuf};
use std::process::Command;
use wasmer_integration_tests_cli::{get_repo_root_path, get_wasmer_path};

fn get_wasmer_dir() -> Result<PathBuf, anyhow::Error> {
    if let Ok(s) = std::env::var("WASMER_DIR") {
        Ok(Path::new(&s).to_path_buf())
    } else if let Some(root_dir) = get_repo_root_path().and_then(|root| {
        if root.join("package").exists() {
            Some(root.join("package"))
        } else {
            None
        }
    }) {
        Ok(root_dir)
    } else {
        let home_dir = dirs::home_dir()
            .ok_or(anyhow::anyhow!("no home dir"))?
            .join(".wasmer");
        if home_dir.exists() {
            Ok(home_dir)
        } else {
            Err(anyhow::anyhow!("no .wasmer home dir"))
        }
    }
}

#[test]
fn wasmer_config_multiget() -> anyhow::Result<()> {
    let bin_path = get_wasmer_dir()?.join("bin");
    let include_path = get_wasmer_dir()?.join("include");

    let bin = format!("{}", bin_path.display());
    let include = format!("-I{}", include_path.display());

    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("--bindir")
        .arg("--cflags")
        .output()?;

    let lines = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .collect::<Vec<_>>();

    let expected = vec![bin, include];

    assert_eq!(lines, expected);

    Ok(())
}

#[test]
fn wasmer_config_error() -> anyhow::Result<()> {
    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("--bindir")
        .arg("--cflags")
        .arg("--pkg-config")
        .output()?;

    let lines = String::from_utf8_lossy(&output.stderr)
        .lines()
        .map(|s| s.trim().to_string())
        .collect::<Vec<_>>();
    #[cfg(not(windows))]
    let expected_1 = "wasmer config --bindir --cflags";
    #[cfg(windows)]
    let expected_1 = "wasmer.exe config --bindir --cflags";

    let expected = vec![
        "error: The argument '--bindir' cannot be used with '--pkg-config'",
        "",
        "USAGE:",
        expected_1,
        "",
        "For more information try --help",
    ];

    assert_eq!(lines, expected);

    Ok(())
}
#[test]
fn config_works() -> anyhow::Result<()> {
    let bindir = Command::new(get_wasmer_path())
        .arg("config")
        .arg("--bindir")
        .output()?;

    let bin_path = get_wasmer_dir()?.join("bin");
    assert_eq!(
        String::from_utf8(bindir.stdout).unwrap(),
        format!("{}\n", bin_path.display())
    );

    let bindir = Command::new(get_wasmer_path())
        .arg("config")
        .arg("--cflags")
        .output()?;

    let include_path = get_wasmer_dir()?.join("include");
    assert_eq!(
        String::from_utf8(bindir.stdout).unwrap(),
        format!("-I{}\n", include_path.display())
    );

    let bindir = Command::new(get_wasmer_path())
        .arg("config")
        .arg("--includedir")
        .output()?;

    let include_path = get_wasmer_dir()?.join("include");
    assert_eq!(
        String::from_utf8(bindir.stdout).unwrap(),
        format!("{}\n", include_path.display())
    );

    let bindir = Command::new(get_wasmer_path())
        .arg("config")
        .arg("--libdir")
        .output()?;

    let lib_path = get_wasmer_dir()?.join("lib");
    assert_eq!(
        String::from_utf8(bindir.stdout).unwrap(),
        format!("{}\n", lib_path.display())
    );

    let bindir = Command::new(get_wasmer_path())
        .arg("config")
        .arg("--libs")
        .output()?;

    let lib_path = get_wasmer_dir()?.join("lib");
    assert_eq!(
        String::from_utf8(bindir.stdout).unwrap(),
        format!("-L{} -lwasmer\n", lib_path.display())
    );

    let bindir = Command::new(get_wasmer_path())
        .arg("config")
        .arg("--prefix")
        .output()?;

    let wasmer_dir = get_wasmer_dir()?;
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

    let lines = String::from_utf8(bindir.stdout)
        .unwrap()
        .lines()
        .map(|s| s.trim().to_string())
        .collect::<Vec<_>>();

    assert_eq!(lines, args);

    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("--config-path")
        .output()?;

    let config_path = get_wasmer_dir()?.join("wasmer.toml");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{}\n", config_path.display())
    );

    // ---- config get

    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("get")
        .arg("registry.token")
        .output()?;

    let original_token = String::from_utf8_lossy(&output.stdout);

    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("set")
        .arg("registry.token")
        .arg("abc123")
        .output()?;

    assert_eq!(String::from_utf8_lossy(&output.stdout), "".to_string());

    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("get")
        .arg("registry.token")
        .output()?;

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "abc123\n".to_string()
    );

    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("set")
        .arg("registry.token")
        .arg(original_token.to_string().trim())
        .output()?;

    assert_eq!(String::from_utf8_lossy(&output.stdout), "".to_string());

    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("get")
        .arg("registry.token")
        .output()?;

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{}\n", original_token.to_string().trim())
    );

    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("get")
        .arg("registry.url")
        .output()?;

    let original_url = String::from_utf8_lossy(&output.stdout);

    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("set")
        .arg("registry.url")
        .arg("wapm.dev")
        .output()?;

    let output_str = String::from_utf8_lossy(&output.stdout);

    assert_eq!(output_str, "".to_string());

    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("get")
        .arg("registry.url")
        .output()?;

    let output_str = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        output_str,
        "https://registry.wapm.dev/graphql\n".to_string()
    );

    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("set")
        .arg("registry.url")
        .arg(original_url.to_string().trim())
        .output()?;

    let output_str = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output_str, "".to_string());

    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("get")
        .arg("registry.url")
        .output()?;

    let output_str = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output_str, original_url.to_string());

    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("get")
        .arg("telemetry.enabled")
        .output()?;

    let original_output = String::from_utf8_lossy(&output.stdout);

    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("set")
        .arg("telemetry.enabled")
        .arg("true")
        .output()?;

    assert_eq!(String::from_utf8_lossy(&output.stdout), "".to_string());

    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("get")
        .arg("telemetry.enabled")
        .output()?;

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "true\n".to_string()
    );

    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("set")
        .arg("telemetry.enabled")
        .arg(original_output.to_string().trim())
        .output()?;

    assert_eq!(String::from_utf8_lossy(&output.stdout), "".to_string());

    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("get")
        .arg("telemetry.enabled")
        .output()?;

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        original_output.to_string()
    );

    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("get")
        .arg("update-notifications.enabled")
        .output()?;

    let original_output = String::from_utf8_lossy(&output.stdout);

    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("set")
        .arg("update-notifications.enabled")
        .arg("true")
        .output()?;

    assert_eq!(String::from_utf8_lossy(&output.stdout), "".to_string());

    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("get")
        .arg("update-notifications.enabled")
        .output()?;

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "true\n".to_string()
    );

    let output = Command::new(get_wasmer_path())
        .arg("config")
        .arg("set")
        .arg("update-notifications.enabled")
        .arg(original_output.to_string().trim())
        .output()?;

    assert_eq!(String::from_utf8_lossy(&output.stdout), "".to_string());

    let output = Command::new(get_wasmer_path())
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
