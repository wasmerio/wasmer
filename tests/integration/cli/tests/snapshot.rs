use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Stdio,
};

#[cfg(test)]
use insta::assert_json_snapshot;

use wasmer_integration_tests_cli::get_wasmer_path;

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq)]
pub struct TestSpec {
    pub name: Option<String>,
    // Uses a hex-encoded String for better review output.
    pub wasm_hash: String,
    /// Name of webc dependencies to inject.
    pub use_packages: Vec<String>,
    pub cli_args: Vec<String>,
    pub stdin: Option<Vec<u8>>,
    pub debug_output: bool,
    pub enable_threads: bool,
}

impl std::fmt::Debug for TestSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestSpec")
            .field("name", &self.name)
            // TODO: show hash of code?
            // .field("wasm_code", &self.wasm_code)
            .field("use_packages", &self.use_packages)
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

pub struct TestBuilder {
    spec: TestSpec,
}

impl TestBuilder {
    pub fn new() -> Self {
        Self {
            spec: TestSpec {
                name: None,
                wasm_hash: String::new(),
                use_packages: Vec::new(),
                cli_args: Vec::new(),
                stdin: None,
                debug_output: false,
                enable_threads: true,
            },
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.spec.cli_args.push(arg.into());
        self
    }

    pub fn args<I: IntoIterator<Item = S>, S: AsRef<str>>(mut self, args: I) -> Self {
        let args = args.into_iter().map(|s| s.as_ref().to_string());
        self.spec.cli_args.extend(args);
        self
    }

    pub fn stdin_str(mut self, s: impl Into<String>) -> Self {
        self.spec.stdin = Some(s.into().into_bytes());
        self
    }

    pub fn use_pkg(mut self, s: impl Into<String>) -> Self {
        self.spec.use_packages.push(s.into());
        self
    }

    pub fn use_coreutils(self) -> Self {
        // TODO: use custom compiled coreutils
        self.use_pkg("sharrattj/coreutils")
    }

    pub fn debug_output(mut self, show_debug: bool) -> Self {
        self.spec.debug_output = show_debug;
        self
    }

    // Enable thread support.
    // NOTE: ENABLED BY DEFAULT.
    pub fn enable_threads(mut self, enabled: bool) -> Self {
        self.spec.enable_threads = enabled;
        self
    }

    pub fn run_file(self, path: impl AsRef<Path>) -> TestSnapshot {
        snapshot_file(path.as_ref(), self.spec)
    }

    pub fn run_wasm(self, code: &[u8]) -> TestSnapshot {
        build_snapshot(self.spec, code)
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

fn build_test_file(contents: &[u8]) -> PathBuf {
    // TODO: use TmpFile crate that auto-deletes files.
    let dir = std::env::temp_dir().join("wasmer-snapshot-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let hash = format!("{:x}.wasm", md5::compute(contents));
    let path = dir.join(hash);
    std::fs::write(&path, contents).unwrap();
    path
}

fn bytes_to_hex_string(bytes: Vec<u8>) -> String {
    if let Ok(s) = String::from_utf8(bytes.clone()) {
        s
    } else {
        hex::encode(bytes)
    }
}

pub fn run_test(spec: TestSpec, code: &[u8]) -> TestResult {
    let wasm_path = build_test_file(code);

    let mut cmd = std::process::Command::new(wasmer_path());

    // let shell = xshell::Shell::new().unwrap();
    // let wasmer = wasmer_path();

    // let mut cmd = xshell::cmd!(shell, "{wasmer}");
    if spec.enable_threads {
        cmd.arg("--enable-threads");
    }
    cmd.arg("--allow-multiple-wasi-versions");
    cmd.arg("--net");

    for pkg in &spec.use_packages {
        cmd.args(&["--use", &pkg]);
    }

    let log_level = if spec.debug_output {
        "debug"
    } else {
        "never=error"
    };
    cmd.env("RUST_LOG", log_level);

    cmd.arg(wasm_path);
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

    let status = match proc.wait() {
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
    TestResult::Success(TestOutput {
        stdout,
        stderr,
        exit_code: status.code().unwrap_or_default(),
    })
}

pub fn build_snapshot(mut spec: TestSpec, code: &[u8]) -> TestSnapshot {
    spec.wasm_hash = format!("{:x}", md5::compute(code));

    let result = run_test(spec.clone(), code);
    let snapshot = TestSnapshot { spec, result };
    snapshot
}

pub fn snapshot_file(path: &Path, spec: TestSpec) -> TestSnapshot {
    let code = std::fs::read(path)
        .map_err(|err| format!("Could not read wasm file '{}': {err}", path.display()))
        .unwrap();
    build_snapshot(spec, &code)
}

#[test]
fn test_snapshot_condvar() {
    let snapshot = TestBuilder::new()
        .debug_output(true)
        .run_wasm(include_bytes!("./wasm/example-condvar.wasm"));
    assert_json_snapshot!(snapshot);
}

// // Test that the expected default directories are present.
// #[test]
// fn test_snapshot_default_file_system_tree() {
//     let snapshot = TestBuilder::new()
//         .arg("ls")
//         .run_wasm(include_bytes!("./wasm/coreutils.wasm"));
//     assert_json_snapshot!(snapshot);
// }

// TODO: figure out why this hangs on Windows and Mac OS
#[cfg(target_os = "linux")]
#[test]
fn test_snapshot_stdin_stdout_stderr() {
    let snapshot = TestBuilder::new()
        .stdin_str("blah")
        .args(&["tee", "/dev/stderr"])
        .run_wasm(include_bytes!("./wasm/coreutils.wasm"));
    assert_json_snapshot!(snapshot);
}

// Piping to cowsay should, well.... display a cow that says something
#[test]
fn test_snapshot_cowsay() {
    let snapshot = TestBuilder::new()
        .stdin_str("blah\n")
        .run_wasm(include_bytes!("./wasm/cowsay.wasm"));
    assert_json_snapshot!(snapshot);
}

// FIXME: output contains timestamps - cant create snapshot
// #[test]
// fn test_snapshot_epoll() {
//     let snapshot = TestBuilder::new().run_file(wasm_dir().join("example-epoll.wasm"));
//     assert_json_snapshot!(snapshot);
// }

// // The ability to fork the current process and run a different image but retain
// // the existing open file handles (which is needed for stdin and stdout redirection)
// #[test]
// fn test_snapshot_fork_and_exec() {
//     let snapshot = TestBuilder::new()
//         .use_coreutils()
//         .run_wasm(include_bytes!("./wasm/example-execve.wasm"));
//     assert_json_snapshot!(snapshot);
// }

// // longjmp is used by C programs that save and restore the stack at specific
// // points - this functionality is often used for exception handling
// #[test]
// fn test_snapshot_longjump() {
//     let snapshot = TestBuilder::new()
//         .use_coreutils()
//         .run_wasm(include_bytes!("./wasm/example-longjmp.wasm"));
//     assert_json_snapshot!(snapshot);
// }

// // Another longjump test.
// // This one is initiated from `rust` code and thus has the risk of leaking memory but uses different interfaces
// #[test]
// fn test_snapshot_longjump2() {
//     let snapshot = TestBuilder::new()
//         .use_coreutils()
//         .run_wasm(include_bytes!("./wasm/example-stack.wasm"));
//     assert_json_snapshot!(snapshot);
// }

// // Simple fork example that is a crude multi-threading implementation - used by `dash`
// #[test]
// fn test_snapshot_fork() {
//     let snapshot = TestBuilder::new()
//         .use_coreutils()
//         .run_wasm(include_bytes!("./wasm/example-fork.wasm"));
//     assert_json_snapshot!(snapshot);
// }

// Uses the `fd_pipe` syscall to create a bidirection pipe with two file
// descriptors then forks the process to write and read to this pipe.
#[test]
fn test_snapshot_pipes() {
    let snapshot = TestBuilder::new()
        .use_coreutils()
        .run_wasm(include_bytes!("./wasm/example-pipe.wasm"));
    assert_json_snapshot!(snapshot);
}

// // Performs a longjmp of a stack that was recorded before the fork.
// // This test ensures that the stacks that have been recorded are preserved
// // after a fork.
// // The behavior is needed for `dash`
// #[test]
// fn test_snapshot_longjump_fork() {
//     let snapshot = TestBuilder::new().run_wasm(include_bytes!("./wasm/example-fork-longjmp.wasm"));
//     assert_json_snapshot!(snapshot);
// }

// // full multi-threading with shared memory and shared compiled modules
// #[test]
// fn test_snapshot_multithreading() {
//     let snapshot =
//         TestBuilder::new().run_wasm(include_bytes!("./wasm/example-multi-threading.wasm"));
//     assert_json_snapshot!(snapshot);
// }

// full multi-threading with shared memory and shared compiled modules
#[cfg(target_os = "linux")]
#[test]
fn test_snapshot_sleep() {
    let snapshot = TestBuilder::new().run_wasm(include_bytes!("./wasm/example-sleep.wasm"));
    assert_json_snapshot!(snapshot);
}

// // Uses `posix_spawn` to launch a sub-process and wait on it to exit
// #[test]
// fn test_snapshot_process_spawn() {
//     let snapshot = TestBuilder::new()
//         .use_coreutils()
//         .run_wasm(include_bytes!("./wasm/example-spawn.wasm"));
//     assert_json_snapshot!(snapshot);
// }

// FIXME: re-enable - hangs on windows and macos
// Connects to 8.8.8.8:53 over TCP to verify TCP clients work
// #[test]
// fn test_snapshot_tcp_client() {
//     let snapshot = TestBuilder::new()
//         .use_coreutils()
//         .run_wasm(include_bytes!("./wasm/example-tcp-client.wasm"));
//     assert_json_snapshot!(snapshot);
// }

// Tests that thread local variables work correctly
#[cfg(target_os = "linux")]
#[test]
fn test_snapshot_thread_locals() {
    let mut snapshot = TestBuilder::new()
        .use_coreutils()
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
// #[test]
// fn test_snapshot_vfork() {
//     let snapshot = TestBuilder::new()
//         .use_coreutils()
//         .run_wasm(include_bytes!("./wasm/example-vfork.wasm"));
//     assert_json_snapshot!(snapshot);
// }

// Tests that signals can be received and processed by WASM applications
// Note: This test requires that a signal is sent to the process asynchronously
// #[test]
// fn test_snapshot_signals() {
//     let snapshot = TestBuilder::new().run_file(wasm_dir().join("example-signal.wasm"));
//     assert_json_snapshot!(snapshot);
// }

#[cfg(target_os = "linux")]
#[test]
fn test_snapshot_dash() {
    let snapshot = TestBuilder::new()
        .stdin_str("echo 2")
        .run_wasm(include_bytes!("./wasm/dash.wasm"));
    // TODO: more tests!
    assert_json_snapshot!(snapshot);
}

#[test]
fn test_snapshot_bash() {
    let snapshot = TestBuilder::new()
        .stdin_str("echo hello")
        .run_wasm(include_bytes!("./wasm/bash.wasm"));
    // TODO: more tests!
    assert_json_snapshot!(snapshot);
}

#[test]
fn test_snapshot_catsay() {
    let snapshot = TestBuilder::new()
        .stdin_str("meoooww")
        .run_wasm(include_bytes!("./wasm/catsay.wasm"));
    assert_json_snapshot!(snapshot);
}

// // FIXME: not working properly, some issue with stdin piping
// // #[test]
// // fn test_snapshot_quickjs() {
// //     let snapshot = TestBuilder::new()
// //         .stdin_str("2+2*2")
// //         .run_wasm(include_bytes!("./wasm/qjs.wasm"));
// //     assert_json_snapshot!(snapshot);
// // }
