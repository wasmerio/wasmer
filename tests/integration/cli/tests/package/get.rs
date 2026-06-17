use assert_cmd::prelude::OutputAssertExt;
use predicates::str::contains;
use wasmer_integration_tests_cli::wasmer_command;

#[test]
fn package_get_named() {
    wasmer_command()
        .arg("package")
        .arg("get")
        .arg("wasmer/cli@=0.1.3")
        .assert()
        .success()
        .stdout(contains("Name:"))
        .stdout(contains("wasmer/cli"))
        .stdout(contains("Version:"))
        .stdout(contains("0.1.3"))
        .stdout(contains(
            "Description:  CLI platform - a wrapper package with many common tools, useful for interactive environments.",
        ))
        .stdout(contains("URL:"))
        .stdout(contains("https://wasmer.io/wasmer/cli@0.1.3"));
}

// A missing version reports the request as unmatched and lists what's available
// (`0.1.3` exists permanently, so the assertion stays stable).
#[test]
fn package_get_missing_version() {
    wasmer_command()
        .arg("package")
        .arg("get")
        .arg("wasmer/cli@9999.0.0")
        .assert()
        .failure()
        .stderr(contains("has no version matching"))
        .stderr(contains("Available versions:"))
        .stderr(contains("0.1.3"));
}
