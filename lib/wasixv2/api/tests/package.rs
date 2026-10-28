//! Package volumes mounted with `Machine::mount_package_volume`, read by hand-written guests.

use std::{
    collections::BTreeMap,
    io,
    sync::{Arc, Mutex},
};

use wasixv2::{ByteSink, EmptySource, LocalFuture, Machine, Stdio};
use webc::v3::{
    ChecksumAlgorithm, SignatureAlgorithm, Timestamps,
    read::OwnedReader,
    write::{DirEntry, Directory, FileEntry, Writer},
};

#[derive(Default)]
struct Capture(Mutex<Vec<u8>>);

impl ByteSink for Capture {
    fn write<'a>(&'a self, data: &'a [u8]) -> LocalFuture<'a, io::Result<usize>> {
        self.0.lock().unwrap().extend_from_slice(data);
        Box::pin(async move { Ok(data.len()) })
    }
}

/// Prints `/data/msg.txt`, then opens it for writing and creates `/data/new.txt`. Exits with
/// 0, or the errno of the first open that failed.
const GUEST: &str = r#"
(module
  (import "wasixv2" "openat" (func $openat (param i32 i32 i32 i32) (result i32)))
  (import "wasixv2" "read" (func $read (param i32 i32 i32) (result i32)))
  (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
  (import "wasixv2" "exit_group" (func $exit_group (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 16) "/data/msg.txt\00")
  (data (i32.const 48) "/data/new.txt\00")
  (func $check (param $ret i32) (result i32)
    (if (i32.lt_s (local.get $ret) (i32.const 0))
      (then (call $exit_group (i32.sub (i32.const 0) (local.get $ret))) unreachable))
    (local.get $ret))
  (func (export "_start")
    (local $fd i32)
    (local.set $fd (call $check (call $openat (i32.const -100) (i32.const 16) (i32.const 0) (i32.const 0))))
    (drop (call $write (i32.const 1) (i32.const 1024)
      (call $check (call $read (local.get $fd) (i32.const 1024) (i32.const 512)))))
    ;; O_WRONLY
    (drop (call $check (call $openat (i32.const -100) (i32.const 16) (i32.const 1) (i32.const 0))))
    ;; O_WRONLY | O_CREAT, 0644
    (drop (call $check (call $openat (i32.const -100) (i32.const 48) (i32.const 0x41) (i32.const 420))))
    (call $exit_group (i32.const 0))
    unreachable))
"#;

fn package() -> OwnedReader {
    let mut children = BTreeMap::new();
    children.insert(
        "msg.txt".parse().unwrap(),
        DirEntry::File(FileEntry::borrowed(
            b"from the package\n",
            Timestamps::default(),
        )),
    );
    let volume = Directory::new(children, Timestamps::default());
    let mut writer = Writer::new(ChecksumAlgorithm::Sha256)
        .write_manifest(&Default::default())
        .unwrap()
        .write_atoms(BTreeMap::new())
        .unwrap();
    writer.write_volume("/data", volume).unwrap();
    OwnedReader::parse(writer.finish(SignatureAlgorithm::None).unwrap()).unwrap()
}

async fn run(writable: bool) -> (i32, Vec<u8>) {
    let machine = Machine::new();
    machine
        .mount_package_volume("/data", &package(), "/data", writable)
        .await
        .unwrap();
    let stdout = Arc::new(Capture::default());
    let stdio = Stdio {
        stdin: Arc::new(EmptySource),
        stdout: stdout.clone(),
        stderr: Arc::new(Capture::default()),
    };
    let wasm = wat::parse_str(GUEST).unwrap();
    let status = machine.run(&wasm, stdio).await.unwrap();
    let out = stdout.0.lock().unwrap().clone();
    (status, out)
}

#[tokio::test]
async fn writable_volume_is_readable_and_copy_on_write() {
    let local = tokio::task::LocalSet::new();
    let (status, stdout) = local.run_until(run(true)).await;
    assert_eq!(stdout, b"from the package\n");
    assert_eq!(status, 0);
}

#[tokio::test]
async fn read_only_volume_refuses_writes() {
    let local = tokio::task::LocalSet::new();
    let (status, stdout) = local.run_until(run(false)).await;
    assert_eq!(stdout, b"from the package\n");
    // EROFS
    assert_eq!(status, 30);
}

/// Prints `/msg.txt`, from the package volume under the root, then opens `/etc/hosts`, from
/// the machine's own layer, and creates `/new.txt`. Exits with 0, or the errno of the first
/// open that failed.
const ROOT_GUEST: &str = r#"
(module
  (import "wasixv2" "openat" (func $openat (param i32 i32 i32 i32) (result i32)))
  (import "wasixv2" "read" (func $read (param i32 i32 i32) (result i32)))
  (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
  (import "wasixv2" "exit_group" (func $exit_group (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 16) "/msg.txt\00")
  (data (i32.const 32) "/etc/hosts\00")
  (data (i32.const 48) "/new.txt\00")
  (func $check (param $ret i32) (result i32)
    (if (i32.lt_s (local.get $ret) (i32.const 0))
      (then (call $exit_group (i32.sub (i32.const 0) (local.get $ret))) unreachable))
    (local.get $ret))
  (func (export "_start")
    (local $fd i32)
    (local.set $fd (call $check (call $openat (i32.const -100) (i32.const 16) (i32.const 0) (i32.const 0))))
    (drop (call $write (i32.const 1) (i32.const 1024)
      (call $check (call $read (local.get $fd) (i32.const 1024) (i32.const 512)))))
    (drop (call $check (call $openat (i32.const -100) (i32.const 32) (i32.const 0) (i32.const 0))))
    ;; O_WRONLY | O_CREAT, 0644
    (drop (call $check (call $openat (i32.const -100) (i32.const 48) (i32.const 0x41) (i32.const 420))))
    (call $exit_group (i32.const 0))
    unreachable))
"#;

#[tokio::test]
async fn root_volume_is_a_writable_layer_under_the_machine_root() {
    let local = tokio::task::LocalSet::new();
    let (status, stdout) = local
        .run_until(async {
            let mut machine = Machine::new();
            machine.mount_root_lower(&package(), "/data").await.unwrap();
            let stdout = Arc::new(Capture::default());
            let stdio = Stdio {
                stdin: Arc::new(EmptySource),
                stdout: stdout.clone(),
                stderr: Arc::new(Capture::default()),
            };
            let wasm = wat::parse_str(ROOT_GUEST).unwrap();
            let status = machine.run(&wasm, stdio).await.unwrap();
            let out = stdout.0.lock().unwrap().clone();
            (status, out)
        })
        .await;
    assert_eq!(stdout, b"from the package\n");
    assert_eq!(status, 0);
}

#[tokio::test]
async fn root_volume_needs_a_fresh_machine() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let mut machine = Machine::new();
            machine
                .mount_package_volume("/data", &package(), "/data", false)
                .await
                .unwrap();
            let err = machine
                .mount_root_lower(&package(), "/data")
                .await
                .unwrap_err();
            // EBUSY
            assert_eq!(err.raw_os_error(), Some(16));
        })
        .await;
}

/// Prints `/a/msg.txt`, then `/b/msg.txt`. Exits with 0, or the errno of a call that failed.
const TWO_MOUNTS_GUEST: &str = r#"
(module
  (import "wasixv2" "openat" (func $openat (param i32 i32 i32 i32) (result i32)))
  (import "wasixv2" "read" (func $read (param i32 i32 i32) (result i32)))
  (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
  (import "wasixv2" "exit_group" (func $exit_group (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 16) "/a/msg.txt\00")
  (data (i32.const 32) "/b/msg.txt\00")
  (func $check (param $ret i32) (result i32)
    (if (i32.lt_s (local.get $ret) (i32.const 0))
      (then (call $exit_group (i32.sub (i32.const 0) (local.get $ret))) unreachable))
    (local.get $ret))
  (func $print (param $path i32) (local $fd i32)
    (local.set $fd (call $check (call $openat (i32.const -100) (local.get $path) (i32.const 0) (i32.const 0))))
    (drop (call $write (i32.const 1) (i32.const 1024)
      (call $check (call $read (local.get $fd) (i32.const 1024) (i32.const 512))))))
  (func (export "_start")
    (call $print (i32.const 16))
    (call $print (i32.const 32))
    (call $exit_group (i32.const 0))
    unreachable))
"#;

/// A layer made once mounts at several paths, read-only and writable.
#[tokio::test]
async fn a_layer_mounts_at_several_paths() {
    let local = tokio::task::LocalSet::new();
    let (status, stdout) = local
        .run_until(async {
            let machine = Machine::new();
            let layer = machine.package_layer(&package(), "/data").unwrap();
            machine.mount_layer("/a", &layer, false).await.unwrap();
            machine.mount_layer("/b", &layer, true).await.unwrap();
            let stdout = Arc::new(Capture::default());
            let stdio = Stdio {
                stdin: Arc::new(EmptySource),
                stdout: stdout.clone(),
                stderr: Arc::new(Capture::default()),
            };
            let wasm = wat::parse_str(TWO_MOUNTS_GUEST).unwrap();
            let status = machine.run(&wasm, stdio).await.unwrap();
            let out = stdout.0.lock().unwrap().clone();
            (status, out)
        })
        .await;
    assert_eq!(stdout, b"from the package\nfrom the package\n");
    assert_eq!(status, 0);
}

/// Layers stacked under the root in one call lie each over the ones before it.
#[tokio::test]
async fn root_layers_stack_in_one_call() {
    let local = tokio::task::LocalSet::new();
    let (status, stdout) = local
        .run_until(async {
            let mut machine = Machine::new();
            let shadowed = shadowing_package("from the lower layer\n");
            let layers = [
                machine.package_layer(&shadowed, "/data").unwrap(),
                machine.package_layer(&package(), "/data").unwrap(),
            ];
            machine.mount_root_layers(layers).await.unwrap();
            let stdout = Arc::new(Capture::default());
            let stdio = Stdio {
                stdin: Arc::new(EmptySource),
                stdout: stdout.clone(),
                stderr: Arc::new(Capture::default()),
            };
            let wasm = wat::parse_str(ROOT_GUEST).unwrap();
            let status = machine.run(&wasm, stdio).await.unwrap();
            let out = stdout.0.lock().unwrap().clone();
            (status, out)
        })
        .await;
    assert_eq!(stdout, b"from the package\n");
    assert_eq!(status, 0);
}

/// A package whose volume `/data` holds `msg.txt` with `text`.
fn shadowing_package(text: &'static str) -> OwnedReader {
    let mut children = BTreeMap::new();
    children.insert(
        "msg.txt".parse().unwrap(),
        DirEntry::File(FileEntry::borrowed(text.as_bytes(), Timestamps::default())),
    );
    let mut writer = Writer::new(ChecksumAlgorithm::Sha256)
        .write_manifest(&Default::default())
        .unwrap()
        .write_atoms(BTreeMap::new())
        .unwrap();
    writer
        .write_volume("/data", Directory::new(children, Timestamps::default()))
        .unwrap();
    OwnedReader::parse(writer.finish(SignatureAlgorithm::None).unwrap()).unwrap()
}
