//! Run with an enabled native compiler backend.
#![cfg(all(feature = "sys", not(target_family = "wasm")))]

use std::time::{Duration, Instant};
use wasmer::{Instance, Module, Store};
use wasmer_wasix::WasiEnv;

fn instance() -> (Store, Instance) {
    let mut store = Store::default();
    let module = Module::new(
        &store,
        r#"(module
          (import "wasi_snapshot_preview1" "poll_oneoff"
            (func $poll (param i32 i32 i32 i32) (result i32)))
          (import "wasi_snapshot_preview1" "clock_time_get"
            (func $clock (param i32 i64 i32) (result i32)))
          (memory (export "memory") 1)
          (func (export "_start"))
          (func $poll_clock (export "poll_clock") (param $timeout i64) (param $flags i32) (result i32)
            i32.const 0 i64.const 42 i64.store
            i32.const 16 i32.const 1 i32.store
            i32.const 24 local.get $timeout i64.store
            i32.const 40 local.get $flags i32.store16
            i32.const 0 i32.const 128 i32.const 1 i32.const 256 call $poll
            if unreachable end
            i32.const 128 i64.load i64.const 42 i64.ne if unreachable end
            i32.const 136 i32.load16_u if unreachable end
            i32.const 138 i32.load8_u if unreachable end
            i32.const 256 i32.load)
          (func (export "expired_absolute") (result i32)
            i32.const 1 i64.const 1 i32.const 512 call $clock
            if unreachable end
            i32.const 512 i64.load i64.const 2 i64.le_u if unreachable end
            i32.const 512 i64.load i32.const 1 call $poll_clock))"#,
    )
    .unwrap();
    let (instance, _env) = WasiEnv::builder("poll-clock-test")
        .engine(store.engine().clone())
        .instantiate(module, &mut store)
        .unwrap();
    (store, instance)
}

#[tokio::test(flavor = "multi_thread")]
async fn zero_clock_timeout_returns_a_ready_event() {
    let (mut store, instance) = instance();
    let poll = instance
        .exports
        .get_typed_function::<(i64, i32), i32>(&store, "poll_clock")
        .unwrap();
    // Zero is due in either mode; the existing 1 ns relative path stays ready.
    for (timeout, flags) in [(0, 0), (0, 1), (1, 0)] {
        let start = Instant::now();
        assert_eq!(poll.call(&mut store, timeout, flags).unwrap(), 1);
        assert!(start.elapsed() < Duration::from_secs(1));
    }
    // Unlike zero, this exercises the absolute-deadline comparison.
    let expired = instance
        .exports
        .get_typed_function::<(), i32>(&store, "expired_absolute")
        .unwrap();
    let start = Instant::now();
    assert_eq!(expired.call(&mut store).unwrap(), 1);
    assert!(start.elapsed() < Duration::from_secs(1));
}
