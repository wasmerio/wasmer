use assert_cmd::prelude::OutputAssertExt;
use wasmer_integration_tests_cli::{fixtures, wasmer_command};

#[test]
fn wasmer_publish_bump() {
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
            .replace("RANDOMVERSION1", &random1)
            .replace("RANDOMVERSION2", &random2)
            .replace("RANDOMVERSION3", &random3),
    )
    .unwrap();

    let mut cmd = wasmer_command();
    cmd.arg("publish")
        .arg("--quiet")
        .arg("--bump")
        .arg("--registry=wasmer.wtf")
        .arg(path);

    if let Some(token) = ciuser_token {
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

/// Building a package emits a `.webcm` sidecar, and publishing that sidecar
/// alone reproduces the package identity from it: the CI-from-a-webc workflow
/// (WARP-68), where publish takes name/version from the sidecar, not a
/// `wasmer.toml`.
#[test]
fn wasmer_publish_webcm_sidecar() {
    let ciuser_token = std::env::var("DEV_BACKEND_CIUSER_TOKEN").ok();
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path();
    let username = "ciuser";

    let random1 = format!("{}", rand::random::<u32>());
    let random2 = format!("{}", rand::random::<u32>());
    let random3 = format!("{}", rand::random::<u32>());
    let version = format!("{random1}.{random2}.{random3}");

    std::fs::copy(fixtures::qjs(), path.join("largewasmfile.wasm")).unwrap();
    std::fs::write(
        path.join("wasmer.toml"),
        include_str!("./fixtures/init6.toml")
            .replace("WAPMUSERNAME", username)
            .replace("RANDOMVERSION1", &random1)
            .replace("RANDOMVERSION2", &random2)
            .replace("RANDOMVERSION3", &random3),
    )
    .unwrap();

    // Build the package. This emits the `.webc` and its `.webcm` sidecar, and
    // needs no registry, so this half runs everywhere, covering the default
    // sidecar emission on its own.
    let webc = path.join("pkg.webc");
    let webcm = webc.with_extension("webcm");
    wasmer_command()
        .arg("package")
        .arg("build")
        .arg("--quiet")
        .arg("-o")
        .arg(&webc)
        .arg(path)
        .assert()
        .success();
    assert!(webc.is_file(), "build did not emit the .webc");
    assert!(webcm.is_file(), "build did not emit the .webcm sidecar");

    // Publishing needs the registry; without a token, the sidecar-emission
    // assertions above are all this test can cover.
    let token = match ciuser_token {
        // Special case: GitHub secrets aren't visible to outside collaborators.
        Some(token) if !token.is_empty() => token,
        _ => return,
    };

    wasmer_command()
        .arg("publish")
        .arg("--quiet")
        .arg("--registry=wasmer.wtf")
        .arg("--token")
        .arg(token)
        .arg(&webcm)
        .assert()
        .success()
        .stderr(predicates::str::contains(format!(
            "wasmer.wtf/{username}/largewasmfile@{version}"
        )));
}
