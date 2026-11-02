#![cfg(not(target_family = "wasm"))]

use wasmer::Module;
use wasmer_types::ModuleHash;
use wasmer_wasix::runners::{
    MappedDirectory,
    wasi::{RuntimeOrEngine, WasiRunner},
};

#[test]
fn unlink_directory_preserves_inode_and_backing_files() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let _guard = runtime.enter();

    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("empty")).unwrap();
    std::fs::create_dir_all(root.path().join("nonempty/child")).unwrap();
    std::fs::write(root.path().join("nonempty/child/file"), "child").unwrap();
    std::fs::write(root.path().join("file"), "file").unwrap();

    let engine = wasmer::Engine::default();
    let module = Module::new(
        &engine,
        r#"
        (module
            (import "wasi_snapshot_preview1" "path_unlink_file"
                (func $unlink (param i32 i32 i32) (result i32)))
            (import "wasi_snapshot_preview1" "path_remove_directory"
                (func $rmdir (param i32 i32 i32) (result i32)))
            (memory (export "memory") 1)
            (data (i32.const 0) "/data/empty")
            (data (i32.const 32) "/data/nonempty")
            (data (i32.const 64) "/data/file")

            (func $assert_eq (param $got i32) (param $want i32)
                (if (i32.ne (local.get $got) (local.get $want))
                    (then unreachable)))

            (func $check_nonempty_directory
                ;; Do not re-stat between unlink and rmdir: that would reload
                ;; the cache entry removed by the rejected unlink.
                (call $assert_eq
                    (call $unlink (i32.const 3) (i32.const 32) (i32.const 14))
                    (i32.const 31)) ;; EISDIR
                (call $assert_eq
                    (call $rmdir (i32.const 3) (i32.const 32) (i32.const 14))
                    (i32.const 55))) ;; ENOTEMPTY

            (func (export "_start")
                (call $assert_eq
                    (call $unlink (i32.const 3) (i32.const 0) (i32.const 11))
                    (i32.const 31))
                (call $assert_eq
                    (call $rmdir (i32.const 3) (i32.const 0) (i32.const 11))
                    (i32.const 0))
                (call $check_nonempty_directory)
                (call $check_nonempty_directory)
                (call $assert_eq
                    (call $unlink (i32.const 3) (i32.const 64) (i32.const 10))
                    (i32.const 0))
                (call $assert_eq
                    (call $unlink (i32.const 3) (i32.const 64) (i32.const 10))
                    (i32.const 44)))) ;; ENOENT
        "#,
    )
    .unwrap();

    WasiRunner::new()
        .with_mapped_directories([MappedDirectory {
            guest: "/data".into(),
            host: root.path().to_path_buf(),
        }])
        .run_wasm(
            RuntimeOrEngine::Engine(engine),
            "unlink-directory",
            module,
            ModuleHash::random(),
        )
        .unwrap();

    assert!(!root.path().join("empty").exists());
    assert!(root.path().join("nonempty").is_dir());
    assert_eq!(
        std::fs::read_to_string(root.path().join("nonempty/child/file")).unwrap(),
        "child"
    );
    assert!(!root.path().join("file").exists());
}
