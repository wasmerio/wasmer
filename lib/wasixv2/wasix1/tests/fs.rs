//! WASIX v1 filesystem calls from WAT guests on a `wasixv2::Machine`, and wasix-libc C programs
//! when a v1 toolchain is available (see [`c_programs`]).
//!
//! Each WAT guest checks its own results and exits with the number of the first check that
//! failed, or 0.

use std::{
    io,
    path::Path,
    process::Command,
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

/// Runs `wasm` and returns its exit status, stdout and stderr.
fn run(wasm: &[u8]) -> (i32, Vec<u8>, Vec<u8>) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, async {
        let machine = Machine::new();
        let stdout = Arc::new(Capture::default());
        let stderr = Arc::new(Capture::default());
        let config = ProcessConfig {
            args: vec!["main".to_string()],
            env: Vec::new(),
            stdio: Stdio {
                stdin: Arc::new(EmptySource),
                stdout: stdout.clone(),
                stderr: stderr.clone(),
            },
        };
        let status = machine.run_process(wasm, config).await.unwrap();
        let out = stdout.0.lock().unwrap().clone();
        let err = stderr.0.lock().unwrap().clone();
        (status, out, err)
    })
}

/// The imports, memory and helpers every WAT guest here shares; `body` is the `_start` body
/// and `data` its data segments (strings from 1024 up).
fn guest(module: &str, data: &str, body: &str) -> Vec<u8> {
    let wat = format!(
        r#"
(module
  (import "{module}" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "{module}" "fd_read" (func $fd_read (param i32 i32 i32 i32) (result i32)))
  (import "{module}" "fd_close" (func $fd_close (param i32) (result i32)))
  (import "{module}" "fd_seek" (func $fd_seek (param i32 i64 i32 i32) (result i32)))
  (import "{module}" "fd_tell" (func $fd_tell (param i32 i32) (result i32)))
  (import "{module}" "fd_pread" (func $fd_pread (param i32 i32 i32 i64 i32) (result i32)))
  (import "{module}" "fd_pwrite" (func $fd_pwrite (param i32 i32 i32 i64 i32) (result i32)))
  (import "{module}" "fd_filestat_get" (func $fd_filestat_get (param i32 i32) (result i32)))
  (import "{module}" "fd_filestat_set_size" (func $fd_filestat_set_size (param i32 i64) (result i32)))
  (import "{module}" "fd_filestat_set_times" (func $fd_filestat_set_times (param i32 i64 i64 i32) (result i32)))
  (import "{module}" "fd_fdstat_get" (func $fd_fdstat_get (param i32 i32) (result i32)))
  (import "{module}" "fd_fdstat_set_flags" (func $fd_fdstat_set_flags (param i32 i32) (result i32)))
  (import "{module}" "fd_fdstat_set_rights" (func $fd_fdstat_set_rights (param i32 i64 i64) (result i32)))
  (import "{module}" "fd_allocate" (func $fd_allocate (param i32 i64 i64) (result i32)))
  (import "{module}" "fd_advise" (func $fd_advise (param i32 i64 i64 i32) (result i32)))
  (import "{module}" "fd_sync" (func $fd_sync (param i32) (result i32)))
  (import "{module}" "fd_datasync" (func $fd_datasync (param i32) (result i32)))
  (import "{module}" "fd_readdir" (func $fd_readdir (param i32 i32 i32 i64 i32) (result i32)))
  (import "{module}" "fd_renumber" (func $fd_renumber (param i32 i32) (result i32)))
  (import "{module}" "path_open" (func $path_open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
  (import "{module}" "path_filestat_get" (func $path_filestat_get (param i32 i32 i32 i32 i32) (result i32)))
  (import "{module}" "path_filestat_set_times" (func $path_filestat_set_times (param i32 i32 i32 i32 i64 i64 i32) (result i32)))
  (import "{module}" "path_create_directory" (func $path_create_directory (param i32 i32 i32) (result i32)))
  (import "{module}" "path_remove_directory" (func $path_remove_directory (param i32 i32 i32) (result i32)))
  (import "{module}" "path_unlink_file" (func $path_unlink_file (param i32 i32 i32) (result i32)))
  (import "{module}" "path_rename" (func $path_rename (param i32 i32 i32 i32 i32 i32) (result i32)))
  (import "{module}" "path_link" (func $path_link (param i32 i32 i32 i32 i32 i32 i32) (result i32)))
  (import "{module}" "path_symlink" (func $path_symlink (param i32 i32 i32 i32 i32) (result i32)))
  (import "{module}" "path_readlink" (func $path_readlink (param i32 i32 i32 i32 i32 i32) (result i32)))
  (import "{module}" "fd_prestat_get" (func $fd_prestat_get (param i32 i32) (result i32)))
  (import "{module}" "proc_exit" (func $proc_exit (param i32)))
  (memory (export "memory") 1)
  {data}
  ;; Exits with `step` unless `errno` is 0.
  (func $ok (param $step i32) (param $errno i32)
    (if (local.get $errno) (then (call $proc_exit (local.get $step)))))
  ;; Exits with `step` unless `a == b`.
  (func $eq (param $step i32) (param $a i32) (param $b i32)
    (if (i32.ne (local.get $a) (local.get $b)) (then (call $proc_exit (local.get $step)))))
  (func $eq64 (param $step i32) (param $a i64) (param $b i64)
    (if (i64.ne (local.get $a) (local.get $b)) (then (call $proc_exit (local.get $step)))))
  ;; One iovec at 0 for `len` bytes at `buf`.
  (func $iov (param $buf i32) (param $len i32)
    (i32.store (i32.const 0) (local.get $buf))
    (i32.store (i32.const 4) (local.get $len)))
  (func (export "_start")
    {body}
    (call $proc_exit (i32.const 0))))
"#
    );
    wat::parse_str(wat).unwrap()
}

const PREVIEW1: &str = "wasi_snapshot_preview1";

/// Rights wasix-libc asks for when it opens for reading and writing.
const RW: &str = "(i64.const 0x1fffffff)";

/// Create, write, seek, read, pread, pwrite, stat and truncate a file under `/tmp` through the
/// root preopen.
#[test]
fn file_io_and_stat() {
    let data = r#"
  (data (i32.const 1024) "tmp/a.txt")
  (data (i32.const 1040) "hello")
"#;
    let body = format!(
        r#"
    ;; path_open(3, follow, "tmp/a.txt", CREAT|TRUNC, rw) -> fd at 16
    (call $ok (i32.const 1) (call $path_open (i32.const 3) (i32.const 1) (i32.const 1024) (i32.const 9)
      (i32.const 9) {RW} {RW} (i32.const 0) (i32.const 16)))
    (call $iov (i32.const 1040) (i32.const 5))
    (call $ok (i32.const 2) (call $fd_write (i32.load (i32.const 16)) (i32.const 0) (i32.const 1) (i32.const 20)))
    (call $eq (i32.const 3) (i32.load (i32.const 20)) (i32.const 5))
    ;; fd_tell is 5, fd_seek(SET 1) is 1
    (call $ok (i32.const 4) (call $fd_tell (i32.load (i32.const 16)) (i32.const 24)))
    (call $eq64 (i32.const 5) (i64.load (i32.const 24)) (i64.const 5))
    (call $ok (i32.const 6) (call $fd_seek (i32.load (i32.const 16)) (i64.const 1) (i32.const 0) (i32.const 24)))
    (call $eq64 (i32.const 7) (i64.load (i32.const 24)) (i64.const 1))
    ;; read 4 bytes: "ello"
    (call $iov (i32.const 2048) (i32.const 4))
    (call $ok (i32.const 8) (call $fd_read (i32.load (i32.const 16)) (i32.const 0) (i32.const 1) (i32.const 20)))
    (call $eq (i32.const 9) (i32.load (i32.const 2048)) (i32.const 0x6f6c6c65))
    ;; pwrite "he" at 5, pread 3 at 4: "ohe"
    (call $iov (i32.const 1040) (i32.const 2))
    (call $ok (i32.const 10) (call $fd_pwrite (i32.load (i32.const 16)) (i32.const 0) (i32.const 1) (i64.const 5) (i32.const 20)))
    (call $iov (i32.const 2048) (i32.const 3))
    (call $ok (i32.const 11) (call $fd_pread (i32.load (i32.const 16)) (i32.const 0) (i32.const 1) (i64.const 4) (i32.const 20)))
    (call $eq (i32.const 12) (i32.load (i32.const 20)) (i32.const 3))
    (call $eq (i32.const 13) (i32.load16_u (i32.const 2048)) (i32.const 0x686f))
    ;; filestat: regular file (4) of 7 bytes, one link
    (call $ok (i32.const 14) (call $fd_filestat_get (i32.load (i32.const 16)) (i32.const 64)))
    (call $eq (i32.const 15) (i32.load8_u (i32.const 80)) (i32.const 4))
    (call $eq64 (i32.const 16) (i64.load (i32.const 88)) (i64.const 1))
    (call $eq64 (i32.const 17) (i64.load (i32.const 96)) (i64.const 7))
    ;; fdstat: regular file, FD_READ and FD_WRITE and FD_SEEK
    (call $ok (i32.const 18) (call $fd_fdstat_get (i32.load (i32.const 16)) (i32.const 128)))
    (call $eq (i32.const 19) (i32.load8_u (i32.const 128)) (i32.const 4))
    (call $eq64 (i32.const 20) (i64.and (i64.load (i32.const 136)) (i64.const 0x46)) (i64.const 0x46))
    ;; set_size 2, then path_filestat_get sees 2 bytes
    (call $ok (i32.const 21) (call $fd_filestat_set_size (i32.load (i32.const 16)) (i64.const 2)))
    (call $ok (i32.const 22) (call $path_filestat_get (i32.const 3) (i32.const 1) (i32.const 1024) (i32.const 9) (i32.const 64)))
    (call $eq64 (i32.const 23) (i64.load (i32.const 96)) (i64.const 2))
    ;; set mtime to 5 s, atime now
    (call $ok (i32.const 24) (call $fd_filestat_set_times (i32.load (i32.const 16)) (i64.const 0) (i64.const 5000000000) (i32.const 6)))
    (call $ok (i32.const 25) (call $fd_filestat_get (i32.load (i32.const 16)) (i32.const 64)))
    (call $eq64 (i32.const 26) (i64.load (i32.const 112)) (i64.const 5000000000))
    ;; allocate grows to 10, advise, sync
    (call $ok (i32.const 27) (call $fd_allocate (i32.load (i32.const 16)) (i64.const 0) (i64.const 10)))
    (call $ok (i32.const 28) (call $fd_filestat_get (i32.load (i32.const 16)) (i32.const 64)))
    (call $eq64 (i32.const 29) (i64.load (i32.const 96)) (i64.const 10))
    (call $ok (i32.const 30) (call $fd_advise (i32.load (i32.const 16)) (i64.const 0) (i64.const 0) (i32.const 1)))
    (call $eq (i32.const 31) (call $fd_advise (i32.load (i32.const 16)) (i64.const 0) (i64.const 0) (i32.const 9)) (i32.const 28))
    (call $ok (i32.const 32) (call $fd_sync (i32.load (i32.const 16))))
    (call $ok (i32.const 33) (call $fd_datasync (i32.load (i32.const 16))))
    ;; append mode via set_flags, reported back
    (call $ok (i32.const 34) (call $fd_fdstat_set_flags (i32.load (i32.const 16)) (i32.const 1)))
    (call $ok (i32.const 35) (call $fd_fdstat_get (i32.load (i32.const 16)) (i32.const 128)))
    (call $eq (i32.const 36) (i32.load16_u (i32.const 130)) (i32.const 1))
    ;; rights: a subset is fine, more than the file has is NOTCAPABLE (76)
    (call $ok (i32.const 37) (call $fd_fdstat_set_rights (i32.load (i32.const 16)) (i64.const 2) (i64.const 0)))
    (call $eq (i32.const 38) (call $fd_fdstat_set_rights (i32.load (i32.const 16)) (i64.const 0x4000) (i64.const 0)) (i32.const 76))
    (call $ok (i32.const 39) (call $fd_close (i32.load (i32.const 16))))
    ;; bad whence (3) is INVAL (28); closed descriptor is BADF (8)
    (call $eq (i32.const 40) (call $fd_seek (i32.const 1) (i64.const 0) (i32.const 3) (i32.const 24)) (i32.const 28))
    (call $eq (i32.const 41) (call $fd_tell (i32.load (i32.const 16)) (i32.const 24)) (i32.const 8))
"#
    );
    assert_eq!(run(&guest(PREVIEW1, data, &body)).0, 0);
}

/// Directories, renames, links and symlinks, and `fd_readdir` with its cookies and truncation.
#[test]
fn directories_and_links() {
    let data = r#"
  (data (i32.const 1024) "tmp/d")
  (data (i32.const 1040) "tmp/d/f")
  (data (i32.const 1056) "tmp/d/g")
  (data (i32.const 1072) "tmp/d/h")
  (data (i32.const 1088) "tmp/d/l")
  (data (i32.const 1104) "g")
"#;
    let body = format!(
        r#"
    (call $ok (i32.const 1) (call $path_create_directory (i32.const 3) (i32.const 1024) (i32.const 5)))
    ;; EXIST (20) the second time
    (call $eq (i32.const 2) (call $path_create_directory (i32.const 3) (i32.const 1024) (i32.const 5)) (i32.const 20))
    ;; create f, rename it to g, hard link g as h, symlink l -> g
    (call $ok (i32.const 3) (call $path_open (i32.const 3) (i32.const 1) (i32.const 1040) (i32.const 7)
      (i32.const 1) {RW} {RW} (i32.const 0) (i32.const 16)))
    (call $ok (i32.const 4) (call $fd_close (i32.load (i32.const 16))))
    (call $ok (i32.const 5) (call $path_rename (i32.const 3) (i32.const 1040) (i32.const 7) (i32.const 3) (i32.const 1056) (i32.const 7)))
    (call $ok (i32.const 6) (call $path_link (i32.const 3) (i32.const 0) (i32.const 1056) (i32.const 7) (i32.const 3) (i32.const 1072) (i32.const 7)))
    (call $ok (i32.const 7) (call $path_symlink (i32.const 1104) (i32.const 1) (i32.const 3) (i32.const 1088) (i32.const 7)))
    ;; readlink: "g", 1 byte
    (call $ok (i32.const 8) (call $path_readlink (i32.const 3) (i32.const 1088) (i32.const 7) (i32.const 2048) (i32.const 64) (i32.const 20)))
    (call $eq (i32.const 9) (i32.load (i32.const 20)) (i32.const 1))
    (call $eq (i32.const 10) (i32.load8_u (i32.const 2048)) (i32.const 0x67))
    ;; nofollow stat of l is a symlink (7), follow is a file (4) with two links
    (call $ok (i32.const 11) (call $path_filestat_get (i32.const 3) (i32.const 0) (i32.const 1088) (i32.const 7) (i32.const 64)))
    (call $eq (i32.const 12) (i32.load8_u (i32.const 80)) (i32.const 7))
    (call $ok (i32.const 13) (call $path_filestat_get (i32.const 3) (i32.const 1) (i32.const 1088) (i32.const 7) (i32.const 64)))
    (call $eq (i32.const 14) (i32.load8_u (i32.const 80)) (i32.const 4))
    (call $eq64 (i32.const 15) (i64.load (i32.const 88)) (i64.const 2))
    ;; opening the symlink without SYMLINK_FOLLOW is LOOP (32)
    (call $eq (i32.const 16) (call $path_open (i32.const 3) (i32.const 0) (i32.const 1088) (i32.const 7)
      (i32.const 0) (i64.const 2) (i64.const 2) (i32.const 0) (i32.const 16)) (i32.const 32))
    ;; open the directory and list it: ".", "..", "g", "h", "l" = 5 entries of 24 + names
    (call $ok (i32.const 17) (call $path_open (i32.const 3) (i32.const 1) (i32.const 1024) (i32.const 5)
      (i32.const 2) (i64.const 0x4000) {RW} (i32.const 0) (i32.const 16)))
    (call $ok (i32.const 18) (call $fd_readdir (i32.load (i32.const 16)) (i32.const 4096) (i32.const 1024) (i64.const 0) (i32.const 20)))
    (call $eq (i32.const 19) (i32.load (i32.const 20)) (i32.const 126))
    ;; the first entry's d_next resumes after it: four entries left
    (call $ok (i32.const 20) (call $fd_readdir (i32.load (i32.const 16)) (i32.const 8192) (i32.const 1024)
      (i64.load (i32.const 4096)) (i32.const 20)))
    (call $eq (i32.const 21) (i32.load (i32.const 20)) (i32.const 101))
    ;; a buffer that cuts an entry is filled to the last byte
    (call $ok (i32.const 22) (call $fd_readdir (i32.load (i32.const 16)) (i32.const 8192) (i32.const 30) (i64.const 0) (i32.const 20)))
    (call $eq (i32.const 23) (i32.load (i32.const 20)) (i32.const 30))
    ;; the directory reports itself as one, with readdir rights; it is not a file to read
    (call $ok (i32.const 24) (call $fd_fdstat_get (i32.load (i32.const 16)) (i32.const 128)))
    (call $eq (i32.const 25) (i32.load8_u (i32.const 128)) (i32.const 3))
    (call $eq64 (i32.const 26) (i64.and (i64.load (i32.const 136)) (i64.const 0x4000)) (i64.const 0x4000))
    (call $ok (i32.const 27) (call $fd_close (i32.load (i32.const 16))))
    ;; NOTEMPTY (55), then clean up
    (call $eq (i32.const 28) (call $path_remove_directory (i32.const 3) (i32.const 1024) (i32.const 5)) (i32.const 55))
    (call $ok (i32.const 29) (call $path_unlink_file (i32.const 3) (i32.const 1056) (i32.const 7)))
    (call $ok (i32.const 30) (call $path_unlink_file (i32.const 3) (i32.const 1072) (i32.const 7)))
    (call $ok (i32.const 31) (call $path_unlink_file (i32.const 3) (i32.const 1088) (i32.const 7)))
    (call $ok (i32.const 32) (call $path_remove_directory (i32.const 3) (i32.const 1024) (i32.const 5)))
    ;; gone: NOENT (44); an empty path is NOENT too
    (call $eq (i32.const 33) (call $path_filestat_get (i32.const 3) (i32.const 1) (i32.const 1024) (i32.const 5) (i32.const 64)) (i32.const 44))
    (call $eq (i32.const 34) (call $path_open (i32.const 5) (i32.const 0) (i32.const 0) (i32.const 0)
      (i32.const 0) (i64.const 0) (i64.const 0) (i32.const 0) (i32.const 16)) (i32.const 44))
    ;; a directory cannot be opened for writing: ISDIR (31)
    (call $eq (i32.const 35) (call $path_open (i32.const 5) (i32.const 0) (i32.const 1024) (i32.const 3)
      (i32.const 0) (i64.const 64) (i64.const 0) (i32.const 0) (i32.const 16)) (i32.const 31))
"#
    );
    assert_eq!(run(&guest(PREVIEW1, data, &body)).0, 0);
}

/// `wasi_unstable` numbers `whence` CUR, END, SET and has a 56-byte `filestat` with a 32-bit
/// link count.
#[test]
fn snapshot0_layouts() {
    let data = r#"
  (data (i32.const 1024) "tmp/s")
  (data (i32.const 1040) "abcdef")
"#;
    let body = format!(
        r#"
    (call $ok (i32.const 1) (call $path_open (i32.const 3) (i32.const 1) (i32.const 1024) (i32.const 5)
      (i32.const 1) {RW} {RW} (i32.const 0) (i32.const 16)))
    (call $iov (i32.const 1040) (i32.const 6))
    (call $ok (i32.const 2) (call $fd_write (i32.load (i32.const 16)) (i32.const 0) (i32.const 1) (i32.const 20)))
    ;; whence 2 is SET here
    (call $ok (i32.const 3) (call $fd_seek (i32.load (i32.const 16)) (i64.const 2) (i32.const 2) (i32.const 24)))
    (call $eq64 (i32.const 4) (i64.load (i32.const 24)) (i64.const 2))
    ;; whence 1 is END
    (call $ok (i32.const 5) (call $fd_seek (i32.load (i32.const 16)) (i64.const -1) (i32.const 1) (i32.const 24)))
    (call $eq64 (i32.const 6) (i64.load (i32.const 24)) (i64.const 5))
    ;; filestat: type at 16, nlink (u32) at 20, size at 24
    (call $ok (i32.const 7) (call $fd_filestat_get (i32.load (i32.const 16)) (i32.const 64)))
    (call $eq (i32.const 8) (i32.load8_u (i32.const 80)) (i32.const 4))
    (call $eq (i32.const 9) (i32.load (i32.const 84)) (i32.const 1))
    (call $eq64 (i32.const 10) (i64.load (i32.const 88)) (i64.const 6))
    (call $ok (i32.const 11) (call $path_filestat_get (i32.const 3) (i32.const 1) (i32.const 1024) (i32.const 5) (i32.const 64)))
    (call $eq64 (i32.const 12) (i64.load (i32.const 88)) (i64.const 6))
"#
    );
    assert_eq!(run(&guest("wasi_unstable", data, &body)).0, 0);
}

/// `fd_renumber` copies (v1's `dup2`). Closing a preopen succeeds and leaves it open, as
/// under v1, until another file is renumbered onto it.
#[test]
fn renumber_and_preopens() {
    let data = r#"
  (data (i32.const 1024) "tmp")
"#;
    let body = r#"
    ;; renumber stdout onto 10; both work
    (call $ok (i32.const 1) (call $fd_renumber (i32.const 1) (i32.const 10)))
    (call $ok (i32.const 2) (call $fd_fdstat_get (i32.const 10) (i32.const 128)))
    (call $ok (i32.const 3) (call $fd_fdstat_get (i32.const 1) (i32.const 128)))
    ;; onto itself: success; a closed one: BADF (8)
    (call $ok (i32.const 4) (call $fd_renumber (i32.const 10) (i32.const 10)))
    (call $eq (i32.const 5) (call $fd_renumber (i32.const 77) (i32.const 77)) (i32.const 8))
    (call $eq (i32.const 6) (call $fd_renumber (i32.const 1) (i32.const -1)) (i32.const 8))
    ;; closing every preopen succeeds; they still resolve paths
    (call $ok (i32.const 7) (call $fd_close (i32.const 3)))
    (call $ok (i32.const 8) (call $fd_close (i32.const 4)))
    (call $ok (i32.const 9) (call $fd_close (i32.const 5)))
    (call $ok (i32.const 10) (call $path_filestat_get (i32.const 3) (i32.const 1) (i32.const 1024) (i32.const 3) (i32.const 64)))
    (call $eq (i32.const 11) (i32.load8_u (i32.const 80)) (i32.const 3))
    (call $ok (i32.const 12) (call $fd_prestat_get (i32.const 5) (i32.const 64)))
    ;; stdout renumbered onto 5: no preopen any more, and closing it closes it
    (call $ok (i32.const 13) (call $fd_renumber (i32.const 1) (i32.const 5)))
    (call $eq (i32.const 14) (call $fd_prestat_get (i32.const 5) (i32.const 64)) (i32.const 8))
    (call $ok (i32.const 15) (call $fd_close (i32.const 5)))
    (call $eq (i32.const 16) (call $fd_close (i32.const 5)) (i32.const 8))
"#;
    assert_eq!(run(&guest(PREVIEW1, data, body)).0, 0);
}

/// The `wasix_32v1` descriptor calls: pipes, dup, descriptor flags, `path_open2`, cwd.
#[test]
fn wasix_descriptor_calls() {
    let wat = r#"
(module
  (import "wasix_32v1" "fd_pipe" (func $fd_pipe (param i32 i32) (result i32)))
  (import "wasix_32v1" "fd_dup" (func $fd_dup (param i32 i32) (result i32)))
  (import "wasix_32v1" "fd_dup2" (func $fd_dup2 (param i32 i32 i32 i32) (result i32)))
  (import "wasix_32v1" "fd_fdflags_get" (func $fd_fdflags_get (param i32 i32) (result i32)))
  (import "wasix_32v1" "fd_fdflags_set" (func $fd_fdflags_set (param i32 i32) (result i32)))
  (import "wasix_32v1" "fd_fdstat_get" (func $fd_fdstat_get (param i32 i32) (result i32)))
  (import "wasix_32v1" "path_open2" (func $path_open2 (param i32 i32 i32 i32 i32 i64 i64 i32 i32 i32) (result i32)))
  (import "wasix_32v1" "getcwd" (func $getcwd (param i32 i32) (result i32)))
  (import "wasix_32v1" "chdir" (func $chdir (param i32 i32) (result i32)))
  (import "wasix_32v1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasix_32v1" "fd_read" (func $fd_read (param i32 i32 i32 i32) (result i32)))
  (import "wasix_32v1" "proc_exit" (func $proc_exit (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 1024) "/tmp")
  (data (i32.const 1040) "ping")
  (data (i32.const 1056) "tmp/c")
  (func $ok (param $step i32) (param $errno i32)
    (if (local.get $errno) (then (call $proc_exit (local.get $step)))))
  (func $eq (param $step i32) (param $a i32) (param $b i32)
    (if (i32.ne (local.get $a) (local.get $b)) (then (call $proc_exit (local.get $step)))))
  (func (export "_start")
    ;; a pipe carries "ping"
    (call $ok (i32.const 1) (call $fd_pipe (i32.const 16) (i32.const 20)))
    (i32.store (i32.const 0) (i32.const 1040))
    (i32.store (i32.const 4) (i32.const 4))
    (call $ok (i32.const 2) (call $fd_write (i32.load (i32.const 20)) (i32.const 0) (i32.const 1) (i32.const 24)))
    (i32.store (i32.const 0) (i32.const 2048))
    (call $ok (i32.const 3) (call $fd_read (i32.load (i32.const 16)) (i32.const 0) (i32.const 1) (i32.const 24)))
    (call $eq (i32.const 4) (i32.load (i32.const 2048)) (i32.const 0x676e6970))
    ;; a pipe is no tty: type UNKNOWN (0); the read end is read-only
    (call $ok (i32.const 5) (call $fd_fdstat_get (i32.load (i32.const 16)) (i32.const 128)))
    (call $eq (i32.const 6) (i32.load8_u (i32.const 128)) (i32.const 0))
    (call $eq (i32.const 7) (i32.wrap_i64 (i64.and (i64.load (i32.const 136)) (i64.const 0x42))) (i32.const 2))
    ;; dup2 from 100 with cloexec lands on 100 and has CLOEXEC; dup has not
    (call $ok (i32.const 8) (call $fd_dup2 (i32.load (i32.const 16)) (i32.const 100) (i32.const 1) (i32.const 28)))
    (call $eq (i32.const 9) (i32.load (i32.const 28)) (i32.const 100))
    (call $ok (i32.const 10) (call $fd_fdflags_get (i32.const 100) (i32.const 32)))
    (call $eq (i32.const 11) (i32.load16_u (i32.const 32)) (i32.const 1))
    (call $ok (i32.const 12) (call $fd_fdflags_set (i32.const 100) (i32.const 0)))
    (call $ok (i32.const 13) (call $fd_fdflags_get (i32.const 100) (i32.const 32)))
    (call $eq (i32.const 14) (i32.load16_u (i32.const 32)) (i32.const 0))
    (call $ok (i32.const 15) (call $fd_dup (i32.const 100) (i32.const 28)))
    (call $ok (i32.const 16) (call $fd_fdflags_get (i32.load (i32.const 28)) (i32.const 32)))
    (call $eq (i32.const 17) (i32.load16_u (i32.const 32)) (i32.const 0))
    ;; dup2 to a minimum past the descriptor limit is INVAL (28)
    (call $eq (i32.const 18) (call $fd_dup2 (i32.const 1) (i32.const 1048576) (i32.const 0) (i32.const 28)) (i32.const 28))
    ;; path_open2 with CLOEXEC
    (call $ok (i32.const 19) (call $path_open2 (i32.const 3) (i32.const 1) (i32.const 1056) (i32.const 5)
      (i32.const 1) (i64.const 0x46) (i64.const 0x46) (i32.const 0) (i32.const 1) (i32.const 28)))
    (call $ok (i32.const 20) (call $fd_fdflags_get (i32.load (i32.const 28)) (i32.const 32)))
    (call $eq (i32.const 21) (i32.load16_u (i32.const 32)) (i32.const 1))
    ;; getcwd: "/" (len 1); too small is RANGE (68) with the size needed
    (i32.store (i32.const 40) (i32.const 64))
    (call $ok (i32.const 22) (call $getcwd (i32.const 3072) (i32.const 40)))
    (call $eq (i32.const 23) (i32.load (i32.const 40)) (i32.const 1))
    (call $eq (i32.const 24) (i32.load16_u (i32.const 3072)) (i32.const 0x2f))
    (call $ok (i32.const 25) (call $chdir (i32.const 1024) (i32.const 4)))
    (i32.store (i32.const 40) (i32.const 2))
    (call $eq (i32.const 26) (call $getcwd (i32.const 3072) (i32.const 40)) (i32.const 68))
    (call $eq (i32.const 27) (i32.load (i32.const 40)) (i32.const 5))
    (call $ok (i32.const 28) (call $getcwd (i32.const 3072) (i32.const 40)))
    (call $eq (i32.const 29) (i32.load (i32.const 40)) (i32.const 4))
    (call $eq (i32.const 30) (i32.load (i32.const 3072)) (i32.const 0x706d742f))
    (call $proc_exit (i32.const 0))))
"#;
    assert_eq!(run(&wat::parse_str(wat).unwrap()).0, 0);
}

/// Builds and runs the wasix-libc programs in `tests/c` when `WASIX1_WASIXCC` names a
/// `wasixcc` set up for the WASIX v1 sysroot (`WASIXCC_SYSROOT` and friends in the
/// environment); skipped otherwise. Each program prints what it checks; the expected output
/// is in its `//# expect: ...` lines (clang-format leaves `//#` pragmas alone).
#[test]
fn c_programs() {
    let Ok(cc) = std::env::var("WASIX1_WASIXCC") else {
        eprintln!("skipped: set WASIX1_WASIXCC to a v1 wasixcc to build the C programs");
        return;
    };
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/c");
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("wasix1-c");
    std::fs::create_dir_all(&out).unwrap();
    let mut sources: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "c"))
        .collect();
    sources.sort();
    assert!(!sources.is_empty());
    for source in sources {
        let name = source.file_stem().unwrap().to_str().unwrap().to_string();
        let wasm = out.join(format!("{name}.wasm"));
        let status = Command::new(&cc)
            .arg("-O1")
            .arg(&source)
            .arg("-o")
            .arg(&wasm)
            .status()
            .unwrap();
        assert!(status.success(), "building {name}");
        let text = std::fs::read_to_string(&source).unwrap();
        let expected: String = text
            .lines()
            .filter_map(|line| line.strip_prefix("//# expect: "))
            .map(|line| format!("{line}\n"))
            .collect();
        let (status, stdout, stderr) = run(&std::fs::read(&wasm).unwrap());
        let stdout = String::from_utf8_lossy(&stdout);
        assert_eq!(
            (status, stdout.as_ref()),
            (0, expected.as_str()),
            "{name}; stderr: {}",
            String::from_utf8_lossy(&stderr)
        );
    }
}
