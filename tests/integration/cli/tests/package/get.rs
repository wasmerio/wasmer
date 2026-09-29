use insta::assert_snapshot;
use wasmer_integration_tests_cli::wasmer_command;

#[test]
fn package_get_named() {
    let output = wasmer_command()
        .arg("package")
        .arg("get")
        .arg("wasmer/cli@=0.1.3")
        .env("RUST_LOG", "off")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "command failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_snapshot!(String::from_utf8_lossy(&output.stdout));
}

#[test]
fn package_get_missing_version() {
    let output = wasmer_command()
        .arg("package")
        .arg("get")
        .arg("wasmer/cli@9999.0.0")
        .env("RUST_LOG", "off")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();

    assert!(!output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);

    // The list of available versions is subject to change,
    // so in order to avoid snapshot churn we mask it out.
    // As a sanity check however, we always check for a single known one.
    assert!(
        stderr.contains("0.1.3"),
        "expected available versions to include 0.1.3:\n{stderr}"
    );

    let masked = stderr
        .lines()
        .map(|line| {
            if line.starts_with("Available versions:") {
                "Available versions: [masked]"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert_snapshot!(masked);
}
