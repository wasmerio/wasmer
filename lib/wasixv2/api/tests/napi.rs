//! Native bindings run through the real facade and protected kernel allocator.
#![cfg(all(
    feature = "napi",
    feature = "sys",
    feature = "tokio",
    not(target_arch = "wasm32")
))]

use wasixv2::{Machine, Stdio, napi::NapiCtx};

const GUEST: &str = r#"(module
    (import "env" "memory" (memory 2 2048 shared))
    (import "wasix_32v1" "proc_exit" (func $exit (param i32)))
    (import "napi_extension_wasmer_v0" "unofficial_napi_configure_runtime"
        (func $configure (param i32) (result i32)))
    (import "napi_extension_wasmer_v0" "unofficial_napi_collect_garbage"
        (func $gc (param i32) (result i32)))
    (import "napi" "napi_wasm_init_env" (func $init (result i32)))
    (import "napi" "napi_create_string_utf8"
        (func $string (param i32 i32 i32 i32) (result i32)))
    (import "napi" "napi_run_script" (func $script (param i32 i32 i32) (result i32)))
    (import "napi" "napi_get_value_int32" (func $integer (param i32 i32 i32) (result i32)))
    (table (export "__indirect_function_table") 1 funcref)
    (global (export "__stack_pointer") (mut i32) (i32.const 65536))
    (global (export "__data_end") i32 (i32.const 65536))
    (global (export "__heap_base") i32 (i32.const 65536))
    (data (i32.const 1024) "globalThis.keep = new Uint8Array(33554432); keep[0] = 41; keep[33554431] = 1; keep[0] + keep[33554431]\00")
    (data (i32.const 2048) "keep[0] + keep[33554431]\00")
    (global $env (mut i32) (i32.const 0))
    (func $check (param i32) local.get 0 if unreachable end)
    (func $eval (param $source i32)
        global.get $env local.get $source i32.const -1 i32.const 8 call $string call $check
        global.get $env i32.const 8 i32.load i32.const 12 call $script call $check
        global.get $env i32.const 12 i32.load i32.const 16 call $integer call $check
        i32.const 16 i32.load i32.const 42 i32.ne if unreachable end)
    (func (export "_start")
        i32.const 0 call $configure call $check
        call $init global.set $env
        global.get $env i32.eqz if unreachable end
        ;; Larger than the initial 16 MiB fund: actual V8 allocator refill.
        i32.const 1024 call $eval
        global.get $env call $gc call $check
        i32.const 2048 call $eval
        i32.const 0 call $exit unreachable))"#;

#[tokio::test]
async fn facade_native_napi_refills_leases_and_retires_session() {
    const CHILD: &str = "WASIXV2_NAPI_FACADE_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "facade_native_napi_refills_leases_and_retires_session",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(45);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "native facade child failed: {status}");
                return;
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("native facade child did not finish");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    tokio::task::LocalSet::new()
        .run_until(async {
            let context = NapiCtx::builder().max_sessions(1).max_envs(1).build();
            let machine = Machine::builder().napi(context.clone()).build();
            let wasm = wat::parse_str(GUEST).unwrap();
            assert_eq!(
                machine
                    .run(&wasm, Stdio::from_fns(|_| {}, |_| {}))
                    .await
                    .unwrap(),
                0
            );
            assert_eq!(context.active_sessions(), 0);
            assert_eq!(context.budget().snapshot().live_isolates, 0);
            assert_eq!(context.budget().snapshot().v8_heap_reserved, 0);
            assert_eq!(context.budget().snapshot().v8_external, 0);
        })
        .await;
}
