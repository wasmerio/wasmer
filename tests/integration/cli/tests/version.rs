use anyhow::bail;
use std::process::Command;
use wasmer_integration_tests_cli::get_wasmer_path;

const WASMER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[test]
fn version_string_is_correct() -> anyhow::Result<()> {
    let expected_version_output = format!("wasmer {}\n", WASMER_VERSION);
    let wasmer_path = get_wasmer_path();

    let outputs = [
        Command::new(&wasmer_path).arg("--version").output()?,
        Command::new(&wasmer_path).arg("-V").output()?,
    ];

    for output in &outputs {
        if !output.status.success() {
            bail!(
                "version failed with: stdout: {}\n\nstderr: {}",
                std::str::from_utf8(&output.stdout)
                    .expect("stdout is not utf8! need to handle arbitrary bytes"),
                std::str::from_utf8(&output.stderr)
                    .expect("stderr is not utf8! need to handle arbitrary bytes")
            );
        }

        let stdout_output = std::str::from_utf8(&output.stdout).unwrap();
        assert_eq!(stdout_output, &expected_version_output);
    }

    Ok(())
}

#[test]
fn help_text_contains_version() -> anyhow::Result<()> {
    let expected_version_output = format!("wasmer {}", WASMER_VERSION);
    let wasmer_path = get_wasmer_path();

    let outputs = [
        Command::new(&wasmer_path).arg("--help").output()?,
        Command::new(&wasmer_path).arg("-h").output()?,
    ];

    for output in &outputs {
        if !output.status.success() {
            bail!(
                "version failed with: stdout: {}\n\nstderr: {}",
                std::str::from_utf8(&output.stdout)
                    .expect("stdout is not utf8! need to handle arbitrary bytes"),
                std::str::from_utf8(&output.stderr)
                    .expect("stderr is not utf8! need to handle arbitrary bytes")
            );
        }

        let stdout_output = std::str::from_utf8(&output.stdout).unwrap();
        assert_eq!(
            stdout_output.lines().next().unwrap(),
            &expected_version_output
        );
    }

    Ok(())
}
