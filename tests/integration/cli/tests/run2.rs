use std::time::{Duration, Instant};

use assert_cmd::{assert::Assert, prelude::OutputAssertExt, Command};
use reqwest::{blocking::Client, IntoUrl};
use tempfile::TempDir;
use wasmer_integration_tests_cli::get_wasmer_path;

fn wasmer_cli() -> Command {
    Command::new(get_wasmer_path())
}

mod webc_on_disk {
    use std::process::Stdio;

    use rand::Rng;

    use super::*;

    #[test]
    fn wasi_runner() {
        let assert = wasmer_cli()
            .arg("run2")
            .arg(fixtures::python())
            .arg("--")
            .arg("--version")
            .assert();

        assert
            .success()
            .stdout(predicates::str::contains("Python 3.6.7"));
    }

    #[test]
    #[ignore]
    fn wasi_runner_with_mounted_directories() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("main.py"), "print('Hello, World!')").unwrap();

        let assert = wasmer_cli()
            .arg("run2")
            .arg(fixtures::python())
            .arg("--mapdir")
            .arg(format!("/app:{}", temp.path().display()))
            .arg("--")
            .arg("/app/main.py")
            .assert();

        assert
            .success()
            .stdout(predicates::str::contains("Hello, World!"));
    }

    #[test]
    #[ignore]
    fn wasi_runner_with_env_vars() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("main.py"), "print('Hello, World!')").unwrap();

        let assert = wasmer_cli()
            .arg("run2")
            .arg(fixtures::python())
            .arg("--env")
            .arg("SOME_VAR=Hello, World!")
            .arg("--")
            .arg("-c")
            .arg("import os; print(os.environ['SOME_VAR'])")
            .assert();

        assert
            .success()
            .stdout(predicates::str::contains("Hello, World!"));
    }

    #[test]
    fn wcgi_runner() {
        // Start the WCGI server in the background
        let port = rand::thread_rng().gen_range(10_000_u16..u16::MAX);
        let mut cmd = std::process::Command::new(get_wasmer_path());
        cmd.arg("run2")
            .env("RUST_LOG", "info,wasmer_wasi::runners=debug")
            .arg(format!("--addr=127.0.0.1:{port}"))
            .arg(fixtures::static_server());
        let child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map(Child::new)
            .unwrap();

        // make the request
        let body = http_get(format!("http://127.0.0.1:{port}/"));
        assert!(body.contains("<title>Index of /</title>"), "{body}");

        // And kill the server, making sure it generated the expected logs
        let assert = child.join();

        assert
            .stdout(predicates::str::contains("Starting the server"))
            .stdout(predicates::str::contains("method=GET url=/"));
    }
}

mod wasm_on_disk {
    use super::*;

    #[test]
    #[ignore]
    fn wasi_executable() {
        let assert = wasmer_cli()
            .arg("run2")
            .arg(fixtures::qjs())
            .arg("--")
            .arg("--eval")
            .arg("console.log('Hello, World!')")
            .assert();

        assert.success().stdout("Hello, World!");
    }

    #[test]
    #[ignore]
    fn no_abi() {
        let assert = wasmer_cli().arg("run2").arg(fixtures::fib()).assert();

        assert.success();
    }

    #[test]
    #[ignore]
    fn error_if_no_start_function_found() {
        let assert = wasmer_cli()
            .arg("run2")
            .arg(fixtures::wat_no_start())
            .assert();

        assert
            .failure()
            .stderr("Can not find any export functions.");
    }
}

#[test]
#[ignore]
fn wasmer_package_directory() {
    let temp = TempDir::new().unwrap();
    std::fs::copy(fixtures::qjs(), temp.path().join("qjs.wasm")).unwrap();
    std::fs::copy(fixtures::qjs_wasmer_toml(), temp.path().join("wasmer.toml")).unwrap();

    let assert = wasmer_cli()
        .arg("run2")
        .arg(temp.path())
        .arg("--")
        .arg("--eval")
        .arg("console.log('Hello, World!')")
        .assert();

    assert.success().stdout("Hello, World!");
}

#[test]
#[ignore]
fn pre_compiled_wasm() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("qjs.wasmu");
    let qjs = fixtures::qjs();
    // Make sure it is compiled
    wasmer_cli()
        .arg("compile")
        .arg("-o")
        .arg(&dest)
        .arg(&qjs)
        .assert()
        .success();
    assert!(dest.exists());

    // Now we can try to run the compiled artifact
    let assert = wasmer_cli()
        .arg("run2")
        .arg(&dest)
        .arg("--")
        .arg("--eval")
        .arg("console.log('Hello, World!')")
        .assert();

    assert.success().stdout("Hello, World!");
}

mod remote_webc {
    use super::*;

    #[test]
    #[ignore]
    fn quickjs_as_package_name() {
        let assert = wasmer_cli()
            .arg("run2")
            .arg("saghul/quickjs")
            .arg("--registry=https://wapm.io/")
            .arg("--")
            .arg("--eval")
            .arg("console.log('Hello, World!')")
            .assert();

        assert.success().stdout("Hello, World!");
    }

    #[test]
    #[ignore]
    fn quickjs_as_url() {
        let assert = wasmer_cli()
            .arg("run2")
            .arg("https://wapm.io/saghul/quickjs")
            .arg("--")
            .arg("--eval")
            .arg("console.log('Hello, World!')")
            .assert();

        assert.success().stdout("Hello, World!");
    }
}

mod fixtures {
    use std::path::{Path, PathBuf};

    use wasmer_integration_tests_cli::{ASSET_PATH, C_ASSET_PATH};

    /// A WEBC file containing the Python interpreter, compiled to WASI.
    pub fn python() -> PathBuf {
        Path::new(C_ASSET_PATH).join("python-0.1.0.wasmer")
    }

    /// A WEBC file containing the WCGI static server.
    pub fn static_server() -> PathBuf {
        Path::new(C_ASSET_PATH).join("staticserver.webc")
    }

    /// The QuickJS interpreter, compiled to a WASI module.
    pub fn qjs() -> PathBuf {
        Path::new(C_ASSET_PATH).join("qjs.wasm")
    }

    /// The `wasmer.toml` file for QuickJS.
    pub fn qjs_wasmer_toml() -> PathBuf {
        Path::new(C_ASSET_PATH).join("qjs-wasmer.toml")
    }

    /// An executable which calculates fib(40) and exits with no output.
    pub fn fib() -> PathBuf {
        Path::new(ASSET_PATH).join("fib.wat")
    }

    pub fn wat_no_start() -> PathBuf {
        Path::new(ASSET_PATH).join("no_start.wat")
    }
}

/// A helper that wraps [`std::process::Child`] to make sure it gets terminated
/// when it is no longer needed.
struct Child(Option<std::process::Child>);

impl Child {
    fn new(child: std::process::Child) -> Self {
        Child(Some(child))
    }

    fn join(mut self) -> Assert {
        let mut child = self.0.take().unwrap();
        child.kill().unwrap();
        child.wait_with_output().unwrap().assert()
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
        }
    }
}

/// Send a GET request to a particular URL, automatically retrying (with
/// a timeout) if there are any connection errors.
fn http_get(url: impl IntoUrl) -> String {
    let start = Instant::now();
    let timeout = Duration::from_secs(5);
    let url = url.into_url().unwrap();

    let client = Client::builder().build().unwrap();

    while start.elapsed() < timeout {
        match client.get(url.clone()).send() {
            Ok(response) => return response.error_for_status().unwrap().text().unwrap(),
            Err(e) if e.is_connect() => continue,
            Err(other) => panic!("An unexpected error occurred: {other}"),
        }
    }

    panic!("Didn't receive a response from \"{url}\" within the allocated time");
}
