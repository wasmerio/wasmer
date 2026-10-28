//! Terminals: the host's console as a process's controlling terminal, its line discipline as
//! the host types, and job control against its foreground process group.

use std::{
    collections::VecDeque,
    task::{Context, Poll, Waker},
    time::Instant,
};

use futures::FutureExt as _;
use wasixv2_abi::{
    IoctlRequest, OpenFlags, Pid, Signal, Termios, TermiosCcIndex, TermiosLflag, Winsize,
};
use wasixv2_host::WindowSize;
use wasixv2_syscalls::{read_value, write_value};
use wasixv2_vfs::MountFlags;

use super::{
    ipc::{errno, get, put},
    mapping::open,
    proc::ForkableMemory,
    signal::ignore,
    *,
};

/// What the host types: input arrives as the test types it, and its end is a hangup.
#[derive(Default)]
struct Keyboard(Mutex<Keys>);

#[derive(Default)]
struct Keys {
    typed: VecDeque<u8>,
    gone: bool,
    waker: Option<Waker>,
}

impl Keyboard {
    fn type_in(&self, bytes: &[u8]) {
        let mut keys = self.0.lock().unwrap();
        keys.typed.extend(bytes);
        if let Some(waker) = keys.waker.take() {
            waker.wake();
        }
    }

    fn unplug(&self) {
        let mut keys = self.0.lock().unwrap();
        keys.gone = true;
        if let Some(waker) = keys.waker.take() {
            waker.wake();
        }
    }
}

impl ByteSource for Keyboard {
    fn poll_read(&self, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        let mut keys = self.0.lock().unwrap();
        if !keys.typed.is_empty() {
            let n = buf.len().min(keys.typed.len());
            for (dst, src) in buf.iter_mut().zip(keys.typed.drain(..n)) {
                *dst = src;
            }
            return Poll::Ready(Ok(n));
        }
        if keys.gone {
            return Poll::Ready(Ok(0));
        }
        keys.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

/// [`TestHost`] with threads, where the console's input pump runs.
struct ThreadHost;

impl Host for ThreadHost {
    fn realtime(&self) -> Duration {
        TestHost.realtime()
    }

    fn monotonic(&self) -> Duration {
        TestHost.monotonic()
    }

    fn sleep(&self, d: Duration) -> LocalFuture<'_, ()> {
        TestHost.sleep(d)
    }

    fn sleep_until(&self, d: Duration) -> LocalFuture<'_, ()> {
        TestHost.sleep_until(d)
    }

    fn random(&self, buf: &mut [u8]) {
        TestHost.random(buf)
    }

    fn executor(&self) -> &dyn Executor {
        self
    }
}

impl Executor for ThreadHost {
    fn spawn_local(&self, _: LocalFuture<'static, ()>) {
        unimplemented!("the console's pump runs on a thread")
    }

    fn spawn_blocking(&self, work: Box<dyn FnOnce() + Send>) -> LocalFuture<'static, ()> {
        Box::pin(async move { work() })
    }

    fn spawn_thread(&self, start: wasixv2_host::ThreadStart) -> io::Result<()> {
        std::thread::spawn(move || block_on(start()));
        Ok(())
    }
}

/// A machine whose console the test types on and reads, and its first process, which has
/// the console as descriptors 0, 1 and 2 and its controlling terminal.
struct Console {
    kernel: Kernel,
    leader: TaskId,
    keyboard: Arc<Keyboard>,
    screen: Arc<Capture>,
}

const ROWS: u16 = 24;
const COLS: u16 = 80;

fn console() -> Console {
    let keyboard = Arc::new(Keyboard::default());
    let screen = Arc::new(Capture::default());
    let host: Arc<dyn Host> = Arc::new(ThreadHost);
    let clock = Arc::new(HostClock(host.clone()));
    let root = MemFs::new(clock.clone(), 0o755, 1, 1);
    let shm = MemFs::new(clock.clone(), 0o1777, 0, 0);
    let console = wasixv2_host::Console {
        input: keyboard.clone(),
        output: screen.clone(),
        size: WindowSize {
            rows: ROWS,
            cols: COLS,
            xpixel: 0,
            ypixel: 0,
        },
    };
    let kernel = Kernel::new(host, Arc::new(root))
        .with_shm(Arc::new(shm))
        .with_init()
        .with_console(console);
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(b"/proc", kernel.procfs(), MountFlags::default(), true)).unwrap();
    let dev = MemFs::new(clock, 0o755, 0, 0);
    block_on(vfs.mount(b"/dev", Arc::new(dev), MountFlags::default(), true)).unwrap();
    block_on(kernel.create_device_nodes(b"/dev")).unwrap();
    let stdin = Arc::new(Input(Mutex::new(Vec::new())));
    let (kernel, leader, _) = spawn_on(kernel, stdin, &[], &[]);
    block_on(kernel.attach_console(leader, [true; 3])).unwrap();
    Console {
        kernel,
        leader,
        keyboard,
        screen,
    }
}

fn memory() -> ForkableMemory {
    ForkableMemory(FlatMemory(RefCell::new(vec![0; 65536])))
}

impl Console {
    fn shown(&self) -> Vec<u8> {
        self.screen.0.lock().unwrap().clone()
    }

    fn fork(&self, caller: &Caller<'_>) -> TaskId {
        let flags = Signal::SIGCHLD.0 as u32;
        let clone = self
            .kernel
            .clone(caller, flags, 0, GuestPtrMut::NULL, 0, GuestPtrMut::NULL);
        let child = TaskId(block_on(clone).expect("fork") as u32);
        while self.kernel.take_pending(caller.task()).is_some() {}
        child
    }

    /// A child of the leader in a process group of its own: a background job.
    fn background_job(&self, caller: &Caller<'_>) -> TaskId {
        let child = self.fork(caller);
        let pid = child.0 as Pid;
        assert_eq!(block_on(self.kernel.setpgid(caller, pid, pid)), Ok(0));
        child
    }

    fn ioctl(
        &self,
        caller: &Caller<'_>,
        fd: i32,
        request: IoctlRequest,
        arg: u32,
    ) -> SysResult<i32> {
        block_on(self.kernel.ioctl(caller, Fd(fd), request, arg))
    }

    /// An `int` result of `request` on descriptor 0, through memory at 0x10.
    fn ioctl_int(&self, caller: &Caller<'_>, request: IoctlRequest) -> SysResult<i32> {
        self.ioctl(caller, 0, request, 0x10)?;
        Ok(block_on(read_value(caller.memory(), GuestPtr::new(0x10))).unwrap())
    }

    fn tcgetattr(&self, caller: &Caller<'_>) -> Termios {
        self.ioctl(caller, 0, IoctlRequest::TCGETS, 0x40).unwrap();
        block_on(read_value(caller.memory(), GuestPtr::new(0x40))).unwrap()
    }

    fn tcsetattr(&self, caller: &Caller<'_>, termios: &Termios) -> SysResult<i32> {
        block_on(write_value(
            caller.memory(),
            GuestPtrMut::new(0x40),
            termios,
        ))
        .unwrap();
        self.ioctl(caller, 0, IoctlRequest::TCSETS, 0x40)
    }

    fn tcsetpgrp(&self, caller: &Caller<'_>, pgrp: Pid) -> SysResult<i32> {
        block_on(write_value(caller.memory(), GuestPtrMut::new(0x10), &pgrp)).unwrap();
        self.ioctl(caller, 0, IoctlRequest::TIOCSPGRP, 0x10)
    }

    fn read(&self, caller: &Caller<'_>, len: u32) -> SysResult<Vec<u8>> {
        let n = block_on(
            self.kernel
                .read(caller, Fd(0), UserSliceMut::new(0x200, len)),
        )?;
        Ok(block_on_read(caller, n))
    }

    fn write(&self, caller: &Caller<'_>, fd: i32, data: &[u8]) -> SysResult<u32> {
        block_on(caller.memory().write(0x200, data)).unwrap();
        let len = data.len() as u32;
        block_on(
            self.kernel
                .write(caller, Fd(fd), UserSlice::new(0x200, len)),
        )
    }

    fn pending(&self, task: TaskId, sig: Signal) -> bool {
        self.kernel.task_signals(task).group().is_pending(sig.0)
    }
}

fn block_on_read(caller: &Caller<'_>, n: u32) -> Vec<u8> {
    let mut buf = vec![0; n as usize];
    block_on(caller.memory().read(0x200, &mut buf)).unwrap();
    buf
}

/// Waits for what the console's pump does on its thread.
fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < Duration::from_secs(10), "never: {what}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn blocked(sig: Signal) -> u64 {
    1 << (sig.0 - 1)
}

/// The first process leads a session whose controlling terminal is the console, its group in
/// the foreground, and descriptors 0 to 2 are one read-write open file of it.
#[test]
fn the_console_is_the_first_process_controlling_terminal() {
    let c = console();
    let memory = memory();
    let caller = Caller::new(c.leader, &memory);
    let pid = c.leader.0 as i32;

    assert_eq!(c.ioctl_int(&caller, IoctlRequest::TIOCGPGRP), Ok(pid));
    assert_eq!(c.ioctl_int(&caller, IoctlRequest::TIOCGSID), Ok(pid));
    assert_eq!(c.tcgetattr(&caller), crate::tty::ldisc::std_termios());
    let winsize: Winsize = {
        c.ioctl(&caller, 1, IoctlRequest::TIOCGWINSZ, 0x20).unwrap();
        block_on(read_value(caller.memory(), GuestPtr::new(0x20))).unwrap()
    };
    assert_eq!((winsize.ws_row, winsize.ws_col), (ROWS, COLS));
    // `TIOCGDEV`: hvc0, 229:0.
    assert_eq!(c.ioctl_int(&caller, IoctlRequest::TIOCGDEV), Ok(229 << 8));
    let files = c.kernel.files(c.leader);
    let files = files.lock().unwrap();
    let file = files.get(Fd(0)).unwrap();
    assert!(Arc::ptr_eq(&file, &files.get(Fd(2)).unwrap()));
    assert_eq!(file.flags() & OpenFlags::O_ACCMODE, OpenFlags::O_RDWR);
}

/// Output processing turns each newline into CR LF, as a terminal needs it.
#[test]
fn writes_reach_the_host_post_processed() {
    let c = console();
    let memory = memory();
    let caller = Caller::new(c.leader, &memory);
    assert_eq!(c.write(&caller, 1, b"one\ntwo"), Ok(7));
    assert_eq!(c.shown(), b"one\r\ntwo");
}

/// What the host types is edited and echoed by the console, and a read gets the line.
#[test]
fn typed_lines_are_edited_echoed_and_read() {
    let c = console();
    let memory = memory();
    let caller = Caller::new(c.leader, &memory);
    c.keyboard.type_in(b"lsx\x7f -l\r");
    assert_eq!(c.read(&caller, 64), Ok(b"ls -l\n".to_vec()));
    eventually("the echo", || c.shown().ends_with(b"\r\n"));
    assert_eq!(c.shown(), b"lsx\x08 \x08 -l\r\n");
}

/// `^C` is SIGINT for the foreground process group only: a background job does not get it,
/// until it is moved to the foreground.
#[test]
fn ctrl_c_interrupts_the_foreground_group_only() {
    let c = console();
    let memory = memory();
    let caller = Caller::new(c.leader, &memory);
    c.kernel
        .task_signals(c.leader)
        .replace_blocked(blocked(Signal::SIGINT));
    let job = c.background_job(&caller);
    c.kernel
        .task_signals(job)
        .replace_blocked(blocked(Signal::SIGINT));

    c.keyboard.type_in(b"\x03");
    eventually("SIGINT", || c.pending(c.leader, Signal::SIGINT));
    assert!(!c.pending(job, Signal::SIGINT));
    eventually("the echo", || c.shown() == b"^C");

    assert_eq!(c.tcsetpgrp(&caller, job.0 as Pid), Ok(0));
    c.keyboard.type_in(b"\x03");
    eventually("the job's SIGINT", || c.pending(job, Signal::SIGINT));
}

/// A background job that reads its controlling terminal stops with SIGTTIN and reads again
/// once continued; with SIGTTIN ignored, the read fails with `EIO`.
#[test]
fn a_background_read_stops_with_sigttin() {
    let c = console();
    let memory = memory();
    let caller = Caller::new(c.leader, &memory);
    let job = c.background_job(&caller);
    let job_memory = self::memory();
    let job_caller = Caller::new(job, &job_memory);

    assert_eq!(c.read(&job_caller, 16), Err(SysErr::Restart));
    assert_eq!(c.kernel.take_pending(job), Some(GuestAction::Stopped));

    ignore(&c.kernel, &job_caller, Signal::SIGTTIN);
    assert_eq!(errno(c.read(&job_caller, 16)), Errno::EIO);
}

/// Changing the terminal from the background stops with SIGTTOU, unless SIGTTOU is ignored or
/// blocked; writing does only under `TOSTOP`.
#[test]
fn background_changes_and_tostop_writes_stop_with_sigttou() {
    let c = console();
    let memory = memory();
    let caller = Caller::new(c.leader, &memory);
    let job = c.background_job(&caller);
    let job_memory = self::memory();
    let job_caller = Caller::new(job, &job_memory);
    let termios = c.tcgetattr(&job_caller);

    assert_eq!(c.tcsetattr(&job_caller, &termios), Err(SysErr::Restart));
    assert_eq!(c.kernel.take_pending(job), Some(GuestAction::Stopped));
    c.kernel.kill(job, Signal::SIGCONT).unwrap();
    assert_eq!(c.kernel.take_pending(job), None);

    assert_eq!(c.write(&job_caller, 1, b"bg"), Ok(2));
    let mut tostop = termios;
    tostop.c_lflag.0 |= TermiosLflag::TOSTOP.0;
    assert_eq!(c.tcsetattr(&caller, &tostop), Ok(0));
    assert_eq!(c.write(&job_caller, 1, b"bg"), Err(SysErr::Restart));
    assert_eq!(c.kernel.take_pending(job), Some(GuestAction::Stopped));
    c.kernel.kill(job, Signal::SIGCONT).unwrap();
    assert_eq!(c.kernel.take_pending(job), None);

    c.kernel
        .task_signals(job)
        .replace_blocked(blocked(Signal::SIGTTOU));
    assert_eq!(c.write(&job_caller, 1, b"bg"), Ok(2));
    assert_eq!(c.tcsetattr(&job_caller, &termios), Ok(0));
}

/// A process group with no parent in another group of its session (the first process's own)
/// cannot be stopped from the terminal: its background access fails with `EIO` instead.
#[test]
fn an_orphaned_background_group_gets_eio() {
    let c = console();
    let memory = memory();
    let caller = Caller::new(c.leader, &memory);
    let job = c.background_job(&caller);
    assert_eq!(c.tcsetpgrp(&caller, job.0 as Pid), Ok(0));

    // The leader's own group, now in the background, is orphaned: init is its parent.
    assert_eq!(errno(c.read(&caller, 16)), Errno::EIO);
    assert_eq!(c.kernel.take_pending(c.leader), None);
}

/// `tcsetpgrp` takes a group of the caller's session, on the caller's controlling terminal.
#[test]
fn tcsetpgrp_checks_the_group_and_the_terminal() {
    let c = console();
    let memory = memory();
    let caller = Caller::new(c.leader, &memory);
    assert_eq!(errno(c.tcsetpgrp(&caller, -1)), Errno::EINVAL);
    assert_eq!(errno(c.tcsetpgrp(&caller, 9999)), Errno::ESRCH);
    // init's group is in another session.
    assert_eq!(errno(c.tcsetpgrp(&caller, 1)), Errno::EPERM);

    let child = c.fork(&caller);
    let child_memory = self::memory();
    let child_caller = Caller::new(child, &child_memory);
    assert_eq!(block_on(c.kernel.setsid(&child_caller)), Ok(child.0 as Pid));
    assert_eq!(
        errno(c.tcsetpgrp(&child_caller, child.0 as Pid)),
        Errno::ENOTTY
    );
    assert_eq!(
        errno(c.ioctl_int(&child_caller, IoctlRequest::TIOCGPGRP)),
        Errno::ENOTTY
    );
}

/// A new window size reaches the foreground group as SIGWINCH, from the host and from
/// `TIOCSWINSZ`; the same size again changes nothing.
#[test]
fn resizing_signals_sigwinch() {
    let c = console();
    let memory = memory();
    let caller = Caller::new(c.leader, &memory);
    c.kernel
        .task_signals(c.leader)
        .replace_blocked(blocked(Signal::SIGWINCH));
    let size = WindowSize {
        rows: ROWS,
        cols: COLS,
        xpixel: 0,
        ypixel: 0,
    };
    c.kernel.console_resize(size);
    assert!(!c.pending(c.leader, Signal::SIGWINCH));
    c.kernel.console_resize(WindowSize { rows: 50, ..size });
    assert!(c.pending(c.leader, Signal::SIGWINCH));
    // Unblocked, its default action drops it.
    let signals = c.kernel.task_signals(c.leader);
    signals.replace_blocked(0);
    assert_eq!(c.kernel.take_pending(c.leader), None);
    assert!(!c.pending(c.leader, Signal::SIGWINCH));
    signals.replace_blocked(blocked(Signal::SIGWINCH));

    let winsize = Winsize {
        ws_row: 10,
        ws_col: 20,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    block_on(write_value(
        caller.memory(),
        GuestPtrMut::new(0x20),
        &winsize,
    ))
    .unwrap();
    assert_eq!(c.ioctl(&caller, 0, IoctlRequest::TIOCSWINSZ, 0x20), Ok(0));
    assert!(c.pending(c.leader, Signal::SIGWINCH));
}

/// `/dev/tty` is the caller's controlling terminal; a new session has none.
#[test]
fn dev_tty_is_the_controlling_terminal() {
    let c = console();
    let memory = memory();
    let caller = Caller::new(c.leader, &memory);
    let fd = open(&c.kernel, &caller, b"/dev/tty", OpenFlags::O_RDWR);
    assert_eq!(c.ioctl(&caller, fd.0, IoctlRequest::TCGETS, 0x40), Ok(0));

    let child = c.fork(&caller);
    let child_memory = self::memory();
    let child_caller = Caller::new(child, &child_memory);
    let fd = open(&c.kernel, &child_caller, b"/dev/tty", OpenFlags::O_RDONLY);
    assert_eq!(
        c.ioctl(&child_caller, fd.0, IoctlRequest::TCGETS, 0x40),
        Ok(0)
    );
    assert_eq!(block_on(c.kernel.setsid(&child_caller)), Ok(child.0 as Pid));
    put(&child_memory.0, 0x800, b"/dev/tty\0");
    let reopen = c.kernel.openat(
        &child_caller,
        Fd(-100),
        GuestPtr::new(0x800),
        OpenFlags::O_RDWR,
        0,
    );
    assert_eq!(errno(block_on(reopen)), Errno::ENXIO);
}

/// A session leader acquires a terminal it opens that no session controls, unless it opens it
/// with `O_NOCTTY`; `TIOCSCTTY` takes one, `TIOCNOTTY` gives it back.
#[test]
fn a_session_leader_acquires_a_free_terminal() {
    let c = console();
    let memory = memory();
    let caller = Caller::new(c.leader, &memory);
    // The leader gives the console up; its group gets SIGHUP and SIGCONT.
    c.kernel
        .task_signals(c.leader)
        .replace_blocked(blocked(Signal::SIGHUP));
    assert_eq!(c.ioctl(&caller, 0, IoctlRequest::TIOCNOTTY, 0), Ok(0));
    assert!(c.pending(c.leader, Signal::SIGHUP));
    assert_eq!(
        errno(c.ioctl_int(&caller, IoctlRequest::TIOCGPGRP)),
        Errno::ENOTTY
    );

    let child = c.fork(&caller);
    let child_memory = self::memory();
    let child_caller = Caller::new(child, &child_memory);
    assert_eq!(block_on(c.kernel.setsid(&child_caller)), Ok(child.0 as Pid));
    let noctty = OpenFlags::O_RDWR | OpenFlags::O_NOCTTY;
    let fd = open(&c.kernel, &child_caller, b"/dev/hvc0", noctty);
    assert_eq!(
        errno(c.ioctl(&child_caller, fd.0, IoctlRequest::TIOCGPGRP, 0x10)),
        Errno::ENOTTY
    );
    assert_eq!(
        c.ioctl(&child_caller, fd.0, IoctlRequest::TIOCSCTTY, 0),
        Ok(0)
    );
    assert_eq!(
        c.ioctl(&child_caller, fd.0, IoctlRequest::TIOCGPGRP, 0x10),
        Ok(0)
    );
    let pgrp: i32 = block_on(read_value(&child_memory, GuestPtr::new(0x10))).unwrap();
    assert_eq!(pgrp, child.0 as i32);

    // The first leader cannot take it back from the child's session without stealing.
    let fd = open(&c.kernel, &caller, b"/dev/hvc0", OpenFlags::O_RDWR);
    assert_eq!(
        errno(c.ioctl(&caller, fd.0, IoctlRequest::TIOCSCTTY, 0)),
        Errno::EPERM
    );
}

/// The session leader's exit hangs the console up: its foreground group gets SIGHUP, and the
/// files open on it end (reads at end of file, writes with `EIO`). Opening it again works.
#[test]
fn the_session_leaders_exit_hangs_up_the_console() {
    let c = console();
    let memory = memory();
    let caller = Caller::new(c.leader, &memory);
    let child = c.fork(&caller);
    let child_memory = self::memory();
    let child_caller = Caller::new(child, &child_memory);
    c.kernel
        .task_signals(child)
        .replace_blocked(blocked(Signal::SIGHUP));

    assert_eq!(block_on(c.kernel.exit_group(&caller, 0)), Ok(()));

    assert!(c.pending(child, Signal::SIGHUP));
    assert_eq!(c.read(&child_caller, 16), Ok(Vec::new()));
    assert_eq!(errno(c.write(&child_caller, 1, b"x")), Errno::EIO);
    assert_eq!(
        errno(c.ioctl(&child_caller, 0, IoctlRequest::TCGETS, 0x40)),
        Errno::EIO
    );
    let fd = open(&c.kernel, &child_caller, b"/dev/hvc0", OpenFlags::O_RDWR);
    assert_eq!(c.write(&child_caller, fd.0, b"x"), Ok(1));
}

/// The host's input ending is the line dropping: the session's leader gets SIGHUP.
#[test]
fn the_end_of_the_hosts_input_is_a_hangup() {
    let c = console();
    let memory = memory();
    let caller = Caller::new(c.leader, &memory);
    c.kernel
        .task_signals(c.leader)
        .replace_blocked(blocked(Signal::SIGHUP));
    // A read waits for input, and ends with the hangup.
    let mut read = Box::pin(c.kernel.read(&caller, Fd(0), UserSliceMut::new(0x200, 16)));
    assert_eq!(read.as_mut().now_or_never(), None);

    c.keyboard.unplug();

    eventually("SIGHUP", || c.pending(c.leader, Signal::SIGHUP));
    assert_eq!(block_on(read), Ok(0));
}

/// `TIOCSTI` pushes input as if typed, for the terminal's own session.
#[test]
fn tiocsti_fakes_input() {
    let c = console();
    let memory = memory();
    let caller = Caller::new(c.leader, &memory);
    for &byte in b"hi\n" {
        put(&memory.0, 0x30, &[byte]);
        assert_eq!(c.ioctl(&caller, 0, IoctlRequest::TIOCSTI, 0x30), Ok(0));
    }
    assert_eq!(c.ioctl_int(&caller, IoctlRequest::TIOCINQ), Ok(3));
    assert_eq!(c.read(&caller, 16), Ok(b"hi\n".to_vec()));
}

/// Raw mode: `VMIN` 0 and `VTIME` 0 make a read return at once, with what there is.
#[test]
fn a_raw_read_with_vmin_zero_returns_at_once() {
    let c = console();
    let memory = memory();
    let caller = Caller::new(c.leader, &memory);
    let mut raw = c.tcgetattr(&caller);
    raw.c_lflag.0 &= !(TermiosLflag::ICANON.0 | TermiosLflag::ECHO.0);
    raw.c_cc[TermiosCcIndex::VMIN.0 as usize] = 0;
    assert_eq!(c.tcsetattr(&caller, &raw), Ok(0));
    assert_eq!(c.read(&caller, 16), Ok(Vec::new()));
    c.keyboard.type_in(b"q");
    eventually("the input", || {
        c.ioctl_int(&caller, IoctlRequest::FIONREAD) == Ok(1)
    });
    assert_eq!(c.read(&caller, 16), Ok(b"q".to_vec()));
    assert!(c.shown().is_empty(), "no echo");
}

/// `/proc/<pid>/stat` names the controlling terminal and its foreground group.
#[test]
fn proc_stat_shows_the_controlling_terminal() {
    let c = console();
    let memory = memory();
    let caller = Caller::new(c.leader, &memory);
    let fd = open(&c.kernel, &caller, b"/proc/self/stat", OpenFlags::O_RDONLY);
    let n = block_on(c.kernel.read(&caller, fd, UserSliceMut::new(0x400, 512))).unwrap();
    let stat = String::from_utf8(get(&memory.0, 0x400, n as usize)).unwrap();
    let fields: Vec<&str> = stat.rsplit_once(") ").unwrap().1.split(' ').collect();
    // state, ppid, pgrp, session, tty_nr, tpgid
    let pid = c.leader.0.to_string();
    assert_eq!(fields[4], (229 << 8).to_string());
    assert_eq!(fields[5], pid);
}

/// `^S` stops output: a write waits, and `^Q`, echoed or not, lets it go on.
#[test]
fn ctrl_s_holds_writes_until_ctrl_q() {
    let c = console();
    let memory = memory();
    let caller = Caller::new(c.leader, &memory);
    let file = c.kernel.file(c.leader, Fd(1)).unwrap();
    c.keyboard.type_in(b"\x13");
    eventually("output stopped", || {
        !wasixv2_wait::Pollable::poll(&*file, None).contains(wasixv2_wait::PollMask::WRITABLE)
    });
    block_on(memory.write(0x200, b"held")).unwrap();
    let mut write = Box::pin(c.kernel.write(&caller, Fd(1), UserSlice::new(0x200, 4)));
    assert_eq!(write.as_mut().now_or_never(), None);

    // An echo while output is stopped waits too, and must not keep `^Q` from coming through.
    c.keyboard.type_in(b"x");
    std::thread::sleep(Duration::from_millis(20));
    c.keyboard.type_in(b"\x11");

    let mut written = None;
    eventually("the write", || {
        written = write.as_mut().now_or_never();
        written.is_some()
    });
    assert_eq!(written, Some(Ok(4)));
    eventually("the echo", || c.shown().len() == 5);
    assert!(c.shown() == b"xheld" || c.shown() == b"heldx");
}
