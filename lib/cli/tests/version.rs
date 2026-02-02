use assert_cmd::cargo::cargo_bin_cmd;
use git_version::git_version;

const WASMER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[test]
fn short_version_string() {
    let version_number = format!("wasmer {WASMER_VERSION}");

    cargo_bin_cmd!("wasmer")
        .arg("--version")
        .assert()
        .success()
        .stdout(predicates::str::contains(&version_number));

    cargo_bin_cmd!("wasmer")
        .arg("-V")
        .assert()
        .success()
        .stdout(predicates::str::contains(&version_number));
}

#[test]
fn long_version_string() {
    let long_version_number = format!("wasmer {}", env!("CARGO_PKG_VERSION"),);
    let mut git_version = git_version!(
        args = [
            "--abbrev=40",
            "--always",
            "--dirty=-modified",
            "--exclude=*"
        ],
        fallback = "",
    )
    .to_string();
    if !git_version.is_empty() {
        git_version = format!("commit-hash: {git_version}");
    }
    let build_date = format!("commit-date: {}", env!("WASMER_BUILD_DATE"));

    cargo_bin_cmd!("wasmer")
        .arg("--version")
        .arg("--verbose")
        .assert()
        .success()
        .stdout(predicates::str::contains(&long_version_number))
        .stdout(predicates::str::contains(&git_version))
        .stdout(predicates::str::contains(&build_date))
        .stdout(predicates::str::contains("binary:"));

    cargo_bin_cmd!("wasmer")
        .arg("-Vv")
        .assert()
        .success()
        .stdout(predicates::str::contains(&long_version_number))
        .stdout(predicates::str::contains(&git_version))
        .stdout(predicates::str::contains(&build_date))
        .stdout(predicates::str::contains("binary:"));
}

#[test]
fn help_text_contains_version() {
    let version_number = format!("wasmer {WASMER_VERSION}");

    cargo_bin_cmd!("wasmer")
        .arg("-h")
        .assert()
        .success()
        .stdout(predicates::str::contains(&version_number));

    cargo_bin_cmd!("wasmer")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains(&version_number));
}
