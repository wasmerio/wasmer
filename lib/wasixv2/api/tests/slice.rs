//! First vertical slice: hand-written guests with no libc, run through the whole stack.

use std::{
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};

use tokio::sync::mpsc;

use wasixv2::{ByteSink, ByteSource, EmptySource, LocalFuture, Machine, RunError, Stdio};

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

/// Exit statuses of death by a signal, as `wasmer run` reports them: 128 + the signal.
const SIGILL: i32 = 128 + 4;
const SIGFPE: i32 = 128 + 8;
const SIGSEGV: i32 = 128 + 11;
const SIGTERM: i32 = 128 + 15;

// Linux kills a process whose entry point returns with SIGSEGV.
#[tokio::test]
async fn returning_from_start_kills_with_sigsegv() {
    let wat = module(r#"(func (export "_start"))"#);

    let run = run(&wat).await;

    assert_eq!(run.status.unwrap(), SIGSEGV);
}

#[tokio::test]
async fn traps_kill_with_the_signal_of_their_fault() {
    let trap = |body: &str| module(&format!(r#"(func (export "_start") {body} unreachable)"#));

    let unreachable = run(&trap("")).await;
    let divide = run(&trap("(drop (i32.div_s (i32.const 1) (i32.const 0)))")).await;
    let load = run(&trap("(drop (i32.load (i32.const 0x7fff_fff0)))")).await;

    assert_eq!(unreachable.status.unwrap(), SIGILL);
    assert_eq!(divide.status.unwrap(), SIGFPE);
    assert_eq!(load.status.unwrap(), SIGSEGV);
}

// The guest never makes a syscall, so only a preemption point can deliver the signal.
#[tokio::test]
async fn a_busy_guest_takes_a_signal_at_a_preemption_point() {
    let wasm = wat::parse_str(module(r#"(func (export "_start") (loop (br 0)))"#)).unwrap();
    let machine = Machine::new();
    let signaller = machine.signaller();
    let ended = Arc::new(AtomicBool::new(false));
    // A signal sent before the process starts (while its module compiles) reaches no one, so
    // keep sending until the run ends.
    let kill = std::thread::spawn({
        let ended = ended.clone();
        move || {
            while !ended.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(50));
                signaller.signal(15).unwrap();
            }
        }
    });
    let stdio = Stdio {
        stdin: Arc::new(EmptySource),
        stdout: Arc::new(Capture::default()),
        stderr: Arc::new(Capture::default()),
    };

    let status = tokio::time::timeout(Duration::from_secs(10), machine.run(&wasm, stdio)).await;

    ended.store(true, Ordering::Relaxed);
    kill.join().unwrap();
    assert_eq!(status.expect("the guest took the signal").unwrap(), SIGTERM);
}

/// A module laid out as lld's `--stack-first` does, as libc2 programs are: the main stack from
/// 0 up to `__stack_pointer` at 128 KiB, the data above it.
fn stack_first(body: &str) -> String {
    format!(
        r#"(module
          (import "wasixv2" "read" (func $read (param i32 i32 i32) (result i32)))
          (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
          (import "wasixv2" "exit_group" (func $exit_group (param i32)))
          (memory (export "memory") 3)
          (global (export "__stack_pointer") (mut i32) (i32.const 0x20000))
          (global (export "__data_end") i32 (i32.const 0x20010))
          (data (i32.const 0x20000) "ok\n")
          {body})"#
    )
}

// The first 64 KiB of a stack-first module's memory are a read-only guard, where Linux leaves the
// pages below `mmap_min_addr` unmapped: a store through a null pointer kills the guest. Loads
// read zeros, since clang's wasm32 backend passes a null va_list buffer to variadic calls
// without variadic arguments and musl reads it anyway (DESIGN.md, "Guard page at 0").
#[tokio::test]
async fn a_stack_first_guest_faults_on_a_store_through_a_null_pointer() {
    let trap = |body: &str| stack_first(&format!(r#"(func (export "_start") {body} unreachable)"#));

    let store = run(&trap("(i32.store (i32.const 0) (i32.const 1))")).await;
    let last = run(&trap("(i32.store (i32.const 0xfffc) (i32.const 1))")).await;
    let load = run(&stack_first(
        r#"(func (export "_start")
          (call $exit_group (i32.or (i32.load (i32.const 0)) (i32.load (i32.const 0xfffc))))
          unreachable)"#,
    ))
    .await;
    let data = run(&stack_first(
        r#"(func (export "_start")
          (drop (call $write (i32.const 1) (i32.const 0x20000) (i32.const 3)))
          (call $exit_group (i32.const 0))
          unreachable)"#,
    ))
    .await;

    assert_eq!(store.status.unwrap(), SIGSEGV);
    assert_eq!(last.status.unwrap(), SIGSEGV);
    assert_eq!(load.status.unwrap(), 0);
    assert_eq!(data.status.unwrap(), 0);
    assert_eq!(data.stdout, b"ok\n");
}

// Syscalls that write to a buffer in the guard fail with EFAULT, as on a buffer below
// `mmap_min_addr`; reading one succeeds, as the guest's own loads do.
#[tokio::test]
async fn syscalls_fail_with_efault_writing_into_the_guard() {
    const EFAULT: i32 = 14;
    let errno_of = |call: &str| {
        stack_first(&format!(
            r#"(func (export "_start")
              (call $exit_group (i32.sub (i32.const 0) {call}))
              unreachable)"#
        ))
    };
    // stdin is open and empty, so `read` reaches the buffer only once data arrives.
    let read_into = |addr: u32, len: u32| {
        let (input, stdin) = mpsc::unbounded_channel();
        input.send(b"xyz".to_vec()).unwrap();
        let call = format!("(call $read (i32.const 0) (i32.const {addr}) (i32.const {len}))");
        (errno_of(&call), stdin)
    };

    let (wat, stdin) = read_into(0, 1);
    let read = run_with_stdin(&wat, stdin).await;
    let (wat, stdin) = read_into(0xfffe, 4);
    let straddling = run_with_stdin(&wat, stdin).await;
    // An empty buffer is never touched.
    let (wat, stdin) = read_into(0, 0);
    let empty = run_with_stdin(&wat, stdin).await;
    // The kernel reading from the guard: `write` returns 1, so the guest exits with -1.
    let write = run(&errno_of(
        "(call $write (i32.const 1) (i32.const 4096) (i32.const 1))",
    ))
    .await;

    assert_eq!(read.status.unwrap(), EFAULT);
    assert_eq!(straddling.status.unwrap(), EFAULT);
    assert_eq!(empty.status.unwrap(), 0);
    assert_eq!(write.status.unwrap(), 255);
    assert_eq!(write.stdout, [0]);
}

// A module with its data first, or with a stack too small to spare 64 KiB, keeps address 0
// usable: something may live there.
#[tokio::test]
async fn other_layouts_keep_address_zero() {
    let at_zero = |stack_pointer: u32, data_end: u32| {
        format!(
            r#"(module
              (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
              (import "wasixv2" "exit_group" (func $exit_group (param i32)))
              (memory (export "memory") 3)
              (global (export "__stack_pointer") (mut i32) (i32.const {stack_pointer}))
              (global (export "__data_end") i32 (i32.const {data_end}))
              (func (export "_start")
                (i32.store (i32.const 0) (i32.const 0x0a6b6f))
                (drop (call $write (i32.const 1) (i32.const 0) (i32.const 3)))
                (call $exit_group (i32.load (i32.const 0x1000)))
                unreachable))"#
        )
    };

    let data_first = run(&at_zero(0x20000 + 0x1000, 0x1000)).await;
    let small_stack = run(&at_zero(0x10000, 0x10010)).await;

    for run in [data_first, small_stack] {
        assert_eq!(run.status.unwrap(), 0);
        assert_eq!(run.stdout, b"ok\n");
    }
}
