use insta::assert_json_snapshot;
use wasmer_integration_tests_cli::wasmer_command;

fn package_search(args: &[&str]) -> std::process::Output {
    wasmer_command()
        .env_remove("WASMER_TOKEN")
        .arg("package")
        .arg("search")
        .arg("--registry=wasmer.io")
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn package_search_against_backend_json() {
    let output = package_search(&[
        "-f",
        "json",
        "--max",
        "1",
        "--curated",
        "--order-by",
        "alphabetically",
        "--sort",
        "asc",
        "rustpython",
    ]);

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let packages: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    let package = packages
        .iter()
        .find(|value| {
            value["package"]["namespace"] == "rustpython"
                && value["package"]["package_name"] == "rustpython"
        })
        .expect("expected the backend to return rustpython/rustpython");

    assert_eq!(package["package"]["namespace"], "rustpython");
    assert_eq!(package["package"]["package_name"], "rustpython");
    assert_eq!(package["package"]["private"], false);
    assert!(package["id"].is_string());
    assert!(package["package"]["id"].is_string());
    assert!(package["version"].is_string());
    assert!(package["created_at"].is_string());
}

#[test]
fn package_search_against_backend_no_results_table() {
    let output = package_search(&["definitely-no-package-with-this-name-000000"]);

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let normalized = serde_json::json!({
        "stdout": String::from_utf8(output.stdout).unwrap(),
        "stderr": String::from_utf8(output.stderr).unwrap(),
    });
    assert_json_snapshot!(normalized);
}
