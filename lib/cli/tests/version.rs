use assert_cmd::Command;

const WASMER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[test]
fn short_version_string() {
    let version_number = format!("wasmer {WASMER_VERSION}");

    Command::cargo_bin("wasmer")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicates::str::contains(&version_number));

    Command::cargo_bin("wasmer")
        .unwrap()
        .arg("-V")
        .assert()
        .success()
        .stdout(predicates::str::contains(&version_number));
}

#[test]
fn long_version_string() {
    let long_version_number = format!(
        "wasmer {} ({} {})",
        env!("CARGO_PKG_VERSION"),
        env!("WASMER_BUILD_GIT_HASH_SHORT"),
        env!("WASMER_BUILD_DATE")
    );

    Command::cargo_bin("wasmer")
        .unwrap()
        .arg("--version")
        .arg("--verbose")
        .assert()
        .success()
        .stdout(predicates::str::contains(&long_version_number))
        .stdout(predicates::str::contains("binary:"));

    Command::cargo_bin("wasmer")
        .unwrap()
        .arg("-Vv")
        .assert()
        .success()
        .stdout(predicates::str::contains(&long_version_number))
        .stdout(predicates::str::contains("binary:"));
}

#[test]
fn help_text_contains_version() {
    let version_number = format!("wasmer {WASMER_VERSION}");

    Command::cargo_bin("wasmer")
        .unwrap()
        .arg("-h")
        .assert()
        .success()
        .stdout(predicates::str::contains(&version_number));

    Command::cargo_bin("wasmer")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains(&version_number));
}
