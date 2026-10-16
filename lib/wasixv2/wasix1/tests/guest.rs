//! WASIX v1 guests, hand-written in WAT, run on a `wasixv2::Machine`.

use std::{
    io,
    sync::{Arc, Mutex},
};

use wasixv2::{ByteSink, EmptySource, LocalFuture, Machine, ProcessConfig, Stdio};

#[derive(Default)]
struct Capture(Mutex<Vec<u8>>);

impl ByteSink for Capture {
    fn write<'a>(&'a self, data: &'a [u8]) -> LocalFuture<'a, io::Result<usize>> {
        self.0.lock().unwrap().extend_from_slice(data);
        Box::pin(async move { Ok(data.len()) })
    }
}

/// Runs `wat` with `args` and returns its exit status and stdout.
async fn run(wat: &str, args: &[&str]) -> (i32, Vec<u8>) {
    let machine = Machine::new();
    let stdout = Arc::new(Capture::default());
    let config = ProcessConfig {
        args: args.iter().map(|arg| arg.to_string()).collect(),
        env: vec!["GREETING=hi".to_string()],
        stdio: Stdio {
            stdin: Arc::new(EmptySource),
            stdout: stdout.clone(),
            stderr: Arc::new(Capture::default()),
        },
    };
    let wasm = wat::parse_str(wat).unwrap();
    let status = machine.run_process(&wasm, config).await.unwrap();
    let out = stdout.0.lock().unwrap().clone();
    (status, out)
}

fn block_on(wat: &str, args: &[&str]) -> (i32, Vec<u8>) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, run(wat, args))
}

const HELLO: &str = r#"
(module
  (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 8) "\10\00\00\00\0c\00\00\00")
  (data (i32.const 16) "hello world\n")
  (func (export "_start")
    (if (call $fd_write (i32.const 1) (i32.const 8) (i32.const 1) (i32.const 32))
      (then (call $proc_exit (i32.const 1))))
    ;; nwritten
    (if (i32.ne (i32.load (i32.const 32)) (i32.const 12))
      (then (call $proc_exit (i32.const 2))))
    (call $proc_exit (i32.const 7))))
"#;

#[test]
fn hello_world() {
    let (status, stdout) = block_on(HELLO, &["hello"]);
    assert_eq!(stdout, b"hello world\n");
    assert_eq!(status, 7);
}

/// Returning from `_start` is `exit(0)` under WASI.
#[test]
fn returning_from_start_exits_zero() {
    let wat = r#"
    (module
      (import "wasi_snapshot_preview1" "sched_yield" (func $yield (result i32)))
      (memory (export "memory") 1)
      (func (export "_start") (drop (call $yield))))
    "#;
    assert_eq!(block_on(wat, &["x"]).0, 0);
}

/// Prints `argv[1]` and the environment, read back through the v1 calls.
const ARGS: &str = r#"
(module
  (import "wasi_snapshot_preview1" "args_sizes_get" (func $args_sizes_get (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "args_get" (func $args_get (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "environ_sizes_get" (func $environ_sizes_get (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "environ_get" (func $environ_get (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  (memory (export "memory") 1)
  ;; 0: argc, 4: argv buf size, 8: envc, 12: env buf size, 16: iovec
  ;; 64: argv[], 128: argv buf, 256: environ[], 320: environ buf
  (func $print (param $ptr i32) (param $len i32)
    (i32.store (i32.const 16) (local.get $ptr))
    (i32.store (i32.const 20) (local.get $len))
    (drop (call $fd_write (i32.const 1) (i32.const 16) (i32.const 1) (i32.const 24))))
  (func (export "_start")
    (drop (call $args_sizes_get (i32.const 0) (i32.const 4)))
    (drop (call $args_get (i32.const 64) (i32.const 128)))
    (drop (call $environ_sizes_get (i32.const 8) (i32.const 12)))
    (drop (call $environ_get (i32.const 256) (i32.const 320)))
    ;; "two\0" is argv[1], at argv[1] up to the end of the argv buffer
    (call $print (i32.load (i32.const 68))
      (i32.sub (i32.add (i32.const 128) (i32.load (i32.const 4))) (i32.add (i32.load (i32.const 68)) (i32.const 1))))
    (call $print (i32.const 320) (i32.sub (i32.load (i32.const 12)) (i32.const 1)))
    (call $proc_exit (i32.load (i32.const 0)))))
"#;

#[test]
fn args_and_environment() {
    let (status, stdout) = block_on(ARGS, &["one", "two"]);
    assert_eq!(status, 2);
    assert_eq!(stdout, b"twoGREETING=hi");
}

/// Exits with the errno of `clock_time_get(MONOTONIC)`, or 100 if the clock does not advance
/// or returns nothing plausible; then an invalid clock must be `EINVAL` (28).
const CLOCK: &str = r#"
(module
  (import "wasi_snapshot_preview1" "clock_time_get" (func $clock_time_get (param i32 i64 i32) (result i32)))
  (import "wasi_snapshot_preview1" "clock_res_get" (func $clock_res_get (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  (memory (export "memory") 1)
  (func (export "_start")
    (local $errno i32)
    (local.set $errno (call $clock_time_get (i32.const 1) (i64.const 1) (i32.const 0)))
    (if (local.get $errno) (then (call $proc_exit (local.get $errno))))
    (local.set $errno (call $clock_time_get (i32.const 1) (i64.const 1) (i32.const 8)))
    (if (local.get $errno) (then (call $proc_exit (local.get $errno))))
    (if (i64.lt_u (i64.load (i32.const 8)) (i64.load (i32.const 0)))
      (then (call $proc_exit (i32.const 100))))
    (local.set $errno (call $clock_res_get (i32.const 0) (i32.const 16)))
    (if (local.get $errno) (then (call $proc_exit (local.get $errno))))
    (if (i64.eqz (i64.load (i32.const 16)))
      (then (call $proc_exit (i32.const 101))))
    (call $proc_exit (call $clock_time_get (i32.const 99) (i64.const 1) (i32.const 0)))))
"#;

#[test]
fn clocks() {
    assert_eq!(block_on(CLOCK, &["clock"]).0, 28);
}

/// `fd_prestat_get` announces `/` on 3, `.` on 4 and `/` on 5, as v1 did, and `EBADF` (8)
/// after them.
const PREOPENS: &str = r#"
(module
  (import "wasi_snapshot_preview1" "fd_prestat_get" (func $prestat_get (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_prestat_dir_name" (func $dir_name (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  (memory (export "memory") 1)
  (func $print_name (param $fd i32)
    (if (call $prestat_get (local.get $fd) (i32.const 0)) (then (call $proc_exit (i32.const 50))))
    (if (call $dir_name (local.get $fd) (i32.const 100) (i32.load (i32.const 4)))
      (then (call $proc_exit (i32.const 51))))
    (i32.store (i32.const 16) (i32.const 100))
    (i32.store (i32.const 20) (i32.load (i32.const 4)))
    (drop (call $fd_write (i32.const 1) (i32.const 16) (i32.const 1) (i32.const 24))))
  (func (export "_start")
    (call $print_name (i32.const 3))
    (call $print_name (i32.const 4))
    (call $print_name (i32.const 5))
    (call $proc_exit (call $prestat_get (i32.const 6) (i32.const 0)))))
"#;

#[test]
fn preopens() {
    let (status, stdout) = block_on(PREOPENS, &["preopens"]);
    assert_eq!(stdout, b"/./");
    assert_eq!(status, 8);
}

/// Stubbed `wasix_32v1` calls return their documented errno: `NOTSUP` (58) for what the v2
/// kernel does not serve, `NOSYS` (52) for what is not implemented yet. The random bytes and
/// thread id must be served.
const WASIX: &str = r#"
(module
  (import "wasix_32v1" "port_addr_clear" (func $port_addr_clear (result i32)))
  (import "wasix_32v1" "dlopen" (func $dlopen (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
  (import "wasix_32v1" "random_get" (func $random_get (param i32 i32) (result i32)))
  (import "wasix_32v1" "thread_id" (func $thread_id (param i32) (result i32)))
  (import "wasix_32v1" "proc_id" (func $proc_id (param i32) (result i32)))
  (import "wasix_32v1" "proc_exit2" (func $proc_exit2 (param i32)))
  (import "wasix_32v1" "proc_signals_sizes_get" (func $proc_signals_sizes_get (param i32) (result i32)))
  (import "wasix_32v1" "proc_signals_get" (func $proc_signals_get (param i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "_start")
    (if (i32.ne (call $port_addr_clear) (i32.const 58)) (then (call $proc_exit2 (i32.const 1))))
    (if (i32.ne (call $dlopen (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0)
                              (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0)) (i32.const 58))
      (then (call $proc_exit2 (i32.const 2))))
    (if (call $random_get (i32.const 200) (i32.const 64)) (then (call $proc_exit2 (i32.const 3))))
    (if (call $thread_id (i32.const 100)) (then (call $proc_exit2 (i32.const 4))))
    (if (call $proc_id (i32.const 104)) (then (call $proc_exit2 (i32.const 5))))
    ;; the main thread's id is its pid
    (if (i32.ne (i32.load (i32.const 100)) (i32.load (i32.const 104)))
      (then (call $proc_exit2 (i32.const 6))))
    ;; libc's startup fails hard unless these succeed; every signal but SIGKILL and SIGSTOP
    ;; is reported, at its default here
    (if (call $proc_signals_sizes_get (i32.const 108)) (then (call $proc_exit2 (i32.const 7))))
    (if (i32.ne (i32.load (i32.const 108)) (i32.const 29)) (then (call $proc_exit2 (i32.const 8))))
    (if (call $proc_signals_get (i32.const 112)) (then (call $proc_exit2 (i32.const 9))))
    (call $proc_exit2 (i32.const 0))))
"#;

#[test]
fn wasix_stubs_and_identity() {
    assert_eq!(block_on(WASIX, &["wasix"]).0, 0);
}

/// A module importing every entry of the import table with its declared signature links, so
/// shims and stubs have the types the table (and wasix-libc) give them.
#[test]
fn every_import_links() {
    use wasixv2_wasix1::imports::TABLE;

    let ty = |ty: &wasmer::Type| match ty {
        wasmer::Type::I32 => "i32",
        wasmer::Type::I64 => "i64",
        other => panic!("unexpected {other:?}"),
    };
    let mut wat = String::from("(module\n");
    for entry in TABLE {
        for module in entry.modules {
            let params: Vec<_> = entry.params.iter().map(ty).collect();
            let results: Vec<_> = entry.results.iter().map(ty).collect();
            wat.push_str(&format!(
                "  (import \"{}\" \"{}\" (func (param {}) (result {})))\n",
                module.name(),
                entry.name,
                params.join(" "),
                results.join(" "),
            ));
        }
    }
    wat.push_str("  (memory (export \"memory\") 1)\n  (func (export \"_start\")))\n");
    assert_eq!(block_on(&wat, &["all"]).0, 0);
}
