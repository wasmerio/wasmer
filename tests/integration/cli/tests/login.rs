use anyhow::bail;

use std::process::Command;
use wasmer_integration_tests_cli::get_wasmer_path;

#[test]
fn login_works() -> anyhow::Result<()> {
    // running test locally: should always pass since
    // developers don't have access to WAPM_DEV_TOKEN
    if std::env::var("GITHUB_TOKEN").is_err() {
        return Ok(());
    }
    let wapm_dev_token = std::env::var("WAPM_DEV_TOKEN").expect("WAPM_DEV_TOKEN env var not set");
    // Special case: GitHub secrets aren't visible to outside collaborators
    if wapm_dev_token.is_empty() {
        return Ok(());
    }
    let output = Command::new(get_wasmer_path())
        .arg("login")
        .arg("--registry")
        .arg("wapm.dev")
        .arg(wapm_dev_token)
        .output()?;

    let stdout = std::str::from_utf8(&output.stdout)
        .expect("stdout is not utf8! need to handle arbitrary bytes");

    let stderr = std::str::from_utf8(&output.stderr)
        .expect("stderr is not utf8! need to handle arbitrary bytes");

    if !output.status.success() {
        bail!(
            "wasmer login failed with: stdout: {}\n\nstderr: {}",
            stdout,
            stderr
        );
    }

    let stdout_output = std::str::from_utf8(&output.stdout).unwrap();
    let expected = "Login for WAPM user \"ciuser\" saved\n";
    if stdout_output != expected {
        println!("expected:");
        println!("{expected}");
        println!("got:");
        println!("{stdout}");
        println!("-----");
        println!("{stderr}");
        panic!("stdout incorrect");
    }

    Ok(())
}
