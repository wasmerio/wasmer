//! Machine configuration through the whole stack: the user processes run as and what `uname`
//! reports, seen by hand-written guests.

use std::{
    io,
    sync::{Arc, Mutex},
};

use wasixv2::{ByteSink, Credentials, EmptySource, LocalFuture, Machine, Stdio, Uname};

#[derive(Default)]
struct Capture(Mutex<Vec<u8>>);

impl ByteSink for Capture {
    fn write<'a>(&'a self, data: &'a [u8]) -> LocalFuture<'a, io::Result<usize>> {
        self.0.lock().unwrap().extend_from_slice(data);
        Box::pin(async move { Ok(data.len()) })
    }
}

async fn run(machine: Machine, wat: &str) -> (i32, Vec<u8>) {
    let wasm = wat::parse_str(wat).expect("valid test module");
    let stdout = Arc::new(Capture::default());
    let stdio = Stdio {
        stdin: Arc::new(EmptySource),
        stdout: stdout.clone(),
        stderr: Arc::new(Capture::default()),
    };
    let status = machine.run(&wasm, stdio).await.unwrap();
    let stdout = stdout.0.lock().unwrap().clone();
    (status, stdout)
}

/// Prints `uname -n` and `uname -r`, space-separated, and exits with its effective uid.
const UNAME_WAT: &str = r#"
  (module
    (import "wasixv2" "uname" (func $uname (param i32) (result i32)))
    (import "wasixv2" "geteuid" (func $geteuid (result i32)))
    (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
    (import "wasixv2" "exit_group" (func $exit_group (param i32)))
    (memory (export "memory") 1)
    (data (i32.const 8) " ")

    (func $strlen (param $p i32) (result i32) (local $n i32)
      (block $done
        (loop $next
          (br_if $done (i32.eqz (i32.load8_u (i32.add (local.get $p) (local.get $n)))))
          (local.set $n (i32.add (local.get $n) (i32.const 1)))
          (br $next)))
      (local.get $n))

    (func $print (param $p i32)
      (drop (call $write (i32.const 1) (local.get $p) (call $strlen (local.get $p)))))

    (func (export "_start")
      (drop (call $uname (i32.const 1024)))
      ;; nodename is the second 65-byte field, release the third.
      (call $print (i32.const 1089))
      (drop (call $write (i32.const 1) (i32.const 8) (i32.const 1)))
      (call $print (i32.const 1154))
      (call $exit_group (call $geteuid))
      unreachable))
"#;

#[tokio::test]
async fn uname_and_user_come_from_the_machine_configuration() {
    let uname = Uname::default()
        .with_nodename("box")
        .and_then(|uname| uname.with_release("6.1.0"))
        .unwrap();
    let machine = Machine::builder()
        .user(Credentials::new(42, 42))
        .uname(uname)
        .build();

    assert_eq!(run(machine, UNAME_WAT).await, (42, b"box 6.1.0".to_vec()));
    assert_eq!(
        run(Machine::new(), UNAME_WAT).await,
        (1, b"localhost 2.0.0".to_vec())
    );
}

/// Binds TCP port 80, then creates `/f`, gives it to uid 1234 and makes it mode 0, and opens
/// it for reading. Exits with 0, or with the step that failed after printing its errno as one
/// byte.
const ROOT_ONLY_WAT: &str = r#"
  (module
    (import "wasixv2" "socket" (func $socket (param i32 i32 i32) (result i32)))
    (import "wasixv2" "bind" (func $bind (param i32 i32 i32) (result i32)))
    (import "wasixv2" "openat" (func $openat (param i32 i32 i32 i32) (result i32)))
    (import "wasixv2" "fchownat" (func $fchownat (param i32 i32 i32 i32 i32) (result i32)))
    (import "wasixv2" "fchmodat" (func $fchmodat (param i32 i32 i32) (result i32)))
    (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
    (import "wasixv2" "exit_group" (func $exit_group (param i32)))
    (memory (export "memory") 1)
    ;; sockaddr_in for 127.0.0.1:80
    (data (i32.const 16) "\02\00\00\50\7f\00\00\01\00\00\00\00\00\00\00\00")
    (data (i32.const 32) "/f\00")

    (func $check (param $v i32) (param $step i32) (result i32)
      (if (i32.lt_s (local.get $v) (i32.const 0))
        (then
          (i32.store8 (i32.const 0) (i32.sub (i32.const 0) (local.get $v)))
          (drop (call $write (i32.const 1) (i32.const 0) (i32.const 1)))
          (call $exit_group (local.get $step))
          unreachable))
      (local.get $v))

    (func (export "_start") (local $fd i32)
      (local.set $fd (call $check (call $socket (i32.const 2) (i32.const 1) (i32.const 0)) (i32.const 1)))
      (drop (call $check (call $bind (local.get $fd) (i32.const 16) (i32.const 16)) (i32.const 2)))
      ;; O_WRONLY | O_CREAT, 0600
      (drop (call $check (call $openat (i32.const -100) (i32.const 32) (i32.const 0x41) (i32.const 384)) (i32.const 3)))
      (drop (call $check (call $fchownat (i32.const -100) (i32.const 32) (i32.const 1234) (i32.const 1234) (i32.const 0)) (i32.const 4)))
      (drop (call $check (call $fchmodat (i32.const -100) (i32.const 32) (i32.const 0)) (i32.const 5)))
      (drop (call $check (call $openat (i32.const -100) (i32.const 32) (i32.const 0) (i32.const 0)) (i32.const 6)))
      (call $exit_group (i32.const 0))
      unreachable))
"#;

const EACCES: u8 = 13;

#[tokio::test]
async fn root_binds_low_ports_and_overrides_file_permissions() {
    let root = Machine::builder().user(Credentials::new(0, 0)).build();
    assert_eq!(run(root, ROOT_ONLY_WAT).await, (0, Vec::new()));

    // The unprivileged default user fails at the bind.
    assert_eq!(run(Machine::new(), ROOT_ONLY_WAT).await, (2, vec![EACCES]));
}

/// The default environment names the user's home directory from `/etc/passwd` and musl's
/// search path, which holds `/bin` and `/usr/bin`, where commands are installed.
#[tokio::test]
async fn the_default_environment_has_the_users_home_and_a_path() {
    let path = "PATH=/usr/local/bin:/bin:/usr/bin";
    assert_eq!(Machine::new().default_env(), ["HOME=/home/user", path]);
    let root = Machine::builder().user(Credentials::new(0, 0)).build();
    assert_eq!(root.default_env(), ["HOME=/root", path]);
}
