#![cfg(not(target_family = "wasm"))]

use wasmer::Module;
use wasmer_types::ModuleHash;
use wasmer_wasix::runners::{
    MappedDirectory,
    wasi::{RuntimeOrEngine, WasiRunner},
};

#[test]
fn failed_directory_rename_preserves_source_entry() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let _guard = runtime.enter();
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("source")).unwrap();
    std::fs::create_dir(root.path().join("destination")).unwrap();
    std::fs::write(root.path().join("destination/child"), "keep").unwrap();

    let engine = wasmer::Engine::default();
    let module = Module::new(
        &engine,
        r#"
        (module
            (import "wasi_snapshot_preview1" "path_rename"
                (func $rename (param i32 i32 i32 i32 i32 i32) (result i32)))
            (import "wasi_snapshot_preview1" "path_remove_directory"
                (func $rmdir (param i32 i32 i32) (result i32)))
            (memory (export "memory") 1)
            (data (i32.const 0) "/data/source")
            (data (i32.const 32) "/data/destination")

            (func (export "_start")
                ;; Replacing a nonempty directory must fail on every host.
                (if (i32.eqz (call $rename
                        (i32.const 3) (i32.const 0) (i32.const 12)
                        (i32.const 3) (i32.const 32) (i32.const 17)))
                    (then unreachable))
                ;; Do not re-stat first: that would reload a lost cache entry.
                (if (call $rmdir (i32.const 3) (i32.const 0) (i32.const 12))
                    (then unreachable))))
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
            "failed-directory-rename",
            module,
            ModuleHash::random(),
        )
        .unwrap();

    assert!(!root.path().join("source").exists());
    assert_eq!(
        std::fs::read_to_string(root.path().join("destination/child")).unwrap(),
        "keep",
    );
}
