//! A package tree's commands, installed with `Machine::install_commands` and started with
//! `Machine::run_file` or `execve`, as hand-written guests.

#![recursion_limit = "256"]

use std::{
    collections::BTreeMap,
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use wasixv2::{
    ByteSink, CommandDefaults, Compile, EmptySource, Engine, ExecError, Image, LoadError, Loader,
    LocalFuture, Machine, OwnedBuffer, ProcessConfig, RunError, Stdio,
};
use wasmer::Module;
use webc::v3::{
    ChecksumAlgorithm, SignatureAlgorithm, Timestamps,
    read::OwnedReader,
    write::{DirEntry, Directory, FileEntry, Writer},
};

const ENOEXEC: i32 = 8;
const ELOOP: i32 = 40;

#[derive(Default)]
struct Capture(Mutex<Vec<u8>>);

impl ByteSink for Capture {
    fn write<'a>(&'a self, data: &'a [u8]) -> LocalFuture<'a, io::Result<usize>> {
        self.0.lock().unwrap().extend_from_slice(data);
        Box::pin(async move { Ok(data.len()) })
    }
}

/// Prints its arguments, separated by spaces and ended by a newline, followed by `suffix`.
fn echo(suffix: &str) -> Vec<u8> {
    wat::parse_str(format!(
        r#"
(module
  (import "wasixv2" "startup_size" (func $startup_size (result i32)))
  (import "wasixv2" "startup_get" (func $startup_get (param i32 i32) (result i32)))
  (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
  (import "wasixv2" "exit_group" (func $exit_group (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) " \n")
  (data (i32.const 16) "{suffix}")
  (func $strlen (param $p i32) (result i32) (local $n i32)
    (block $done (loop $next
      (br_if $done (i32.eqz (i32.load8_u (i32.add (local.get $p) (local.get $n)))))
      (local.set $n (i32.add (local.get $n) (i32.const 1)))
      (br $next)))
    (local.get $n))
  (func (export "_start") (local $argc i32) (local $i i32) (local $arg i32)
    (drop (call $startup_get (i32.const 1024) (call $startup_size)))
    (local.set $argc (i32.load (i32.const 1024)))
    (block $done (loop $next
      (br_if $done (i32.ge_u (local.get $i) (local.get $argc)))
      (if (local.get $i) (then (drop (call $write (i32.const 1) (i32.const 0) (i32.const 1)))))
      (local.set $arg (i32.load (i32.add (i32.const 1028) (i32.shl (local.get $i) (i32.const 2)))))
      (drop (call $write (i32.const 1) (local.get $arg) (call $strlen (local.get $arg))))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $next)))
    (drop (call $write (i32.const 1) (i32.const 1) (i32.const 1)))
    (drop (call $write (i32.const 1) (i32.const 16) (call $strlen (i32.const 16))))
    (call $exit_group (i32.const 0))
    unreachable))
"#
    ))
    .unwrap()
}

/// [`echo`] for WASI preview1, which the v1 compatibility ABI serves.
const V1_ECHO: &str = r#"
(module
  (import "wasi_snapshot_preview1" "args_sizes_get" (func $sizes (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "args_get" (func $args (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 8) " \n")
  (func $print (param $p i32) (param $len i32)
    (i32.store (i32.const 16) (local.get $p))
    (i32.store (i32.const 20) (local.get $len))
    (drop (call $fd_write (i32.const 1) (i32.const 16) (i32.const 1) (i32.const 24))))
  (func $strlen (param $p i32) (result i32) (local $n i32)
    (block $done (loop $next
      (br_if $done (i32.eqz (i32.load8_u (i32.add (local.get $p) (local.get $n)))))
      (local.set $n (i32.add (local.get $n) (i32.const 1)))
      (br $next)))
    (local.get $n))
  (func (export "_start") (local $i i32) (local $arg i32)
    (drop (call $sizes (i32.const 0) (i32.const 4)))
    (drop (call $args (i32.const 1024) (i32.const 2048)))
    (block $done (loop $next
      (br_if $done (i32.ge_u (local.get $i) (i32.load (i32.const 0))))
      (if (local.get $i) (then (call $print (i32.const 8) (i32.const 1))))
      (local.set $arg (i32.load (i32.add (i32.const 1024) (i32.shl (local.get $i) (i32.const 2)))))
      (call $print (local.get $arg) (call $strlen (local.get $arg)))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $next)))
    (call $print (i32.const 9) (i32.const 1))
    (call $proc_exit (i32.const 0))))
"#;

/// Checks that `/bin/echo` is a regular `0755` file, prints the first four bytes of
/// `/usr/bin/echo`, then `execve`s `/bin/echo from-main`. Exits with the errno of a call that
/// failed, or 100 when the mode is wrong.
const MAIN: &str = r#"
(module
  (import "wasixv2" "statx" (func $statx (param i32 i32 i32 i32 i32) (result i32)))
  (import "wasixv2" "openat" (func $openat (param i32 i32 i32 i32) (result i32)))
  (import "wasixv2" "read" (func $read (param i32 i32 i32) (result i32)))
  (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
  (import "wasixv2" "execve" (func $execve (param i32 i32 i32) (result i32)))
  (import "wasixv2" "exit_group" (func $exit_group (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 16) "/bin/echo\00")
  (data (i32.const 32) "echo\00")
  (data (i32.const 48) "from-main\00")
  (data (i32.const 64) "\20\00\00\00\30\00\00\00\00\00\00\00")
  (data (i32.const 80) "/usr/bin/echo\00")
  (func $check (param $ret i32) (result i32)
    (if (i32.lt_s (local.get $ret) (i32.const 0))
      (then (call $exit_group (i32.sub (i32.const 0) (local.get $ret))) unreachable))
    (local.get $ret))
  (func (export "_start") (local $fd i32)
    ;; STATX_BASIC_STATS
    (drop (call $check (call $statx (i32.const -100) (i32.const 16) (i32.const 0) (i32.const 0x7ff) (i32.const 512))))
    ;; S_IFREG | 0755
    (if (i32.ne (i32.load16_u (i32.const 540)) (i32.const 0x81ed))
      (then (call $exit_group (i32.const 100)) unreachable))
    (local.set $fd (call $check (call $openat (i32.const -100) (i32.const 80) (i32.const 0) (i32.const 0))))
    (drop (call $write (i32.const 1) (i32.const 1024)
      (call $check (call $read (local.get $fd) (i32.const 1024) (i32.const 4)))))
    (call $exit_group (i32.sub (i32.const 0) (call $execve (i32.const 16) (i32.const 64) (i32.const 0))))
    unreachable))
"#;

fn commands(list: &[(&str, &[u8])]) -> Vec<(String, OwnedBuffer)> {
    list.iter()
        .map(|(name, image)| (name.to_string(), OwnedBuffer::from(image.to_vec())))
        .collect()
}

fn wat(source: &str) -> Vec<u8> {
    wat::parse_str(source).unwrap()
}

/// Runs `path` on `machine` with `args`, and returns its exit status and stdout.
async fn run_file(machine: &Machine, path: &str, args: &[&str]) -> (Result<i32, RunError>, String) {
    let stdout = Arc::new(Capture::default());
    let config = ProcessConfig {
        args: args.iter().map(|arg| arg.to_string()).collect(),
        env: Vec::new(),
        stdio: Stdio {
            stdin: Arc::new(EmptySource),
            stdout: stdout.clone(),
            stderr: Arc::new(Capture::default()),
        },
    };
    let status = machine.run_file(&Engine::default(), path, config).await;
    let out = String::from_utf8_lossy(&stdout.0.lock().unwrap()).into_owned();
    (status, out)
}

async fn machine_with(list: &[(&str, &[u8])]) -> Machine {
    let mut machine = Machine::new();
    machine.install_commands(commands(list)).await.unwrap();
    machine
}

fn execve_errno(status: Result<i32, RunError>) -> i32 {
    match status {
        Err(RunError::Execve { err, .. }) => err.raw_os_error().unwrap(),
        other => panic!("expected an execve failure, got {other:?}"),
    }
}

/// Runs `test` in a `LocalSet`, which the machine's host work needs.
async fn local<F: Future>(test: F) -> F::Output {
    tokio::task::LocalSet::new().run_until(test).await
}

#[tokio::test]
async fn a_command_is_an_executable_file_in_bin_and_usr_bin() {
    local(async {
        let machine = machine_with(&[("main", &wat(MAIN)), ("echo", &echo(""))]).await;
        let (status, out) = run_file(&machine, "/usr/bin/main", &["main"]).await;
        assert_eq!(out, "\0asmecho from-main\n");
        assert_eq!(status.unwrap(), 0);
    })
    .await;
}

/// The first command of a name wins, as the resolver orders the tree: the root package's,
/// then its dependencies'.
#[tokio::test]
async fn the_first_command_of_a_name_wins() {
    local(async {
        let machine = machine_with(&[
            ("greet", &echo("root")),
            ("other", &echo("dependency")),
            ("greet", &echo("dependency")),
        ])
        .await;
        let (status, out) = run_file(&machine, "/bin/greet", &["greet"]).await;
        assert_eq!(out, "greet\nroot");
        assert_eq!(status.unwrap(), 0);
        let (_, out) = run_file(&machine, "/usr/bin/other", &["other"]).await;
        assert_eq!(out, "other\ndependency");
    })
    .await;
}

/// A command whose program is a script runs its interpreter, here another command: the
/// interpreter, its argument, the script's path, then the arguments after argv[0].
#[tokio::test]
async fn a_script_command_runs_its_interpreter() {
    local(async {
        let machine = machine_with(&[
            ("script", b"#!/usr/bin/echo  -x \nignored\n"),
            ("echo", &echo("")),
        ])
        .await;
        let (status, out) = run_file(&machine, "/bin/script", &["anything", "a", "b"]).await;
        assert_eq!(out, "/usr/bin/echo -x /bin/script a b\n");
        assert_eq!(status.unwrap(), 0);
    })
    .await;
}

#[tokio::test]
async fn a_script_may_name_a_wasix_v1_interpreter() {
    local(async {
        let machine =
            machine_with(&[("script", b"#!/bin/v1echo\n"), ("v1echo", &wat(V1_ECHO))]).await;
        let (status, out) = run_file(&machine, "/bin/script", &["script", "a"]).await;
        assert_eq!(out, "/bin/v1echo /bin/script a\n");
        assert_eq!(status.unwrap(), 0);
    })
    .await;
}

/// Five scripts may hand over in a row, as Linux's `exec_binprm` allows; a sixth is `ELOOP`.
/// Each level passes the previous interpreter's name as the script.
#[tokio::test]
async fn interpreters_nest_five_deep() {
    local(async {
        let machine = machine_with(&[
            ("s1", b"#!/bin/s2\n"),
            ("s2", b"#!/bin/s3\n"),
            ("s3", b"#!/bin/s4\n"),
            ("s4", b"#!/bin/s5\n"),
            ("s5", b"#!/bin/s6\n"),
            ("s6", b"#!/bin/echo\n"),
            ("echo", &echo("")),
        ])
        .await;
        let (status, out) = run_file(&machine, "/bin/s2", &["s2", "a"]).await;
        assert_eq!(out, "/bin/echo /bin/s6 /bin/s5 /bin/s4 /bin/s3 /bin/s2 a\n");
        assert_eq!(status.unwrap(), 0);
        let (status, out) = run_file(&machine, "/bin/s1", &["s1"]).await;
        assert_eq!(execve_errno(status), ELOOP);
        assert_eq!(out, "");
    })
    .await;
}

#[tokio::test]
async fn a_file_that_is_neither_wasm_nor_a_script_is_enoexec() {
    local(async {
        let machine = machine_with(&[("text", b"echo hi\n"), ("script", b"#!/bin/text\n")]).await;
        let (status, _) = run_file(&machine, "/bin/text", &["text"]).await;
        assert_eq!(execve_errno(status), ENOEXEC);
        let (status, _) = run_file(&machine, "/bin/script", &["script"]).await;
        assert_eq!(execve_errno(status), ENOEXEC);
    })
    .await;
}

/// `execve(argv[1], &argv[1], NULL)`; when that fails, prints `kept` and exits with the errno,
/// which shows that the caller's image survived.
const EXEC_ARG: &str = r#"
(module
  (import "wasixv2" "startup_size" (func $startup_size (result i32)))
  (import "wasixv2" "startup_get" (func $startup_get (param i32 i32) (result i32)))
  (import "wasixv2" "execve" (func $execve (param i32 i32 i32) (result i32)))
  (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
  (import "wasixv2" "exit_group" (func $exit_group (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "kept")
  (func (export "_start") (local $ret i32)
    (drop (call $startup_get (i32.const 1024) (call $startup_size)))
    (local.set $ret (call $execve (i32.load (i32.const 1032)) (i32.const 1032) (i32.const 0)))
    (drop (call $write (i32.const 1) (i32.const 0) (i32.const 4)))
    (call $exit_group (i32.sub (i32.const 0) (local.get $ret)))
    unreachable))
"#;

/// Images in no format the machine runs, by the name they are installed under: a module without imports of any ABI, a `wasix_64v1` module, a component and a module cut
/// off inside its import section.
fn unservable() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        (
            "bare",
            wat(r#"(module (memory (export "memory") 1) (func (export "_start")))"#),
        ),
        (
            "wa64",
            wat(
                r#"(module (import "wasix_64v1" "proc_exit" (func (param i32))) (func (export "_start")))"#,
            ),
        ),
        ("comp", b"\0asm\x0d\0\x01\0".to_vec()),
        ("trnc", b"\0asm\x01\0\0\0\x02\x7f\x01".to_vec()),
    ]
}

/// `execve` refuses an image no ABI or loader serves with `ENOEXEC` before the point of no
/// return, as Linux's `search_binary_handler` does: the caller goes on with its own image.
#[tokio::test]
async fn an_image_nothing_serves_fails_execve_with_enoexec() {
    local(async {
        let images = unservable();
        let mut list: Vec<(&str, &[u8])> = images.iter().map(|(n, i)| (*n, i.as_slice())).collect();
        let exec = wat(EXEC_ARG);
        list.push(("exec", &exec));
        let machine = machine_with(&list).await;
        for (name, _) in &images {
            let path = format!("/bin/{name}");
            let (status, out) = run_file(&machine, "/bin/exec", &["exec", &path]).await;
            assert_eq!((status.unwrap(), out.as_str()), (ENOEXEC, "kept"), "{name}");
            let (status, _) = run_file(&machine, &path, &[name]).await;
            assert_eq!(execve_errno(status), ENOEXEC, "{name}");
        }
    })
    .await;
}

/// Refuses every image with `EACCES`, as a format handler may.
struct Refusing;

impl Loader for Refusing {
    fn probe(&self, _header: &[u8]) -> Result<(), LoadError> {
        Err(LoadError::Format(wasixv2_syscalls::Errno::EACCES))
    }

    fn load<'a>(
        &'a self,
        engine: &'a Engine,
        image: &'a Image,
    ) -> LocalFuture<'a, Result<(Engine, Module), LoadError>> {
        Compile.load(engine, image)
    }
}

/// The loader's probe decides which images `execve` starts, and its errno is `execve`'s.
#[tokio::test]
async fn the_loader_probe_refuses_images_before_execve_commits() {
    local(async {
        let mut machine = Machine::builder().loader(Arc::new(Refusing)).build();
        machine
            .install_commands(commands(&[("echo", &echo(""))]))
            .await
            .unwrap();
        let (status, _) = run_file(&machine, "/bin/echo", &["echo"]).await;
        // EACCES
        assert_eq!(execve_errno(status), 13);
    })
    .await;
}

fn root_volume(tool: &'static [u8]) -> OwnedReader {
    let mut bin = BTreeMap::new();
    for name in ["tool", "echo"] {
        bin.insert(
            name.parse().unwrap(),
            DirEntry::File(FileEntry::borrowed(tool, Timestamps::default())),
        );
    }
    let mut root = BTreeMap::new();
    root.insert(
        "bin".parse().unwrap(),
        DirEntry::Dir(Directory::new(bin, Timestamps::default())),
    );
    let mut writer = Writer::new(ChecksumAlgorithm::Sha256)
        .write_manifest(&Default::default())
        .unwrap()
        .write_atoms(BTreeMap::new())
        .unwrap();
    writer
        .write_volume("root", Directory::new(root, Timestamps::default()))
        .unwrap();
    OwnedReader::parse(writer.finish(SignatureAlgorithm::None).unwrap()).unwrap()
}

/// Commands join a root volume's `/bin`, and hide its files of their names.
#[tokio::test]
async fn commands_merge_with_a_root_volume() {
    local(async {
        let tool: &'static [u8] = echo("volume").leak();
        let mut machine = Machine::new();
        machine
            .mount_root_lower(&root_volume(tool), "root")
            .await
            .unwrap();
        machine
            .install_commands(commands(&[("echo", &echo("command"))]))
            .await
            .unwrap();
        let (_, out) = run_file(&machine, "/bin/tool", &["tool"]).await;
        assert_eq!(out, "tool\nvolume");
        let (_, out) = run_file(&machine, "/bin/echo", &["echo"]).await;
        assert_eq!(out, "echo\ncommand");
    })
    .await;
}

#[tokio::test]
async fn names_that_are_no_file_names_are_refused() {
    local(async {
        for name in ["", ".", "..", "a/b", "a\0b"] {
            let err = Machine::new()
                .install_commands(commands(&[(name, b"")]))
                .await
                .unwrap_err();
            // EINVAL
            assert_eq!(err.raw_os_error(), Some(22), "{name:?}");
        }
        let long = "x".repeat(256);
        let err = Machine::new()
            .install_commands(commands(&[(long.as_str(), b"")]))
            .await
            .unwrap_err();
        // ENAMETOOLONG
        assert_eq!(err.raw_os_error(), Some(36));
    })
    .await;
}

/// Counts the images it compiles, and records whether each came shared by its filesystem.
#[derive(Default)]
struct Counting(AtomicUsize, Mutex<Vec<bool>>);

impl Loader for Counting {
    fn load<'a>(
        &'a self,
        engine: &'a Engine,
        image: &'a Image,
    ) -> LocalFuture<'a, Result<(Engine, Module), LoadError>> {
        self.0.fetch_add(1, Ordering::Relaxed);
        self.1.lock().unwrap().push(image.identity().is_some());
        Compile.load(engine, image)
    }
}

/// The loader compiles the image `run_file` starts and every image `execve` loads.
#[tokio::test]
async fn the_loader_compiles_every_image() {
    local(async {
        let loader = Arc::new(Counting::default());
        let mut machine = Machine::builder().loader(loader.clone()).build();
        machine
            .install_commands(commands(&[("main", &wat(MAIN)), ("echo", &echo(""))]))
            .await
            .unwrap();
        let (status, _) = run_file(&machine, "/bin/main", &["main"]).await;
        assert_eq!(status.unwrap(), 0);
        assert_eq!(loader.0.load(Ordering::Relaxed), 2);
        // Commands are package files: `execve` shares their bytes instead of copying them, so
        // a loader can tell them apart without reading them.
        assert_eq!(*loader.1.lock().unwrap(), [true, true]);
    })
    .await;
}

/// A module the probe takes that does not compile: wasm's header, then a cut-off import
/// section.
fn cut_off() -> Vec<u8> {
    let mut cut = echo("");
    cut.truncate(cut.len() - 8);
    cut
}

/// A module that compiles but cannot instantiate: it imports a function no ABI has.
const UNKNOWN_IMPORT: &str = r#"
(module
  (import "wasixv2" "no_such_call" (func))
  (memory (export "memory") 1)
  (func (export "_start")))
"#;

/// Past `execve`'s point of no return, an image the probe took that does not compile or
/// instantiate kills the process with SIGSEGV, as a corrupt ELF does on Linux.
#[tokio::test]
async fn an_execve_image_that_does_not_start_dies_of_sigsegv() {
    local(async {
        let (cut, unknown) = (cut_off(), wat(UNKNOWN_IMPORT));
        let exec = wat(EXEC_ARG);
        let list: [(&str, &[u8]); 3] = [("cut", &cut), ("unknown", &unknown), ("exec", &exec)];
        let machine = machine_with(&list).await;
        for path in ["/bin/cut", "/bin/unknown"] {
            let (status, out) = run_file(&machine, "/bin/exec", &["exec", path]).await;
            // 128 + SIGSEGV
            assert_eq!((status.unwrap(), out.as_str()), (139, ""), "{path}");
        }
    })
    .await;
}

/// The program `run_file` starts is the embedder's: when it does not compile or instantiate,
/// the run fails with the error, as `run_module` does, instead of reporting a SIGSEGV.
#[tokio::test]
async fn run_file_of_a_program_that_does_not_start_fails() {
    local(async {
        let (cut, unknown) = (cut_off(), wat(UNKNOWN_IMPORT));
        let machine = machine_with(&[("cut", &cut), ("unknown", &unknown)]).await;
        let (status, _) = run_file(&machine, "/bin/cut", &["cut"]).await;
        assert!(
            matches!(status, Err(RunError::Exec(ExecError::Load(_)))),
            "{status:?}"
        );
        let (status, _) = run_file(&machine, "/bin/unknown", &["unknown"]).await;
        assert!(
            matches!(status, Err(RunError::Exec(ExecError::Instantiate(_)))),
            "{status:?}"
        );
    })
    .await;
}

/// Prints its arguments as [`V1_ECHO`] does, then each environment string on a line.
const V1_SHOW: &str = r#"
(module
  (import "wasi_snapshot_preview1" "args_sizes_get" (func $sizes (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "args_get" (func $args (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "environ_sizes_get" (func $env_sizes (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "environ_get" (func $env (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 8) " \n")
  (func $print (param $p i32) (param $len i32)
    (i32.store (i32.const 16) (local.get $p))
    (i32.store (i32.const 20) (local.get $len))
    (drop (call $fd_write (i32.const 1) (i32.const 16) (i32.const 1) (i32.const 24))))
  (func $strlen (param $p i32) (result i32) (local $n i32)
    (block $done (loop $next
      (br_if $done (i32.eqz (i32.load8_u (i32.add (local.get $p) (local.get $n)))))
      (local.set $n (i32.add (local.get $n) (i32.const 1)))
      (br $next)))
    (local.get $n))
  (func (export "_start") (local $i i32) (local $arg i32)
    (drop (call $sizes (i32.const 0) (i32.const 4)))
    (drop (call $args (i32.const 1024) (i32.const 2048)))
    (block $done (loop $next
      (br_if $done (i32.ge_u (local.get $i) (i32.load (i32.const 0))))
      (if (local.get $i) (then (call $print (i32.const 8) (i32.const 1))))
      (local.set $arg (i32.load (i32.add (i32.const 1024) (i32.shl (local.get $i) (i32.const 2)))))
      (call $print (local.get $arg) (call $strlen (local.get $arg)))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $next)))
    (call $print (i32.const 9) (i32.const 1))
    (drop (call $env_sizes (i32.const 0) (i32.const 4)))
    (drop (call $env (i32.const 1024) (i32.const 4096)))
    (local.set $i (i32.const 0))
    (block $done (loop $next
      (br_if $done (i32.ge_u (local.get $i) (i32.load (i32.const 0))))
      (local.set $arg (i32.load (i32.add (i32.const 1024) (i32.shl (local.get $i) (i32.const 2)))))
      (call $print (local.get $arg) (call $strlen (local.get $arg)))
      (call $print (i32.const 9) (i32.const 1))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $next)))
    (call $proc_exit (i32.const 0))))
"#;

/// A WASIX v1 program that runs `show user` through `proc_exec3`, searching the default path,
/// with the environment `A=caller`. Exits with 99 when the call returns.
const V1_EXEC_SHOW: &str = r#"
(module
  (import "wasix_32v1" "proc_exec3" (func $exec (param i32 i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "show")
  (data (i32.const 16) "show\nuser")
  (data (i32.const 32) "A=caller")
  (func (export "_start")
    (drop (call $exec (i32.const 0) (i32.const 4) (i32.const 16) (i32.const 9)
      (i32.const 32) (i32.const 8) (i32.const 1) (i32.const 0) (i32.const 0)))
    (call $proc_exit (i32.const 99))))
"#;

fn show_defaults() -> CommandDefaults {
    CommandDefaults {
        argv0: Some("shown".into()),
        args: vec!["--main".into()],
        env: vec!["A=default".into(), "B=default".into(), "ignored".into()],
    }
}

/// A WASIX v1 program that starts a package command passes it the command's annotation, as v1
/// did: `exec-name` for argv[0], `main-args` after it, and `env` for names not set.
#[tokio::test]
async fn v1_programs_start_commands_with_their_defaults() {
    local(async {
        let mut machine =
            machine_with(&[("main", &wat(V1_EXEC_SHOW)), ("show", &wat(V1_SHOW))]).await;
        machine
            .set_wasix1_command_defaults([("show".to_string(), show_defaults())])
            .unwrap();
        let (status, out) = run_file(&machine, "/bin/main", &["main"]).await;
        assert_eq!(out, "shown --main user\nA=caller\nB=default\n");
        assert_eq!(status.unwrap(), 0);
    })
    .await;
}

/// [`V1_EXEC_SHOW`] with `./show`, which it does not search for.
const V1_EXEC_RELATIVE: &str = r#"
(module
  (import "wasix_32v1" "proc_exec3" (func $exec (param i32 i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "./show")
  (data (i32.const 16) "show\nuser")
  (data (i32.const 32) "A=caller")
  (func (export "_start")
    (drop (call $exec (i32.const 0) (i32.const 6) (i32.const 16) (i32.const 9)
      (i32.const 32) (i32.const 8) (i32.const 0) (i32.const 0) (i32.const 0)))
    (call $proc_exit (i32.const 99))))
"#;

/// The defaults belong to the command's file, not to a path string: a v1 program in `/bin`
/// that starts `./show` passes them too, and one that starts another file does not.
#[tokio::test]
async fn v1_defaults_follow_the_file_whatever_names_it() {
    local(async {
        let mut machine = machine_with(&[
            ("main", &wat(V1_EXEC_RELATIVE)),
            ("show", &wat(V1_SHOW)),
            ("other", &wat(V1_SHOW)),
        ])
        .await;
        machine
            .set_wasix1_command_defaults([("other".to_string(), show_defaults())])
            .unwrap();
        machine.set_working_dir("/usr/bin");
        let (status, out) = run_file(&machine, "/bin/main", &["main"]).await;
        assert_eq!(out, "show user\nA=caller\n");
        assert_eq!(status.unwrap(), 0);

        let mut machine =
            machine_with(&[("main", &wat(V1_EXEC_RELATIVE)), ("show", &wat(V1_SHOW))]).await;
        machine
            .set_wasix1_command_defaults([("show".to_string(), show_defaults())])
            .unwrap();
        machine.set_working_dir("/usr/bin");
        let (status, out) = run_file(&machine, "/bin/main", &["main"]).await;
        assert_eq!(out, "shown --main user\nA=caller\nB=default\n");
        assert_eq!(status.unwrap(), 0);
    })
    .await;
}

/// The defaults are v1's: a native `execve` of the same command runs it as Linux runs any
/// file.
#[tokio::test]
async fn native_execve_ignores_the_v1_defaults() {
    local(async {
        let mut machine = machine_with(&[("main", &wat(MAIN)), ("echo", &echo(""))]).await;
        machine
            .set_wasix1_command_defaults([("echo".to_string(), show_defaults())])
            .unwrap();
        let (status, out) = run_file(&machine, "/bin/main", &["main"]).await;
        assert_eq!(out, "\0asmecho from-main\n");
        assert_eq!(status.unwrap(), 0);
    })
    .await;
}

/// Writes the target of `/proc/self/exe`.
const EXE: &str = r#"
(module
  (import "wasixv2" "readlinkat" (func $readlinkat (param i32 i32 i32 i32) (result i32)))
  (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
  (import "wasixv2" "exit_group" (func $exit_group (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "/proc/self/exe\00")
  (func (export "_start") (local $len i32)
    (local.set $len (call $readlinkat (i32.const -100) (i32.const 0) (i32.const 64) (i32.const 256)))
    (if (i32.lt_s (local.get $len) (i32.const 0))
      (then (call $exit_group (i32.sub (i32.const 0) (local.get $len))) unreachable))
    (drop (call $write (i32.const 1) (i32.const 64) (local.get $len)))
    (call $exit_group (i32.const 0))
    unreachable))
"#;

/// `/proc/self/exe` names the command's file, and for a script the interpreter's.
#[tokio::test]
async fn proc_self_exe_names_the_command() {
    local(async {
        let machine = machine_with(&[("exe", &wat(EXE)), ("script", b"#!/bin/exe\n")]).await;
        let (status, out) = run_file(&machine, "/usr/bin/exe", &["exe"]).await;
        assert_eq!((status.unwrap(), out.as_str()), (0, "/usr/bin/exe"));
        let (status, out) = run_file(&machine, "/usr/bin/script", &["script"]).await;
        assert_eq!((status.unwrap(), out.as_str()), (0, "/bin/exe"));
    })
    .await;
}

/// A program whose `execve` fails is gone, and the machine runs the next one.
#[tokio::test]
async fn a_failed_start_leaves_the_machine_usable() {
    local(async {
        let machine = machine_with(&[("echo", &echo(""))]).await;
        let (status, _) = run_file(&machine, "/bin/missing", &["missing"]).await;
        // ENOENT
        assert_eq!(execve_errno(status), 2);
        let (status, out) = run_file(&machine, "/bin/echo", &["echo", "ok"]).await;
        assert_eq!((status.unwrap(), out.as_str()), (0, "echo ok\n"));
    })
    .await;
}

/// A WASIX v1 program that writes `#!/bin/echo -x` to `/tmp/script` and runs it with
/// `proc_exec3`. v1 has no `chmod`, so the file it creates must be executable already.
const V1_WRITE_AND_RUN: &str = r##"
(module
  (import "wasi_snapshot_preview1" "path_open" (func $path_open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_close" (func $fd_close (param i32) (result i32)))
  (import "wasix_32v1" "proc_exec3" (func $exec (param i32 i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "tmp/script")
  (data (i32.const 16) "#!/bin/echo -x\n")
  (data (i32.const 48) "/tmp/script")
  (data (i32.const 64) "script\narg")
  (func $ok (param $code i32) (param $ret i32)
    (if (local.get $ret) (then (call $proc_exit (local.get $code)))))
  (func (export "_start")
    ;; path_open(3, follow, "tmp/script", CREAT|TRUNC, write) -> fd at 128
    (call $ok (i32.const 1) (call $path_open (i32.const 3) (i32.const 1) (i32.const 0) (i32.const 10)
      (i32.const 9) (i64.const 64) (i64.const 64) (i32.const 0) (i32.const 128)))
    (i32.store (i32.const 136) (i32.const 16))
    (i32.store (i32.const 140) (i32.const 15))
    (call $ok (i32.const 2) (call $fd_write (i32.load (i32.const 128)) (i32.const 136) (i32.const 1) (i32.const 144)))
    (call $ok (i32.const 3) (call $fd_close (i32.load (i32.const 128))))
    (drop (call $exec (i32.const 48) (i32.const 11) (i32.const 64) (i32.const 10)
      (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0)))
    (call $proc_exit (i32.const 99))))
"##;

#[tokio::test]
async fn a_v1_program_runs_a_script_it_wrote() {
    local(async {
        let machine = machine_with(&[("main", &wat(V1_WRITE_AND_RUN)), ("echo", &echo(""))]).await;
        let (status, out) = run_file(&machine, "/bin/main", &["main"]).await;
        assert_eq!(out, "/bin/echo -x /tmp/script arg\n");
        assert_eq!(status.unwrap(), 0);
    })
    .await;
}

/// No commands add no layer under the root: the layers' limit is not used up by empty lists.
#[tokio::test]
async fn no_commands_add_no_layer() {
    local(async {
        let mut machine = Machine::new();
        for _ in 0..300 {
            machine.install_commands(commands(&[])).await.unwrap();
        }
        machine
            .install_commands(commands(&[("echo", &echo(""))]))
            .await
            .unwrap();
        let (status, out) = run_file(&machine, "/bin/echo", &["echo", "ok"]).await;
        assert_eq!((status.unwrap(), out.as_str()), (0, "echo ok\n"));
    })
    .await;
}

/// Handing out a `Signaller` fixes the root: it signals the kernel booted now.
#[tokio::test]
async fn the_root_is_fixed_once_a_signaller_is_handed_out() {
    local(async {
        let mut machine = Machine::new();
        let _signaller = machine.signaller();
        let err = machine
            .install_commands(commands(&[("echo", &echo(""))]))
            .await
            .unwrap_err();
        // EBUSY
        assert_eq!(err.raw_os_error(), Some(16));
    })
    .await;
}

/// Opens `/proc/2` and `/proc/3` and exits with 0 when neither exists, or the number of the
/// one that does; `/proc/1`, init, and `/proc/4`, its own, must exist (else 1 or 4).
const NO_PIDS_2_3: &str = r#"
(module
  (import "wasixv2" "openat" (func $openat (param i32 i32 i32 i32) (result i32)))
  (import "wasixv2" "exit_group" (func $exit_group (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 16) "/proc/2\00")
  (data (i32.const 32) "/proc/3\00")
  (data (i32.const 48) "/proc/4\00")
  (data (i32.const 64) "/proc/1\00")
  (func (export "_start")
    (if (i32.lt_s (call $openat (i32.const -100) (i32.const 64) (i32.const 0x10000) (i32.const 0)) (i32.const 0))
      (then (call $exit_group (i32.const 1))))
    (if (i32.lt_s (call $openat (i32.const -100) (i32.const 48) (i32.const 0x10000) (i32.const 0)) (i32.const 0))
      (then (call $exit_group (i32.const 4))))
    ;; O_RDONLY | O_DIRECTORY; anything but ENOENT means the process is there.
    (if (i32.ne (call $openat (i32.const -100) (i32.const 16) (i32.const 0x10000) (i32.const 0)) (i32.const -2))
      (then (call $exit_group (i32.const 2))))
    (if (i32.ne (call $openat (i32.const -100) (i32.const 32) (i32.const 0x10000) (i32.const 0)) (i32.const -2))
      (then (call $exit_group (i32.const 3))))
    (call $exit_group (i32.const 0))
    unreachable))
"#;

/// A process `run_file` started is gone once the run returns, whether its `execve` failed
/// (pid 2) or it ran and exited (pid 3): no zombie is left in `/proc`. Init (pid 1) stays.
#[tokio::test]
async fn run_file_leaves_no_zombie() {
    local(async {
        let check = wat(NO_PIDS_2_3);
        let machine = machine_with(&[("echo", &echo("")), ("check", &check)]).await;
        let (status, _) = run_file(&machine, "/bin/missing", &["missing"]).await;
        // ENOENT
        assert_eq!(execve_errno(status), 2);
        let (status, _) = run_file(&machine, "/bin/echo", &["echo"]).await;
        assert_eq!(status.unwrap(), 0);
        let (status, _) = run_file(&machine, "/bin/check", &["check"]).await;
        assert_eq!(status.unwrap(), 0);
    })
    .await;
}
