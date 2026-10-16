//! First vertical slice: hand-written guests with no libc, run through the whole stack.

use std::{
    io,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};

use tokio::sync::mpsc;

use wasixv2::{ByteSink, ByteSource, ExecError, LocalFuture, Machine, RunError, Stdio};

#[derive(Default)]
struct Capture(Mutex<Vec<u8>>);

impl Capture {
    fn contents(&self) -> Vec<u8> {
        self.0.lock().unwrap().clone()
    }
}

impl ByteSink for Capture {
    fn write<'a>(&'a self, data: &'a [u8]) -> LocalFuture<'a, io::Result<usize>> {
        self.0.lock().unwrap().extend_from_slice(data);
        Box::pin(async move { Ok(data.len()) })
    }
}

/// Stdin fed by the test while the guest runs.
struct ChannelInput(Mutex<mpsc::UnboundedReceiver<Vec<u8>>>);

impl ByteSource for ChannelInput {
    fn poll_read(&self, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        let mut input = self.0.lock().unwrap();
        let Some(data) = std::task::ready!(input.poll_recv(cx)) else {
            return Poll::Ready(Ok(0));
        };
        assert!(data.len() <= buf.len(), "the tests send short lines");
        buf[..data.len()].copy_from_slice(&data);
        Poll::Ready(Ok(data.len()))
    }
}

struct Run {
    status: Result<i32, RunError>,
    stdout: Vec<u8>,
}

async fn run_with_stdin(wat: &str, stdin: mpsc::UnboundedReceiver<Vec<u8>>) -> Run {
    let wasm = wat::parse_str(wat).expect("valid test module");
    let stdout = Arc::new(Capture::default());
    let stdio = Stdio {
        stdin: Arc::new(ChannelInput(Mutex::new(stdin))),
        stdout: stdout.clone(),
        stderr: Arc::new(Capture::default()),
    };
    let status = Machine::new().run(&wasm, stdio).await;
    Run {
        status,
        stdout: stdout.contents(),
    }
}

async fn run(wat: &str) -> Run {
    let (_, stdin) = mpsc::unbounded_channel();
    run_with_stdin(wat, stdin).await
}

const IMPORTS: &str = r#"
  (import "wasixv2" "read" (func $read (param i32 i32 i32) (result i32)))
  (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
  (import "wasixv2" "exit_group" (func $exit_group (param i32)))
  (memory (export "memory") 1)
"#;

fn module(body: &str) -> String {
    format!("(module {IMPORTS} {body})")
}

#[tokio::test]
async fn hello_world_exits_with_status() {
    let wat = module(
        r#"
        (data (i32.const 16) "hello, wasixv2\n")
        (func (export "_start")
          (drop (call $write (i32.const 1) (i32.const 16) (i32.const 15)))
          (call $exit_group (i32.const 3))
          unreachable)
        "#,
    );

    let run = run(&wat).await;

    assert_eq!(run.status.unwrap(), 3);
    assert_eq!(run.stdout, b"hello, wasixv2\n");
}

// The runtime has one thread, so the input can only be sent if `read` suspended the guest
// instead of blocking that thread.
#[tokio::test]
async fn read_suspends_the_guest_until_input_arrives() {
    let wat = module(
        r#"
        (func (export "_start") (local $n i32)
          (local.set $n (call $read (i32.const 0) (i32.const 64) (i32.const 32)))
          (drop (call $write (i32.const 1) (i32.const 64) (local.get $n)))
          (call $exit_group (local.get $n))
          unreachable)
        "#,
    );

    let (send, stdin) = mpsc::unbounded_channel();
    let feed = async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        send.send(b"echo me".to_vec()).unwrap();
    };

    let (run, ()) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(run_with_stdin(&wat, stdin), feed)
    })
    .await
    .expect("guest blocked the runtime thread");

    assert_eq!(run.status.unwrap(), 7);
    assert_eq!(run.stdout, b"echo me");
}

/// Exits with the negated return value of one `write`, i.e. its errno.
fn exit_with_errno_of_write(fd: i32, addr: u32) -> String {
    module(&format!(
        r#"
        (func (export "_start")
          (call $exit_group
            (i32.sub (i32.const 0) (call $write (i32.const {fd}) (i32.const {addr}) (i32.const 4))))
          unreachable)
        "#
    ))
}

#[tokio::test]
async fn errors_come_back_as_negated_errno() {
    const EBADF: i32 = 9;
    const EFAULT: i32 = 14;
    let past_memory = 0xffff_fff0u32;

    let bad_fd = run(&exit_with_errno_of_write(42, 16)).await;
    let bad_ptr = run(&exit_with_errno_of_write(1, past_memory)).await;
    let both = run(&exit_with_errno_of_write(42, past_memory)).await;

    assert_eq!(bad_fd.status.unwrap(), EBADF);
    assert_eq!(bad_ptr.status.unwrap(), EFAULT);
    // Linux checks the descriptor before touching the buffer.
    assert_eq!(both.status.unwrap(), EBADF);
}

// The kernel does not implement `vhangup`, so the generated default answers.
#[tokio::test]
async fn unimplemented_syscalls_return_enosys() {
    const ENOSYS: i32 = 38;
    let wat = r#"
      (module
        (import "wasixv2" "vhangup" (func $vhangup (result i32)))
        (import "wasixv2" "exit_group" (func $exit_group (param i32)))
        (memory (export "memory") 1)
        (func (export "_start")
          (call $exit_group (i32.sub (i32.const 0) (call $vhangup)))
          unreachable))
    "#;

    let run = run(wat).await;

    assert_eq!(run.status.unwrap(), ENOSYS);
}

#[tokio::test]
async fn returning_from_start_is_an_error() {
    let wat = module(r#"(func (export "_start"))"#);

    let run = run(&wat).await;

    assert!(matches!(
        run.status,
        Err(RunError::Exec(ExecError::ReturnedFromStart))
    ));
}
