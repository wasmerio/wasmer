//! WASIX v1 networking guests, hand-written in WAT, run on a `wasixv2::Machine`: sockets over
//! the loopback network, `poll_oneoff`, epoll, eventfd, socket options, `resolve` and the
//! `port_*` queries. Each guest exits with 0 on success or the number of the check that
//! failed.
//!
//! `c_loopback` also builds and runs a C program against wasix-libc when a v1 compiler is
//! configured (see there).

#![recursion_limit = "256"]

use std::{
    io,
    path::PathBuf,
    process::Command,
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

/// Runs `wasm` and returns its exit status, stdout and stderr.
fn run_wasm(wasm: &[u8], args: &[&str]) -> (i32, String, String) {
    run_wasm_on(Machine::new, wasm, args)
}

/// [`run_wasm`] on the machine `machine` makes.
fn run_wasm_on(
    machine: impl FnOnce() -> Machine,
    wasm: &[u8],
    args: &[&str],
) -> (i32, String, String) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, async {
        let machine = machine();
        let stdout = Arc::new(Capture::default());
        let stderr = Arc::new(Capture::default());
        let config = ProcessConfig {
            args: args.iter().map(|arg| arg.to_string()).collect(),
            env: Vec::new(),
            stdio: Stdio {
                stdin: Arc::new(EmptySource),
                stdout: stdout.clone(),
                stderr: stderr.clone(),
            },
        };
        let status = machine.run_process(wasm, config).await.unwrap();
        let out = String::from_utf8_lossy(&stdout.0.lock().unwrap()).into_owned();
        let err = String::from_utf8_lossy(&stderr.0.lock().unwrap()).into_owned();
        (status, out, err)
    })
}

/// The imports and helpers every guest shares; `$body` is the guest's `_start`.
const PRELUDE: &str = r#"
  (import "wasix_32v1" "sock_open" (func $sock_open (param i32 i32 i32 i32) (result i32)))
  (import "wasix_32v1" "sock_bind" (func $sock_bind (param i32 i32) (result i32)))
  (import "wasix_32v1" "sock_listen" (func $sock_listen (param i32 i32) (result i32)))
  (import "wasix_32v1" "sock_connect" (func $sock_connect (param i32 i32) (result i32)))
  (import "wasix_32v1" "sock_accept_v2" (func $sock_accept_v2 (param i32 i32 i32 i32) (result i32)))
  (import "wasix_32v1" "sock_addr_local" (func $sock_addr_local (param i32 i32) (result i32)))
  (import "wasix_32v1" "sock_addr_peer" (func $sock_addr_peer (param i32 i32) (result i32)))
  (import "wasix_32v1" "sock_status" (func $sock_status (param i32 i32) (result i32)))
  (import "wasix_32v1" "sock_send" (func $sock_send (param i32 i32 i32 i32 i32) (result i32)))
  (import "wasix_32v1" "sock_recv" (func $sock_recv (param i32 i32 i32 i32 i32 i32) (result i32)))
  (import "wasix_32v1" "sock_send_to" (func $sock_send_to (param i32 i32 i32 i32 i32 i32) (result i32)))
  (import "wasix_32v1" "sock_recv_from" (func $sock_recv_from (param i32 i32 i32 i32 i32 i32 i32) (result i32)))
  (import "wasix_32v1" "sock_shutdown" (func $sock_shutdown (param i32 i32) (result i32)))
  (import "wasix_32v1" "sock_set_opt_flag" (func $set_flag (param i32 i32 i32) (result i32)))
  (import "wasix_32v1" "sock_get_opt_flag" (func $get_flag (param i32 i32 i32) (result i32)))
  (import "wasix_32v1" "sock_set_opt_size" (func $set_size (param i32 i32 i64) (result i32)))
  (import "wasix_32v1" "sock_get_opt_size" (func $get_size (param i32 i32 i32) (result i32)))
  (import "wasix_32v1" "sock_set_opt_time" (func $set_time (param i32 i32 i32) (result i32)))
  (import "wasix_32v1" "sock_get_opt_time" (func $get_time (param i32 i32 i32) (result i32)))
  (import "wasix_32v1" "poll_oneoff" (func $poll_oneoff (param i32 i32 i32 i32) (result i32)))
  (import "wasi_unstable" "poll_oneoff" (func $poll_oneoff0 (param i32 i32 i32 i32) (result i32)))
  (import "wasix_32v1" "epoll_create" (func $epoll_create (param i32) (result i32)))
  (import "wasix_32v1" "epoll_ctl" (func $epoll_ctl (param i32 i32 i32 i32) (result i32)))
  (import "wasix_32v1" "epoll_wait" (func $epoll_wait (param i32 i32 i32 i64 i32) (result i32)))
  (import "wasix_32v1" "fd_event" (func $fd_event (param i64 i32 i32) (result i32)))
  (import "wasix_32v1" "fd_dup" (func $fd_dup (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_renumber" (func $fd_renumber (param i32 i32) (result i32)))
  (import "wasix_32v1" "resolve" (func $resolve (param i32 i32 i32 i32 i32 i32) (result i32)))
  (import "wasix_32v1" "port_addr_list" (func $port_addr_list (param i32 i32) (result i32)))
  (import "wasix_32v1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasix_32v1" "fd_close" (func $fd_close (param i32) (result i32)))
  (import "wasix_32v1" "proc_exit2" (func $exit (param i32)))
  (import "wasix_32v1" "proc_fork_env" (func $vfork (param i32) (result i32)))
  (import "wasix_32v1" "proc_join" (func $join (param i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  ;; Exits with `code` unless `errno` is 0.
  (func $ok (param $errno i32) (param $code i32)
    (if (local.get $errno) (then (call $exit (local.get $code)))))
  ;; Exits with `code` unless `cond` holds.
  (func $assert (param $cond i32) (param $code i32)
    (if (i32.eqz (local.get $cond)) (then (call $exit (local.get $code)))))
  ;; Writes `len` bytes at `ptr` to stdout, through the iovec at 4080.
  (func $print (param $ptr i32) (param $len i32)
    (i32.store (i32.const 4080) (local.get $ptr))
    (i32.store (i32.const 4084) (local.get $len))
    (drop (call $fd_write (i32.const 1) (i32.const 4080) (i32.const 1) (i32.const 4088))))
"#;

/// A guest module of the prelude, `data` segments and a `_start` with `body`.
fn guest(data: &str, body: &str) -> Vec<u8> {
    let wat = format!(
        "(module {PRELUDE}\n{data}\n(func (export \"_start\")\n{body}\n(call $exit (i32.const 0))))"
    );
    wat::parse_str(&wat).unwrap_or_else(|err| panic!("{err}\n{wat}"))
}

fn run(data: &str, body: &str) -> (i32, String) {
    let (status, out, err) = run_wasm(&guest(data, body), &["net"]);
    assert!(err.is_empty(), "stderr: {err}");
    (status, out)
}

/// `127.0.0.1:0` as an `addr_port`, at 1024.
const LOOPBACK_ADDR: &str = r#"(data (i32.const 1024) "\01\00\00\00\7f\00\00\01")"#;

/// A TCP echo over the loopback network: listen, connect, accept, send, `poll_oneoff` for
/// the data, receive and send it back.
#[test]
fn tcp_loopback_echo() {
    let data = format!(
        r#"{LOOPBACK_ADDR}
        (data (i32.const 2000) "ping")
        ;; iovecs: 1600 send "ping", 1610 receive at 2100, 1620 echo 4 bytes, 1630 receive at 2200
        (data (i32.const 1600) "\d0\07\00\00\04\00\00\00")
        (data (i32.const 1610) "\34\08\00\00\10\00\00\00")
        (data (i32.const 1620) "\34\08\00\00\04\00\00\00")
        (data (i32.const 1630) "\98\08\00\00\10\00\00\00")
        ;; subscription at 3000: userdata 42, FD_READ; the fd is filled in
        (data (i32.const 3000) "\2a\00\00\00\00\00\00\00\01")"#
    );
    let body = r#"
    ;; server at [0], bound to an ephemeral port on 127.0.0.1
    (call $ok (call $sock_open (i32.const 1) (i32.const 1) (i32.const 6) (i32.const 0)) (i32.const 1))
    (call $ok (call $sock_bind (i32.load (i32.const 0)) (i32.const 1024)) (i32.const 2))
    (call $ok (call $sock_listen (i32.load (i32.const 0)) (i32.const 8)) (i32.const 3))
    (call $ok (call $sock_addr_local (i32.load (i32.const 0)) (i32.const 1200)) (i32.const 4))
    (call $assert (i32.eq (i32.load8_u (i32.const 1200)) (i32.const 1)) (i32.const 40))
    (call $assert (i32.ne (i32.load16_u (i32.const 1202)) (i32.const 0)) (i32.const 41))
    (call $assert (i32.eq (i32.load (i32.const 1204)) (i32.const 0x0100007f)) (i32.const 42))
    ;; client at [4] connects to the listener's port
    (i32.store16 (i32.const 1026) (i32.load16_u (i32.const 1202)))
    (call $ok (call $sock_open (i32.const 1) (i32.const 1) (i32.const 6) (i32.const 4)) (i32.const 5))
    (call $ok (call $sock_connect (i32.load (i32.const 4)) (i32.const 1024)) (i32.const 6))
    ;; the connection at [8]; its peer is the client
    (call $ok (call $sock_accept_v2 (i32.load (i32.const 0)) (i32.const 0) (i32.const 8) (i32.const 1400)) (i32.const 7))
    (call $assert (i32.eq (i32.load8_u (i32.const 1400)) (i32.const 1)) (i32.const 8))
    (call $assert (i32.eq (i32.load (i32.const 1404)) (i32.const 0x0100007f)) (i32.const 9))
    (call $ok (call $sock_addr_peer (i32.load (i32.const 4)) (i32.const 2400)) (i32.const 10))
    (call $assert (i32.eq (i32.load16_u (i32.const 2402)) (i32.load16_u (i32.const 1202))) (i32.const 11))
    (call $ok (call $sock_status (i32.load (i32.const 4)) (i32.const 60)) (i32.const 12))
    (call $assert (i32.eq (i32.load8_u (i32.const 60)) (i32.const 1)) (i32.const 13))
    (call $ok (call $sock_send (i32.load (i32.const 4)) (i32.const 1600) (i32.const 1) (i32.const 0) (i32.const 12)) (i32.const 14))
    (call $assert (i32.eq (i32.load (i32.const 12)) (i32.const 4)) (i32.const 15))
    ;; poll the connection: readable with 4 bytes
    (i32.store (i32.const 3016) (i32.load (i32.const 8)))
    (call $ok (call $poll_oneoff (i32.const 3000) (i32.const 3100) (i32.const 1) (i32.const 16)) (i32.const 16))
    (call $assert (i32.eq (i32.load (i32.const 16)) (i32.const 1)) (i32.const 17))
    (call $assert (i64.eq (i64.load (i32.const 3100)) (i64.const 42)) (i32.const 18))
    (call $assert (i32.eqz (i32.load16_u (i32.const 3108))) (i32.const 19))
    (call $assert (i32.eq (i32.load8_u (i32.const 3110)) (i32.const 1)) (i32.const 20))
    (call $assert (i64.eq (i64.load (i32.const 3116)) (i64.const 4)) (i32.const 21))
    (call $assert (i32.eqz (i32.load16_u (i32.const 3124))) (i32.const 22))
    (call $ok (call $sock_recv (i32.load (i32.const 8)) (i32.const 1610) (i32.const 1) (i32.const 0) (i32.const 20) (i32.const 24)) (i32.const 23))
    (call $assert (i32.eq (i32.load (i32.const 20)) (i32.const 4)) (i32.const 24))
    (call $ok (call $sock_send (i32.load (i32.const 8)) (i32.const 1620) (i32.const 1) (i32.const 0) (i32.const 28)) (i32.const 25))
    (call $ok (call $sock_recv (i32.load (i32.const 4)) (i32.const 1630) (i32.const 1) (i32.const 0) (i32.const 32) (i32.const 36)) (i32.const 26))
    (call $print (i32.const 2200) (i32.load (i32.const 32)))
    ;; after the peer shuts down, the client reads end of file and sees a hangup
    (call $ok (call $sock_shutdown (i32.load (i32.const 8)) (i32.const 3)) (i32.const 27))
    (i32.store (i32.const 3016) (i32.load (i32.const 4)))
    (call $ok (call $poll_oneoff (i32.const 3000) (i32.const 3100) (i32.const 1) (i32.const 16)) (i32.const 28))
    (call $assert (i32.eq (i32.load16_u (i32.const 3124)) (i32.const 1)) (i32.const 29))
    (call $ok (call $sock_recv (i32.load (i32.const 4)) (i32.const 1630) (i32.const 1) (i32.const 0) (i32.const 32) (i32.const 36)) (i32.const 30))
    (call $assert (i32.eqz (i32.load (i32.const 32))) (i32.const 31))
    "#;
    assert_eq!(run(&data, body), (0, "ping".to_string()));
}

/// UDP: `sock_send_to` and `sock_recv_from`, which reports the sender.
#[test]
fn udp_send_to_recv_from() {
    let data = format!(
        r#"{LOOPBACK_ADDR}
        (data (i32.const 2000) "hey!")
        (data (i32.const 1600) "\d0\07\00\00\04\00\00\00")
        (data (i32.const 1610) "\34\08\00\00\10\00\00\00")
        ;; subscription at 3000: FD_READ; the fd is filled in
        (data (i32.const 3000) "\00\00\00\00\00\00\00\00\01")"#
    );
    let body = r#"
    (call $ok (call $sock_open (i32.const 1) (i32.const 2) (i32.const 17) (i32.const 0)) (i32.const 1))
    (call $ok (call $sock_bind (i32.load (i32.const 0)) (i32.const 1024)) (i32.const 2))
    (call $ok (call $sock_addr_local (i32.load (i32.const 0)) (i32.const 1200)) (i32.const 3))
    (call $ok (call $sock_open (i32.const 1) (i32.const 2) (i32.const 17) (i32.const 4)) (i32.const 4))
    (call $ok (call $sock_bind (i32.load (i32.const 4)) (i32.const 1024)) (i32.const 5))
    (call $ok (call $sock_addr_local (i32.load (i32.const 4)) (i32.const 2400)) (i32.const 6))
    (call $ok (call $sock_send_to (i32.load (i32.const 4)) (i32.const 1600) (i32.const 1) (i32.const 0) (i32.const 1200) (i32.const 8)) (i32.const 7))
    (call $assert (i32.eq (i32.load (i32.const 8)) (i32.const 4)) (i32.const 8))
    (call $ok (call $sock_recv_from (i32.load (i32.const 0)) (i32.const 1610) (i32.const 1) (i32.const 0) (i32.const 12) (i32.const 16) (i32.const 1400)) (i32.const 9))
    (call $assert (i32.eq (i32.load (i32.const 12)) (i32.const 4)) (i32.const 10))
    (call $assert (i32.eq (i32.load8_u (i32.const 1400)) (i32.const 1)) (i32.const 11))
    (call $assert (i32.eq (i32.load16_u (i32.const 1402)) (i32.load16_u (i32.const 2402))) (i32.const 12))
    (call $assert (i32.eq (i32.load (i32.const 1404)) (i32.const 0x0100007f)) (i32.const 13))
    ;; a datagram that does not fit is truncated, and says so with MSG_TRUNC
    (call $ok (call $sock_send_to (i32.load (i32.const 4)) (i32.const 1600) (i32.const 1) (i32.const 0) (i32.const 1200) (i32.const 8)) (i32.const 14))
    (i32.store (i32.const 1614) (i32.const 2))
    (call $ok (call $sock_recv_from (i32.load (i32.const 0)) (i32.const 1610) (i32.const 1) (i32.const 0) (i32.const 12) (i32.const 16) (i32.const 1400)) (i32.const 15))
    (call $assert (i32.eq (i32.load (i32.const 12)) (i32.const 2)) (i32.const 16))
    (call $assert (i32.eq (i32.load16_u (i32.const 16)) (i32.const 1)) (i32.const 17))
    (call $print (i32.const 2100) (i32.const 2))
    ;; a datagram to a closed port: poll reports an error, and the next receive still gets
    ;; the refusal (CONNREFUSED, 14), as on Linux
    (call $ok (call $sock_open (i32.const 1) (i32.const 2) (i32.const 17) (i32.const 20)) (i32.const 18))
    (call $ok (call $sock_bind (i32.load (i32.const 20)) (i32.const 1024)) (i32.const 19))
    (call $ok (call $sock_addr_local (i32.load (i32.const 20)) (i32.const 2600)) (i32.const 20))
    (call $ok (call $fd_close (i32.load (i32.const 20))) (i32.const 21))
    (call $ok (call $sock_connect (i32.load (i32.const 4)) (i32.const 2600)) (i32.const 22))
    (call $ok (call $sock_send (i32.load (i32.const 4)) (i32.const 1600) (i32.const 1) (i32.const 0) (i32.const 8)) (i32.const 23))
    (i32.store (i32.const 3016) (i32.load (i32.const 4)))
    (call $ok (call $poll_oneoff (i32.const 3000) (i32.const 3100) (i32.const 1) (i32.const 16)) (i32.const 24))
    (call $assert (i32.eq (i32.load16_u (i32.const 3108)) (i32.const 29)) (i32.const 25))
    (call $assert (i32.eq (call $sock_recv (i32.load (i32.const 4)) (i32.const 1610) (i32.const 1) (i32.const 0) (i32.const 12) (i32.const 16)) (i32.const 14)) (i32.const 26))
    "#;
    assert_eq!(run(&data, body), (0, "he".to_string()));
}

/// Clock subscriptions: a timeout of 0 never fires (v1), a short one does; a descriptor that
/// is not open is reported with `BADF`; `wasi_unstable`'s layout works the same.
#[test]
fn poll_oneoff_clocks_and_bad_descriptors() {
    let data = r#"
        ;; 3000: userdata 1, clock MONOTONIC, timeout 0 (never)
        (data (i32.const 3000) "\01\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\01\00\00\00")
        ;; 3048: userdata 2, clock MONOTONIC, timeout 1 ms
        (data (i32.const 3048) "\02\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\01\00\00\00\00\00\00\00\40\42\0f\00\00\00\00\00")
        ;; 3200: userdata 5, FD_WRITE on 77, which is not open
        (data (i32.const 3200) "\05\00\00\00\00\00\00\00\02\00\00\00\00\00\00\00\4d\00\00\00")
        ;; 3400 (wasi_unstable, 56 bytes): userdata 9, clock MONOTONIC (at 24), 1 ms (at 32)
        (data (i32.const 3400) "\09\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\01\00\00\00\00\00\00\00\40\42\0f\00\00\00\00\00")"#;
    let body = r#"
    (call $ok (call $poll_oneoff (i32.const 3000) (i32.const 3600) (i32.const 2) (i32.const 0)) (i32.const 1))
    (call $assert (i32.eq (i32.load (i32.const 0)) (i32.const 1)) (i32.const 2))
    (call $assert (i64.eq (i64.load (i32.const 3600)) (i64.const 2)) (i32.const 3))
    (call $assert (i32.eqz (i32.load8_u (i32.const 3610))) (i32.const 4))
    (call $ok (call $poll_oneoff (i32.const 3200) (i32.const 3600) (i32.const 1) (i32.const 0)) (i32.const 5))
    (call $assert (i32.eq (i32.load (i32.const 0)) (i32.const 1)) (i32.const 6))
    (call $assert (i32.eq (i32.load16_u (i32.const 3608)) (i32.const 8)) (i32.const 7))
    (call $assert (i32.eq (i32.load8_u (i32.const 3610)) (i32.const 2)) (i32.const 8))
    (call $ok (call $poll_oneoff0 (i32.const 3400) (i32.const 3600) (i32.const 1) (i32.const 0)) (i32.const 9))
    (call $assert (i32.eq (i32.load (i32.const 0)) (i32.const 1)) (i32.const 10))
    (call $assert (i64.eq (i64.load (i32.const 3600)) (i64.const 9)) (i32.const 11))
    ;; no subscriptions is EINVAL (28)
    (call $assert (i32.eq (call $poll_oneoff (i32.const 3000) (i32.const 3600) (i32.const 0) (i32.const 0)) (i32.const 28)) (i32.const 12))
    "#;
    assert_eq!(run(data, body).0, 0);
}

/// An eventfd in an epoll set: the guest's 24-byte `epoll_data` comes back as registered.
#[test]
fn epoll_keeps_v1_data() {
    let body = r#"
    ;; eventfd [0] with a count of 1, epoll set [4]
    (call $ok (call $fd_event (i64.const 1) (i32.const 0) (i32.const 0)) (i32.const 1))
    (call $ok (call $epoll_create (i32.const 4)) (i32.const 2))
    ;; event at 1000: EPOLLIN, data {ptr 0x11, fd, data1 0x22, data2 0x1234567890}
    (i32.store (i32.const 1000) (i32.const 1))
    (i32.store (i32.const 1008) (i32.const 0x11))
    (i32.store (i32.const 1012) (i32.load (i32.const 0)))
    (i32.store (i32.const 1016) (i32.const 0x22))
    (i64.store (i32.const 1024) (i64.const 0x1234567890))
    (call $ok (call $epoll_ctl (i32.load (i32.const 4)) (i32.const 0) (i32.load (i32.const 0)) (i32.const 1000)) (i32.const 3))
    (call $ok (call $epoll_wait (i32.load (i32.const 4)) (i32.const 2000) (i32.const 4) (i64.const 0) (i32.const 8)) (i32.const 4))
    (call $assert (i32.eq (i32.load (i32.const 8)) (i32.const 1)) (i32.const 5))
    (call $assert (i32.eq (i32.load (i32.const 2000)) (i32.const 1)) (i32.const 6))
    (call $assert (i32.eq (i32.load (i32.const 2008)) (i32.const 0x11)) (i32.const 7))
    (call $assert (i32.eq (i32.load (i32.const 2012)) (i32.load (i32.const 0))) (i32.const 8))
    (call $assert (i32.eq (i32.load (i32.const 2016)) (i32.const 0x22)) (i32.const 9))
    (call $assert (i64.eq (i64.load (i32.const 2024)) (i64.const 0x1234567890)) (i32.const 10))
    ;; MOD to EPOLLOUT: an eventfd is writable too, and v1's EPOLLOUT bit is 2
    (i32.store (i32.const 1000) (i32.const 2))
    (call $ok (call $epoll_ctl (i32.load (i32.const 4)) (i32.const 3) (i32.load (i32.const 0)) (i32.const 1000)) (i32.const 11))
    (call $ok (call $epoll_wait (i32.load (i32.const 4)) (i32.const 2000) (i32.const 4) (i64.const 0) (i32.const 8)) (i32.const 12))
    (call $assert (i32.eq (i32.load (i32.const 2000)) (i32.const 2)) (i32.const 13))
    ;; after DEL nothing is ready
    (call $ok (call $epoll_ctl (i32.load (i32.const 4)) (i32.const 2) (i32.load (i32.const 0)) (i32.const 0)) (i32.const 14))
    (call $ok (call $epoll_wait (i32.load (i32.const 4)) (i32.const 2000) (i32.const 4) (i64.const 1000000) (i32.const 8)) (i32.const 15))
    (call $assert (i32.eqz (i32.load (i32.const 8))) (i32.const 16))
    (call $assert (i32.eq (call $epoll_wait (i32.load (i32.const 4)) (i32.const 2000) (i32.const 0) (i64.const 0) (i32.const 8)) (i32.const 28)) (i32.const 17))
    "#;
    assert_eq!(run("", body).0, 0);
}

/// A set used through two descriptors, as mio does (it registers through a `dup` of the set
/// and waits on the original): events carry the data registered through either.
#[test]
fn epoll_data_across_duplicate_descriptors() {
    let body = r#"
    ;; eventfd [0] with a count of 1, epoll set [4], its duplicate [12]
    (call $ok (call $fd_event (i64.const 1) (i32.const 0) (i32.const 0)) (i32.const 1))
    (call $ok (call $epoll_create (i32.const 4)) (i32.const 2))
    (call $ok (call $fd_dup (i32.load (i32.const 4)) (i32.const 12)) (i32.const 3))
    ;; event at 1000: EPOLLIN, data {ptr 0x11, fd, data1 0x22, data2 0x1234567890}
    (i32.store (i32.const 1000) (i32.const 1))
    (i32.store (i32.const 1008) (i32.const 0x11))
    (i32.store (i32.const 1012) (i32.load (i32.const 0)))
    (i32.store (i32.const 1016) (i32.const 0x22))
    (i64.store (i32.const 1024) (i64.const 0x1234567890))
    (call $ok (call $epoll_ctl (i32.load (i32.const 12)) (i32.const 1) (i32.load (i32.const 0)) (i32.const 1000)) (i32.const 4))
    (call $ok (call $epoll_wait (i32.load (i32.const 4)) (i32.const 2000) (i32.const 4) (i64.const 0) (i32.const 8)) (i32.const 5))
    (call $assert (i32.eq (i32.load (i32.const 8)) (i32.const 1)) (i32.const 6))
    (call $assert (i32.eq (i32.load (i32.const 2008)) (i32.const 0x11)) (i32.const 7))
    (call $assert (i32.eq (i32.load (i32.const 2016)) (i32.const 0x22)) (i32.const 8))
    (call $assert (i64.eq (i64.load (i32.const 2024)) (i64.const 0x1234567890)) (i32.const 9))
    ;; MOD through the original with new data, wait on the duplicate
    (i64.store (i32.const 1024) (i64.const 0x77))
    (call $ok (call $epoll_ctl (i32.load (i32.const 4)) (i32.const 3) (i32.load (i32.const 0)) (i32.const 1000)) (i32.const 10))
    (call $ok (call $epoll_wait (i32.load (i32.const 12)) (i32.const 2000) (i32.const 4) (i64.const 0) (i32.const 8)) (i32.const 11))
    (call $assert (i32.eq (i32.load (i32.const 8)) (i32.const 1)) (i32.const 12))
    (call $assert (i64.eq (i64.load (i32.const 2024)) (i64.const 0x77)) (i32.const 13))
    "#;
    assert_eq!(run("", body).0, 0);
}

/// v1 let `sock_send`/`sock_recv` without flags work on any stream; an eventfd stands in for
/// a pipe. A bad output pointer is `MEMVIOLATION` (78), as the v1 host reported it.
#[test]
fn stream_fallback_and_bad_pointers() {
    let data = r#"
        (data (i32.const 1600) "\d0\07\00\00\08\00\00\00")
        (data (i32.const 1610) "\34\08\00\00\08\00\00\00")
        (data (i32.const 2000) "\05\00\00\00\00\00\00\00")"#;
    let body = r#"
    (call $ok (call $fd_event (i64.const 0) (i32.const 0) (i32.const 0)) (i32.const 1))
    (call $ok (call $sock_send (i32.load (i32.const 0)) (i32.const 1600) (i32.const 1) (i32.const 0) (i32.const 8)) (i32.const 2))
    (call $assert (i32.eq (i32.load (i32.const 8)) (i32.const 8)) (i32.const 3))
    (call $ok (call $sock_recv (i32.load (i32.const 0)) (i32.const 1610) (i32.const 1) (i32.const 0) (i32.const 12) (i32.const 16)) (i32.const 4))
    (call $assert (i64.eq (i64.load (i32.const 2100)) (i64.const 5)) (i32.const 5))
    ;; with flags it is a socket call, and an eventfd is no socket (NOTSOCK, 57)
    (call $assert (i32.eq (call $sock_send (i32.load (i32.const 0)) (i32.const 1600) (i32.const 1) (i32.const 1) (i32.const 8)) (i32.const 57)) (i32.const 6))
    (call $assert (i32.eq (call $epoll_create (i32.const 0xfffffff0)) (i32.const 78)) (i32.const 7))
    "#;
    assert_eq!(run(data, body).0, 0);
}

/// Options: flags, sizes and times, the compat-side timeouts, kind mismatches and the one
/// option Linux cannot serve.
#[test]
fn socket_options() {
    let data = r#"
        ;; option_timestamp at 1000: Some(1.5 s)
        (data (i32.const 1000) "\01\00\00\00\00\00\00\00\00\2f\68\59\00\00\00\00")"#;
    let body = r#"
    (call $ok (call $sock_open (i32.const 1) (i32.const 1) (i32.const 6) (i32.const 0)) (i32.const 1))
    (call $ok (call $set_flag (i32.load (i32.const 0)) (i32.const 2) (i32.const 1)) (i32.const 2))
    (call $ok (call $get_flag (i32.load (i32.const 0)) (i32.const 2) (i32.const 8)) (i32.const 3))
    (call $assert (i32.eq (i32.load8_u (i32.const 8)) (i32.const 1)) (i32.const 4))
    (call $ok (call $set_time (i32.load (i32.const 0)) (i32.const 19) (i32.const 1000)) (i32.const 5))
    (call $ok (call $get_time (i32.load (i32.const 0)) (i32.const 19) (i32.const 1100)) (i32.const 6))
    (call $assert (i32.eq (i32.load8_u (i32.const 1100)) (i32.const 1)) (i32.const 7))
    (call $assert (i64.eq (i64.load (i32.const 1108)) (i64.const 1500000000)) (i32.const 8))
    (call $ok (call $set_time (i32.load (i32.const 0)) (i32.const 21) (i32.const 1000)) (i32.const 9))
    (call $ok (call $get_time (i32.load (i32.const 0)) (i32.const 21) (i32.const 1200)) (i32.const 10))
    (call $assert (i64.eq (i64.load (i32.const 1208)) (i64.const 1500000000)) (i32.const 11))
    (call $ok (call $get_size (i32.load (i32.const 0)) (i32.const 25) (i32.const 16)) (i32.const 12))
    (call $assert (i64.eq (i64.load (i32.const 16)) (i64.const 1)) (i32.const 13))
    (call $ok (call $set_size (i32.load (i32.const 0)) (i32.const 16) (i64.const 65536)) (i32.const 14))
    (call $ok (call $get_size (i32.load (i32.const 0)) (i32.const 16) (i32.const 16)) (i32.const 15))
    (call $assert (i64.ge_u (i64.load (i32.const 16)) (i64.const 65536)) (i32.const 16))
    ;; REUSE_ADDR is a flag, not a size: EINVAL (28); PROMISCUOUS: NOTSUP (58)
    (call $assert (i32.eq (call $set_size (i32.load (i32.const 0)) (i32.const 2) (i64.const 1)) (i32.const 28)) (i32.const 17))
    (call $assert (i32.eq (call $get_flag (i32.load (i32.const 0)) (i32.const 9) (i32.const 8)) (i32.const 58)) (i32.const 18))
    ;; not listening yet
    (call $ok (call $get_flag (i32.load (i32.const 0)) (i32.const 10) (i32.const 8)) (i32.const 19))
    (call $assert (i32.eqz (i32.load8_u (i32.const 8))) (i32.const 20))
    "#;
    assert_eq!(run(data, body).0, 0);
}

/// `CONNECT_TIMEOUT` still reports a refused connection as such, and `ACCEPT_TIMEOUT` gives
/// up with `TIMEDOUT` (73).
#[test]
fn compat_timeouts() {
    let data = format!(
        r#"{LOOPBACK_ADDR}
        ;; option_timestamp at 1000: Some(10 ms)
        (data (i32.const 1000) "\01\00\00\00\00\00\00\00\80\96\98\00\00\00\00\00")"#
    );
    let body = r#"
    ;; a listener on an ephemeral port, then closed: nobody listens there any more
    (call $ok (call $sock_open (i32.const 1) (i32.const 1) (i32.const 6) (i32.const 0)) (i32.const 1))
    (call $ok (call $sock_bind (i32.load (i32.const 0)) (i32.const 1024)) (i32.const 2))
    (call $ok (call $sock_addr_local (i32.load (i32.const 0)) (i32.const 1200)) (i32.const 3))
    (call $ok (call $fd_close (i32.load (i32.const 0))) (i32.const 4))
    (call $ok (call $sock_open (i32.const 1) (i32.const 1) (i32.const 6) (i32.const 4)) (i32.const 5))
    (call $ok (call $set_time (i32.load (i32.const 4)) (i32.const 21) (i32.const 1000)) (i32.const 6))
    (call $assert (i32.eq (call $sock_connect (i32.load (i32.const 4)) (i32.const 1200)) (i32.const 14)) (i32.const 7))
    ;; a listener nobody connects to
    (call $ok (call $sock_open (i32.const 1) (i32.const 1) (i32.const 6) (i32.const 8)) (i32.const 8))
    (call $ok (call $sock_bind (i32.load (i32.const 8)) (i32.const 1024)) (i32.const 9))
    (call $ok (call $sock_listen (i32.load (i32.const 8)) (i32.const 1)) (i32.const 10))
    (call $ok (call $set_time (i32.load (i32.const 8)) (i32.const 22) (i32.const 1000)) (i32.const 11))
    (call $assert (i32.eq (call $sock_accept_v2 (i32.load (i32.const 8)) (i32.const 0) (i32.const 12) (i32.const 1400)) (i32.const 73)) (i32.const 12))
    "#;
    assert_eq!(run(&data, body).0, 0);
}

/// The compat timeouts belong to the socket under its number: a copy (`dup`) has them, a number
/// that is closed or renumbered onto forgets them.
#[test]
fn compat_timeouts_follow_the_descriptors() {
    let data = r#"
        ;; option_timestamp at 1100: Some(10 ms)
        (data (i32.const 1100) "\01\00\00\00\00\00\00\00\80\96\98\00\00\00\00\00")"#;
    let body = r#"
    ;; socket [0] with an accept timeout, copied to [4]
    (call $ok (call $sock_open (i32.const 1) (i32.const 1) (i32.const 6) (i32.const 0)) (i32.const 1))
    (call $ok (call $set_time (i32.load (i32.const 0)) (i32.const 22) (i32.const 1100)) (i32.const 2))
    (call $ok (call $fd_dup (i32.load (i32.const 0)) (i32.const 4)) (i32.const 3))
    (call $ok (call $get_time (i32.load (i32.const 4)) (i32.const 22) (i32.const 1200)) (i32.const 4))
    (call $assert (i32.eq (i32.load8_u (i32.const 1200)) (i32.const 1)) (i32.const 5))
    (call $assert (i64.eq (i64.load (i32.const 1208)) (i64.const 10000000)) (i32.const 6))
    ;; socket [8] without, renumbered onto [4]: [4] has none now
    (call $ok (call $sock_open (i32.const 1) (i32.const 1) (i32.const 6) (i32.const 8)) (i32.const 7))
    (call $ok (call $fd_renumber (i32.load (i32.const 8)) (i32.load (i32.const 4))) (i32.const 8))
    (call $ok (call $get_time (i32.load (i32.const 4)) (i32.const 22) (i32.const 1200)) (i32.const 9))
    (call $assert (i32.eqz (i32.load8_u (i32.const 1200))) (i32.const 10))
    ;; [0] closed, and [8] renumbered onto its number: none either
    (call $ok (call $fd_close (i32.load (i32.const 0))) (i32.const 11))
    (call $ok (call $fd_renumber (i32.load (i32.const 8)) (i32.load (i32.const 0))) (i32.const 12))
    (call $ok (call $get_time (i32.load (i32.const 0)) (i32.const 22) (i32.const 1200)) (i32.const 13))
    (call $assert (i32.eqz (i32.load8_u (i32.const 1200))) (i32.const 14))
    "#;
    assert_eq!(run(data, body).0, 0);
}

/// A vfork child has its parent's descriptors, and with them the compat state about them: the
/// v1 data of its epoll registrations and the sockets' compat timeouts. The child checks them
/// and exits with 0, or the number of the check that failed, which the parent exits with.
#[test]
fn a_vfork_child_keeps_the_epoll_data_and_timeouts() {
    let data = format!(
        r#"{LOOPBACK_ADDR}
        ;; option_timestamp at 1100: Some(10 ms)
        (data (i32.const 1100) "\01\00\00\00\00\00\00\00\80\96\98\00\00\00\00\00")"#
    );
    let body = r#"
    ;; eventfd [0] with a count of 1, epoll set [4], data {ptr 0x11, fd, data1 0x22}
    (call $ok (call $fd_event (i64.const 1) (i32.const 0) (i32.const 0)) (i32.const 1))
    (call $ok (call $epoll_create (i32.const 4)) (i32.const 2))
    (i32.store (i32.const 1000) (i32.const 1))
    (i32.store (i32.const 1008) (i32.const 0x11))
    (i32.store (i32.const 1012) (i32.load (i32.const 0)))
    (i32.store (i32.const 1016) (i32.const 0x22))
    (call $ok (call $epoll_ctl (i32.load (i32.const 4)) (i32.const 0) (i32.load (i32.const 0)) (i32.const 1000)) (i32.const 3))
    ;; a listener [8] whose accepts time out after 10 ms
    (call $ok (call $sock_open (i32.const 1) (i32.const 1) (i32.const 6) (i32.const 8)) (i32.const 4))
    (call $ok (call $sock_bind (i32.load (i32.const 8)) (i32.const 1024)) (i32.const 5))
    (call $ok (call $sock_listen (i32.load (i32.const 8)) (i32.const 1)) (i32.const 6))
    (call $ok (call $set_time (i32.load (i32.const 8)) (i32.const 22) (i32.const 1100)) (i32.const 7))
    (call $ok (call $vfork (i32.const 12)) (i32.const 8))
    ;; the child
    (call $ok (call $epoll_wait (i32.load (i32.const 4)) (i32.const 2000) (i32.const 4) (i64.const 0) (i32.const 16)) (i32.const 9))
    (call $assert (i32.eq (i32.load (i32.const 16)) (i32.const 1)) (i32.const 10))
    (call $assert (i32.eq (i32.load (i32.const 2008)) (i32.const 0x11)) (i32.const 11))
    (call $assert (i32.eq (i32.load (i32.const 2016)) (i32.const 0x22)) (i32.const 12))
    ;; TIMEDOUT
    (call $assert (i32.eq (call $sock_accept_v2 (i32.load (i32.const 8)) (i32.const 0) (i32.const 20) (i32.const 1400)) (i32.const 73)) (i32.const 13))
    (call $exit (i32.const 0))
    ;; the parent: the child's status
    (i32.store8 (i32.const 200) (i32.const 1))
    (i32.store (i32.const 204) (i32.load (i32.const 12)))
    (call $ok (call $join (i32.const 200) (i32.const 0) (i32.const 220)) (i32.const 14))
    (call $exit (i32.load16_u (i32.const 222)))
    "#;
    assert_eq!(run(&data, body).0, 0);
}

/// `resolve` answers numeric names and `/etc/hosts` entries itself.
#[test]
fn resolve_hosts_and_numeric() {
    let data = r#"
        (data (i32.const 100) "localhost")
        (data (i32.const 200) "10.1.2.3")"#;
    let body = r#"
    (call $ok (call $resolve (i32.const 100) (i32.const 9) (i32.const 0) (i32.const 1000) (i32.const 4) (i32.const 8)) (i32.const 1))
    (call $assert (i32.eq (i32.load (i32.const 8)) (i32.const 2)) (i32.const 2))
    (call $assert (i32.eq (i32.load8_u (i32.const 1000)) (i32.const 1)) (i32.const 3))
    (call $assert (i32.eq (i32.load (i32.const 1002)) (i32.const 0x0100007f)) (i32.const 4))
    (call $assert (i32.eq (i32.load8_u (i32.const 1018)) (i32.const 2)) (i32.const 5))
    (call $assert (i32.eq (i32.load8_u (i32.const 1035)) (i32.const 1)) (i32.const 6))
    ;; room for one address only
    (call $ok (call $resolve (i32.const 100) (i32.const 9) (i32.const 0) (i32.const 1000) (i32.const 1) (i32.const 8)) (i32.const 7))
    (call $assert (i32.eq (i32.load (i32.const 8)) (i32.const 1)) (i32.const 8))
    (call $ok (call $resolve (i32.const 200) (i32.const 8) (i32.const 0) (i32.const 1000) (i32.const 4) (i32.const 8)) (i32.const 9))
    (call $assert (i32.eq (i32.load (i32.const 8)) (i32.const 1)) (i32.const 10))
    (call $assert (i32.eq (i32.load (i32.const 1002)) (i32.const 0x0302010a)) (i32.const 11))
    "#;
    assert_eq!(run(data, body).0, 0);
}

/// By default `/etc/resolv.conf` names the machine's DNS server, which answers the names
/// under `localhost` itself (RFC 6761): their A and then their AAAA record.
#[test]
fn resolve_through_the_machine_dns_server() {
    let data = r#"(data (i32.const 100) "dns-test.localhost")"#;
    let body = r#"
    (call $ok (call $resolve (i32.const 100) (i32.const 18) (i32.const 0) (i32.const 1000) (i32.const 4) (i32.const 8)) (i32.const 1))
    (call $assert (i32.eq (i32.load (i32.const 8)) (i32.const 2)) (i32.const 2))
    (call $assert (i32.eq (i32.load (i32.const 1002)) (i32.const 0x0100007f)) (i32.const 3))
    (call $assert (i32.eq (i32.load8_u (i32.const 1035)) (i32.const 1)) (i32.const 4))
    "#;
    assert_eq!(run(data, body).0, 0);
}

/// Without name servers in `/etc/resolv.conf`, `resolve` asks the local host, as libc
/// resolvers do; nothing listens there, so the refused queries end the lookup at once
/// instead of after each try's timeout.
#[test]
fn resolve_without_name_servers_fails_fast() {
    let data = r#"(data (i32.const 100) "name.invalid")"#;
    let body = r#"
    (call $exit (call $resolve (i32.const 100) (i32.const 12) (i32.const 0) (i32.const 1000) (i32.const 4) (i32.const 8)))
    "#;
    let no_resolv_conf = || {
        let host: Arc<dyn wasixv2::Host> = Arc::new(wasixv2::TokioHost::default());
        let release = wasixv2::Uname::LINUX_RELEASE;
        let root = wasixv2::default_root(&host, "", &wasixv2::Credentials::default(), release);
        Machine::with_root(host, root)
    };
    let started = std::time::Instant::now();
    let (status, _, _) = run_wasm_on(no_resolv_conf, &guest(data, body), &["net"]);
    // TIMEDOUT
    assert_eq!(status, 73);
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
}

/// `port_addr_list` lists the loopback interface's address, with its prefix.
#[test]
fn port_addr_list_has_loopback() {
    let body = r#"
    (local $i i32) (local $found i32)
    ;; room for one address only: OVERFLOW (61) with the count written
    (i32.store (i32.const 8) (i32.const 0))
    (call $assert (i32.eq (call $port_addr_list (i32.const 1000) (i32.const 8)) (i32.const 61)) (i32.const 1))
    (call $assert (i32.ge_u (i32.load (i32.const 8)) (i32.const 1)) (i32.const 2))
    (i32.store (i32.const 8) (i32.const 32))
    (call $ok (call $port_addr_list (i32.const 1000) (i32.const 8)) (i32.const 3))
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $i) (i32.load (i32.const 8))))
        (if (i32.and
              (i32.eq (i32.load (i32.add (i32.const 1002) (i32.mul (local.get $i) (i32.const 28)))) (i32.const 0x0100007f))
              (i32.eq (i32.load8_u (i32.add (i32.const 1006) (i32.mul (local.get $i) (i32.const 28)))) (i32.const 8)))
          (then (local.set $found (i32.const 1))))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $next)))
    (call $assert (local.get $found) (i32.const 4))
    "#;
    assert_eq!(run("", body).0, 0);
}

/// The C counterpart: `net-loopback.c` built against wasix-libc, using `socket`, `poll`,
/// `getaddrinfo` and friends. Set `WASIX1_TEST_CC` to a v1 C compiler command (e.g.
/// `wasixcc` with a v1 sysroot); without it the test only says it was skipped.
#[test]
fn c_loopback() {
    let Ok(cc) = std::env::var("WASIX1_TEST_CC") else {
        eprintln!("skipped: set WASIX1_TEST_CC to a WASIX v1 C compiler");
        return;
    };
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/c/net-loopback.c");
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("net-loopback.wasm");
    let status = Command::new(&cc)
        .arg("-O1")
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .status()
        .expect("run the v1 C compiler");
    assert!(status.success(), "{cc} failed");
    let wasm = std::fs::read(&out).unwrap();
    let (status, stdout, stderr) = run_wasm(&wasm, &["net-loopback"]);
    assert_eq!(status, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert_eq!(
        stdout,
        "localhost: 127.0.0.1\naccepted\nreadable\necho: hello\nepoll: 1\nok\n"
    );
}
