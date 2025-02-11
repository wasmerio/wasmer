use assert_cmd::prelude::OutputAssertExt;
use predicates::str::contains;
use wasmer_integration_tests_cli::{fixtures, get_wasmer_path};

#[test]
fn wasmer_publish_bump() {
    let wapm_dev_token = std::env::var("WAPM_DEV_TOKEN").ok();
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path();
    let username = "ciuser";

    let random1 = format!("{}", rand::random::<u32>());
    let random2 = format!("{}", rand::random::<u32>());
    let random3 = format!("{}", rand::random::<u32>());

    std::fs::copy(fixtures::qjs(), path.join("largewasmfile.wasm")).unwrap();
    std::fs::write(
        path.join("wasmer.toml"),
        include_str!("./fixtures/init6.toml")
            .replace("WAPMUSERNAME", username) // <-- TODO!
            .replace("RANDOMVERSION1", &random1)
            .replace("RANDOMVERSION2", &random2)
            .replace("RANDOMVERSION3", &random3),
    )
    .unwrap();

    let mut cmd = std::process::Command::new(get_wasmer_path());
    cmd.arg("publish")
        .arg("--quiet")
        .arg("--bump")
        .arg("--registry=wasmer.wtf")
        .arg(path);

    if let Some(token) = wapm_dev_token {
        // Special case: GitHub secrets aren't visible to outside collaborators
        if token.is_empty() {
            return;
        }
        cmd.arg("--token").arg(token);
    }

    // What comes to mind is that we should check that the actual published version is
    // random1.random2.(random3 + 1), but if a higher version is already in the registry bumping
    // will actually bump the *other* version..
    cmd.assert()
        .success()
        .stderr(predicates::str::contains(format!(
            "wasmer.wtf/{username}/largewasmfile"
        )));
}

#[test]
fn wasmer_publish() {
    let wapm_dev_token = std::env::var("WAPM_DEV_TOKEN").ok();
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path();
    let username = "ciuser";

    let random1 = format!("{}", rand::random::<u32>());
    let random2 = format!("{}", rand::random::<u32>());
    let random3 = format!("{}", rand::random::<u32>());

    std::fs::copy(fixtures::qjs(), path.join("largewasmfile.wasm")).unwrap();
    std::fs::write(
        path.join("wasmer.toml"),
        include_str!("./fixtures/init6.toml")
            .replace("WAPMUSERNAME", username) // <-- TODO!
            .replace("RANDOMVERSION1", &random1)
            .replace("RANDOMVERSION2", &random2)
            .replace("RANDOMVERSION3", &random3),
    )
    .unwrap();

    let mut cmd = std::process::Command::new(get_wasmer_path());
    cmd.arg("publish")
        .arg("--quiet")
        .arg("--registry=wasmer.wtf")
        .arg(path);

    if let Some(token) = wapm_dev_token {
        // Special case: GitHub secrets aren't visible to outside collaborators
        if token.is_empty() {
            return;
        }
        cmd.arg("--token").arg(token);
    }

    cmd.assert()
        .success()
        .stderr(predicates::str::contains(format!(
            "wasmer.wtf/{username}/largewasmfile@{random1}.{random2}.{random3}"
        )));
}

// Runs a full integration test to test that the flow wasmer init - cargo build -
// wasmer publish is working
#[test]
fn wasmer_init_publish() {
    let wapm_dev_token = std::env::var("WAPM_DEV_TOKEN").ok();
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path();
    let username = "ciuser";

    let random1 = format!("{}", rand::random::<u32>());
    let random2 = format!("{}", rand::random::<u32>());
    let random3 = format!("{}", rand::random::<u32>());

    // Create a new Rust project and build it
    std::process::Command::new("cargo")
        .arg("init")
        .arg("--bin")
        .arg(path.join("randomversion"))
        .assert()
        .success();
    std::process::Command::new("cargo")
        .arg("build")
        .arg("--release")
        .arg("--target")
        .arg("wasm32-wasip1")
        .arg("--manifest-path")
        .arg(path.join("randomversion").join("Cargo.toml"))
        .assert()
        .success();

    // generate the wasmer.toml
    std::process::Command::new(get_wasmer_path())
        .arg("init")
        .arg("--namespace")
        .arg(username)
        .arg("--version")
        .arg(format!("{random1}.{random2}.{random3}"))
        .arg(path.join("randomversion"))
        .assert()
        .success();

    let _s = std::fs::read_to_string(path.join("randomversion").join("wasmer.toml")).unwrap();

    // publish
    let mut cmd = std::process::Command::new(get_wasmer_path());
    cmd.arg("publish")
        .arg("--quiet")
        .arg("--registry=wasmer.wtf")
        .arg(path.join("randomversion"));

    if let Some(token) = wapm_dev_token {
        // Special case: GitHub secrets aren't visible to outside collaborators
        if token.is_empty() {
            return;
        }
        cmd.arg("--token").arg(token);
    }

    let assert = cmd.assert();

    assert.success().stderr(predicates::str::contains(format!(
        "wasmer.wtf/{username}/randomversion@{random1}.{random2}.{random3}"
    )));
}

#[test]
fn wasmer_publish_and_run() {
    let wapm_dev_token = std::env::var("WAPM_DEV_TOKEN").ok();
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path();
    let username = "ciuser";

    let random_major = format!("{}", rand::random::<u32>());
    let random_minor = format!("{}", rand::random::<u32>());
    let random_patch = format!("{}", rand::random::<u32>());

    std::fs::copy(fixtures::qjs(), path.join("largewasmfile.wasm")).unwrap();
    std::fs::write(
        path.join("wasmer.toml"),
        include_str!("./fixtures/init6.toml")
            .replace("WAPMUSERNAME", username) // <-- TODO!
            .replace("RANDOMVERSION1", &random_major)
            .replace("RANDOMVERSION2", &random_minor)
            .replace("RANDOMVERSION3", &random_patch),
    )
    .unwrap();

    let package_name =
        format!("{username}/largewasmfile@{random_major}.{random_minor}.{random_patch}");

    let mut cmd = std::process::Command::new(get_wasmer_path());
    cmd.arg("publish")
        .arg("--quiet")
        .arg("--wait")
        .arg("--timeout=60s")
        .arg("--registry=wasmer.wtf")
        .arg(path);

    if let Some(token) = wapm_dev_token {
        // Special case: GitHub secrets aren't visible to outside collaborators
        if token.is_empty() {
            return;
        }
        cmd.arg("--token").arg(token);
    }

    cmd.assert()
        .success()
        .stderr(predicates::str::contains(format!(
            "wasmer.wtf/{package_name}"
        )));

    let assert = std::process::Command::new(get_wasmer_path())
        .arg("run")
        .arg(format!("https://wasmer.wtf/{package_name}"))
        .arg("--")
        .arg("--eval")
        .arg("console.log('Hello, World!')")
        .assert();

    assert.success().stdout(contains("Hello, World!"));
}
