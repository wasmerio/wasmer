#![cfg(not(target_family = "wasm"))]

use wasmer::Module;
use wasmer_types::ModuleHash;
use wasmer_wasix::runners::wasi::{RuntimeOrEngine, WasiRunner};

#[test]
fn creating_an_existing_root_directory_is_eexist() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let _guard = runtime.enter();

    let engine = wasmer::Engine::default();
    let module = Module::new(
        &engine,
        r#"
        (module
            (import "wasi_snapshot_preview1" "path_create_directory"
                (func $mkdir (param i32 i32 i32) (result i32)))
            (import "wasi_snapshot_preview1" "proc_exit"
                (func $exit (param i32)))
            (memory (export "memory") 1)
            (data (i32.const 0) "/")
            (data (i32.const 8) "//")
            (data (i32.const 16) "/.")
            (data (i32.const 24) "/./")
            (data (i32.const 32) "///./")
            (data (i32.const 40) "/a")
            (data (i32.const 48) "/a/b")
            (data (i32.const 56) "/a/b/c")

            ;; Exit with the errno on a mismatch, so a failure reads as
            ;; ExitCode::(100 + errno): 100 is an unexpected Success, 120 is
            ;; EEXIST (20), 128 is EINVAL (28).
            (func $assert_eq (param $got i32) (param $want i32)
                (if (i32.ne (local.get $got) (local.get $want))
                    (then (call $exit (i32.add (i32.const 100) (local.get $got))))))

            ;; The first `mkdir -p` pass either creates a prefix or finds it.
            (func $create_prefixes
                (call $assert_eq (call $mkdir (i32.const 3) (i32.const 40) (i32.const 2))
                    (i32.const 0))
                (call $assert_eq (call $mkdir (i32.const 3) (i32.const 48) (i32.const 4))
                    (i32.const 0))
                (call $assert_eq (call $mkdir (i32.const 3) (i32.const 56) (i32.const 6))
                    (i32.const 0)))

            ;; ...and the second one must find every prefix already there.
            (func $find_prefixes
                (call $assert_eq (call $mkdir (i32.const 3) (i32.const 40) (i32.const 2))
                    (i32.const 20))
                (call $assert_eq (call $mkdir (i32.const 3) (i32.const 48) (i32.const 4))
                    (i32.const 20))
                (call $assert_eq (call $mkdir (i32.const 3) (i32.const 56) (i32.const 6))
                    (i32.const 20)))

            (func (export "_start")
                ;; Every spelling of the root names a directory that exists.
                (call $assert_eq
                    (call $mkdir (i32.const 3) (i32.const 0) (i32.const 1))
                    (i32.const 20))
                (call $assert_eq
                    (call $mkdir (i32.const 3) (i32.const 8) (i32.const 2))
                    (i32.const 20))
                (call $assert_eq
                    (call $mkdir (i32.const 3) (i32.const 16) (i32.const 2))
                    (i32.const 20))
                (call $assert_eq
                    (call $mkdir (i32.const 3) (i32.const 24) (i32.const 3))
                    (i32.const 20))
                (call $assert_eq
                    (call $mkdir (i32.const 3) (i32.const 32) (i32.const 5))
                    (i32.const 20))
                ;; What `mkdir -p /a/b/c` does, twice.
                (call $create_prefixes)
                (call $find_prefixes)))
        "#,
    )
    .unwrap();

    WasiRunner::new()
        .run_wasm(
            RuntimeOrEngine::Engine(engine),
            "create-root-directory",
            module,
            ModuleHash::random(),
        )
        .unwrap();
}
