//! Run with `cargo test -p wasmer-wasix --test timestamp_syscalls --features singlepass`.
#![cfg(all(feature = "sys", feature = "singlepass"))]

use std::{path::Path, sync::Arc};
use virtual_fs::{FileSystem, TmpFileSystem};
use wasmer::{Instance, Module, Store};
use wasmer_wasix::{
    WasiEnv,
    wasmer_wasix_types::wasi::{Errno, Filetype, Rights},
};

fn instance(fs: Arc<dyn FileSystem + Send + Sync>) -> (Store, Instance) {
    let mut store = Store::default();
    let rights = (Rights::FD_FILESTAT_GET | Rights::FD_FILESTAT_SET_TIMES).bits();
    let module = Module::new(&store, format!(r#"
        (module
          (import "wasi_snapshot_preview1" "path_filestat_set_times" (func $path_times (param i32 i32 i32 i32 i64 i64 i32) (result i32)))
          (import "wasi_snapshot_preview1" "fd_filestat_set_times" (func $fd_times (param i32 i64 i64 i32) (result i32)))
          (import "wasi_snapshot_preview1" "fd_filestat_get" (func $fd_stat (param i32 i32) (result i32)))
          (import "wasi_snapshot_preview1" "path_filestat_get" (func $path_stat (param i32 i32 i32 i32 i32) (result i32)))
          (import "wasi_snapshot_preview1" "path_open" (func $open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
          (import "wasi_snapshot_preview1" "path_symlink" (func $symlink (param i32 i32 i32 i32 i32) (result i32)))
          (memory (export "memory") 1)
          (data (i32.const 0) "lock")
          (data (i32.const 16) "missing")
          (data (i32.const 32) "lock/link")
          (data (i32.const 64) ".")
          (func (export "_start"))
          (func (export "path_times") (param i64 i64 i32) (result i32)
            i32.const 3 i32.const 1 i32.const 0 i32.const 4 local.get 0 local.get 1 local.get 2 call $path_times)
          (func (export "missing_times") (result i32)
            i32.const 3 i32.const 1 i32.const 16 i32.const 7 i64.const 1 i64.const 2 i32.const 5 call $path_times)
          (func (export "fd_times") (param i32 i64 i64 i32) (result i32)
            local.get 0 local.get 1 local.get 2 local.get 3 call $fd_times)
          (func (export "open_lock") (result i32)
            i32.const 3 i32.const 1 i32.const 0 i32.const 4 i32.const 2
            i64.const {rights} i64.const 0 i32.const 0 i32.const 96 call $open
            if unreachable end
            i32.const 96 i32.load)
          (func (export "fd_mtime") (param i32) (result i64)
            local.get 0 i32.const 128 call $fd_stat if unreachable end
            i32.const 176 i64.load)
          (func (export "symlink_type") (result i32)
            i32.const 64 i32.const 1 i32.const 3 i32.const 32 i32.const 9 call $symlink if unreachable end
            i32.const 3 i32.const 0 i32.const 32 i32.const 9 i32.const 128 call $path_stat if unreachable end
            i32.const 144 i32.load8_u)
        )
    "#)).unwrap();
    let (instance, _env) = WasiEnv::builder("timestamp-test")
        .engine(store.engine().clone())
        .fs(fs as Arc<dyn FileSystem + Send + Sync>)
        .preopen_dir("/")
        .unwrap()
        .instantiate(module, &mut store)
        .unwrap();
    (store, instance)
}

#[tokio::test(flavor = "multi_thread")]
async fn timestamp_syscalls_persist_and_refresh_other_processes_open_directories() {
    let fs = Arc::new(TmpFileSystem::new());
    fs.create_dir(Path::new("/lock")).unwrap();
    let (mut first_store, first) = instance(fs.clone());
    let (mut second_store, second) = instance(fs.clone());
    let open = second
        .exports
        .get_typed_function::<(), i32>(&second_store, "open_lock")
        .unwrap();
    let fd = open.call(&mut second_store).unwrap();
    let mtime = second
        .exports
        .get_typed_function::<i32, i64>(&second_store, "fd_mtime")
        .unwrap();
    let set = first
        .exports
        .get_typed_function::<(i64, i64, i32), i32>(&first_store, "path_times")
        .unwrap();
    assert_eq!(
        set.call(&mut first_store, 123456789, 987654321, 5).unwrap(),
        0
    );
    assert_eq!(mtime.call(&mut second_store, fd).unwrap(), 987654321);
    assert_eq!(
        fs.metadata(Path::new("/lock")).unwrap().accessed(),
        123456789
    );
    assert_eq!(set.call(&mut first_store, 0, 987654322, 4).unwrap(), 0);
    assert_eq!(
        fs.metadata(Path::new("/lock")).unwrap().accessed(),
        123456789
    );
    let set_fd = second
        .exports
        .get_typed_function::<(i32, i64, i64, i32), i32>(&second_store, "fd_times")
        .unwrap();
    assert_eq!(
        set_fd.call(&mut second_store, fd, 0, 987654323, 4).unwrap(),
        0
    );
    assert_eq!(
        fs.metadata(Path::new("/lock")).unwrap().modified(),
        987654323
    );
    assert_eq!(
        set.call(&mut first_store, 0, 0, 12).unwrap(),
        Errno::Inval as i32
    );
    assert_eq!(
        fs.metadata(Path::new("/lock")).unwrap().modified(),
        987654323
    );
    assert_eq!(
        set_fd.call(&mut second_store, 999, 1, 2, 5).unwrap(),
        Errno::Badf as i32
    );
    let missing = first
        .exports
        .get_typed_function::<(), i32>(&first_store, "missing_times")
        .unwrap();
    assert_eq!(missing.call(&mut first_store).unwrap(), Errno::Noent as i32);
    assert_eq!(set.call(&mut first_store, 0, 0, 8).unwrap(), 0);
    assert_eq!(
        fs.metadata(Path::new("/lock")).unwrap().accessed(),
        123456789
    );
    assert!(mtime.call(&mut second_store, fd).unwrap() > 987654323);
    assert!(fs.read_dir(Path::new("/lock")).unwrap().next().is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn timestamp_stat_refresh_preserves_new_symlink_type() {
    let fs = Arc::new(TmpFileSystem::new());
    fs.create_dir(Path::new("/lock")).unwrap();
    let (mut store, instance) = instance(fs);
    let symlink_type = instance
        .exports
        .get_typed_function::<(), i32>(&store, "symlink_type")
        .unwrap();
    assert_eq!(
        symlink_type.call(&mut store).unwrap(),
        Filetype::SymbolicLink as i32
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn timestamp_backend_errors_are_propagated_without_changing_cached_stat() {
    let secondary = Arc::new(TmpFileSystem::new());
    secondary.create_dir(Path::new("/lock")).unwrap();
    secondary
        .set_times(Path::new("/lock"), Some(123), Some(456), true)
        .unwrap();
    let fs = Arc::new(virtual_fs::OverlayFileSystem::new(
        TmpFileSystem::new(),
        [secondary.clone()],
    ));
    let (mut store, instance) = instance(fs);
    let open = instance
        .exports
        .get_typed_function::<(), i32>(&store, "open_lock")
        .unwrap();
    let fd = open.call(&mut store).unwrap();
    let set_fd = instance
        .exports
        .get_typed_function::<(i32, i64, i64, i32), i32>(&store, "fd_times")
        .unwrap();
    assert_eq!(
        set_fd.call(&mut store, fd, 987, 654, 5).unwrap(),
        Errno::Perm as i32
    );
    let set_path = instance
        .exports
        .get_typed_function::<(i64, i64, i32), i32>(&store, "path_times")
        .unwrap();
    assert_eq!(
        set_path.call(&mut store, 987, 654, 5).unwrap(),
        Errno::Perm as i32
    );
    let mtime = instance
        .exports
        .get_typed_function::<i32, i64>(&store, "fd_mtime")
        .unwrap();
    assert_eq!(mtime.call(&mut store, fd).unwrap(), 456);
    assert_eq!(
        secondary.metadata(Path::new("/lock")).unwrap().modified(),
        456
    );
}
