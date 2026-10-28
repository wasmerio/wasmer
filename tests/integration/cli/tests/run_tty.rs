//! `wasmer run` on a terminal: a pseudo-terminal stands for the user's, with `wasmer` as the
//! leader of its session in the foreground, as a shell would start it. The guest gets the
//! terminal as its console, whose line discipline is the guest kernel's, and the terminal is
//! raw while the guest runs and as it was afterwards.
#![cfg(unix)]

use std::{
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::process::CommandExt,
    },
    process::{Child, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use tempfile::TempDir;
use wasmer_integration_tests_cli::wasmer_command;

const ROWS: u16 = 24;
const COLS: u16 = 80;

/// The user's terminal: the master side the test types on and reads, and the slave side
/// `wasmer` runs on.
struct Terminal {
    master: OwnedFd,
    slave: OwnedFd,
    shown: Arc<Mutex<Vec<u8>>>,
}

fn winsize(rows: u16) -> libc::winsize {
    libc::winsize {
        ws_row: rows,
        ws_col: COLS,
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}

impl Terminal {
    fn open() -> Self {
        let (mut master, mut slave) = (0, 0);
        let size = winsize(ROWS);
        // SAFETY: `openpty` fills both descriptors when it succeeds.
        let ret = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null(),
                &size,
            )
        };
        assert_eq!(ret, 0, "openpty: {}", std::io::Error::last_os_error());
        // SAFETY: just opened, owned here.
        let (master, slave) =
            unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
        for fd in [&master, &slave] {
            // SAFETY: a flag on our own descriptor; `wasmer` gets duplicates of the slave.
            unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) };
        }
        let shown = Arc::new(Mutex::new(Vec::new()));
        let mut reader = std::fs::File::from(master.try_clone().unwrap());
        let sink = shown.clone();
        std::thread::spawn(move || {
            let mut buf = [0; 4096];
            // Ends with EIO once the terminal has no slave open any more.
            while let Ok(n @ 1..) = reader.read(&mut buf) {
                sink.lock().unwrap().extend_from_slice(&buf[..n]);
            }
        });
        Self {
            master,
            slave,
            shown,
        }
    }

    /// The terminal's settings now.
    fn termios(&self) -> libc::termios {
        let mut termios = std::mem::MaybeUninit::<libc::termios>::uninit();
        // SAFETY: `tcgetattr` fills `termios` when it succeeds.
        assert_eq!(
            unsafe { libc::tcgetattr(self.slave.as_raw_fd(), termios.as_mut_ptr()) },
            0
        );
        unsafe { termios.assume_init() }
    }

    fn is_raw(&self) -> bool {
        let termios = self.termios();
        termios.c_lflag & (libc::ICANON | libc::ECHO | libc::ISIG) == 0
    }

    /// Starts `wasmer run <args>` as a login shell would be: the leader of a new session
    /// whose controlling terminal this is, so its group is in the foreground.
    fn run(&self, args: &[&str]) -> Child {
        let slave = |_| Stdio::from(self.slave.try_clone().unwrap());
        let mut command = wasmer_command();
        command
            .arg("run")
            .args(args)
            .stdin(slave(0))
            .stdout(slave(1))
            .stderr(slave(2));
        // SAFETY: async-signal-safe calls between fork and exec.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        command.spawn().expect("wasmer starts")
    }

    fn type_in(&self, bytes: &[u8]) {
        // SAFETY: writes from a valid buffer.
        let n = unsafe { libc::write(self.master.as_raw_fd(), bytes.as_ptr().cast(), bytes.len()) };
        assert_eq!(n, bytes.len() as isize);
    }

    fn shown(&self) -> Vec<u8> {
        self.shown.lock().unwrap().clone()
    }

    /// Waits until the terminal shows `text`.
    fn wait_for(&self, text: &[u8]) {
        eventually(&format!("{:?}", String::from_utf8_lossy(text)), || {
            self.shown().windows(text.len()).any(|w| w == text)
        });
    }

    fn resize(&self, rows: u16) {
        let size = winsize(rows);
        // SAFETY: `TIOCSWINSZ` reads a `winsize`.
        assert_eq!(
            unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ, &size) },
            0
        );
    }
}

fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(
            start.elapsed() < Duration::from_secs(120),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The exit status of `child`, which is killed if it takes too long.
fn exit_code(child: &mut Child) -> i32 {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status.code().expect("wasmer exits");
        }
        if start.elapsed() > Duration::from_secs(120) {
            let _ = child.kill();
            panic!("wasmer did not exit");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn same(a: &libc::termios, b: &libc::termios) -> bool {
    (a.c_iflag, a.c_oflag, a.c_cflag, a.c_lflag, a.c_cc)
        == (b.c_iflag, b.c_oflag, b.c_cflag, b.c_lflag, b.c_cc)
}

fn module(dir: &TempDir, wat: &str) -> String {
    let path = dir.path().join("guest.wat");
    std::fs::write(&path, wat).unwrap();
    path.to_str().unwrap().to_string()
}

/// Checks that descriptors 0 to 2 are terminals with Linux's default settings, the window
/// size of the host's terminal, and the guest's group in the foreground; prints `tty`. Exits
/// with 0, or the number of the check that failed.
const CONSOLE_WAT: &str = r#"
  (module
    (import "wasixv2" "ioctl" (func $ioctl (param i32 i32 i32) (result i32)))
    (import "wasixv2" "getpid" (func $getpid (result i32)))
    (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
    (import "wasixv2" "exit_group" (func $exit_group (param i32)))
    (memory (export "memory") 1)
    (data (i32.const 16) "tty\n")
    (func $check (param $ok i32) (param $n i32)
      (if (i32.eqz (local.get $ok)) (then (call $exit_group (local.get $n)) unreachable)))
    (func (export "_start")
      ;; TCGETS into 256 on each descriptor.
      (call $check (i32.eqz (call $ioctl (i32.const 0) (i32.const 0x5401) (i32.const 256))) (i32.const 1))
      (call $check (i32.eqz (call $ioctl (i32.const 1) (i32.const 0x5401) (i32.const 256))) (i32.const 2))
      (call $check (i32.eqz (call $ioctl (i32.const 2) (i32.const 0x5401) (i32.const 256))) (i32.const 3))
      ;; c_iflag ICRNL|IXON, c_oflag OPOST|ONLCR, c_lflag ISIG|ICANON|ECHO among others.
      (call $check (i32.eq (i32.load (i32.const 256)) (i32.const 0x500)) (i32.const 4))
      (call $check (i32.eq (i32.load (i32.const 260)) (i32.const 0x5)) (i32.const 5))
      (call $check (i32.eq (i32.and (i32.load (i32.const 268)) (i32.const 0xb)) (i32.const 0xb)) (i32.const 6))
      ;; TIOCGPGRP into 128, TIOCGWINSZ into 136.
      (call $check (i32.eqz (call $ioctl (i32.const 0) (i32.const 0x540f) (i32.const 128))) (i32.const 7))
      (call $check (i32.eq (i32.load (i32.const 128)) (call $getpid)) (i32.const 8))
      (call $check (i32.eqz (call $ioctl (i32.const 0) (i32.const 0x5413) (i32.const 136))) (i32.const 9))
      (call $check (i32.eq (i32.load16_u (i32.const 136)) (i32.const 24)) (i32.const 10))
      (drop (call $write (i32.const 1) (i32.const 16) (i32.const 4)))
      (call $exit_group (i32.const 0))
      unreachable))
"#;

#[test]
fn a_terminal_on_stdin_is_the_guests_console() {
    let dir = TempDir::new().unwrap();
    let terminal = Terminal::open();
    let before = terminal.termios();

    let mut wasmer = terminal.run(&[&module(&dir, CONSOLE_WAT)]);

    assert_eq!(exit_code(&mut wasmer), 0);
    // The guest's line discipline turned its newline into CR LF; the host's terminal was raw.
    terminal.wait_for(b"tty\r\n");
    assert!(
        same(&terminal.termios(), &before),
        "the terminal is restored"
    );
}

/// `--no-tty` keeps the streams: no descriptor is a terminal (TCGETS fails with ENOTTY), and
/// the terminal is never put in raw mode.
const NOT_A_TTY_WAT: &str = r#"
  (module
    (import "wasixv2" "ioctl" (func $ioctl (param i32 i32 i32) (result i32)))
    (import "wasixv2" "exit_group" (func $exit_group (param i32)))
    (memory (export "memory") 1)
    (func (export "_start")
      (call $exit_group (i32.add (call $ioctl (i32.const 0) (i32.const 0x5401) (i32.const 256))
                                 (i32.const 25)))
      unreachable))
"#;

#[test]
fn no_tty_keeps_plain_streams() {
    let dir = TempDir::new().unwrap();
    let terminal = Terminal::open();
    let before = terminal.termios();

    let mut wasmer = terminal.run(&["--no-tty", &module(&dir, NOT_A_TTY_WAT)]);

    assert_eq!(exit_code(&mut wasmer), 0);
    assert!(same(&terminal.termios(), &before));
}

/// Prints `ready`, reads a line and prints it back after `got:`.
const LINE_WAT: &str = r#"
  (module
    (import "wasixv2" "read" (func $read (param i32 i32 i32) (result i32)))
    (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
    (import "wasixv2" "exit_group" (func $exit_group (param i32)))
    (memory (export "memory") 1)
    (data (i32.const 16) "ready\n")
    (data (i32.const 32) "got:")
    (func (export "_start") (local $n i32)
      (drop (call $write (i32.const 1) (i32.const 16) (i32.const 6)))
      (local.set $n (call $read (i32.const 0) (i32.const 64) (i32.const 64)))
      (drop (call $write (i32.const 1) (i32.const 32) (i32.const 4)))
      (drop (call $write (i32.const 1) (i32.const 64) (local.get $n)))
      (call $exit_group (i32.const 0))
      unreachable))
"#;

/// The guest's line discipline edits and echoes what is typed; the host's terminal only
/// passes bytes.
#[test]
fn typed_lines_are_edited_by_the_guest() {
    let dir = TempDir::new().unwrap();
    let terminal = Terminal::open();
    let before = terminal.termios();
    let mut wasmer = terminal.run(&[&module(&dir, LINE_WAT)]);
    terminal.wait_for(b"ready\r\n");
    assert!(terminal.is_raw(), "raw while the guest runs");

    terminal.type_in(b"abx\x7fc\r");

    assert_eq!(exit_code(&mut wasmer), 0);
    terminal.wait_for(b"got:abc\r\n");
    terminal.wait_for(b"abx\x08 \x08c\r\n");
    assert!(same(&terminal.termios(), &before));
}

/// Prints `ready` and waits for input forever.
const WAIT_WAT: &str = r#"
  (module
    (import "wasixv2" "read" (func $read (param i32 i32 i32) (result i32)))
    (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 16) "ready\n")
    (func (export "_start")
      (drop (call $write (i32.const 1) (i32.const 16) (i32.const 6)))
      (loop $forever
        (drop (call $read (i32.const 0) (i32.const 64) (i32.const 64)))
        (br $forever))))
"#;

/// `^C` is a byte for the guest's line discipline, which sends SIGINT to its foreground group:
/// the guest dies of it, `wasmer run` exits with 128 + 2, and the terminal is restored.
#[test]
fn ctrl_c_interrupts_the_guest() {
    let dir = TempDir::new().unwrap();
    let terminal = Terminal::open();
    let before = terminal.termios();
    let mut wasmer = terminal.run(&[&module(&dir, WAIT_WAT)]);
    terminal.wait_for(b"ready\r\n");
    assert!(
        terminal.is_raw(),
        "the host's terminal sends no SIGINT itself"
    );

    terminal.type_in(b"\x03");

    assert_eq!(exit_code(&mut wasmer), 130);
    terminal.wait_for(b"^C");
    assert!(same(&terminal.termios(), &before));
}

/// Blocks SIGWINCH, prints `ready`, waits for SIGWINCH and exits with the window's rows.
const WINCH_WAT: &str = r#"
  (module
    (import "wasixv2" "rt_sigprocmask" (func $sigprocmask (param i32 i32 i32 i32) (result i32)))
    (import "wasixv2" "rt_sigtimedwait_time64"
      (func $sigtimedwait (param i32 i32 i32 i32) (result i32)))
    (import "wasixv2" "ioctl" (func $ioctl (param i32 i32 i32) (result i32)))
    (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
    (import "wasixv2" "exit_group" (func $exit_group (param i32)))
    (memory (export "memory") 1)
    (data (i32.const 16) "ready\n")
    ;; SIGWINCH (28) at 32.
    (data (i32.const 32) "\00\00\00\08\00\00\00\00")
    (func (export "_start")
      (drop (call $sigprocmask (i32.const 0) (i32.const 32) (i32.const 0) (i32.const 8)))
      (drop (call $write (i32.const 1) (i32.const 16) (i32.const 6)))
      (drop (call $sigtimedwait (i32.const 32) (i32.const 0) (i32.const 0) (i32.const 8)))
      (drop (call $ioctl (i32.const 0) (i32.const 0x5413) (i32.const 136)))
      (call $exit_group (i32.load16_u (i32.const 136)))
      unreachable))
"#;

/// A new size of the host's terminal reaches the guest as SIGWINCH and `TIOCGWINSZ`.
#[test]
fn resizing_the_terminal_signals_sigwinch() {
    let dir = TempDir::new().unwrap();
    let terminal = Terminal::open();
    let mut wasmer = terminal.run(&[&module(&dir, WINCH_WAT)]);
    terminal.wait_for(b"ready\r\n");

    terminal.resize(40);

    assert_eq!(exit_code(&mut wasmer), 40);
}

/// Stopped (SIGTSTP from outside: the terminal sends none while raw), `wasmer` gives the
/// terminal its settings back; continued, it is raw again for the guest.
#[test]
fn a_stopped_wasmer_gives_the_terminal_back() {
    let dir = TempDir::new().unwrap();
    let terminal = Terminal::open();
    let before = terminal.termios();
    let mut wasmer = terminal.run(&[&module(&dir, WAIT_WAT)]);
    terminal.wait_for(b"ready\r\n");
    let pid = wasmer.id() as libc::pid_t;

    // SAFETY: signals and a wait for our own child.
    unsafe {
        libc::kill(pid, libc::SIGTSTP);
        let mut status = 0;
        assert_eq!(libc::waitpid(pid, &mut status, libc::WUNTRACED), pid);
        assert!(libc::WIFSTOPPED(status));
    }
    assert!(same(&terminal.termios(), &before), "stopped: restored");
    unsafe { libc::kill(pid, libc::SIGCONT) };
    eventually("raw mode again", || terminal.is_raw());

    terminal.type_in(b"\x03");
    assert_eq!(exit_code(&mut wasmer), 130);
    assert!(same(&terminal.termios(), &before));
}
