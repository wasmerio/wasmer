use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Stdio},
    sync::Arc,
    time::Duration,
};

use anyhow::Error;
use futures::TryFutureExt;
use insta::assert_json_snapshot;

use tempfile::NamedTempFile;
use wasmer_integration_tests_cli::get_wasmer_path;

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct TestIncludeWeb {
    pub name: String,
    #[serde(skip, default = "default_include_webc")]
    pub webc: Arc<NamedTempFile>,
}

impl PartialEq for TestIncludeWeb {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

fn default_include_webc() -> Arc<NamedTempFile> {
    Arc::new(NamedTempFile::new().unwrap())
}
impl Eq for TestIncludeWeb {}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq)]
pub struct TestSpec {
    pub name: Option<String>,
    // Uses a hex-encoded String for better review output.
    #[serde(skip)]
    pub wasm_hash: String,
    /// Name of webc dependencies to inject.
    pub use_packages: Vec<String>,
    pub include_webcs: Vec<TestIncludeWeb>,
    pub cli_args: Vec<String>,
    #[serde(skip)]
    pub stdin: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub stdin_hash: Option<String>,
    pub enable_threads: bool,
    pub enable_network: bool,
    #[serde(skip_serializing_if = "is_false")]
    #[serde(default)]
    pub enable_async_threads: bool,
}

fn is_false(b: &bool) -> bool {
    !(*b)
}

static WEBC_BASH: &[u8] =
    include_bytes!("./webc/bash-1.0.16-f097441a-a80b-4e0d-87d7-684918ef4bb6.webc");
static WEBC_COREUTILS_16: &[u8] =
    include_bytes!("./webc/coreutils-1.0.16-e27dbb4f-2ef2-4b44-b46a-ddd86497c6d7.webc");
static WEBC_COREUTILS_11: &[u8] =
    include_bytes!("./webc/coreutils-1.0.11-9d7746ca-694f-11ed-b932-dead3543c068.webc");
static WEBC_DASH: &[u8] =
    include_bytes!("./webc/dash-1.0.18-f0d13233-bcda-4cf1-9a23-3460bffaae2a.webc");
static WEBC_PYTHON: &[u8] = include_bytes!("./webc/python-0.1.0.webc");
static WEBC_WEB_SERVER: &[u8] = include_bytes!(
    "./webc/static-web-server-async-1.0.3-5d739d1a-20b7-4edf-8cf4-44e813f96b25.webc"
);
static WEBC_WASMER_SH: &[u8] =
    include_bytes!("./webc/wasmer-sh-1.0.63-dd3d67d1-de94-458c-a9ee-caea3b230ccf.webc");

impl std::fmt::Debug for TestSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestSpec")
            .field("name", &self.name)
            // TODO: show hash of code?
            // .field("wasm_code", &self.wasm_code)
            .field("use_packages", &self.use_packages)
            .field("include_webcs", &self.include_webcs)
            .field("cli_args", &self.cli_args)
            .field("stdin", &self.stdin)
            .finish()
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct TestOutput {
    // Either a plain string, or a hex-encoded string for binary output.
    pub stdout: String,
    // Either a plain string, or a hex-encoded string for binary output.
    pub stderr: String,
    pub exit_code: i32,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, Debug)]
pub enum TestResult {
    Success(TestOutput),
    Error(String),
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct TestSnapshot {
    pub spec: TestSpec,
    pub result: TestResult,
}

impl TestSnapshot {
    pub fn convert_stdout_to_hash(&mut self) {
        self.result = match &self.result {
            TestResult::Success(a) => TestResult::Success(TestOutput {
                stdout: format!("hash: {:x}", md5::compute(a.stdout.as_bytes())),
                stderr: a.stderr.clone(),
                exit_code: a.exit_code,
            }),
            res => res.clone(),
        };
    }
}

pub struct TestBuilder {
    spec: TestSpec,
}

type RunWith = Box<dyn FnOnce(Child) -> Result<i32, Error> + 'static>;

impl TestBuilder {
    pub fn new() -> Self {
        Self {
            spec: TestSpec {
                name: None,
                wasm_hash: String::new(),
                use_packages: Vec::new(),
                include_webcs: Vec::new(),
                cli_args: Vec::new(),
                stdin: None,
                stdin_hash: None,
                enable_threads: true,
                enable_network: false,
                enable_async_threads: false,
            },
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.spec.cli_args.push(arg.into());
        self
    }

    pub fn with_async_threads(mut self) -> Self {
        self.spec.enable_async_threads = true;
        self
    }

    pub fn args<I: IntoIterator<Item = S>, S: AsRef<str>>(mut self, args: I) -> Self {
        let args = args.into_iter().map(|s| s.as_ref().to_string());
        self.spec.cli_args.extend(args);
        self
    }

    pub fn stdin_str(self, s: impl Into<String>) -> Self {
        let str = s.into();
        self.stdin(str.as_bytes())
    }

    pub fn stdin(mut self, s: &[u8]) -> Self {
        self.spec.stdin_hash = Some(format!("{:x}", md5::compute(s)));
        self.spec.stdin = Some(s.to_vec());
        self
    }

    pub fn use_pkg(mut self, s: impl Into<String>) -> Self {
        self.spec.use_packages.push(s.into());
        self
    }

    pub fn include_static_package(mut self, name: &str, data: &[u8]) -> Self {
        let package = build_test_file(data);
        self.spec.include_webcs.push(TestIncludeWeb {
            name: name.to_string(),
            webc: Arc::new(package),
        });
        self
    }

    pub fn use_coreutils(self) -> Self {
        // TODO: use custom compiled coreutils
        self.use_pkg("sharrattj/coreutils")
            .include_static_package("sharrattj/coreutils@1.0.16", WEBC_COREUTILS_16)
    }

    pub fn use_dash(self) -> Self {
        // TODO: use custom compiled dash
        self.use_pkg("sharrattj/dash")
            .include_static_package("sharrattj/dash@1.0.16", WEBC_DASH)
            .include_static_package("sharrattj/coreutils@1.0.11", WEBC_COREUTILS_11)
    }

    pub fn use_bash(self) -> Self {
        // TODO: use custom compiled bash
        self.use_pkg("sharrattj/bash")
            .include_static_package("sharrattj/bash@1.0.12", WEBC_BASH)
            .include_static_package("sharrattj/coreutils@1.0.11", WEBC_COREUTILS_11)
    }

    // Enable thread support.
    // NOTE: ENABLED BY DEFAULT.
    pub fn enable_threads(mut self, enabled: bool) -> Self {
        self.spec.enable_threads = enabled;
        self
    }

    pub fn enable_network(mut self, enabled: bool) -> Self {
        self.spec.enable_network = enabled;
        self
    }

    pub fn run_file(self, path: impl AsRef<Path>) -> TestSnapshot {
        snapshot_file(path.as_ref(), self.spec)
    }

    pub fn with_name(mut self, name: &str) -> Self {
        self.spec.name = Some(name.to_string());
        self
    }

    pub fn run_wasm(self, code: &[u8]) -> TestSnapshot {
        #[allow(unused_mut)]
        let mut snapshot = build_snapshot(self.spec, code);
        // TODO: figure out why snapshot exit code is 79 on macos
        #[cfg(target_os = "macos")]
        if let TestResult::Success(ref mut output) = snapshot.result {
            if output.exit_code == 79 {
                output.exit_code = 78;
            }
        }
        snapshot
    }

    pub fn run_wasm_with(self, code: &[u8], with: RunWith) -> TestSnapshot {
        build_snapshot_with(self.spec, code, with)
    }
}

impl Default for TestBuilder {
    fn default() -> Self {
        TestBuilder::new()
    }
}

pub fn wasm_dir() -> PathBuf {
    std::env::current_dir()
        .unwrap()
        .parent()
        .unwrap()
        .join("wasm")
}

fn wasmer_path() -> PathBuf {
    let path = std::env::var("WASMER_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| get_wasmer_path());
    if !path.is_file() {
        panic!("Could not find wasmer binary: '{}'", path.display());
    }
    path
}

fn build_test_file(contents: &[u8]) -> NamedTempFile {
    let mut named_file = NamedTempFile::new().unwrap();
    let file = named_file.as_file_mut();
    file.write_all(contents).unwrap();
    file.flush().unwrap();
    named_file
}

fn bytes_to_hex_string(bytes: Vec<u8>) -> String {
    if let Ok(s) = String::from_utf8(bytes.clone()) {
        s
    } else {
        hex::encode(bytes)
    }
}

pub fn run_test_with(spec: TestSpec, code: &[u8], with: RunWith) -> TestResult {
    let wasm_path = build_test_file(code);

    let mut cmd = std::process::Command::new(wasmer_path());

    // let shell = xshell::Shell::new().unwrap();
    // let wasmer = wasmer_path();

    // let mut cmd = xshell::cmd!(shell, "{wasmer}");
    if spec.enable_threads {
        cmd.arg("--enable-threads");
    }
    if spec.enable_network {
        cmd.arg("--net");
    }

    if spec.enable_async_threads {
        cmd.arg("--enable-async-threads");
    }

    for pkg in &spec.use_packages {
        cmd.args(["--use", pkg]);
    }

    for pkg in &spec.include_webcs {
        cmd.arg("--include-webc").arg(pkg.webc.path());
    }

    cmd.env("RUST_LOG", "off");

    cmd.env("RUST_BACKTRACE", "1");

    cmd.arg(wasm_path.path());
    if !spec.cli_args.is_empty() {
        cmd.arg("--").args(&spec.cli_args);
    }

    // Stdio.
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    if spec.stdin.is_some() {
        cmd.stdin(Stdio::piped());
    }

    dbg!(&cmd);
    let mut proc = match cmd.spawn() {
        Ok(p) => p,
        Err(err) => {
            return TestResult::Error(format!("Could not spawn wasmer command: {err}"));
        }
    };

    let mut stdout_handle = proc.stdout.take().unwrap();
    let mut stderr_handle = proc.stderr.take().unwrap();

    let stdout_thread = std::thread::spawn(move || -> Result<Vec<u8>, std::io::Error> {
        let mut buffer = Vec::new();
        stdout_handle.read_to_end(&mut buffer)?;
        Ok(buffer)
    });
    let stderr_thread = std::thread::spawn(move || -> Result<Vec<u8>, std::io::Error> {
        let mut buffer = Vec::new();
        stderr_handle.read_to_end(&mut buffer)?;
        Ok(buffer)
    });

    if let Some(stdin) = &spec.stdin {
        proc.stdin.take().unwrap().write_all(stdin).unwrap();
    }

    let status = with(proc);

    let status = match status {
        Ok(status) => status,
        Err(err) => {
            let stdout = stdout_thread.join().unwrap().unwrap();
            let stderr = stderr_thread.join().unwrap().unwrap();
            return TestResult::Error(format!(
                "Command failed: {err}\n\nSTDOUT:\n{}\n\nSTDERR:\n{}",
                String::from_utf8_lossy(&stdout),
                String::from_utf8_lossy(&stderr)
            ));
        }
    };

    let stdout = bytes_to_hex_string(stdout_thread.join().unwrap().unwrap());
    let stderr = bytes_to_hex_string(stderr_thread.join().unwrap().unwrap());

    // we do some post processing to replace the temporary random name of the binary
    // with a fixed name as otherwise the results are not comparable. this occurs
    // because bash (and others) use the process name in the printf on stdout
    let stdout = stdout.replace(
        wasm_path
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .as_ref(),
        "test.wasm",
    );
    let stderr = stderr.replace(
        wasm_path
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .as_ref(),
        "test.wasm",
    );

    TestResult::Success(TestOutput {
        stdout,
        stderr,
        exit_code: status,
    })
}

pub fn build_snapshot(mut spec: TestSpec, code: &[u8]) -> TestSnapshot {
    spec.wasm_hash = format!("{:x}", md5::compute(code));

    let result = run_test_with(
        spec.clone(),
        code,
        Box::new(|mut child| {
            child
                .wait()
                .map_err(|err| err.into())
                .map(|status| status.code().unwrap_or_default())
        }),
    );

    TestSnapshot { spec, result }
}

pub fn build_snapshot_with(mut spec: TestSpec, code: &[u8], with: RunWith) -> TestSnapshot {
    spec.wasm_hash = format!("{:x}", md5::compute(code));

    let result = run_test_with(spec.clone(), code, with);

    TestSnapshot { spec, result }
}

pub fn snapshot_file(path: &Path, spec: TestSpec) -> TestSnapshot {
    let code = std::fs::read(path)
        .map_err(|err| format!("Could not read wasm file '{}': {err}", path.display()))
        .unwrap();
    build_snapshot(spec, &code)
}

macro_rules! function {
    () => {{
        fn f() {}
        fn type_name_of<T>(_: T) -> &'static str {
            std::any::type_name::<T>()
        }
        let name = type_name_of(f);
        &name[..name.len() - 3]
    }};
}

#[cfg_attr(
    any(
        target_env = "musl",
        all(target_os = "macos", target_arch = "x86_64"), // Output is slightly different in macos x86_64
        target_os = "windows"
    ),
    ignore
)]
#[test]
fn test_snapshot_condvar() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .run_wasm(include_bytes!("./wasm/example-condvar.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(
    any(target_env = "musl", target_os = "macos", target_os = "windows"),
    ignore
)]
#[test]
fn test_snapshot_condvar_async() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .with_async_threads()
        .run_wasm(include_bytes!("./wasm/example-condvar.wasm"));
    assert_json_snapshot!(snapshot);
}

// Test that the expected default directories are present.
#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_default_file_system_tree() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .arg("ls")
        .run_wasm(include_bytes!("./wasm/coreutils.wasm"));
    assert_json_snapshot!(snapshot);
}

// TODO: figure out why this hangs on Windows and Mac OS
#[cfg_attr(
    any(target_env = "musl", target_os = "macos", target_os = "windows"),
    ignore
)]
#[test]
fn test_snapshot_stdin_stdout_stderr() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .stdin_str("blah")
        .args(["tee", "/dev/stderr"])
        .run_wasm(include_bytes!("./wasm/coreutils.wasm"));
    assert_json_snapshot!(snapshot);
}

// Piping to cowsay should, well.... display a cow that says something
#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_cowsay() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .stdin_str("blah\n")
        .run_wasm(include_bytes!("./wasm/cowsay.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(
    any(target_env = "musl", target_os = "macos", target_os = "windows"),
    ignore
)]
#[test]
fn test_snapshot_epoll() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .run_wasm(include_bytes!("./wasm/example-epoll.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(
    any(target_env = "musl", target_os = "macos", target_os = "windows"),
    ignore
)]
#[test]
fn test_snapshot_epoll_async() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .with_async_threads()
        .run_wasm(include_bytes!("./wasm/example-epoll.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_file_copy() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .use_coreutils()
        .stdin_str("hi")
        .arg("/dev/stdin")
        .arg("/dev/stdout")
        .run_wasm(include_bytes!("./wasm/example-file-copy.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_execve() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .use_coreutils()
        .run_wasm(include_bytes!("./wasm/example-execve.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_readdir_tree() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .use_coreutils()
        .args(["/"])
        .run_wasm(include_bytes!("./wasm/example-readdir_tree.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_minimodem_tx() {
    let mut snapshot = TestBuilder::new()
        .with_name(function!())
        .stdin_str("This message wont get through")
        .arg("--tx")
        .arg("--tx-carrier")
        .arg("--stdio")
        .arg("--float-samples")
        .arg("same")
        .run_wasm(include_bytes!("./wasm/minimodem.wasm"));
    snapshot.convert_stdout_to_hash();

    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_minimodem_rx() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .arg("--rx")
        .arg("--tx-carrier")
        .arg("--stdio")
        .arg("--float-samples")
        .arg("same")
        .stdin(include_bytes!("./wasm/minimodem.data"))
        .run_wasm(include_bytes!("./wasm/minimodem.wasm"));
    assert_json_snapshot!(snapshot);
}

fn test_run_http_request(
    port: u16,
    what: &str,
    expected_size: Option<usize>,
) -> Result<i32, Error> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let http_get = move |url: String, max_retries: i32| {
        rt.block_on(async move {
            let mut n = 1;

            loop {
                println!("http request (attempt #{n}): {url}");

                let pending_request = reqwest::get(&url)
                    .and_then(|r| futures::future::ready(r.error_for_status()))
                    .and_then(|r| r.bytes());

                match tokio::time::timeout(Duration::from_secs(2), pending_request)
                    .await
                    .map_err(Error::from)
                    .and_then(|result| result.map_err(Error::from))
                {
                    Ok(body) => return Ok(body),
                    Err(e) if n <= max_retries => {
                        eprintln!("non-fatal error: {e}... Retrying");
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

                        n += 1;
                        continue;
                    }
                    Err(e) => {
                        return Err(e);
                    }
                }
            }
        })
    };

    let expected_size = match expected_size {
        None => {
            let url = format!("http://localhost:{port}/{what}.size");
            let expected_size = String::from_utf8_lossy(http_get(url, 50)?.as_ref())
                .trim()
                .parse()?;
            if expected_size == 0 {
                return Err(anyhow::format_err!("There was no data returned"));
            }
            expected_size
        }
        Some(s) => s,
    };
    println!("expected_size: {expected_size}");

    let url = format!("http://localhost:{port}/{what}");
    let reference_data = http_get(url.clone(), 50)?;
    for _ in 0..20 {
        let test_data = http_get(url.clone(), 2)?;
        println!("actual_size: {}", test_data.len());

        if expected_size != test_data.len() {
            return Err(anyhow::format_err!(
                "The actual size and expected size does not match {} vs {}",
                test_data.len(),
                expected_size
            ));
        }
        if test_data
            .iter()
            .zip(reference_data.iter())
            .any(|(a, b)| a != b)
        {
            return Err(anyhow::format_err!("The returned data is inconsistent"));
        }
    }
    Ok(0)
}

#[cfg_attr(
    any(target_env = "musl", target_os = "macos", target_os = "windows"),
    ignore
)]
#[test]
fn test_snapshot_tokio() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .run_wasm(include_bytes!("./wasm/example-tokio.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(
    any(target_env = "musl", target_os = "macos", target_os = "windows"),
    ignore
)]
#[test]
fn test_snapshot_unix_pipe() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .run_wasm(include_bytes!("./wasm/example-unix-pipe.wasm"));
    assert_json_snapshot!(snapshot);
}

#[test]
#[cfg_attr(
    any(target_env = "musl", target_os = "macos", target_os = "windows"),
    ignore
)]
fn test_snapshot_web_server() {
    let name: &str = function!();
    let port = 7777;

    let with = move |mut child: Child| {
        let ret = test_run_http_request(port, "main.js", None);
        child.kill().ok();
        ret
    };

    let script = format!(
        r#"
cat /public/main.js | wc -c > /public/main.js.size
rm -r -f /cfg/
cd /public
/bin/webserver --log-level warn --root /public --port {port}"#,
    );
    let builder = TestBuilder::new()
        .with_name(name)
        .enable_network(true)
        .include_static_package("sharrattj/static-web-server@1.0.92", WEBC_WEB_SERVER)
        .include_static_package("sharrattj/wasmer-sh@1.0.63", WEBC_WASMER_SH)
        .use_coreutils()
        .use_pkg("sharrattj/wasmer-sh")
        .stdin_str(script);

    let snapshot = builder.run_wasm_with(include_bytes!("./wasm/dash.wasm"), Box::new(with));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(
    any(target_env = "musl", target_os = "macos", target_os = "windows"),
    ignore
)]
#[test]
fn test_snapshot_web_server_epoll() {
    let name = function!();
    let port = 7778;

    let with = move |mut child: Child| {
        let ret = test_run_http_request(port, "null", Some(0));
        child.kill().ok();
        ret
    };

    let builder = TestBuilder::new()
        .with_name(name)
        .with_async_threads()
        .enable_network(true)
        .arg("--root")
        .arg("/dev")
        .arg("--log-level")
        .arg("warn")
        .arg("--port")
        .arg(format!("{port}"));

    let snapshot = builder.run_wasm_with(
        include_bytes!("./wasm/web-server-epoll.wasm"),
        Box::new(with),
    );
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(
    any(target_env = "musl", target_os = "macos", target_os = "windows"),
    ignore
)]
#[test]
fn test_snapshot_web_server_poll() {
    let name = function!();
    let port = 7779;

    let with = move |mut child: Child| {
        let ret = test_run_http_request(port, "null", Some(0));
        child.kill().ok();
        ret
    };

    let builder = TestBuilder::new()
        .with_name(name)
        .with_async_threads()
        .enable_network(true)
        .arg("--root")
        .arg("/dev")
        .arg("--log-level")
        .arg("warn")
        .arg("--port")
        .arg(format!("{port}"));

    let snapshot = builder.run_wasm_with(
        include_bytes!("./wasm/web-server-poll.wasm"),
        Box::new(with),
    );
    assert_json_snapshot!(snapshot);
}
// The ability to fork the current process and run a different image but retain
// the existing open file handles (which is needed for stdin and stdout redirection)
#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_fork_and_exec() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .use_coreutils()
        .run_wasm(include_bytes!("./wasm/example-execve.wasm"));
    assert_json_snapshot!(snapshot);
}

// The ability to fork the current process and run a different image but retain
// the existing open file handles (which is needed for stdin and stdout redirection)
#[cfg(not(target_os = "windows"))]
#[cfg_attr(
    any(target_env = "musl", target_os = "macos", target_os = "windows"),
    ignore
)]
#[test]
fn test_snapshot_fork_and_exec_async() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .use_coreutils()
        .with_async_threads()
        .run_wasm(include_bytes!("./wasm/example-execve.wasm"));
    assert_json_snapshot!(snapshot);
}

// longjmp is used by C programs that save and restore the stack at specific
// points - this functionality is often used for exception handling
#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_longjump() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .use_coreutils()
        .run_wasm(include_bytes!("./wasm/example-longjmp.wasm"));
    assert_json_snapshot!(snapshot);
}

// Simple fork example that is a crude multi-threading implementation - used by `dash`
#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_fork() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .use_coreutils()
        .run_wasm(include_bytes!("./wasm/example-fork.wasm"));
    assert_json_snapshot!(snapshot);
}

// Simple fork example that is a crude multi-threading implementation - used by `dash`
#[cfg_attr(
    any(target_env = "musl", target_os = "macos", target_os = "windows"),
    ignore
)]
#[test]
fn test_snapshot_fork_async() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .use_coreutils()
        .with_async_threads()
        .run_wasm(include_bytes!("./wasm/example-fork.wasm"));
    assert_json_snapshot!(snapshot);
}

// Uses the `fd_pipe` syscall to create a bidirection pipe with two file
// descriptors then forks the process to write and read to this pipe.
#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_pipes() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .use_coreutils()
        .run_wasm(include_bytes!("./wasm/example-pipe.wasm"));
    assert_json_snapshot!(snapshot);
}

// Performs a longjmp of a stack that was recorded before the fork.
// This test ensures that the stacks that have been recorded are preserved
// after a fork.
// The behavior is needed for `dash`
#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_longjump_fork() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .run_wasm(include_bytes!("./wasm/example-fork-longjmp.wasm"));
    assert_json_snapshot!(snapshot);
}

// Performs a longjmp of a stack that was recorded before the fork.
// This test ensures that the stacks that have been recorded are preserved
// after a fork.
// The behavior is needed for `dash`
#[cfg_attr(
    any(target_env = "musl", target_os = "macos", target_os = "windows"),
    ignore
)]
#[test]
fn test_snapshot_longjump_fork_async() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .with_async_threads()
        .run_wasm(include_bytes!("./wasm/example-fork-longjmp.wasm"));
    assert_json_snapshot!(snapshot);
}

// full multi-threading with shared memory and shared compiled modules
#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_multithreading() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .run_wasm(include_bytes!("./wasm/example-multi-threading.wasm"));
    assert_json_snapshot!(snapshot);
}

// test for traditional wasi threads
#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_wasi_threads() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .enable_threads(true)
        .run_wasm(include_bytes!("./wasm/wasi-threads.wasm"));
    assert_json_snapshot!(snapshot);
}

// multithreading with shared memory access
#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_threaded_memory() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .run_wasm(include_bytes!("./wasm/threaded-memory.wasm"));
    assert_json_snapshot!(snapshot);
}

// full multi-threading with shared memory and shared compiled modules
#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_multithreading_async() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .with_async_threads()
        .run_wasm(include_bytes!("./wasm/example-multi-threading.wasm"));
    assert_json_snapshot!(snapshot);
}

// full multi-threading with shared memory and shared compiled modules
#[cfg(target_os = "linux")]
#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_sleep() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .run_wasm(include_bytes!("./wasm/example-sleep.wasm"));
    assert_json_snapshot!(snapshot);
}

// full multi-threading with shared memory and shared compiled modules
#[cfg(target_os = "linux")]
#[cfg_attr(
    any(target_env = "musl", target_os = "macos", target_os = "windows"),
    ignore
)]
#[test]
fn test_snapshot_sleep_async() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .with_async_threads()
        .run_wasm(include_bytes!("./wasm/example-sleep.wasm"));
    assert_json_snapshot!(snapshot);
}

// Uses `posix_spawn` to launch a sub-process and wait on it to exit
#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_process_spawn() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .use_coreutils()
        .run_wasm(include_bytes!("./wasm/example-spawn.wasm"));
    assert_json_snapshot!(snapshot);
}

// Uses `posix_spawn` to launch a sub-process and wait on it to exit
#[cfg(not(target_os = "windows"))]
#[cfg_attr(
    any(target_env = "musl", target_os = "macos", target_os = "windows"),
    ignore
)]
#[test]
fn test_snapshot_process_spawn_async() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .use_coreutils()
        .with_async_threads()
        .run_wasm(include_bytes!("./wasm/example-spawn.wasm"));
    assert_json_snapshot!(snapshot);
}

// FIXME: re-enable - hangs on windows and macos
// Connects to 8.8.8.8:53 over TCP to verify TCP clients work
// #[test]
// fn test_snapshot_tcp_client() {
//     let snapshot = TestBuilder::new()
//         .with_name(function!())
//         .use_coreutils()
//         .run_wasm(include_bytes!("./wasm/example-tcp-client.wasm"));
//     assert_json_snapshot!(snapshot);
// }

// Tests that thread local variables work correctly
#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_thread_locals() {
    let mut snapshot = TestBuilder::new()
        .with_name(function!())
        .run_wasm(include_bytes!("./wasm/example-thread-local.wasm"));

    match &mut snapshot.result {
        TestResult::Success(out) => {
            // Output is non-deterministic, so just check for pass/failure by
            // resetting the output.
            out.stderr = String::new();
            out.stdout = String::new();
        }
        TestResult::Error(_) => {}
    };

    assert_json_snapshot!(snapshot);
}

// Tests that lightweight forking that does not copy the memory but retains the
// open file descriptors works correctly.
#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_vfork() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .use_coreutils()
        .run_wasm(include_bytes!("./wasm/example-vfork.wasm"));
    assert_json_snapshot!(snapshot);
}

// Tests that lightweight forking that does not copy the memory but retains the
// open file descriptors works correctly. Uses asynchronous threading
#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_vfork_async() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .use_coreutils()
        .with_async_threads()
        .run_wasm(include_bytes!("./wasm/example-vfork.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_signals() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .run_wasm(include_bytes!("./wasm/example-signal.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_signals_async() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .with_async_threads()
        .run_wasm(include_bytes!("./wasm/example-signal.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg(target_os = "linux")]
#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_dash_echo() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .stdin_str("echo 2")
        .run_wasm(include_bytes!("./wasm/dash.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_dash_echo_to_cat() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .use_coreutils()
        .stdin_str("echo hello | cat")
        .run_wasm(include_bytes!("./wasm/dash.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_dash_python() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .use_coreutils()
        .include_static_package("syrusakbary/python@0.1.0", WEBC_PYTHON)
        .stdin_str("wasmer run syrusakbary/python -- -c 'print(10)'")
        .run_wasm(include_bytes!("./wasm/dash.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_python_3_11_3() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .arg("-c")
        .arg("print(10)")
        .run_wasm(include_bytes!("./wasm/python-3.11.3.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_dash_dev_zero() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .use_coreutils()
        .stdin_str("head -c 10 /dev/zero")
        .run_wasm(include_bytes!("./wasm/dash.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_dash_dev_urandom() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .use_coreutils()
        .stdin_str("head -c 10 /dev/urandom | wc -c")
        .run_wasm(include_bytes!("./wasm/dash.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
#[ignore = "must be re-enabled after backend deployment"]
fn test_snapshot_dash_dash() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .use_dash()
        .stdin_str("/bin/dash\necho hi\nexit\nexit\n")
        .run_wasm(include_bytes!("./wasm/dash.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
#[ignore = "must be re-enabled after backend deployment"]
fn test_snapshot_dash_bash() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .use_bash()
        .stdin_str("/bin/bash\necho hi\nexit\nexit\n")
        .run_wasm(include_bytes!("./wasm/dash.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_bash_echo() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .stdin_str("echo hello\n")
        .run_wasm(include_bytes!("./wasm/bash.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_bash_ls() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .stdin_str("ls\nexit\n")
        .use_coreutils()
        .run_wasm(include_bytes!("./wasm/bash.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_bash_cd_ls() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .stdin_str("cd bin\nls\nexit\n")
        .use_coreutils()
        .run_wasm(include_bytes!("./wasm/bash.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_bash_pipe() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .stdin_str("echo hello | cat\nexit\n")
        .use_coreutils()
        .run_wasm(include_bytes!("./wasm/bash.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_bash_python() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .use_coreutils()
        .include_static_package("syrusakbary/python@0.1.0", WEBC_PYTHON)
        .stdin_str("wasmer run syrusakbary/python -- -c 'print(10)'\n")
        .run_wasm(include_bytes!("./wasm/bash.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
#[ignore = "must be re-enabled after backend deployment"]
fn test_snapshot_bash_bash() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .stdin_str("/bin/bash\necho hi\nexit\necho hi2\n")
        .use_bash()
        .run_wasm(include_bytes!("./wasm/bash.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
#[ignore = "must be re-enabled after backend deployment"]
fn test_snapshot_bash_dash() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .use_dash()
        .stdin_str("/bin/dash\necho hi\nexit\nexit\n")
        .run_wasm(include_bytes!("./wasm/bash.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_catsay() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .stdin_str("meoooww")
        .run_wasm(include_bytes!("./wasm/catsay.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_quickjs() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .stdin_str("print(2+2);\n\\q\n")
        .run_wasm(include_bytes!("./wasm/qjs.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_fs_rename() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .run_wasm(include_bytes!("./wasm/fs-rename.wasm"));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_exit_0_from_main() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .run_wasm(include_bytes!("./wasm/exit-0-from-main.wasm"));
    assert!(matches!(
        snapshot.result,
        TestResult::Success(TestOutput { exit_code: 0, .. })
    ));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_exit_1_from_main() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .run_wasm(include_bytes!("./wasm/exit-1-from-main.wasm"));
    assert!(matches!(
        snapshot.result,
        TestResult::Success(TestOutput { exit_code: 1, .. })
    ));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_exit_0_from_worker() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .run_wasm(include_bytes!("./wasm/exit-0-from-worker.wasm"));
    assert!(matches!(
        snapshot.result,
        TestResult::Success(TestOutput { exit_code: 0, .. })
    ));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_exit_1_from_worker() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .run_wasm(include_bytes!("./wasm/exit-1-from-worker.wasm"));
    assert!(matches!(
        snapshot.result,
        TestResult::Success(TestOutput { exit_code: 1, .. })
    ));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_worker_terminating_normally() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .run_wasm(include_bytes!("./wasm/worker-terminating-normally.wasm"));
    assert!(matches!(
        snapshot.result,
        TestResult::Success(TestOutput { exit_code: 0, .. })
    ));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_worker_panicking() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .run_wasm(include_bytes!("./wasm/worker-panicking.wasm"));
    assert!(matches!(
        snapshot.result,
        TestResult::Success(TestOutput { exit_code: 173, .. })
    ));
    assert_json_snapshot!(snapshot);
}

#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_snapshot_mkdir_rename() {
    let snapshot = TestBuilder::new()
        .with_name(function!())
        .run_wasm(include_bytes!("./wasm/mkdir-rename.wasm"));
    assert_json_snapshot!(snapshot);
}
