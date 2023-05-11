//! Integration tests for `wasmer run2`.
//!
//! Note that you will need to manually compile the `wasmer` CLI in release mode
//! before running any of these tests.
use std::{
    io::{ErrorKind, Read},
    process::Stdio,
    time::{Duration, Instant},
};

use assert_cmd::{assert::Assert, prelude::OutputAssertExt};
use predicates::str::contains;
use reqwest::{blocking::Client, IntoUrl};
use tempfile::TempDir;
use wasmer_integration_tests_cli::get_wasmer_path;

const RUST_LOG: &str = "info,wasmer_wasi::runners=debug,virtual_fs::trace_fs=trace";
const HTTP_GET_TIMEOUT: Duration = Duration::from_secs(5);

fn wasmer_run_unstable() -> std::process::Command {
    let mut cmd = std::process::Command::new("cargo");
    cmd.arg("run")
        .arg("--quiet")
        .arg("--package=wasmer-cli")
        .arg("--features=singlepass,cranelift")
        .arg("--")
        .arg("run-unstable");
    cmd.env("RUST_LOG", RUST_LOG);
    cmd
}

mod webc_on_disk {
    use super::*;
    use rand::Rng;

    #[test]
    #[cfg_attr(
        all(target_env = "musl", target_os = "linux"),
        ignore = "wasmer run-unstable segfaults on musl"
    )]
    fn wasi_runner() {
        let assert = wasmer_run_unstable()
            .arg(fixtures::qjs())
            .arg("--")
            .arg("--eval")
            .arg("console.log('Hello, World!')")
            .assert();

        assert.success().stdout(contains("Hello, World!"));
    }

    #[test]
    #[cfg_attr(
        all(target_env = "musl", target_os = "linux"),
        ignore = "wasmer run-unstable segfaults on musl"
    )]
    fn wasi_runner_with_mounted_directories() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("index.js"), "console.log('Hello, World!')").unwrap();

        let assert = wasmer_run_unstable()
            .arg(fixtures::qjs())
            .arg(format!("--mapdir=/app:{}", temp.path().display()))
            .arg("--")
            .arg("/app/index.js")
            .assert();

        assert.success().stdout(contains("Hello, World!"));
    }

    #[test]
    #[cfg_attr(
        all(target_env = "musl", target_os = "linux"),
        ignore = "wasmer run-unstable segfaults on musl"
    )]
    fn wasi_runner_with_mounted_directories_and_webc_volumes() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("main.py"), "print('Hello, World!')").unwrap();

        let assert = wasmer_run_unstable()
            .arg(fixtures::python())
            .arg(format!("--mapdir=/app:{}", temp.path().display()))
            .arg("--")
            .arg("-B")
            .arg("/app/main.py")
            .assert();

        assert.success().stdout(contains("Hello, World!"));
    }

    #[test]
    #[cfg_attr(
        all(target_env = "musl", target_os = "linux"),
        ignore = "wasmer run-unstable segfaults on musl"
    )]
    fn webc_files_with_multiple_commands_require_an_entrypoint_flag() {
        let assert = wasmer_run_unstable().arg(fixtures::wabt()).assert();

        let msg = r#"Unable to determine the WEBC file's entrypoint. Please choose one of ["wat2wasm", "wast2json", "wasm2wat", "wasm-interp", "wasm-validate", "wasm-strip"]"#;
        assert.failure().stderr(contains(msg));
    }

    #[test]
    #[cfg_attr(
        all(target_env = "musl", target_os = "linux"),
        ignore = "wasmer run-unstable segfaults on musl"
    )]
    fn wasi_runner_with_env_vars() {
        let assert = wasmer_run_unstable()
            .arg(fixtures::python())
            .arg("--env=SOME_VAR=Hello, World!")
            .arg("--")
            .arg("-B")
            .arg("-c")
            .arg("import os; print(os.environ['SOME_VAR'])")
            .assert();

        assert.success().stdout(contains("Hello, World!"));
    }

    #[test]
    #[cfg_attr(
        all(target_env = "musl", target_os = "linux"),
        ignore = "wasmer run-unstable segfaults on musl"
    )]
    fn wcgi_runner() {
        // Start the WCGI server in the background
        let port = rand::thread_rng().gen_range(10_000_u16..u16::MAX);
        let mut cmd = wasmer_run_unstable();
        cmd.arg(format!("--addr=127.0.0.1:{port}"))
            .arg(fixtures::static_server());

        // Let's run the command and wait until the server has started
        let mut child = JoinableChild::spawn(cmd);
        child.wait_for_stdout("WCGI Server running");

        // make the request
        let body = http_get(format!("http://127.0.0.1:{port}/")).unwrap();
        assert!(body.contains("<title>Index of /</title>"), "{body}");

        // Let's make sure 404s work too
        let err =
            http_get(format!("http://127.0.0.1:{port}/this/does/not/exist.html")).unwrap_err();
        assert_eq!(err.status().unwrap(), reqwest::StatusCode::NOT_FOUND);

        // And kill the server, making sure it generated the expected logs
        let assert = child.join();

        assert
            .stderr(contains("Starting the server"))
            .stderr(contains("response generated method=GET uri=/ status_code=200 OK"))
            .stderr(contains("response generated method=GET uri=/this/does/not/exist.html status_code=404 Not Found"));
    }

    #[test]
    #[cfg_attr(
        all(target_env = "musl", target_os = "linux"),
        ignore = "wasmer run-unstable segfaults on musl"
    )]
    fn wcgi_runner_with_mounted_directories() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("file.txt"), "Hello, World!").unwrap();
        // Start the WCGI server in the background
        let port = rand::thread_rng().gen_range(10_000_u16..u16::MAX);
        let mut cmd = wasmer_run_unstable();
        cmd.arg(format!("--addr=127.0.0.1:{port}"))
            .arg(format!("--mapdir=/path/to:{}", temp.path().display()))
            .arg(fixtures::static_server());

        // Let's run the command and wait until the server has started
        let mut child = JoinableChild::spawn(cmd);
        child.wait_for_stdout("WCGI Server running");

        let body = http_get(format!("http://127.0.0.1:{port}/path/to/file.txt")).unwrap();
        assert!(body.contains("Hello, World!"), "{body}");

        // And kill the server, making sure it generated the expected logs
        let assert = child.join();

        assert
            .stderr(contains("Starting the server"))
            .stderr(contains(
                "response generated method=GET uri=/path/to/file.txt status_code=200 OK",
            ));
    }
}

mod wasm_on_disk {
    use std::process::Command;

    use super::*;
    use predicates::str::contains;

    #[test]
    #[cfg_attr(
        all(target_env = "musl", target_os = "linux"),
        ignore = "wasmer run-unstable segfaults on musl"
    )]
    fn wasi_executable() {
        let assert = wasmer_run_unstable()
            .arg(fixtures::qjs())
            .arg("--")
            .arg("--eval")
            .arg("console.log('Hello, World!')")
            .assert();

        assert.success().stdout(contains("Hello, World!"));
    }

    #[test]
    #[cfg_attr(
        all(target_env = "musl", target_os = "linux"),
        ignore = "wasmer run-unstable segfaults on musl"
    )]
    fn no_abi() {
        let assert = wasmer_run_unstable().arg(fixtures::fib()).assert();

        assert.success();
    }

    #[test]
    #[cfg_attr(
        all(target_env = "musl", target_os = "linux"),
        ignore = "wasmer run-unstable segfaults on musl"
    )]
    fn error_if_no_start_function_found() {
        let assert = wasmer_run_unstable().arg(fixtures::wat_no_start()).assert();

        assert
            .failure()
            .stderr(contains("The module doesn't contain a \"_start\" function"));
    }

    #[test]
    #[cfg_attr(
        all(target_env = "musl", target_os = "linux"),
        ignore = "wasmer run-unstable segfaults on musl"
    )]
    fn pre_compiled() {
        let temp = TempDir::new().unwrap();
        let dest = temp.path().join("qjs.wasmu");
        let qjs = fixtures::qjs();
        // Make sure it is compiled
        Command::new(get_wasmer_path())
            .arg("compile")
            .arg("-o")
            .arg(&dest)
            .arg(&qjs)
            .assert()
            .success();
        assert!(dest.exists());

        // Now we can try to run the compiled artifact
        let assert = wasmer_run_unstable()
            .arg(&dest)
            .arg("--")
            .arg("--eval")
            .arg("console.log('Hello, World!')")
            .assert();

        assert.success().stdout(contains("Hello, World!"));
    }
}

#[test]
#[cfg_attr(
    all(target_env = "musl", target_os = "linux"),
    ignore = "wasmer run-unstable segfaults on musl"
)]
fn wasmer_package_directory() {
    let temp = TempDir::new().unwrap();
    std::fs::copy(fixtures::qjs(), temp.path().join("qjs.wasm")).unwrap();
    std::fs::copy(fixtures::qjs_wasmer_toml(), temp.path().join("wasmer.toml")).unwrap();

    let assert = wasmer_run_unstable()
        .arg(temp.path())
        .arg("--")
        .arg("--eval")
        .arg("console.log('Hello, World!')")
        .assert();

    assert.success().stdout(contains("Hello, World!"));
}

mod remote_webc {
    use super::*;

    #[test]
    #[cfg_attr(
        all(target_env = "musl", target_os = "linux"),
        ignore = "wasmer run-unstable segfaults on musl"
    )]
    fn quickjs_as_package_name() {
        let assert = wasmer_run_unstable()
            .arg("saghul/quickjs")
            .arg("--entrypoint=quickjs")
            .arg("--registry=https://wapm.io/")
            .arg("--")
            .arg("--eval")
            .arg("console.log('Hello, World!')")
            .assert();

        assert.success().stdout(contains("Hello, World!"));
    }

    #[test]
    #[cfg_attr(
        all(target_env = "musl", target_os = "linux"),
        ignore = "wasmer run-unstable segfaults on musl"
    )]
    fn quickjs_as_url() {
        let assert = wasmer_run_unstable()
            .arg("https://wapm.io/saghul/quickjs")
            .arg("--entrypoint=quickjs")
            .arg("--")
            .arg("--eval")
            .arg("console.log('Hello, World!')")
            .assert();

        assert.success().stdout(contains("Hello, World!"));
    }
}

mod fixtures {
    use std::path::{Path, PathBuf};

    use wasmer_integration_tests_cli::{ASSET_PATH, C_ASSET_PATH};

    /// A WEBC file containing the Python interpreter, compiled to WASI.
    pub fn python() -> PathBuf {
        Path::new(C_ASSET_PATH).join("python-0.1.0.wasmer")
    }

    /// A WEBC file containing `wat2wasm`, `wasm-validate`, and other helpful
    /// WebAssembly-related commands.
    pub fn wabt() -> PathBuf {
        Path::new(C_ASSET_PATH).join("wabt-1.0.37.wasmer")
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
struct JoinableChild(Option<std::process::Child>);

impl JoinableChild {
    fn spawn(mut cmd: std::process::Command) -> Self {
        let child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        JoinableChild(Some(child))
    }

    /// Keep reading lines from the child's stdout until a line containing the
    /// desired text is found.
    fn wait_for_stdout(&mut self, text: &str) -> String {
        let stderr = self
            .0
            .as_mut()
            .and_then(|child| child.stdout.as_mut())
            .unwrap();

        let mut all_output = String::new();

        loop {
            let line = read_line(stderr).unwrap();
            let found = line.contains(text);
            all_output.push_str(&line);

            if found {
                return all_output;
            }
        }
    }

    /// Kill the underlying [`std::process::Child`] and get an [`Assert`] we
    /// can use to check it.
    fn join(mut self) -> Assert {
        let mut child = self.0.take().unwrap();
        child.kill().unwrap();
        child.wait_with_output().unwrap().assert()
    }
}

fn read_line(reader: &mut dyn Read) -> Result<String, std::io::Error> {
    let mut line = Vec::new();

    while !line.ends_with(&[b'\n']) {
        let mut buffer = [0_u8];
        match reader.read_exact(&mut buffer) {
            Ok(_) => {
                line.push(buffer[0]);
            }
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e),
        }
    }

    let line = String::from_utf8(line).map_err(|e| std::io::Error::new(ErrorKind::Other, e))?;
    Ok(line)
}

impl Drop for JoinableChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            eprintln!("==== WARNING: Child was dropped before being joined ====");

            let _ = child.kill();

            if let Some(mut stderr) = child.stderr.take() {
                let mut buffer = String::new();
                if stderr.read_to_string(&mut buffer).is_ok() {
                    eprintln!("---- STDERR ----");
                    eprintln!("{buffer}");
                }
            }

            if let Some(mut stdout) = child.stdout.take() {
                let mut buffer = String::new();
                if stdout.read_to_string(&mut buffer).is_ok() {
                    eprintln!("---- STDOUT ----");
                    eprintln!("{buffer}");
                }
            }

            if !std::thread::panicking() {
                panic!("Child was dropped before being joined");
            }
        }
    }
}

/// Send a GET request to a particular URL, automatically retrying (with
/// a timeout) if there are any connection errors.
fn http_get(url: impl IntoUrl) -> Result<String, reqwest::Error> {
    let start = Instant::now();
    let url = url.into_url().unwrap();

    let client = Client::new();

    while start.elapsed() < HTTP_GET_TIMEOUT {
        match client.get(url.clone()).send() {
            Ok(response) => {
                return response.error_for_status()?.text();
            }
            Err(e) if e.is_connect() => continue,
            Err(other) => return Err(other),
        }
    }

    panic!("Didn't receive a response from \"{url}\" within the allocated time");
}
