use assert_cmd::prelude::OutputAssertExt;
use predicates::str::contains;
use serde_json::Value;
use wasmer_integration_tests_cli::wasmer_command;

const USERNAME: &str = "ciuser";
const REGISTRY: &str = "wasmer.wtf";
const HOURLY_JOB: &str = "hourly-check";
const DAILY_JOB: &str = "daily-check";

#[test]
fn cron_commands_work_against_deployed_app() -> anyhow::Result<()> {
    if std::env::var("GITHUB_TOKEN").is_err() {
        return Ok(());
    }

    let ciuser_token = std::env::var("DEV_BACKEND_CIUSER_TOKEN")
        .expect("DEV_BACKEND_CIUSER_TOKEN env var not set");
    if ciuser_token.is_empty() {
        return Ok(());
    }

    let app_name = format!("ci-cron-{}", rand::random::<u32>());
    let app_ident = format!("{USERNAME}/{app_name}");
    let tempdir = tempfile::tempdir()?;
    let app_dir = tempdir.path();

    std::fs::write(
        app_dir.join("app.yaml"),
        format!(
            r#"kind: wasmer.io/App.v0
name: {app_name}
owner: {USERNAME}
package: wasmer/hello
jobs:
  - name: {HOURLY_JOB}
    trigger: "@hourly"
    action:
      fetch:
        path: /
  - name: {DAILY_JOB}
    trigger: "@daily"
    action:
      fetch:
        path: /
"#
        ),
    )?;

    wasmer_command()
        .arg("deploy")
        .arg("--non-interactive")
        .arg("--quiet")
        .arg(format!("--dir={}", app_dir.display()))
        .arg(format!("--registry={REGISTRY}"))
        .arg("--token")
        .arg(&ciuser_token)
        .assert()
        .success();

    let cleanup = AppCleanup {
        ident: app_ident.clone(),
        token: ciuser_token.clone(),
    };

    let list_output = wasmer_command()
        .arg("cron")
        .arg("list")
        .arg(&app_ident)
        .arg("--format=json")
        .arg(format!("--registry={REGISTRY}"))
        .arg("--token")
        .arg(&ciuser_token)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let cron_jobs: Value = serde_json::from_slice(&list_output)?;
    assert!(
        cron_jobs
            .as_array()
            .is_some_and(|jobs| jobs.iter().any(|job| job["name"] == HOURLY_JOB)
                && jobs.iter().any(|job| job["name"] == DAILY_JOB)),
        "cron list did not contain expected jobs: {}",
        String::from_utf8_lossy(&list_output)
    );

    wasmer_command()
        .arg("cron")
        .arg("get")
        .arg(&app_ident)
        .arg(HOURLY_JOB)
        .arg("--format=json")
        .arg(format!("--registry={REGISTRY}"))
        .arg("--token")
        .arg(&ciuser_token)
        .assert()
        .success()
        .stdout(contains(format!(r#""name": "{HOURLY_JOB}""#)));

    wasmer_command()
        .arg("cron")
        .arg("disable")
        .arg(&app_ident)
        .arg(HOURLY_JOB)
        .arg(format!("--registry={REGISTRY}"))
        .arg("--token")
        .arg(&ciuser_token)
        .assert()
        .success()
        .stderr(contains(format!("Cron job {HOURLY_JOB} is now disabled.")));

    wasmer_command()
        .arg("cron")
        .arg("get")
        .arg(&app_ident)
        .arg(HOURLY_JOB)
        .arg("--format=json")
        .arg(format!("--registry={REGISTRY}"))
        .arg("--token")
        .arg(&ciuser_token)
        .assert()
        .success()
        .stdout(contains(r#""enabled": false"#));

    wasmer_command()
        .arg("cron")
        .arg("enable")
        .arg(&app_ident)
        .arg(HOURLY_JOB)
        .arg(format!("--registry={REGISTRY}"))
        .arg("--token")
        .arg(&ciuser_token)
        .assert()
        .success()
        .stderr(contains(format!("Cron job {HOURLY_JOB} is now enabled.")));

    wasmer_command()
        .arg("cron")
        .arg("invocations")
        .arg(&app_ident)
        .arg(HOURLY_JOB)
        .arg("--all")
        .arg("--page-size=1")
        .arg("--format=json")
        .arg(format!("--registry={REGISTRY}"))
        .arg("--token")
        .arg(&ciuser_token)
        .output()
        .map(|output| {
            assert!(
                output.status.success(),
                "cron invocations failed: stdout: {}\nstderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );

            if output.stdout.is_empty() {
                assert!(
                    String::from_utf8_lossy(&output.stderr)
                        .contains(&format!("Cron job {HOURLY_JOB} has no invocations!")),
                    "cron invocations returned no JSON and no empty-invocations message: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            } else {
                let invocations: Value = serde_json::from_slice(&output.stdout)
                    .expect("cron invocations output should be valid JSON");
                assert!(
                    invocations.is_array(),
                    "cron invocations JSON should be an array: {}",
                    String::from_utf8_lossy(&output.stdout)
                );
            }
        })?;

    drop(cleanup);
    Ok(())
}

struct AppCleanup {
    ident: String,
    token: String,
}

impl Drop for AppCleanup {
    fn drop(&mut self) {
        let _ = wasmer_command()
            .arg("app")
            .arg("delete")
            .arg("--non-interactive")
            .arg(&self.ident)
            .arg(format!("--registry={REGISTRY}"))
            .arg("--token")
            .arg(&self.token)
            .output();
    }
}
