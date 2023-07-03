use std::{
    io::{ErrorKind, Read},
    process::Stdio,
    time::{Duration, Instant},
};

use assert_cmd::{assert::Assert, prelude::OutputAssertExt};
use once_cell::sync::Lazy;
use predicates::str::contains;
use rand::Rng;
use reqwest::{blocking::Client, IntoUrl};
use tempfile::TempDir;
use wasmer_integration_tests_cli::get_wasmer_path;

const HTTP_GET_TIMEOUT: Duration = Duration::from_secs(5);

static RUST_LOG: Lazy<String> = Lazy::new(|| {
    [
        "info",
        "wasmer_wasix::resolve=debug",
        "wasmer_wasix::runners=debug",
        "wasmer_wasix=debug",
        "virtual_fs::trace_fs=trace",
    ]
    .join(",")
});

fn wasmer_run_unstable() -> std::process::Command {
    let mut cmd = std::process::Command::new("cargo");
    cmd.arg("run")
        .arg("--quiet")
        .arg("--package=wasmer-cli")
        .arg("--features=singlepass,cranelift,compiler")
        .arg("--color=never")
        .arg("--")
        .arg("run");
    cmd.env("RUST_LOG", &*RUST_LOG);
    cmd
}

mod webc_on_disk {
    use super::*;

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

    /// See <https://github.com/wasmerio/wasmer/issues/4010> for more.
    #[test]
    fn wasi_runner_mount_using_relative_directory_on_the_host() {
        let temp = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
        std::fs::write(temp.path().join("main.py"), "print('Hello, World!')").unwrap();

        let assert = wasmer_run_unstable()
            .arg(fixtures::python())
            .arg("--mapdir=/app:.")
            .arg("--")
            .arg("/app/main.py")
            .current_dir(temp.path())
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
    fn wasi_runner_with_dependencies() {
        let mut cmd = wasmer_run_unstable();
        let port = random_port();
        cmd.arg(fixtures::hello())
            .arg(format!("--env=SERVER_PORT={port}"))
            .arg("--net")
            .arg("--")
            .arg("--log-level=info");
        let mut child = JoinableChild::spawn(cmd);
        child.wait_for_stderr("listening");

        // Make sure we get the page we want
        let html = reqwest::blocking::get(format!("http://localhost:{port}/"))
            .unwrap()
            .text()
            .unwrap();
        assert!(html.contains("<title>Hello World</title>"), "{html}");

        // and make sure our request was logged
        child
            .join()
            .stderr(contains("incoming request: method=GET uri=/"));
    }

    #[test]
    #[cfg_attr(
        all(target_env = "musl", target_os = "linux"),
        ignore = "wasmer run-unstable segfaults on musl"
    )]
    fn webc_files_with_multiple_commands_require_an_entrypoint_flag() {
        let assert = wasmer_run_unstable().arg(fixtures::wabt()).assert();

        let msg = r#"Unable to determine the WEBC file's entrypoint. Please choose one of ["wasm-interp", "wasm-strip", "wasm-validate", "wasm2wat", "wast2json", "wat2wasm"]"#;
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
        let port = random_port();
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
        let port = random_port();
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

    /// See https://github.com/wasmerio/wasmer/issues/3794
    #[test]
    #[cfg_attr(
        all(target_env = "musl", target_os = "linux"),
        ignore = "wasmer run-unstable segfaults on musl"
    )]
    fn issue_3794_unable_to_mount_relative_paths() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("message.txt"), b"Hello, World!").unwrap();

        let assert = wasmer_run_unstable()
            .arg(fixtures::coreutils())
            .arg(format!("--mapdir=./some-dir/:{}", temp.path().display()))
            .arg("--command-name=cat")
            .arg("--")
            .arg("./some-dir/message.txt")
            .assert();

        assert.success().stdout(contains("Hello, World!"));
    }

    #[test]
    #[cfg_attr(
        all(target_env = "musl", target_os = "linux"),
        ignore = "wasmer run-unstable segfaults on musl"
    )]
    #[cfg_attr(
        windows,
        ignore = "FIXME(Michael-F-Bryan): Temporarily broken on Windows - https://github.com/wasmerio/wasmer/issues/3929"
    )]
    fn merged_filesystem_contains_all_files() {
        let assert = wasmer_run_unstable()
            .arg(fixtures::bash())
            .arg("--entrypoint=bash")
            .arg("--use")
            .arg(fixtures::coreutils())
            .arg("--use")
            .arg(fixtures::python())
            .arg("--")
            .arg("-c")
            .arg("ls -l /usr/coreutils/*.md && ls -l /lib/python3.6/*.py")
            .assert();

        assert
            .success()
            .stdout(contains("/usr/coreutils/README.md"))
            .stdout(contains("/lib/python3.6/this.py"));
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

mod local_directory {
    use super::*;

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
            .arg("--registry=wapm.io")
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

    #[test]
    #[cfg_attr(
        all(target_env = "musl", target_os = "linux"),
        ignore = "wasmer run-unstable segfaults on musl"
    )]
    #[cfg_attr(
        windows,
        ignore = "TODO(Michael-F-Bryan): Figure out why WasiFs::get_inode_at_path_inner() returns Errno::notcapable on Windows"
    )]
    fn bash_using_coreutils() {
        let assert = wasmer_run_unstable()
            .arg("sharrattj/bash")
            .arg("--entrypoint=bash")
            .arg("--use=sharrattj/coreutils")
            .arg("--registry=wapm.io")
            .arg("--")
            .arg("-c")
            .arg("ls /bin")
            .assert();

        // Note: the resulting filesystem should contain the main command as
        // well as the commands from all the --use packages

        let some_expected_binaries = [
            "arch", "base32", "base64", "baseenc", "basename", "bash", "cat",
        ]
        .join("\n");
        assert.success().stdout(contains(some_expected_binaries));
    }
}

mod fixtures {
    use std::path::{Path, PathBuf};

    use wasmer_integration_tests_cli::{ASSET_PATH, C_ASSET_PATH};

    /// A WEBC file containing the Python interpreter, compiled to WASI.
    pub fn python() -> PathBuf {
        Path::new(C_ASSET_PATH).join("python-0.1.0.wasmer")
    }

    pub fn coreutils() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("webc")
            .join("coreutils-1.0.16-e27dbb4f-2ef2-4b44-b46a-ddd86497c6d7.webc")
    }

    pub fn bash() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("webc")
            .join("bash-1.0.16-f097441a-a80b-4e0d-87d7-684918ef4bb6.webc")
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

    pub fn hello() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("webc")
            .join("hello-0.1.0-665d2ddc-80e6-4845-85d3-4587b1693bb7.webc")
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
struct JoinableChild {
    command: std::process::Command,
    child: Option<std::process::Child>,
}

impl JoinableChild {
    fn spawn(mut cmd: std::process::Command) -> Self {
        let child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        JoinableChild {
            child: Some(child),
            command: cmd,
        }
    }

    /// Keep reading lines from the child's stdout until a line containing the
    /// desired text is found.
    fn wait_for_stdout(&mut self, text: &str) -> String {
        let stdout = self
            .child
            .as_mut()
            .and_then(|child| child.stdout.as_mut())
            .unwrap();

        wait_for(text, stdout)
    }

    /// Keep reading lines from the child's stderr until a line containing the
    /// desired text is found.
    fn wait_for_stderr(&mut self, text: &str) -> String {
        let stderr = self
            .child
            .as_mut()
            .and_then(|child| child.stderr.as_mut())
            .unwrap();

        wait_for(text, stderr)
    }

    /// Kill the underlying [`std::process::Child`] and get an [`Assert`] we
    /// can use to check it.
    fn join(mut self) -> Assert {
        let mut child = self.child.take().unwrap();
        child.kill().unwrap();
        child.wait_with_output().unwrap().assert()
    }
}

fn wait_for(text: &str, reader: &mut dyn Read) -> String {
    let mut all_output = String::new();

    loop {
        let line = read_line(reader).unwrap();

        if line.is_empty() {
            eprintln!("=== All Output === ");
            eprintln!("{all_output}");
            panic!("EOF before \"{text}\" was found");
        }

        let found = line.contains(text);
        all_output.push_str(&line);

        if found {
            return all_output;
        }
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
        if let Some(mut child) = self.child.take() {
            eprintln!("==== WARNING: Child was dropped before being joined ====");
            eprintln!("Command: {:?}", self.command);

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

fn random_port() -> u16 {
    rand::thread_rng().gen_range(10_000_u16..u16::MAX)
}
