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
