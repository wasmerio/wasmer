//! Threads, futexes, signals, processes and the terminal of WASIX v1 guests, hand-written in
//! WAT, run on a `wasixv2::Machine`. Each guest reports through its exit status: 0, or the
//! number of the check that failed.

use std::{
    io,
    path::{Path, PathBuf},
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

/// Runs `wat`, with the host directory `programs` at `/prog` if given; returns its exit status
/// and stdout.
fn run(wat: &str, programs: Option<&Path>) -> (i32, Vec<u8>) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, async {
        let machine = Machine::new();
        if let Some(dir) = programs {
            machine.mount_host_dir("/prog", dir, false).await.unwrap();
        }
        let stdout = Arc::new(Capture::default());
        let config = ProcessConfig {
            args: vec!["test".to_string()],
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
    })
}

fn status(wat: &str) -> i32 {
    run(wat, None).0
}

/// A host directory with `programs` compiled from WAT, removed on drop.
struct Programs(PathBuf);

impl Programs {
    fn new(name: &str, programs: &[(&str, &str)]) -> Self {
        let dir = std::env::temp_dir().join(format!("wasix1-proc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (file, wat) in programs {
            let path = dir.join(file);
            std::fs::write(&path, wat::parse_str(wat).unwrap()).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        Self(dir)
    }
}

impl Drop for Programs {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Signals reach `__wasm_signal` once libc named it, are held back while it names something
/// it does not export, and are delivered when it names the callback again. Signals whose
/// default is to be ignored are not delivered.
const SIGNALS: &str = r#"
(module
  (import "wasix_32v1" "callback_signal" (func $callback (param i32 i32)))
  (import "wasi_snapshot_preview1" "proc_raise" (func $raise (param i32) (result i32)))
  (import "wasix_32v1" "thread_signal" (func $thread_signal (param i32 i32) (result i32)))
  (import "wasix_32v1" "proc_signal" (func $proc_signal (param i32 i32) (result i32)))
  (import "wasix_32v1" "thread_id" (func $thread_id (param i32) (result i32)))
  (import "wasix_32v1" "proc_id" (func $proc_id (param i32) (result i32)))
  (import "wasix_32v1" "proc_exit2" (func $exit (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 16) "__wasm_signal")
  (data (i32.const 48) "__wasm_signal_blocked")
  (global $count (mut i32) (i32.const 0))
  (global $last (mut i32) (i32.const 0))
  (func (export "__wasm_signal") (param $sig i32)
    (global.set $count (i32.add (global.get $count) (i32.const 1)))
    (global.set $last (local.get $sig)))
  (func $deliver (call $callback (i32.const 16) (i32.const 13)))
  (func $hold (call $callback (i32.const 48) (i32.const 21)))
  (func (export "_start")
    (call $deliver)
    (if (call $raise (i32.const 10)) (then (call $exit (i32.const 1))))
    (if (i32.ne (global.get $last) (i32.const 10)) (then (call $exit (i32.const 2))))
    ;; held back until the callback is named again
    (call $hold)
    (if (call $raise (i32.const 12)) (then (call $exit (i32.const 3))))
    (if (i32.ne (global.get $count) (i32.const 1)) (then (call $exit (i32.const 4))))
    (call $deliver)
    (if (i32.ne (global.get $last) (i32.const 12)) (then (call $exit (i32.const 5))))
    ;; to the calling thread by tid, and to the process
    (drop (call $thread_id (i32.const 0)))
    (if (call $thread_signal (i32.load (i32.const 0)) (i32.const 2)) (then (call $exit (i32.const 6))))
    (if (i32.ne (global.get $last) (i32.const 2)) (then (call $exit (i32.const 7))))
    (drop (call $proc_id (i32.const 4)))
    (if (call $proc_signal (i32.load (i32.const 4)) (i32.const 15)) (then (call $exit (i32.const 8))))
    (if (i32.ne (global.get $last) (i32.const 15)) (then (call $exit (i32.const 9))))
    ;; SIGCHLD keeps its default: ignored
    (if (call $raise (i32.const 17)) (then (call $exit (i32.const 10))))
    (if (i32.ne (global.get $count) (i32.const 4)) (then (call $exit (i32.const 11))))
    (call $exit (i32.const 0))))
"#;

#[test]
fn signals() {
    assert_eq!(status(SIGNALS), 0);
}

/// wasix-libc before v2026-07-30.1 holds signals back in `pthread_create` and never ends the
/// hold (its `__restore_sigs` is compiled out). The spawn ends it, for the creating thread and
/// the new one: both still take signals afterwards.
const SPAWN_HOLD: &str = r#"
(module
  (import "env" "memory" (memory 1 1 shared))
  (import "wasix_32v1" "callback_signal" (func $callback (param i32 i32)))
  (import "wasix_32v1" "thread_spawn_v2" (func $spawn (param i32 i32) (result i32)))
  (import "wasix_32v1" "thread_signal" (func $thread_signal (param i32 i32) (result i32)))
  (import "wasix_32v1" "futex_wait" (func $wait (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_raise" (func $raise (param i32) (result i32)))
  (import "wasix_32v1" "proc_exit2" (func $exit (param i32)))
  (data (i32.const 16) "__wasm_signal")
  (data (i32.const 48) "__wasm_signal_blocked")
  ;; The signals each thread took, at 200 (main) and 204 (the thread).
  (func (export "__wasm_signal") (param $sig i32)
    (i32.store (i32.const 200) (local.get $sig)))
  (func (export "wasi_thread_start") (param $tid i32) (param $arg i32)
    (i32.atomic.store (i32.const 208) (local.get $tid))
    (drop (memory.atomic.notify (i32.const 208) (i32.const 1)))
    ;; Waits until the main thread signalled it; the handler's mark is the proof.
    (block $done
      (loop $again
        (br_if $done (i32.load (i32.const 200)))
        (drop (call $wait (i32.const 212) (i32.const 0) (i32.const 0) (i32.const 300)))
        (br $again)))
    (i32.atomic.store (i32.const 204) (i32.load (i32.const 200)))
    (drop (memory.atomic.notify (i32.const 204) (i32.const 1))))
  (func (export "_start")
    (call $callback (i32.const 16) (i32.const 13))
    ;; pthread_create's hold, then the spawn
    (call $callback (i32.const 48) (i32.const 21))
    (if (call $spawn (i32.const 100) (i32.const 104)) (then (call $exit (i32.const 1))))
    (if (call $raise (i32.const 10)) (then (call $exit (i32.const 2))))
    (if (i32.ne (i32.load (i32.const 200)) (i32.const 10)) (then (call $exit (i32.const 3))))
    (i32.store (i32.const 200) (i32.const 0))
    ;; the thread takes SIGUSR2
    (block $started
      (loop $again
        (br_if $started (i32.atomic.load (i32.const 208)))
        (drop (memory.atomic.wait32 (i32.const 208) (i32.const 0) (i64.const 1000000)))
        (br $again)))
    (if (call $thread_signal (i32.atomic.load (i32.const 208)) (i32.const 12)) (then (call $exit (i32.const 4))))
    (block $taken
      (loop $again
        (br_if $taken (i32.atomic.load (i32.const 204)))
        (drop (memory.atomic.wait32 (i32.const 204) (i32.const 0) (i64.const 1000000)))
        (br $again)))
    (if (i32.ne (i32.atomic.load (i32.const 204)) (i32.const 12)) (then (call $exit (i32.const 5))))
    (call $exit (i32.const 0))))
"#;

#[test]
fn the_spawn_ends_pthread_creates_hold_on_signals() {
    assert_eq!(status(SPAWN_HOLD), 0);
}

/// Before libc names its callback, signals take their default action.
#[test]
fn signals_before_the_callback_terminate() {
    let wat = r#"
    (module
      (import "wasi_snapshot_preview1" "proc_raise" (func $raise (param i32) (result i32)))
      (import "wasix_32v1" "proc_exit2" (func $exit (param i32)))
      (memory (export "memory") 1)
      (func (export "_start")
        (drop (call $raise (i32.const 15)))
        (call $exit (i32.const 1))))
    "#;
    assert_eq!(status(wat), 128 + 15);
}

/// A module that names a callback it does not export, and has no `__wasm_signal` either, has
/// nothing to run handlers through: its signals take their default action.
#[test]
fn signals_without_any_callback_terminate() {
    let wat = r#"
    (module
      (import "wasix_32v1" "callback_signal" (func $callback (param i32 i32)))
      (import "wasi_snapshot_preview1" "proc_raise" (func $raise (param i32) (result i32)))
      (import "wasix_32v1" "proc_exit2" (func $exit (param i32)))
      (memory (export "memory") 1)
      (data (i32.const 16) "no_such_export")
      (func (export "_start")
        (call $callback (i32.const 16) (i32.const 14))
        (drop (call $raise (i32.const 15)))
        (call $exit (i32.const 1))))
    "#;
    assert_eq!(status(wat), 128 + 15);
}

/// `proc_raise_interval` arms `ITIMER_REAL`: `SIGALRM` interrupts a sleep, which returns
/// `INTR` (27) once the handler ran. Other signals are `NOTSUP` (58).
#[test]
fn alarm() {
    let wat = r#"
    (module
      (import "wasix_32v1" "callback_signal" (func $callback (param i32 i32)))
      (import "wasix_32v1" "proc_raise_interval" (func $interval (param i32 i64 i32) (result i32)))
      (import "wasix_32v1" "thread_sleep" (func $sleep (param i64) (result i32)))
      (import "wasix_32v1" "proc_exit2" (func $exit (param i32)))
      (memory (export "memory") 1)
      (data (i32.const 16) "__wasm_signal")
      (global $last (mut i32) (i32.const 0))
      (func (export "__wasm_signal") (param $sig i32) (global.set $last (local.get $sig)))
      (func (export "_start")
        (call $callback (i32.const 16) (i32.const 13))
        (if (call $interval (i32.const 14) (i64.const 1000000) (i32.const 0))
          (then (call $exit (i32.const 1))))
        (if (i32.ne (call $sleep (i64.const 10000000000)) (i32.const 27))
          (then (call $exit (i32.const 2))))
        (if (i32.ne (global.get $last) (i32.const 14)) (then (call $exit (i32.const 3))))
        (if (i32.ne (call $interval (i32.const 10) (i64.const 1) (i32.const 0)) (i32.const 58))
          (then (call $exit (i32.const 4))))
        ;; a short sleep without a timer completes
        (if (call $sleep (i64.const 1000)) (then (call $exit (i32.const 5))))
        (call $exit (i32.const 0))))
    "#;
    assert_eq!(status(wat), 0);
}

/// A thread starts at `wasi_thread_start(tid, start_arg)` on the shared memory, publishes
/// `start_arg + 1` and wakes the main thread, which waits on the futex. Then the futex
/// timeout (not woken), a stale expected value (woken) and a wake with no waiters.
const THREADS: &str = r#"
(module
  (import "env" "memory" (memory 1 1 shared))
  (import "wasix_32v1" "thread_spawn_v2" (func $spawn (param i32 i32) (result i32)))
  (import "wasi" "thread-spawn" (func $wasi_spawn (param i32) (result i32)))
  (import "wasix_32v1" "futex_wait" (func $wait (param i32 i32 i32 i32) (result i32)))
  (import "wasix_32v1" "futex_wake" (func $wake (param i32 i32) (result i32)))
  (import "wasix_32v1" "futex_wake_all" (func $wake_all (param i32 i32) (result i32)))
  (import "wasix_32v1" "thread_parallelism" (func $parallelism (param i32) (result i32)))
  (import "wasix_32v1" "thread_id" (func $thread_id (param i32) (result i32)))
  (import "wasix_32v1" "proc_exit2" (func $exit (param i32)))
  (func (export "wasi_thread_start") (param $tid i32) (param $arg i32)
    (i32.atomic.store (local.get $arg) (local.get $tid))
    (i32.atomic.store (i32.const 200) (i32.add (i32.atomic.load (i32.const 200)) (i32.const 1)))
    (drop (call $wake_all (i32.const 200) (i32.const 308))))
  ;; Waits until the counter at 200 reaches `n`.
  (func $await (param $n i32)
    (block $done
      (loop $again
        (br_if $done (i32.ge_u (i32.atomic.load (i32.const 200)) (local.get $n)))
        (if (call $wait (i32.const 200) (i32.atomic.load (i32.const 200)) (i32.const 0) (i32.const 304))
          (then (call $exit (i32.const 20))))
        (br $again))))
  (func (export "_start")
    (if (call $spawn (i32.const 100) (i32.const 104)) (then (call $exit (i32.const 1))))
    (call $await (i32.const 1))
    ;; the tid the call returned is the one the thread started with
    (if (i32.ne (i32.atomic.load (i32.const 100)) (i32.load (i32.const 104)))
      (then (call $exit (i32.const 2))))
    (drop (call $thread_id (i32.const 108)))
    (if (i32.eq (i32.load (i32.const 104)) (i32.load (i32.const 108)))
      (then (call $exit (i32.const 3))))
    ;; wasi-threads' form returns the tid
    (if (i32.le_s (call $wasi_spawn (i32.const 112)) (i32.const 0)) (then (call $exit (i32.const 4))))
    (call $await (i32.const 2))
    ;; a timeout of 1ms: not woken
    (i32.store8 (i32.const 408) (i32.const 1))
    (i64.store (i32.const 416) (i64.const 1000000))
    (if (call $wait (i32.const 400) (i32.const 0) (i32.const 408) (i32.const 304))
      (then (call $exit (i32.const 5))))
    (if (i32.load8_u (i32.const 304)) (then (call $exit (i32.const 6))))
    ;; the value is not the expected one: woken
    (if (call $wait (i32.const 400) (i32.const 1) (i32.const 0) (i32.const 304))
      (then (call $exit (i32.const 7))))
    (if (i32.eqz (i32.load8_u (i32.const 304))) (then (call $exit (i32.const 8))))
    ;; nobody waits
    (if (call $wake (i32.const 400) (i32.const 304)) (then (call $exit (i32.const 9))))
    (if (i32.load8_u (i32.const 304)) (then (call $exit (i32.const 10))))
    (if (call $parallelism (i32.const 312)) (then (call $exit (i32.const 11))))
    (if (i32.eqz (i32.load (i32.const 312))) (then (call $exit (i32.const 12))))
    (call $exit (i32.const 0))))
"#;

#[test]
fn threads_and_futexes() {
    assert_eq!(status(THREADS), 0);
}

/// `memory.atomic.wait32` and `wait64` wait on the kernel's futexes: `SIGALRM` interrupts a
/// timed wait, which returns as woken once the handler ran (v1's handlers do not restart; an
/// untimed wait would go on, as the instruction cannot fail), and timed waits and stale values
/// return 2 and 1. A thread waiting in the instruction is woken by
/// `futex_wake`, and one waiting in `futex_wait` by `memory.atomic.notify`, which also wakes the
/// main thread's `futex_wait` on the thread's steps.
const ATOMIC_WAITS: &str = r#"
(module
  (import "env" "memory" (memory 1 1 shared))
  (import "wasix_32v1" "callback_signal" (func $callback (param i32 i32)))
  (import "wasix_32v1" "proc_raise_interval" (func $interval (param i32 i64 i32) (result i32)))
  (import "wasix_32v1" "thread_spawn_v2" (func $spawn (param i32 i32) (result i32)))
  (import "wasix_32v1" "futex_wait" (func $wait (param i32 i32 i32 i32) (result i32)))
  (import "wasix_32v1" "futex_wake" (func $wake (param i32 i32) (result i32)))
  (import "wasix_32v1" "proc_exit2" (func $exit (param i32)))
  (data (i32.const 16) "__wasm_signal")
  (global $last (mut i32) (i32.const 0))
  (func (export "__wasm_signal") (param $sig i32) (global.set $last (local.get $sig)))
  ;; Counts its steps at 200, and fails with a status left at 700.
  (func $step (param $n i32)
    (i32.atomic.store (i32.const 200) (local.get $n))
    (drop (memory.atomic.notify (i32.const 200) (i32.const 1))))
  (func (export "wasi_thread_start") (param $tid i32) (param $arg i32)
    (call $step (i32.const 1))
    (if (memory.atomic.wait32 offset=8 (i32.const 592) (i32.const 0) (i64.const -1))
      (then (i32.atomic.store (i32.const 700) (i32.const 30))))
    (call $step (i32.const 2))
    (if (call $wait (i32.const 604) (i32.const 0) (i32.const 0) (i32.const 300))
      (then (i32.atomic.store (i32.const 700) (i32.const 31))))
    (call $step (i32.const 3)))
  (func $await (param $n i32)
    (block $done
      (loop $again
        (br_if $done (i32.ge_u (i32.atomic.load (i32.const 200)) (local.get $n)))
        (drop (call $wait (i32.const 200) (i32.atomic.load (i32.const 200)) (i32.const 0) (i32.const 304)))
        (br $again))))
  (func (export "_start")
    (call $callback (i32.const 16) (i32.const 13))
    (if (call $interval (i32.const 14) (i64.const 1000000) (i32.const 0))
      (then (call $exit (i32.const 1))))
    (if (memory.atomic.wait32 (i32.const 400) (i32.const 0) (i64.const 10000000000))
      (then (call $exit (i32.const 2))))
    (if (i32.ne (global.get $last) (i32.const 14)) (then (call $exit (i32.const 3))))
    (if (i32.ne (memory.atomic.wait32 (i32.const 400) (i32.const 0) (i64.const 1000000)) (i32.const 2))
      (then (call $exit (i32.const 4))))
    (if (i32.ne (memory.atomic.wait32 (i32.const 400) (i32.const 1) (i64.const -1)) (i32.const 1))
      (then (call $exit (i32.const 5))))
    (i64.store (i32.const 408) (i64.const 0x100000000))
    (if (i32.ne (memory.atomic.wait64 (i32.const 408) (i64.const 0x100000000) (i64.const 0)) (i32.const 2))
      (then (call $exit (i32.const 6))))
    (if (i32.ne (memory.atomic.wait64 (i32.const 408) (i64.const 0) (i64.const -1)) (i32.const 1))
      (then (call $exit (i32.const 7))))
    ;; nobody waits
    (if (memory.atomic.notify (i32.const 400) (i32.const 1)) (then (call $exit (i32.const 8))))
    (if (call $spawn (i32.const 100) (i32.const 104)) (then (call $exit (i32.const 9))))
    (call $await (i32.const 1))
    ;; the syscall wakes the instruction's waiter, once it waits
    (block $woken
      (loop $again
        (if (call $wake (i32.const 600) (i32.const 304)) (then (call $exit (i32.const 10))))
        (br_if $woken (i32.load8_u (i32.const 304)))
        (br $again)))
    (call $await (i32.const 2))
    ;; and the instruction the syscall's
    (block $woken
      (loop $again
        (br_if $woken (memory.atomic.notify (i32.const 604) (i32.const 1)))
        (br $again)))
    (call $await (i32.const 3))
    (call $exit (i32.atomic.load (i32.const 700)))))
"#;

#[test]
fn atomic_waits() {
    assert_eq!(status(ATOMIC_WAITS), 0);
}

/// A thread blocked in `memory.atomic.wait32` ends with its group: by `proc_exit2`, and by a
/// signal whose default action kills the process.
#[test]
fn the_group_ends_threads_blocked_in_atomic_waits() {
    let wat = |end: &str| {
        format!(
            r#"
    (module
      (import "env" "memory" (memory 1 1 shared))
      (import "wasix_32v1" "thread_spawn_v2" (func $spawn (param i32 i32) (result i32)))
      (import "wasi_snapshot_preview1" "proc_raise" (func $raise (param i32) (result i32)))
      (import "wasix_32v1" "proc_exit2" (func $exit (param i32)))
      (func (export "wasi_thread_start") (param $tid i32) (param $arg i32)
        (i32.atomic.store (i32.const 200) (i32.const 1))
        (drop (memory.atomic.wait32 (i32.const 400) (i32.const 0) (i64.const -1))))
      (func (export "_start")
        (if (call $spawn (i32.const 100) (i32.const 104)) (then (call $exit (i32.const 1))))
        (block $started
          (loop $again
            (br_if $started (i32.atomic.load (i32.const 200)))
            (br $again)))
        {end}
        (call $exit (i32.const 2))))
    "#
        )
    };
    assert_eq!(status(&wat("(call $exit (i32.const 7))")), 7);
    assert_eq!(
        status(&wat("(drop (call $raise (i32.const 15)))")),
        128 + 15
    );
}

/// `proc_fork_env` continues as the child, whose `proc_exit2` returns in the parent;
/// `proc_join` then reports the child's status, and `CHILD` (12) once there are no children.
#[test]
fn vfork_exit_and_join() {
    let wat = r#"
    (module
      (import "wasix_32v1" "proc_fork_env" (func $vfork (param i32) (result i32)))
      (import "wasix_32v1" "proc_exit2" (func $exit (param i32)))
      (import "wasix_32v1" "proc_join" (func $join (param i32 i32 i32) (result i32)))
      (import "wasix_32v1" "proc_id" (func $proc_id (param i32) (result i32)))
      (import "wasix_32v1" "proc_parent" (func $proc_parent (param i32 i32) (result i32)))
      (memory (export "memory") 1)
      (func (export "_start")
        (drop (call $proc_id (i32.const 96)))
        (if (call $vfork (i32.const 100)) (then (call $exit (i32.const 1))))
        ;; the child: its own pid, whose parent is the caller
        (drop (call $proc_id (i32.const 104)))
        (if (i32.ne (i32.load (i32.const 104)) (i32.load (i32.const 100))) (then (call $exit (i32.const 2))))
        (if (call $proc_parent (i32.load (i32.const 104)) (i32.const 108)) (then (call $exit (i32.const 3))))
        (if (i32.ne (i32.load (i32.const 108)) (i32.load (i32.const 96))) (then (call $exit (i32.const 4))))
        (call $exit (i32.const 5))
        ;; the parent again
        (i32.store8 (i32.const 200) (i32.const 1))
        (i32.store (i32.const 204) (i32.load (i32.const 100)))
        (if (call $join (i32.const 200) (i32.const 0) (i32.const 220)) (then (call $exit (i32.const 6))))
        (if (i32.ne (i32.load8_u (i32.const 220)) (i32.const 1)) (then (call $exit (i32.const 7))))
        (if (i32.ne (i32.load16_u (i32.const 222)) (i32.const 5)) (then (call $exit (i32.const 8))))
        (if (i32.ne (i32.load (i32.const 204)) (i32.load (i32.const 100))) (then (call $exit (i32.const 9))))
        (i32.store8 (i32.const 200) (i32.const 0))
        (if (i32.ne (call $join (i32.const 200) (i32.const 1) (i32.const 220)) (i32.const 12))
          (then (call $exit (i32.const 10))))
        ;; another process's parent needs /proc
        (if (i32.ne (call $proc_parent (i32.add (i32.load (i32.const 96)) (i32.const 1000)) (i32.const 108)) (i32.const 58))
          (then (call $exit (i32.const 11))))
        (call $exit (i32.const 0))))
    "#;
    assert_eq!(status(wat), 0);
}

/// Writes "child" to fd 1 and exits with 3 plus the number of signals it inherited as
/// ignored.
const CHILD: &str = r#"
(module
  (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasix_32v1" "proc_signals_sizes_get" (func $count (param i32) (result i32)))
  (import "wasix_32v1" "proc_signals_get" (func $signals (param i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $exit (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 8) "\10\00\00\00\05\00\00\00")
  (data (i32.const 16) "child")
  (func (export "_start") (local $i i32) (local $ignored i32)
    (drop (call $fd_write (i32.const 1) (i32.const 8) (i32.const 1) (i32.const 32)))
    (drop (call $count (i32.const 40)))
    (drop (call $signals (i32.const 64)))
    ;; (signal, disposition) pairs; IGNORE is 1
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $i) (i32.load (i32.const 40))))
        (if (i32.eq (i32.load8_u (i32.add (i32.const 65) (i32.shl (local.get $i) (i32.const 1)))) (i32.const 1))
          (then (local.set $ignored (i32.add (local.get $ignored) (i32.const 1)))))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $next)))
    (call $exit (i32.add (i32.const 3) (local.get $ignored)))))
"#;

/// `proc_spawn2`: a child found directly and through the search path, one with its stdout
/// redirected to a file and `SIGUSR1` ignored, and a missing program (`NOENT`, 44).
const SPAWN: &str = r#"
(module
  (import "wasix_32v1" "proc_spawn2" (func $spawn
    (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
  (import "wasix_32v1" "proc_join" (func $join (param i32 i32 i32) (result i32)))
  (import "wasix_32v1" "proc_exit2" (func $exit (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 1000) "/prog/child.wasm")
  (data (i32.const 1032) "child\n")
  (data (i32.const 1040) "child.wasm")
  (data (i32.const 1056) "/nope:/prog")
  (data (i32.const 1072) "/prog/missing")
  (data (i32.const 1088) "/prog/out.txt")
  ;; the open action (56 bytes at 2000): OPEN 2, fd 1, path, CREAT|TRUNC, FD_WRITE, follow
  (data (i32.const 2000) "\02")
  (data (i32.const 2004) "\01")
  (data (i32.const 2012) "\40\04\00\00\0d\00\00\00\01\00\00\00\09\00")
  (data (i32.const 2032) "\40")
  ;; signal dispositions at 2100: SIGUSR1 ignored
  (data (i32.const 2100) "\0a\01")
  ;; Waits for the child at 100 and returns its exit code.
  (func $wait_child (result i32)
    (i32.store8 (i32.const 200) (i32.const 1))
    (i32.store (i32.const 204) (i32.load (i32.const 100)))
    (if (call $join (i32.const 200) (i32.const 0) (i32.const 220)) (then (call $exit (i32.const 90))))
    (if (i32.ne (i32.load8_u (i32.const 220)) (i32.const 1)) (then (call $exit (i32.const 91))))
    (i32.load16_u (i32.const 222)))
  (func (export "_start")
    (if (call $spawn (i32.const 1000) (i32.const 16) (i32.const 1032) (i32.const 6) (i32.const 0) (i32.const 0)
                     (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0)
                     (i32.const 100))
      (then (call $exit (i32.const 1))))
    (if (i32.ne (call $wait_child) (i32.const 3)) (then (call $exit (i32.const 2))))
    (if (call $spawn (i32.const 1040) (i32.const 10) (i32.const 1032) (i32.const 6) (i32.const 0) (i32.const 0)
                     (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 1) (i32.const 1056) (i32.const 11)
                     (i32.const 100))
      (then (call $exit (i32.const 3))))
    (if (i32.ne (call $wait_child) (i32.const 3)) (then (call $exit (i32.const 4))))
    (if (call $spawn (i32.const 1000) (i32.const 16) (i32.const 1032) (i32.const 6) (i32.const 0) (i32.const 0)
                     (i32.const 2000) (i32.const 1) (i32.const 2100) (i32.const 1) (i32.const 0) (i32.const 0) (i32.const 0)
                     (i32.const 100))
      (then (call $exit (i32.const 5))))
    (if (i32.ne (call $wait_child) (i32.const 4)) (then (call $exit (i32.const 6))))
    (if (i32.ne (call $spawn (i32.const 1072) (i32.const 13) (i32.const 1032) (i32.const 6) (i32.const 0) (i32.const 0)
                     (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0)
                     (i32.const 100))
                (i32.const 44))
      (then (call $exit (i32.const 7))))
    ;; the failed child was reaped
    (i32.store8 (i32.const 200) (i32.const 0))
    (if (i32.ne (call $join (i32.const 200) (i32.const 1) (i32.const 220)) (i32.const 12))
      (then (call $exit (i32.const 8))))
    (call $exit (i32.const 0))))
"#;

#[test]
fn spawn() {
    let programs = Programs::new("spawn", &[("child.wasm", CHILD)]);
    let (status, stdout) = run(SPAWN, Some(&programs.0));
    assert_eq!(status, 0);
    // The first two children wrote to the inherited stdout, the third to the file.
    assert_eq!(stdout, b"childchild");
    assert_eq!(std::fs::read(programs.0.join("out.txt")).unwrap(), b"child");
}

/// vfork, then `proc_exec3` in the child: the call returns 0 in the parent, whose `proc_join`
/// sees the new image's exit. A failing `proc_exec3` returns its errno to the child.
#[test]
fn vfork_and_exec() {
    let wat = r#"
    (module
      (import "wasix_32v1" "proc_fork_env" (func $vfork (param i32) (result i32)))
      (import "wasix_32v1" "proc_exec3" (func $exec (param i32 i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
      (import "wasix_32v1" "proc_join" (func $join (param i32 i32 i32) (result i32)))
      (import "wasix_32v1" "proc_exit2" (func $exit (param i32)))
      (memory (export "memory") 1)
      (data (i32.const 1000) "/prog/child.wasm")
      (data (i32.const 1032) "child\n")
      (data (i32.const 1072) "/prog/missing")
      (func (export "_start")
        (if (call $vfork (i32.const 100)) (then (call $exit (i32.const 1))))
        ;; the child: a missing program fails here, the real one returns in the parent
        (if (i32.ne (call $exec (i32.const 1072) (i32.const 13) (i32.const 1032) (i32.const 6)
                                (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0))
                    (i32.const 44))
          (then (call $exit (i32.const 2))))
        (if (call $exec (i32.const 1000) (i32.const 16) (i32.const 1032) (i32.const 6)
                        (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0))
          (then (call $exit (i32.const 3))))
        (i32.store8 (i32.const 200) (i32.const 1))
        (i32.store (i32.const 204) (i32.load (i32.const 100)))
        (if (call $join (i32.const 200) (i32.const 0) (i32.const 220)) (then (call $exit (i32.const 4))))
        (if (i32.ne (i32.load16_u (i32.const 222)) (i32.const 3)) (then (call $exit (i32.const 5))))
        (call $exit (i32.const 0))))
    "#;
    let programs = Programs::new("exec", &[("child.wasm", CHILD)]);
    let (status, stdout) = run(wat, Some(&programs.0));
    assert_eq!(status, 0);
    assert_eq!(stdout, b"child");
}

/// Without a terminal on stdio, `tty_get` reports none and v1's default size; the modes
/// `tty_set` stores come back.
#[test]
fn tty() {
    let wat = r#"
    (module
      (import "wasix_32v1" "tty_get" (func $get (param i32) (result i32)))
      (import "wasix_32v1" "tty_set" (func $set (param i32) (result i32)))
      (import "wasix_32v1" "proc_exit2" (func $exit (param i32)))
      (memory (export "memory") 1)
      (func (export "_start")
        (if (call $get (i32.const 100)) (then (call $exit (i32.const 1))))
        (if (i32.ne (i32.load (i32.const 100)) (i32.const 80)) (then (call $exit (i32.const 2))))
        (if (i32.ne (i32.load (i32.const 104)) (i32.const 25)) (then (call $exit (i32.const 3))))
        (if (i32.load8_u (i32.const 117)) (then (call $exit (i32.const 4))))
        ;; echo on
        (i32.store8 (i32.const 119) (i32.const 1))
        (if (call $set (i32.const 100)) (then (call $exit (i32.const 5))))
        (i32.store8 (i32.const 119) (i32.const 0))
        (if (call $get (i32.const 100)) (then (call $exit (i32.const 6))))
        (if (i32.ne (i32.load8_u (i32.const 119)) (i32.const 1)) (then (call $exit (i32.const 7))))
        (call $exit (i32.const 0))))
    "#;
    assert_eq!(status(wat), 0);
}

/// The compat state a shim finds is the calling process's, though the parent's pid is cached
/// in the instance its vfork children run in: each child changes the terminal modes of its own
/// copy, and the parent and its next child still see the parent's.
#[test]
fn vfork_children_have_their_own_compat_state() {
    let wat = r#"
    (module
      (import "wasix_32v1" "tty_get" (func $get (param i32) (result i32)))
      (import "wasix_32v1" "tty_set" (func $set (param i32) (result i32)))
      (import "wasix_32v1" "proc_fork_env" (func $vfork (param i32) (result i32)))
      (import "wasix_32v1" "proc_exit2" (func $exit (param i32)))
      (memory (export "memory") 1)
      ;; A vfork child: sees the parent's echo, turns it off in its own state and exits.
      (func $child
        (if (call $get (i32.const 300)) (then (call $exit (i32.const 20))))
        (if (i32.ne (i32.load8_u (i32.const 319)) (i32.const 1)) (then (call $exit (i32.const 21))))
        (i32.store8 (i32.const 319) (i32.const 0))
        (if (call $set (i32.const 300)) (then (call $exit (i32.const 22))))
        (if (call $get (i32.const 300)) (then (call $exit (i32.const 23))))
        (if (i32.load8_u (i32.const 319)) (then (call $exit (i32.const 24))))
        (call $exit (i32.const 0)))
      (func (export "_start")
        (if (call $get (i32.const 100)) (then (call $exit (i32.const 1))))
        (i32.store8 (i32.const 119) (i32.const 1))
        (if (call $set (i32.const 100)) (then (call $exit (i32.const 2))))
        (if (call $vfork (i32.const 200)) (then (call $exit (i32.const 3))))
        ;; the parent resumes from the child's `proc_exit2`, at the end of `$child`
        (call $child)
        (if (call $get (i32.const 100)) (then (call $exit (i32.const 4))))
        (if (i32.ne (i32.load8_u (i32.const 119)) (i32.const 1)) (then (call $exit (i32.const 5))))
        (if (call $vfork (i32.const 200)) (then (call $exit (i32.const 6))))
        (call $child)
        (if (call $get (i32.const 100)) (then (call $exit (i32.const 7))))
        (if (i32.ne (i32.load8_u (i32.const 119)) (i32.const 1)) (then (call $exit (i32.const 8))))
        (call $exit (i32.const 0))))
    "#;
    assert_eq!(status(wat), 0);
}

/// `stack_checkpoint` needs the asyncify build configuration: `NOTSUP` (58) otherwise, and so
/// is `proc_fork` without a memory copy; a real fork is the kernel's `NOSYS` (52).
#[test]
fn asyncify_calls_without_asyncify() {
    let wat = r#"
    (module
      (import "wasix_32v1" "stack_checkpoint" (func $checkpoint (param i32 i32) (result i32)))
      (import "wasix_32v1" "proc_fork" (func $fork (param i32 i32) (result i32)))
      (import "wasix_32v1" "proc_exit2" (func $exit (param i32)))
      (memory (export "memory") 1)
      (func (export "_start")
        (if (i32.ne (call $checkpoint (i32.const 100) (i32.const 200)) (i32.const 58))
          (then (call $exit (i32.const 1))))
        (if (i32.ne (call $fork (i32.const 0) (i32.const 200)) (i32.const 58))
          (then (call $exit (i32.const 2))))
        (if (i32.ne (call $fork (i32.const 1) (i32.const 200)) (i32.const 52))
          (then (call $exit (i32.const 3))))
        (call $exit (i32.const 0))))
    "#;
    assert_eq!(status(wat), 0);
}
