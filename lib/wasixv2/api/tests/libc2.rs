//! Prebuilt libc2 (musl) programs from `fixtures/`, run through the whole stack. See
//! `fixtures/README.md` for how they were built.

use std::{
    io,
    sync::{Arc, Mutex},
};

use wasixv2::{
    ByteSink, ByteSource, LocalFuture, Machine, ProcessConfig, RunError, Stdio, is_wasixv2_module,
};

const HELLO: &[u8] = include_bytes!("fixtures/hello.wasm");
const SMOKE: &[u8] = include_bytes!("fixtures/smoke.wasm");

#[derive(Default)]
struct Capture(Mutex<Vec<u8>>);

impl Capture {
    fn contents(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).expect("UTF-8 output")
    }
}

impl ByteSink for Capture {
    fn write<'a>(&'a self, data: &'a [u8]) -> LocalFuture<'a, io::Result<usize>> {
        self.0.lock().unwrap().extend_from_slice(data);
        Box::pin(async move { Ok(data.len()) })
    }
}

struct NoInput;

impl ByteSource for NoInput {
    fn read<'a>(&'a self, _: &'a mut [u8]) -> LocalFuture<'a, io::Result<usize>> {
        Box::pin(async { Ok(0) })
    }
}

struct Run {
    status: Result<i32, RunError>,
    stdout: String,
    stderr: String,
}

async fn run(wasm: &[u8], args: &[&str], env: &[&str]) -> Run {
    let stdout = Arc::new(Capture::default());
    let stderr = Arc::new(Capture::default());
    let config = ProcessConfig {
        args: args.iter().map(|s| s.to_string()).collect(),
        env: env.iter().map(|s| s.to_string()).collect(),
        stdio: Stdio {
            stdin: Arc::new(NoInput),
            stdout: stdout.clone(),
            stderr: stderr.clone(),
        },
    };
    let status = Machine::new().run_process(wasm, config).await;
    Run {
        status,
        stdout: stdout.contents(),
        stderr: stderr.contents(),
    }
}

#[tokio::test]
async fn hello_prints_its_arguments() {
    let run = run(HELLO, &["hello", "a", "b"], &[]).await;

    assert_eq!(run.status.unwrap(), 0);
    assert_eq!(run.stdout, "hello from hello with 3 args\n");
    assert_eq!(run.stderr, "");
}

// Covers argv/env, file I/O with 64-bit offsets on the in-memory `/tmp`, clocks and sleep,
// setjmp, TLS, compiler-rt builtins and malloc over mmap.
#[tokio::test]
async fn libc2_smoke_test_passes() {
    let run = run(SMOKE, &["smoke", "/tmp"], &["WASIXV2_SMOKE=1"]).await;

    assert!(!run.stdout.contains("FAIL"), "{}", run.stdout);
    assert_eq!(run.status.unwrap(), 0);
    assert!(run.stdout.starts_with("wasixv2 smoke test\n"));
    assert!(
        run.stdout.ends_with("smoke: all 38 checks passed\n"),
        "{}",
        run.stdout
    );
}

#[test]
fn detects_modules_that_import_wasixv2() {
    let imports_wasixv2 =
        wat::parse_str(r#"(module (import "wasixv2" "getpid" (func (result i32))))"#).unwrap();
    let imports_other =
        wat::parse_str(r#"(module (import "env" "f" (func)) (func (export "_start")))"#).unwrap();

    assert!(is_wasixv2_module(HELLO));
    assert!(is_wasixv2_module(&imports_wasixv2));
    assert!(!is_wasixv2_module(&imports_other));
    assert!(!is_wasixv2_module(b"not wasm"));
}
