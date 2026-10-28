//! A machine with the host's console: programs read what is typed through the kernel's line
//! discipline, and `^C` interrupts the console's foreground process group.

use std::{
    io,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};

use tokio::sync::mpsc;
use wasixv2::{
    ByteSink, ByteSource, Console, EmptySource, LocalFuture, Machine, Stdio, WindowSize,
};

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

/// The host's terminal: what the test types.
struct Keyboard(Mutex<mpsc::UnboundedReceiver<Vec<u8>>>);

impl ByteSource for Keyboard {
    fn poll_read(&self, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        let mut input = self.0.lock().unwrap();
        let Some(data) = std::task::ready!(input.poll_recv(cx)) else {
            return Poll::Ready(Ok(0));
        };
        buf[..data.len()].copy_from_slice(&data);
        Poll::Ready(Ok(data.len()))
    }
}

/// A machine whose console shows on `screen` what the returned sender types.
fn console_machine(screen: &Arc<Capture>) -> (Machine, mpsc::UnboundedSender<Vec<u8>>) {
    let (keys, input) = mpsc::unbounded_channel();
    let console = Console {
        input: Arc::new(Keyboard(Mutex::new(input))),
        output: screen.clone(),
        size: WindowSize {
            rows: 33,
            cols: 100,
            xpixel: 0,
            ypixel: 0,
        },
    };
    let machine = Machine::builder().console(console, [true; 3]).build();
    (machine, keys)
}

/// Stdio that must not be used: the console takes descriptors 0 to 2.
fn unused_stdio() -> (Stdio, Arc<Capture>) {
    let unused = Arc::new(Capture::default());
    let stdio = Stdio {
        stdin: Arc::new(EmptySource),
        stdout: unused.clone(),
        stderr: unused.clone(),
    };
    (stdio, unused)
}

/// Writes `ready\n`, reads a line from descriptor 0 and writes it back to descriptor 1, then
/// exits with the console's row count if it is its controlling terminal with its own group
/// in the foreground, or 99.
const LINE_WAT: &str = r#"
  (module
    (import "wasixv2" "read" (func $read (param i32 i32 i32) (result i32)))
    (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
    (import "wasixv2" "ioctl" (func $ioctl (param i32 i32 i32) (result i32)))
    (import "wasixv2" "getpid" (func $getpid (result i32)))
    (import "wasixv2" "exit_group" (func $exit_group (param i32)))
    (memory (export "memory") 1)
    (data (i32.const 16) "ready\n")
    (func (export "_start") (local $n i32)
      (drop (call $write (i32.const 1) (i32.const 16) (i32.const 6)))
      (local.set $n (call $read (i32.const 0) (i32.const 64) (i32.const 32)))
      (drop (call $write (i32.const 1) (i32.const 64) (local.get $n)))
      ;; TIOCGPGRP into 128, TIOCGWINSZ into 136.
      (if (i32.or
            (call $ioctl (i32.const 0) (i32.const 0x540f) (i32.const 128))
            (i32.ne (i32.load (i32.const 128)) (call $getpid)))
        (then (call $exit_group (i32.const 99))))
      (drop (call $ioctl (i32.const 2) (i32.const 0x5413) (i32.const 136)))
      (call $exit_group (i32.load16_u (i32.const 136)))
      unreachable))
"#;

async fn shown(screen: &Capture, what: &[u8]) {
    while !screen.contents().ends_with(what) {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// What the host types is echoed and edited by the kernel and read as a line; the program's
/// output gets CR LF; the console is the program's controlling terminal, with its size.
#[tokio::test]
async fn a_program_reads_what_is_typed_on_the_console() {
    let screen = Arc::new(Capture::default());
    let (machine, keys) = console_machine(&screen);
    let wasm = wat::parse_str(LINE_WAT).unwrap();
    let (stdio, unused) = unused_stdio();
    let typing = async {
        shown(&screen, b"ready\r\n").await;
        keys.send(b"hix\x7f\r".to_vec()).unwrap();
    };
    let (status, ()) = tokio::time::timeout(Duration::from_secs(30), async {
        tokio::join!(machine.run(&wasm, stdio), typing)
    })
    .await
    .expect("the program read its line");

    assert_eq!(status.unwrap(), 33);
    assert_eq!(screen.contents(), b"ready\r\nhix\x08 \x08\r\nhi\r\n");
    assert!(unused.contents().is_empty());
}

/// Writes `ready\n` and waits for input forever.
const WAIT_WAT: &str = r#"
  (module
    (import "wasixv2" "read" (func $read (param i32 i32 i32) (result i32)))
    (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 16) "ready\n")
    (func (export "_start")
      (drop (call $write (i32.const 1) (i32.const 16) (i32.const 6)))
      (loop $forever
        (drop (call $read (i32.const 0) (i32.const 64) (i32.const 32)))
        (br $forever))))
"#;

/// `^C` typed on the console kills a program with SIGINT's default action.
#[tokio::test]
async fn ctrl_c_on_the_console_interrupts_the_program() {
    let screen = Arc::new(Capture::default());
    let (machine, keys) = console_machine(&screen);
    let wasm = wat::parse_str(WAIT_WAT).unwrap();
    let (stdio, _) = unused_stdio();
    let typing = async {
        shown(&screen, b"ready\r\n").await;
        keys.send(b"\x03".to_vec()).unwrap();
    };
    let (status, ()) = tokio::time::timeout(Duration::from_secs(30), async {
        tokio::join!(machine.run(&wasm, stdio), typing)
    })
    .await
    .expect("the program died");

    assert_eq!(status.unwrap(), 128 + 2);
    assert_eq!(screen.contents(), b"ready\r\n^C");
}

/// The signaller's terminal signals go to the console's foreground process group, and a
/// hangup of the console sends its session leader SIGHUP.
#[tokio::test]
async fn the_signaller_reaches_the_console() {
    for (sig, status) in [(Some(3), 128 + 3), (None, 128 + 1)] {
        let screen = Arc::new(Capture::default());
        let (machine, _keys) = console_machine(&screen);
        let signaller = machine.signaller();
        let wasm = wat::parse_str(WAIT_WAT).unwrap();
        let (stdio, _) = unused_stdio();
        let signalling = async {
            shown(&screen, b"ready\r\n").await;
            match sig {
                Some(sig) => assert!(signaller.signal_foreground(sig).unwrap()),
                None => signaller.console_hangup(),
            }
        };
        let (run, ()) = tokio::time::timeout(Duration::from_secs(30), async {
            tokio::join!(machine.run(&wasm, stdio), signalling)
        })
        .await
        .expect("the program died");
        assert_eq!(run.unwrap(), status);
    }
}
