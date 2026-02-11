use assert_cmd::prelude::OutputAssertExt;
use wasmer_integration_tests_cli::{fixtures, get_wasmer_path};

#[test]
fn wasmer_publish_bump() {
    let wapm_dev_token = std::env::var("DEV_BACKEND_CIUSER_TOKEN").ok();
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
    let wapm_dev_token = std::env::var("DEV_BACKEND_CIUSER_TOKEN").ok();
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
