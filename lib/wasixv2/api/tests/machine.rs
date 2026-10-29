//! Machine configuration through the whole stack: the user processes run as and what `uname`
//! reports, seen by hand-written guests.

use std::{
    io,
    sync::{Arc, Mutex},
};

use wasixv2::{ByteSink, Credentials, EmptySource, LocalFuture, Machine, Stdio, Uname};

#[derive(Default)]
struct Capture(Mutex<Vec<u8>>);

impl ByteSink for Capture {
    fn write<'a>(&'a self, data: &'a [u8]) -> LocalFuture<'a, io::Result<usize>> {
        self.0.lock().unwrap().extend_from_slice(data);
        Box::pin(async move { Ok(data.len()) })
    }
}

async fn run(machine: Machine, wat: &str) -> (i32, Vec<u8>) {
    let wasm = wat::parse_str(wat).expect("valid test module");
    let stdout = Arc::new(Capture::default());
    let stdio = Stdio {
        stdin: Arc::new(EmptySource),
        stdout: stdout.clone(),
        stderr: Arc::new(Capture::default()),
    };
    let status = machine.run(&wasm, stdio).await.unwrap();
    let stdout = stdout.0.lock().unwrap().clone();
    (status, stdout)
}

/// Prints `uname -n` and `uname -r`, space-separated, and exits with its effective uid.
const UNAME_WAT: &str = r#"
  (module
    (import "wasixv2" "uname" (func $uname (param i32) (result i32)))
    (import "wasixv2" "geteuid" (func $geteuid (result i32)))
    (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
    (import "wasixv2" "exit_group" (func $exit_group (param i32)))
    (memory (export "memory") 1)
    (data (i32.const 8) " ")

    (func $strlen (param $p i32) (result i32) (local $n i32)
      (block $done
        (loop $next
          (br_if $done (i32.eqz (i32.load8_u (i32.add (local.get $p) (local.get $n)))))
          (local.set $n (i32.add (local.get $n) (i32.const 1)))
          (br $next)))
      (local.get $n))

    (func $print (param $p i32)
      (drop (call $write (i32.const 1) (local.get $p) (call $strlen (local.get $p)))))

    (func (export "_start")
      (drop (call $uname (i32.const 1024)))
      ;; nodename is the second 65-byte field, release the third.
      (call $print (i32.const 1089))
      (drop (call $write (i32.const 1) (i32.const 8) (i32.const 1)))
      (call $print (i32.const 1154))
      (call $exit_group (call $geteuid))
      unreachable))
"#;

#[tokio::test]
async fn uname_and_user_come_from_the_machine_configuration() {
    let uname = Uname::default()
        .with_nodename("box")
        .and_then(|uname| uname.with_release("6.1.0"))
        .unwrap();
    let machine = Machine::builder()
        .user(Credentials::new(42, 42))
        .uname(uname)
        .build();

    assert_eq!(run(machine, UNAME_WAT).await, (42, b"box 6.1.0".to_vec()));
    assert_eq!(
        run(Machine::new(), UNAME_WAT).await,
        (1, b"localhost 7.2.0-wasixv2".to_vec())
    );
}

/// Binds TCP port 80, then creates `/f`, gives it to uid 1234 and makes it mode 0, and opens
/// it for reading. Exits with 0, or with the step that failed after printing its errno as one
/// byte.
const ROOT_ONLY_WAT: &str = r#"
  (module
    (import "wasixv2" "socket" (func $socket (param i32 i32 i32) (result i32)))
    (import "wasixv2" "bind" (func $bind (param i32 i32 i32) (result i32)))
    (import "wasixv2" "openat" (func $openat (param i32 i32 i32 i32) (result i32)))
    (import "wasixv2" "fchownat" (func $fchownat (param i32 i32 i32 i32 i32) (result i32)))
    (import "wasixv2" "fchmodat" (func $fchmodat (param i32 i32 i32) (result i32)))
    (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
    (import "wasixv2" "exit_group" (func $exit_group (param i32)))
    (memory (export "memory") 1)
    ;; sockaddr_in for 127.0.0.1:80
    (data (i32.const 16) "\02\00\00\50\7f\00\00\01\00\00\00\00\00\00\00\00")
    (data (i32.const 32) "/f\00")

    (func $check (param $v i32) (param $step i32) (result i32)
      (if (i32.lt_s (local.get $v) (i32.const 0))
        (then
          (i32.store8 (i32.const 0) (i32.sub (i32.const 0) (local.get $v)))
          (drop (call $write (i32.const 1) (i32.const 0) (i32.const 1)))
          (call $exit_group (local.get $step))
          unreachable))
      (local.get $v))

    (func (export "_start") (local $fd i32)
      (local.set $fd (call $check (call $socket (i32.const 2) (i32.const 1) (i32.const 0)) (i32.const 1)))
      (drop (call $check (call $bind (local.get $fd) (i32.const 16) (i32.const 16)) (i32.const 2)))
      ;; O_WRONLY | O_CREAT, 0600
      (drop (call $check (call $openat (i32.const -100) (i32.const 32) (i32.const 0x41) (i32.const 384)) (i32.const 3)))
      (drop (call $check (call $fchownat (i32.const -100) (i32.const 32) (i32.const 1234) (i32.const 1234) (i32.const 0)) (i32.const 4)))
      (drop (call $check (call $fchmodat (i32.const -100) (i32.const 32) (i32.const 0)) (i32.const 5)))
      (drop (call $check (call $openat (i32.const -100) (i32.const 32) (i32.const 0) (i32.const 0)) (i32.const 6)))
      (call $exit_group (i32.const 0))
      unreachable))
"#;

const EACCES: u8 = 13;

#[tokio::test]
async fn root_binds_low_ports_and_overrides_file_permissions() {
    let root = Machine::builder().user(Credentials::new(0, 0)).build();
    assert_eq!(run(root, ROOT_ONLY_WAT).await, (0, Vec::new()));

    // The unprivileged default user fails at the bind.
    assert_eq!(run(Machine::new(), ROOT_ONLY_WAT).await, (2, vec![EACCES]));
}

/// The default environment names the user's home directory from `/etc/passwd` and musl's
/// search path, which holds `/bin` and `/usr/bin`, where commands are installed.
#[tokio::test]
async fn the_default_environment_has_the_users_home_and_a_path() {
    let path = "PATH=/usr/local/bin:/bin:/usr/bin";
    assert_eq!(Machine::new().default_env(), ["HOME=/home/user", path]);
    let root = Machine::builder().user(Credentials::new(0, 0)).build();
    assert_eq!(root.default_env(), ["HOME=/root", path]);
}

/// Prints `/etc/passwd`, then `/etc/group`; exits with 1 if it cannot open one.
const ACCOUNTS_WAT: &str = r#"
  (module
    (import "wasixv2" "openat" (func $openat (param i32 i32 i32 i32) (result i32)))
    (import "wasixv2" "read" (func $read (param i32 i32 i32) (result i32)))
    (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
    (import "wasixv2" "exit_group" (func $exit_group (param i32)))
    (memory (export "memory") 1)
    (data (i32.const 16) "/etc/passwd\00")
    (data (i32.const 32) "/etc/group\00")

    (func $cat (param $path i32) (local $fd i32) (local $n i32)
      (local.set $fd (call $openat (i32.const -100) (local.get $path) (i32.const 0) (i32.const 0)))
      (if (i32.lt_s (local.get $fd) (i32.const 0))
        (then (call $exit_group (i32.const 1)) unreachable))
      (loop $more
        (local.set $n (call $read (local.get $fd) (i32.const 1024) (i32.const 4096)))
        (if (i32.gt_s (local.get $n) (i32.const 0))
          (then
            (drop (call $write (i32.const 1) (i32.const 1024) (local.get $n)))
            (br $more)))))

    (func (export "_start")
      (call $cat (i32.const 16))
      (call $cat (i32.const 32))
      (call $exit_group (i32.const 0))
      unreachable))
"#;

/// The default root has Debian's names for root and the unprivileged `nobody` (LTP's root-only
/// tests switch to it) and for the `users` group, besides the user's own entries; a user that
/// is one of those accounts gets no second entry.
#[tokio::test]
async fn the_default_root_names_root_nobody_and_the_user() {
    let system_users = "root:x:0:0:root:/root:/bin/sh\n\
                        nobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin\n";
    let system_groups = "root:x:0:\nusers:x:100:\nnogroup:x:65534:\n";

    let (status, out) = run(Machine::new(), ACCOUNTS_WAT).await;
    let expected =
        format!("{system_users}user:x:1:1:user:/home/user:/bin/sh\n{system_groups}user:x:1:\n");
    assert_eq!((status, String::from_utf8(out).unwrap()), (0, expected));

    let root = Machine::builder().user(Credentials::new(0, 0)).build();
    let (status, out) = run(root, ACCOUNTS_WAT).await;
    let expected = format!("{system_users}{system_groups}");
    assert_eq!((status, String::from_utf8(out).unwrap()), (0, expected));

    let nobody = Machine::builder()
        .user(Credentials::new(65534, 100))
        .build();
    assert_eq!(nobody.default_env()[0], "HOME=/nonexistent");
    let (status, out) = run(nobody, ACCOUNTS_WAT).await;
    let expected = format!("{system_users}{system_groups}");
    assert_eq!((status, String::from_utf8(out).unwrap()), (0, expected));
}

/// Prints the target of `/var/run`, a newline, then `/lib/modules/6.1.0/modules.builtin`;
/// exits with the errno of a call that fails.
const KERNEL_FILES_WAT: &str = r#"
  (module
    (import "wasixv2" "readlinkat" (func $readlinkat (param i32 i32 i32 i32) (result i32)))
    (import "wasixv2" "openat" (func $openat (param i32 i32 i32 i32) (result i32)))
    (import "wasixv2" "read" (func $read (param i32 i32 i32) (result i32)))
    (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
    (import "wasixv2" "exit_group" (func $exit_group (param i32)))
    (memory (export "memory") 1)
    (data (i32.const 16) "/var/run\00")
    (data (i32.const 32) "/lib/modules/6.1.0/modules.builtin\00")
    (data (i32.const 96) "\n")

    (func $check (param $v i32) (result i32)
      (if (i32.lt_s (local.get $v) (i32.const 0))
        (then (call $exit_group (i32.sub (i32.const 0) (local.get $v))) unreachable))
      (local.get $v))

    (func (export "_start") (local $fd i32) (local $n i32)
      (local.set $n (call $check (call $readlinkat (i32.const -100) (i32.const 16) (i32.const 1024) (i32.const 64))))
      (drop (call $write (i32.const 1) (i32.const 1024) (local.get $n)))
      (drop (call $write (i32.const 1) (i32.const 96) (i32.const 1)))
      (local.set $fd (call $check (call $openat (i32.const -100) (i32.const 32) (i32.const 0) (i32.const 0))))
      (loop $more
        (local.set $n (call $check (call $read (local.get $fd) (i32.const 1024) (i32.const 4096))))
        (if (i32.gt_s (local.get $n) (i32.const 0))
          (then
            (drop (call $write (i32.const 1) (i32.const 1024) (local.get $n)))
            (br $more))))
      (call $exit_group (i32.const 0))
      unreachable))
"#;

/// The default root links `/var/run` to `/run` and describes the kernel in
/// `/lib/modules/<release>`, for the release `uname` reports: the drivers it has built in.
#[tokio::test]
async fn the_default_root_has_run_and_the_kernels_modules_list() {
    let machine = Machine::builder()
        .uname(Uname::default().with_release("6.1.0").unwrap())
        .build();
    let (status, out) = run(machine, KERNEL_FILES_WAT).await;
    let out = String::from_utf8(out).unwrap();
    assert_eq!(status, 0, "{out}");
    let (link, builtin) = out.split_once('\n').unwrap();
    assert_eq!(link, "/run");
    assert!(builtin.lines().any(|l| l == "kernel/drivers/net/veth.ko"));
    assert!(builtin.lines().any(|l| l == "kernel/drivers/net/dummy.ko"));
}

/// The default root lists the IP protocols in `/etc/protocols`, as a Linux system's `netbase`
/// does, including the IPv6 extension headers RFC 3542 names.
#[tokio::test]
async fn the_default_root_lists_the_ip_protocols() {
    let wat = ACCOUNTS_WAT
        .replace(r#""/etc/passwd\00""#, r#""/etc/protocols\00""#)
        .replace("(call $cat (i32.const 32))\n", "");
    let (status, out) = run(Machine::new(), &wat).await;
    assert_eq!(status, 0);
    let out = String::from_utf8(out).unwrap();
    let number = |name: &str| {
        out.lines()
            .filter(|line| !line.starts_with('#'))
            .find_map(|line| {
                let mut fields = line.split_whitespace();
                (fields.next() == Some(name))
                    .then(|| fields.next().unwrap().parse::<u32>().unwrap())
            })
    };
    for (name, proto) in [
        ("ip", 0),
        ("hopopt", 0),
        ("tcp", 6),
        ("udp", 17),
        ("ipv6-icmp", 58),
    ] {
        assert_eq!(number(name), Some(proto), "{name}");
    }
}

/// Prints `ready`, then waits in `ppoll` with nothing to poll and no timeout, forever.
const BLOCK_WAT: &str = r#"
  (module
    (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
    (import "wasixv2" "ppoll_time64" (func $ppoll (param i32 i32 i32 i32 i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 16) "ready\n")
    (func (export "_start")
      (drop (call $write (i32.const 1) (i32.const 16) (i32.const 6)))
      (loop $forever
        (drop (call $ppoll (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 8)))
        (br $forever))))
"#;

/// Waits until `stdout` holds `ready\n`.
/// Prints `/proc/1/comm` (nothing when it is missing) and exits with
/// `getppid() * 16 + getpid()`.
const INIT_WAT: &str = r#"
  (module
    (import "wasixv2" "openat" (func $openat (param i32 i32 i32 i32) (result i32)))
    (import "wasixv2" "read" (func $read (param i32 i32 i32) (result i32)))
    (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
    (import "wasixv2" "getpid" (func $getpid (result i32)))
    (import "wasixv2" "getppid" (func $getppid (result i32)))
    (import "wasixv2" "exit_group" (func $exit_group (param i32)))
    (memory (export "memory") 1)
    (data (i32.const 16) "/proc/1/comm\00")

    (func (export "_start") (local $fd i32) (local $n i32)
      (local.set $fd (call $openat (i32.const -100) (i32.const 16) (i32.const 0) (i32.const 0)))
      (if (i32.ge_s (local.get $fd) (i32.const 0))
        (then
          (local.set $n (call $read (local.get $fd) (i32.const 64) (i32.const 64)))
          (drop (call $write (i32.const 1) (i32.const 64) (local.get $n)))))
      (call $exit_group
        (i32.add (i32.mul (call $getppid) (i32.const 16)) (call $getpid)))
      unreachable))
"#;

/// By default the machine boots with an init as pid 1, whose child the program is; without
/// one the program is pid 1 with no parent.
#[tokio::test]
async fn programs_run_as_children_of_init_unless_the_machine_has_none() {
    assert_eq!(
        run(Machine::new(), INIT_WAT).await,
        (16 + 2, b"init\n".to_vec())
    );
    let without = Machine::builder().init(false).build();
    let (status, _) = run(without, INIT_WAT).await;
    assert_eq!(status, 1);
}

async fn ready(stdout: &Capture) {
    while stdout.0.lock().unwrap().as_slice() != b"ready\n" {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

fn stdio(stdout: &Arc<Capture>) -> Stdio {
    Stdio {
        stdin: Arc::new(EmptySource),
        stdout: stdout.clone(),
        stderr: Arc::new(Capture::default()),
    }
}

/// `shutdown` kills every process and returns once they are gone; their runs end with
/// 128 + SIGKILL, and the machine runs what comes next.
#[tokio::test]
async fn shutdown_ends_every_process() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let machine = std::rc::Rc::new(Machine::new());
            let wasm = wat::parse_str(BLOCK_WAT).unwrap();
            let stdout = Arc::new(Capture::default());
            let run = tokio::task::spawn_local({
                let (machine, stdio) = (machine.clone(), stdio(&stdout));
                async move { machine.run(&wasm, stdio).await }
            });
            ready(&stdout).await;
            let deadline = std::time::Duration::from_secs(10);
            tokio::time::timeout(deadline, machine.shutdown())
                .await
                .expect("shutdown returned");
            assert_eq!(run.await.unwrap().unwrap(), 128 + 9);
            let (status, out) = run_on(&machine, UNAME_WAT).await;
            assert_eq!(
                (status, out.as_slice()),
                (1, &b"localhost 7.2.0-wasixv2"[..])
            );
        })
        .await;
}

/// A run whose future is dropped kills its process, which `shutdown` then finds gone.
#[tokio::test]
async fn a_dropped_run_kills_its_process() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let machine = Machine::new();
            let wasm = wat::parse_str(BLOCK_WAT).unwrap();
            let stdout = Arc::new(Capture::default());
            // The run's future is dropped once the guest waits.
            tokio::select! {
                _ = machine.run(&wasm, stdio(&stdout)) => panic!("the guest never exits"),
                () = ready(&stdout) => {}
            }
            let deadline = std::time::Duration::from_secs(10);
            tokio::time::timeout(deadline, machine.shutdown())
                .await
                .expect("shutdown returned");
        })
        .await;
}

/// Runs `wat` on `machine` and returns its exit status and stdout.
async fn run_on(machine: &Machine, wat: &str) -> (i32, Vec<u8>) {
    let wasm = wat::parse_str(wat).expect("valid test module");
    let stdout = Arc::new(Capture::default());
    let status = machine.run(&wasm, stdio(&stdout)).await.unwrap();
    let stdout = stdout.0.lock().unwrap().clone();
    (status, stdout)
}

/// Maps a page `PROT_NONE`, then exits with the first check that went wrong, or 0: `write` from
/// it and `uname` into it fail with `EFAULT`, the guest's own store there goes through (wasm
/// has no page protection), and once `mprotect` makes it read-only, `write` from it works and
/// `uname` into it still faults.
const PROT_NONE_WAT: &str = r#"
  (module
    (import "wasixv2" "mmap" (func $mmap (param i32 i32 i32 i32 i32 i64) (result i32)))
    (import "wasixv2" "mprotect" (func $mprotect (param i32 i32 i32) (result i32)))
    (import "wasixv2" "uname" (func $uname (param i32) (result i32)))
    (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
    (import "wasixv2" "exit_group" (func $exit_group (param i32)))
    (memory (export "memory") 1)
    (func $check (param $got i32) (param $want i32) (param $code i32)
      (if (i32.ne (local.get $got) (local.get $want))
        (then (call $exit_group (local.get $code)))))
    (func (export "_start") (local $page i32)
      ;; PROT_NONE, MAP_PRIVATE | MAP_ANONYMOUS.
      (local.set $page
        (call $mmap (i32.const 0) (i32.const 65536) (i32.const 0) (i32.const 0x22)
          (i32.const -1) (i64.const 0)))
      (call $check (i32.lt_s (local.get $page) (i32.const 0)) (i32.const 0) (i32.const 1))
      (call $check (call $write (i32.const 1) (local.get $page) (i32.const 4)) (i32.const -14)
        (i32.const 2))
      (call $check (call $uname (local.get $page)) (i32.const -14) (i32.const 3))
      (i32.store (local.get $page) (i32.const 0x6b6f))
      (call $check (call $mprotect (local.get $page) (i32.const 65536) (i32.const 1))
        (i32.const 0) (i32.const 4))
      (call $check (call $write (i32.const 1) (local.get $page) (i32.const 2)) (i32.const 2)
        (i32.const 5))
      (call $check (call $uname (local.get $page)) (i32.const -14) (i32.const 6))
      (call $exit_group (i32.const 0))
      unreachable))
"#;

/// The kernel's copies fault on pages the address space keeps from them, as Linux's
/// `copy_from_user` and `copy_to_user` do; the guest's own loads and stores do not.
#[tokio::test]
async fn copies_fault_on_pages_the_page_map_keeps_from_them() {
    let (status, out) = run(Machine::new(), PROT_NONE_WAT).await;
    assert_eq!((status, out.as_slice()), (0, &b"ok"[..]));
}
