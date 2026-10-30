//! WASIX-level coverage for `fd_sync` on a directory descriptor.
//!
//! The `virtual-fs` unit tests cover routing inside the mount table; this one
//! drives the imported syscall from a real guest and checks that the *host*
//! notices. A mapped directory is preopened, the guest flushes every descriptor
//! it can see, the host directory is then removed, and the guest flushes again:
//! exactly the descriptor backed by that directory must start failing.
//!
//! That negative step is the point of the test. `FileSystem::sync_dir` has a
//! no-op default, and several layers between the syscall and the host filesystem
//! can absorb the call into it - `impl<D: Deref<Target = F>> FileSystem for D` in
//! `virtual-fs/src/lib.rs` matches an `Arc<dyn FileSystem>` before auto-deref
//! does, so a forward it forgets silently answers "success" for every mounted
//! filesystem. Return values alone cannot tell a flush from a swallow; only
//! observing the host can.
//!
//! Like `tests/stdio.rs` this needs an engine with a compiler backend:
//! `cargo test -p wasmer-wasix --features wasmer/cranelift --test fd_sync_dir`.
//!
//! The `make test-js` job builds `--tests` for `wasm32-unknown-unknown` with
//! `--no-default-features`, where tokio's multi-thread runtime does not exist,
//! so the whole target is skipped there.
#![cfg(not(target_arch = "wasm32"))]

use std::path::Path;
use std::sync::Arc;

use tokio::runtime::Handle;
use virtual_fs::{FileSystem, MountFileSystem, RootFileSystemBuilder, host_fs};
use wasmer::{Instance, Module, Store};
use wasmer_wasix::{WasiEnvBuilder, WasiFunctionEnv, wasmer_wasix_types::wasi::Errno};

const FD_COUNT: usize = 16;
const ERRNO_TABLE: u64 = 200;
const FILETYPE_TABLE: u64 = 400;
/// WASI `__wasi_filetype_t::directory`.
const FILETYPE_DIRECTORY: u8 = 3;
/// The guest path the host directory is preopened at.
const MOUNT: &str = "/probe";

/// Synchronize fds `0..16` and record, per descriptor, the low byte of the
/// `fd_sync` errno and the `fd_fdstat_get` file type (left at 0 for descriptors
/// the call rejects, since linear memory starts zeroed).
const PROBE_WAT: &str = r#"
(module
  (import "wasix_32v1" "fd_sync" (func $fd_sync (param i32) (result i32)))
  (import "wasix_32v1" "fd_fdstat_get" (func $fd_fdstat_get (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "probe")
    (local $fd i32)
    (local $stat i32)
    (i32.const 0)
    (local.set $fd)
    (loop $each
      (i32.store8
        (i32.add (i32.const 200) (local.get $fd))
        (call $fd_sync (local.get $fd))
      )
      ;; 504 keeps the fdstat struct 8-byte aligned, and a 32-byte stride
      ;; keeps one descriptor's scratch from overlapping the next (it is 24 bytes).
      (local.set $stat (i32.add (i32.const 504) (i32.mul (local.get $fd) (i32.const 32))))
      (drop (call $fd_fdstat_get (local.get $fd) (local.get $stat)))
      (i32.store8
        (i32.add (i32.const 400) (local.get $fd))
        (i32.load8_u (local.get $stat))
      )
      (local.set $fd (i32.add (local.get $fd) (i32.const 1)))
      (br_if $each (i32.lt_u (local.get $fd) (i32.const 16)))
    )
  )
)
"#;

struct Probe {
    /// Kept alive so the tokio handle the environment was built against stays
    /// available for the syscalls issued from [`Probe::sync`].
    _runtime: tokio::runtime::Runtime,
    store: Store,
    instance: Instance,
    _env: WasiFunctionEnv,
}

impl Probe {
    fn spawn(host_dir: &Path) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let engine = wasmer::Engine::default();
        let module = match Module::new(&engine, PROBE_WAT) {
            Ok(module) => module,
            Err(err) => panic!(
                "{err} - this test needs a compiler backend, e.g. --features wasmer/cranelift"
            ),
        };

        // The same shape the CLI builds for `--volume HOST:/probe`: a memory
        // root with the host directory mounted below it.
        let table = MountFileSystem::new();
        table
            .mount(
                Path::new("/"),
                Arc::new(RootFileSystemBuilder::default().build_tmp()),
            )
            .unwrap();
        let guard = runtime.enter();
        table
            .mount(
                Path::new(MOUNT),
                Arc::new(host_fs::FileSystem::new(Handle::current(), host_dir).unwrap())
                    as Arc<dyn FileSystem + Send + Sync>,
            )
            .unwrap();

        let mut builder = WasiEnvBuilder::new("fd_sync_probe")
            .engine(engine.clone())
            .mount_fs(table);
        // Preopen the mount so the guest holds a Kind::Dir descriptor whose path
        // resolves through the mount table into the host filesystem.
        builder.add_preopen_dir(MOUNT).unwrap();

        let mut store = Store::new(engine);
        let (instance, env) = builder.instantiate(module, &mut store).unwrap();
        drop(guard);

        let probe = Probe {
            _runtime: runtime,
            store,
            instance,
            _env: env,
        };
        // Instantiate runs the module's initializers, so this guest must start
        // with every table slot still zero.
        assert_eq!(probe.read_tables().1, vec![0u8; FD_COUNT]);
        probe
    }

    /// Flush every descriptor the guest can see and return the recorded errno
    /// and file-type tables.
    fn sync(&mut self) -> (Vec<u8>, Vec<u8>) {
        let _guard = self._runtime.enter();
        let probe = self
            .instance
            .exports
            .get_function("probe")
            .expect("probe should be exported");
        probe.call(&mut self.store, &[]).expect("probe trapped");
        self.read_tables()
    }

    fn read_tables(&self) -> (Vec<u8>, Vec<u8>) {
        let memory = self.instance.exports.get_memory("memory").unwrap();
        let view = memory.view(&self.store);
        let read = |base: u64| {
            let mut buf = vec![0u8; FD_COUNT];
            view.read(base, &mut buf).unwrap();
            buf
        };
        (read(ERRNO_TABLE), read(FILETYPE_TABLE))
    }
}

fn directory_fds(filetypes: &[u8]) -> Vec<usize> {
    filetypes
        .iter()
        .enumerate()
        .filter(|(_, kind)| **kind == FILETYPE_DIRECTORY)
        .map(|(fd, _)| fd)
        .collect()
}

#[test]
fn fd_sync_of_a_preopened_directory_is_answered_by_the_host_filesystem() {
    let temp = tempfile::TempDir::new().unwrap();
    let host_dir = temp.path().join("data");
    std::fs::create_dir(&host_dir).unwrap();

    let mut probe = Probe::spawn(&host_dir);
    let (errnos, filetypes) = probe.sync();
    let dirs = directory_fds(&filetypes);

    // The probe has to see the preopen it was given, and it has to see
    // descriptors that are not directories - otherwise "everything succeeded"
    // would be vacuous.
    assert!(!dirs.is_empty(), "the mapped directory should be preopened");
    let success = u16::from(Errno::Success) as u8;
    assert!(
        errnos.iter().any(|errno| *errno != success),
        "some descriptors should reject a flush: {errnos:?}"
    );

    // Measured layout: fd3 is `VIRTUAL_ROOT_FD` (`fs/mod.rs:117`), which WASIX
    // preopens unconditionally and which names the synthesized mount table
    // (`Kind::Root`) rather than any one host directory - it answers ISDIR by
    // design, because everything mounted below it flushes itself. fd4 is the
    // host-backed `/probe`, and it is the only descriptor here whose flush the
    // host can actually observe.
    let isdir = u16::from(Errno::Isdir) as u8;
    let root_dirs: Vec<usize> = dirs
        .iter()
        .copied()
        .filter(|fd| errnos[*fd] == isdir)
        .collect();
    let live_dirs: Vec<usize> = dirs
        .iter()
        .copied()
        .filter(|fd| errnos[*fd] == success)
        .collect();
    assert_eq!(
        root_dirs.len(),
        1,
        "the root preopen should be reported as a directory that cannot be flushed: {errnos:?}"
    );
    assert!(
        !live_dirs.is_empty(),
        "flushing a directory that exists on the host must succeed: {errnos:?}"
    );

    std::fs::remove_dir(&host_dir).unwrap();
    let (errnos, _) = probe.sync();

    // Only the preopen backed by the removed directory can fail here: the other
    // directory descriptors live in the memory layer, whose sync is a no-op by
    // design, and the root one is excluded above. A swallowed forward would
    // leave all of them reporting success.
    let failing: Vec<usize> = live_dirs
        .iter()
        .copied()
        .filter(|fd| errnos[*fd] != success)
        .collect();
    assert_eq!(
        failing.len(),
        1,
        "exactly the mounted directory should start reporting failure: {errnos:?}"
    );
    assert_eq!(
        errnos[failing[0]],
        u16::from(Errno::Noent) as u8,
        "the host filesystem's EntryNotFound should reach the guest as ENOENT"
    );
    assert_eq!(
        errnos[root_dirs[0]], isdir,
        "the root preopen keeps its answer whether or not the mount below it exists"
    );
}
