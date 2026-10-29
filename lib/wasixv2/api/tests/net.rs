//! Networking through the whole stack: a hand-written guest's socket syscalls reach the
//! machine's vnet namespace.

#![recursion_limit = "256"]

use std::{
    io,
    sync::{Arc, Mutex},
};

use wasixv2::{ByteSink, EmptySource, LocalFuture, Machine, Stdio};

#[derive(Default)]
struct Capture(Mutex<Vec<u8>>);

impl ByteSink for Capture {
    fn write<'a>(&'a self, data: &'a [u8]) -> LocalFuture<'a, io::Result<usize>> {
        self.0.lock().unwrap().extend_from_slice(data);
        Box::pin(async move { Ok(data.len()) })
    }
}

// A failing call exits with `step << 8 | errno`.
#[tokio::test]
async fn loopback_tcp_carries_data_between_two_sockets() {
    let wat = r#"
      (module
        (import "wasixv2" "socket" (func $socket (param i32 i32 i32) (result i32)))
        (import "wasixv2" "bind" (func $bind (param i32 i32 i32) (result i32)))
        (import "wasixv2" "listen" (func $listen (param i32 i32) (result i32)))
        (import "wasixv2" "connect" (func $connect (param i32 i32 i32) (result i32)))
        (import "wasixv2" "accept" (func $accept (param i32 i32 i32) (result i32)))
        (import "wasixv2" "read" (func $read (param i32 i32 i32) (result i32)))
        (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
        (import "wasixv2" "exit_group" (func $exit_group (param i32)))
        (memory (export "memory") 1)
        ;; sockaddr_in for 127.0.0.1:8080
        (data (i32.const 16) "\02\00\1f\90\7f\00\00\01\00\00\00\00\00\00\00\00")
        (data (i32.const 32) "ping")

        (func $check (param $v i32) (param $step i32) (result i32)
          (if (i32.lt_s (local.get $v) (i32.const 0))
            (then
              (call $exit_group
                (i32.or (i32.shl (local.get $step) (i32.const 8))
                        (i32.sub (i32.const 0) (local.get $v))))
              unreachable))
          (local.get $v))

        (func (export "_start") (local $srv i32) (local $cli i32) (local $conn i32) (local $n i32)
          ;; AF_INET, SOCK_STREAM
          (local.set $srv (call $check (call $socket (i32.const 2) (i32.const 1) (i32.const 0)) (i32.const 1)))
          (drop (call $check (call $bind (local.get $srv) (i32.const 16) (i32.const 16)) (i32.const 2)))
          (drop (call $check (call $listen (local.get $srv) (i32.const 1)) (i32.const 3)))
          (local.set $cli (call $check (call $socket (i32.const 2) (i32.const 1) (i32.const 0)) (i32.const 4)))
          (drop (call $check (call $connect (local.get $cli) (i32.const 16) (i32.const 16)) (i32.const 5)))
          (local.set $conn (call $check (call $accept (local.get $srv) (i32.const 0) (i32.const 0)) (i32.const 6)))
          (drop (call $check (call $write (local.get $cli) (i32.const 32) (i32.const 4)) (i32.const 7)))
          (local.set $n (call $check (call $read (local.get $conn) (i32.const 64) (i32.const 16)) (i32.const 8)))
          (drop (call $check (call $write (i32.const 1) (i32.const 64) (local.get $n)) (i32.const 9)))
          (call $exit_group (i32.const 0))
          unreachable))
    "#;
    let wasm = wat::parse_str(wat).expect("valid test module");
    let stdout = Arc::new(Capture::default());
    let stdio = Stdio {
        stdin: Arc::new(EmptySource),
        stdout: stdout.clone(),
        stderr: Arc::new(Capture::default()),
    };

    let status = Machine::new().run(&wasm, stdio).await.unwrap();

    assert_eq!(status, 0, "step {}, errno {}", status >> 8, status & 0xff);
    assert_eq!(*stdout.0.lock().unwrap(), b"ping");
}
