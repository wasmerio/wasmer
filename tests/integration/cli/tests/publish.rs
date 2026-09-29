use assert_cmd::prelude::OutputAssertExt;
use wasmer_integration_tests_cli::{fixtures, wasmer_command};

#[test]
fn wasmer_publish_bump() {
    let ciuser_token = std::env::var("DEV_BACKEND_CIUSER_TOKEN").ok();
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path();
    let username = "ciuser";

    if let Some(token) = &ciuser_token {
        // Special case: GitHub secrets aren't visible to outside collaborators
        if token.is_empty() {
            return;
        }
    }

    // Use a unique per-run package name: `--bump` publishes `latest.patch + 1`
    // of the package, so concurrent CI runs sharing one package race for the
    // same version and lose with a duplicate-key error from the registry.
    let pkgname = format!("largewasmfile{}", rand::random::<u32>());

    let write_manifest = |description: &str| {
        std::fs::write(
            path.join("wasmer.toml"),
            include_str!("./fixtures/init6.toml")
                .replace("WAPMUSERNAME", username) // <-- TODO!
                .replace("PKGNAME", &pkgname)
                .replace("RANDOMVERSION1", "1")
                .replace("RANDOMVERSION2", "0")
                .replace("RANDOMVERSION3", "0")
                .replace("published from wasmer largewasmfile", description),
        )
        .unwrap();
    };

    let publish = |bump: bool| {
        let mut cmd = wasmer_command();
        cmd.arg("publish")
            .arg("--quiet")
            .arg("--registry=wasmer.wtf")
            .arg(path);
        if bump {
            cmd.arg("--bump");
        }
        if let Some(token) = &ciuser_token {
            cmd.arg("--token").arg(token);
        }
        cmd
    };

    std::fs::copy(fixtures::qjs(), path.join("largewasmfile.wasm")).unwrap();

    // Create the package at version 1.0.0.
    write_manifest("bump test, first release");
    publish(false)
        .assert()
        .success()
        .stderr(predicates::str::contains(format!(
            "wasmer.wtf/{username}/{pkgname}@1.0.0"
        )));

    // Republish new contents under the same version with `--bump`: the
    // registry already has 1.0.0, so this must publish 1.0.1.
    write_manifest("bump test, second release");
    publish(true)
        .assert()
        .success()
        .stderr(predicates::str::contains(format!(
            "wasmer.wtf/{username}/{pkgname}@1.0.1"
        )));
}

#[test]
fn wasmer_publish() {
    let ciuser_token = std::env::var("DEV_BACKEND_CIUSER_TOKEN").ok();
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
            .replace("PKGNAME", "largewasmfile")
            .replace("RANDOMVERSION1", &random1)
            .replace("RANDOMVERSION2", &random2)
            .replace("RANDOMVERSION3", &random3),
    )
    .unwrap();

    let mut cmd = wasmer_command();
    cmd.arg("publish")
        .arg("--quiet")
        .arg("--registry=wasmer.wtf")
        .arg(path);

    if let Some(token) = ciuser_token {
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
