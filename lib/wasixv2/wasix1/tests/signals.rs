//! V1 interval signals through the existing kernel timers, using the WAT/Machine pattern
//! from `proc.rs`. Guests report the failed check through their exit status.

use std::path::PathBuf;

use wasixv2::{Machine, ProcessConfig, Stdio};

fn guest(body: &str) -> Vec<u8> {
    wat::parse_str(format!(
        r#"(module
          (import "wasix_32v1" "callback_signal" (func $callback (param i32 i32)))
          (import "wasix_32v1" "proc_raise_interval" (func $interval (param i32 i64 i32) (result i32)))
          (import "wasix_32v1" "thread_sleep" (func $sleep (param i64) (result i32)))
          (import "wasix_32v1" "proc_fork_env" (func $vfork (param i32) (result i32)))
          (import "wasix_32v1" "proc_exec" (func $exec (param i32 i32 i32 i32)))
          (import "wasix_32v1" "proc_exit2" (func $exit (param i32)))
          (import "wasi_snapshot_preview1" "clock_time_get" (func $clock (param i32 i64 i32) (result i32)))
          (memory (export "memory") 1)
          (data (i32.const 16) "__wasm_signal")
          (data (i32.const 200) "/prog/next.wasm")
          (data (i32.const 232) "next\n")
          (global $ten (mut i32) (i32.const 0))
          (global $twelve (mut i32) (i32.const 0))
          (func (export "__wasm_signal") (param $sig i32)
            (if (i32.eq (local.get $sig) (i32.const 10))
              (then (global.set $ten (i32.add (global.get $ten) (i32.const 1)))))
            (if (i32.eq (local.get $sig) (i32.const 12))
              (then (global.set $twelve (i32.add (global.get $twelve) (i32.const 1))))))
          (func $count (param $sig i32) (result i32)
            (select (global.get $ten) (global.get $twelve) (i32.eq (local.get $sig) (i32.const 10))))
          (func $await (param $sig i32) (param $count i32) (local $tries i32)
            (block $done
              (loop $again
                (br_if $done (i32.ge_u (call $count (local.get $sig)) (local.get $count)))
                (if (i32.ne (call $sleep (i64.const 1000000000)) (i32.const 27))
                  (then (call $exit (i32.const 90))))
                (local.set $tries (i32.add (local.get $tries) (i32.const 1)))
                (if (i32.ge_u (local.get $tries) (i32.const 50))
                  (then (call $exit (i32.const 91))))
                (br $again))))
          (func (export "_start")
            (call $callback (i32.const 16) (i32.const 13))
            {body}
            (call $exit (i32.const 0))))"#
    ))
    .unwrap()
}

/// Source-built temporary executable, as in `proc.rs::Programs`.
struct Program(PathBuf);

impl Program {
    fn new(body: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("wasix1-signal-exec-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("next.wasm");
        std::fs::write(&file, guest(body)).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        Self(dir)
    }
}

impl Drop for Program {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn status(body: &str) -> i32 {
    status_with_program(body, None)
}

fn status_with_program(body: &str, program: Option<&Program>) -> i32 {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    tokio::task::LocalSet::new().block_on(&runtime, async {
        let machine = Machine::new();
        if let Some(program) = program {
            machine
                .mount_host_dir("/prog", &program.0, false)
                .await
                .unwrap();
        }
        machine
            .run_process(
                &guest(body),
                ProcessConfig {
                    args: vec!["signals".to_string()],
                    env: vec![],
                    stdio: Stdio::from_fns(|_| {}, |_| {}),
                },
            )
            .await
            .unwrap()
    })
}

#[test]
fn arbitrary_signal_one_shot_uses_nanoseconds() {
    assert_eq!(
        status(
            r#"
            (drop (call $clock (i32.const 1) (i64.const 0) (i32.const 64)))
            (if (call $interval (i32.const 10) (i64.const 20000000) (i32.const 0))
              (then (call $exit (i32.const 1))))
            (call $await (i32.const 10) (i32.const 1))
            (drop (call $clock (i32.const 1) (i64.const 0) (i32.const 72)))
            ;; A 20ms request must not become 20us, or the old host's 20000000ms.
            (if (i64.lt_u (i64.sub (i64.load (i32.const 72)) (i64.load (i32.const 64))) (i64.const 1000000))
              (then (call $exit (i32.const 2))))
            (if (call $sleep (i64.const 30000000)) (then (call $exit (i32.const 3))))
            (if (i32.ne (global.get $ten) (i32.const 1)) (then (call $exit (i32.const 4))))
            "#
        ),
        0
    );
}

#[test]
fn repeating_signals_are_independent_replaceable_and_disarmable() {
    assert_eq!(
        status(
            r#"
            ;; The later SIGUSR1 timer must not replace SIGUSR2's independent timer.
            (if (call $interval (i32.const 12) (i64.const 50000000) (i32.const 0))
              (then (call $exit (i32.const 1))))
            (if (call $interval (i32.const 10) (i64.const 5000000) (i32.const 1))
              (then (call $exit (i32.const 2))))
            (call $await (i32.const 10) (i32.const 2))
            (if (call $interval (i32.const 10) (i64.const 0) (i32.const 1))
              (then (call $exit (i32.const 4))))
            (call $await (i32.const 12) (i32.const 1))
            ;; Replacement reuses SIGUSR2's timer and updates both deadline and repeat.
            (if (call $interval (i32.const 12) (i64.const 5000000) (i32.const 1))
              (then (call $exit (i32.const 5))))
            (call $await (i32.const 12) (i32.const 3))
            (if (call $interval (i32.const 12) (i64.const 0) (i32.const 0))
              (then (call $exit (i32.const 6))))
            (if (call $sleep (i64.const 20000000)) (then (call $exit (i32.const 7))))
            "#
        ),
        0
    );
}

#[test]
fn interval_signal_must_be_a_valid_v1_signal() {
    for sig in [0, -1, 32, 64, 256] {
        assert_eq!(
            status(&format!(
                r#"(if (i32.ne (call $interval (i32.const {sig}) (i64.const 0) (i32.const 0)) (i32.const 28))
                    (then (call $exit (i32.const 1))))"#
            )),
            0,
            "signal {sig}"
        );
    }
}

#[test]
fn fork_child_gets_its_own_signal_timer_ids() {
    assert_eq!(
        status(
            r#"
            (if (call $interval (i32.const 10) (i64.const 5000000000) (i32.const 0))
              (then (call $exit (i32.const 1))))
            (if (call $vfork (i32.const 96)) (then (call $exit (i32.const 2))))
            ;; The child's kernel timer table is empty: a copied parent ID would be EINVAL.
            (if (call $interval (i32.const 10) (i64.const 5000000) (i32.const 0))
              (then (call $exit (i32.const 3))))
            (call $await (i32.const 10) (i32.const 1))
            (call $exit (i32.const 0))
            ;; The parent's independent ID remains usable after the child exits.
            (if (call $interval (i32.const 10) (i64.const 0) (i32.const 0))
              (then (call $exit (i32.const 4))))
            (if (call $sleep (i64.const 20000000)) (then (call $exit (i32.const 5))))
            "#
        ),
        0
    );
}

#[test]
fn exec_stops_old_signal_timers_and_starts_with_fresh_ids() {
    let next = Program::new(
        r#"
        ;; The old image's periodic SIGUSR1 must not interrupt the new image.
        (if (call $sleep (i64.const 300000000)) (then (call $exit (i32.const 10))))
        (if (global.get $ten) (then (call $exit (i32.const 11))))
        ;; The kernel removed old IDs: a copied compat ID would fail with EINVAL.
        (if (call $interval (i32.const 10) (i64.const 5000000) (i32.const 0))
          (then (call $exit (i32.const 12))))
        (call $await (i32.const 10) (i32.const 1))
        "#,
    );
    assert_eq!(
        status_with_program(
            r#"
            (if (call $interval (i32.const 10) (i64.const 50000000) (i32.const 1))
              (then (call $exit (i32.const 1))))
            (call $exec (i32.const 200) (i32.const 15) (i32.const 232) (i32.const 5))
            (call $exit (i32.const 2))
            "#,
            Some(&next)
        ),
        0
    );
}
