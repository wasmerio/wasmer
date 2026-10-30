//! Follow kernel/src/tests/tty.rs, using only the public kernel boundary and real browser
//! queue adapters. No wasm engine/JS or replacement line discipline is involved.
#![cfg(not(target_arch = "wasm32"))]

use futures::executor::block_on;
use std::{
    cell::RefCell,
    future::Future,
    io,
    sync::Arc,
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};
use wasixv2_abi::{
    GuestPtr, GuestPtrMut, IoctlRequest, Sigaction, Signal, Sigset, Termios, TermiosCcIndex,
    TermiosLflag, Winsize,
};
use wasixv2_host::{Executor, Host, LocalFuture, WindowSize};
use wasixv2_kernel::{Kernel, ProcessConfig};
use wasixv2_stdio_web::{Session, Streams};
use wasixv2_syscalls::{
    Caller, Control, Fault, Fd, GuestAction, SysErr, Syscalls, TaskId, UserMemory, UserSliceMut,
    read_value, write_value,
};
use wasixv2_vfs::{Clock, MountFlags, Timestamp};
use wasixv2_vfs_mem::MemFs;

struct Memory(RefCell<Vec<u8>>);
impl Default for Memory {
    fn default() -> Self {
        Self(RefCell::new(vec![0; 65536]))
    }
}
impl UserMemory for Memory {
    fn read<'a>(&'a self, addr: u32, buf: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        let result = self
            .0
            .borrow()
            .get(addr as usize..addr as usize + buf.len())
            .map(|bytes| buf.copy_from_slice(bytes))
            .ok_or(Fault);
        Box::pin(async move { result })
    }
    fn write<'a>(&'a self, addr: u32, data: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        let result = self
            .0
            .borrow_mut()
            .get_mut(addr as usize..addr as usize + data.len())
            .map(|bytes| bytes.copy_from_slice(data))
            .ok_or(Fault);
        Box::pin(async move { result })
    }
    fn size(&self) -> LocalFuture<'_, u64> {
        Box::pin(async { self.0.borrow().len() as u64 })
    }
    fn grow(&self, _: u32) -> LocalFuture<'_, Option<u64>> {
        Box::pin(async { None })
    }
    fn can_duplicate(&self) -> LocalFuture<'_, bool> {
        Box::pin(async { true })
    }
}
struct TestClock;
impl Clock for TestClock {
    fn now(&self) -> Timestamp {
        Timestamp::default()
    }
}
struct ThreadHost;
impl Host for ThreadHost {
    fn realtime(&self) -> Duration {
        Duration::from_secs(1_700_000_000)
    }
    fn monotonic(&self) -> Duration {
        Duration::ZERO
    }
    fn sleep(&self, _: Duration) -> LocalFuture<'_, ()> {
        Box::pin(std::future::pending())
    }
    fn random(&self, buf: &mut [u8]) {
        buf.fill(0x5a);
    }
    fn executor(&self) -> &dyn Executor {
        self
    }
}
impl Executor for ThreadHost {
    fn spawn_local(&self, _: LocalFuture<'static, ()>) {
        panic!("TTY pump must have its own worker");
    }
    fn spawn_blocking(&self, work: Box<dyn FnOnce() + Send>) -> LocalFuture<'static, ()> {
        Box::pin(async move { work() })
    }
    fn spawn_thread(&self, start: wasixv2_host::ThreadStart) -> io::Result<()> {
        std::thread::Builder::new().spawn(move || block_on(start()))?;
        Ok(())
    }
}
struct Terminal {
    kernel: Kernel,
    task: TaskId,
    session: Session,
    streams: Streams,
}
impl Terminal {
    fn new() -> Self {
        let (session, stdio, streams) = Session::new(4096).unwrap();
        let size = WindowSize {
            rows: 24,
            cols: 80,
            ..WindowSize::default()
        };
        let root = MemFs::new(Arc::new(TestClock), 0o755, 1, 1);
        let kernel = Kernel::new(Arc::new(ThreadHost), Arc::new(root))
            .with_init()
            .with_console(session.console(size));
        let dev = MemFs::new(Arc::new(TestClock), 0o755, 0, 0);
        block_on(
            kernel
                .vfs()
                .mount(b"/dev", Arc::new(dev), MountFlags::default(), true),
        )
        .unwrap();
        block_on(kernel.create_device_nodes(b"/dev")).unwrap();
        let task = kernel
            .spawn(ProcessConfig {
                args: vec!["test".into()],
                env: vec![],
                stdio,
            })
            .unwrap();
        block_on(kernel.attach_console(task, [true, true, false])).unwrap();
        Self {
            kernel,
            task,
            session,
            streams,
        }
    }
    fn handle(&self, caller: &Caller<'_>, sig: Signal) {
        // kernel/src/tests/signal.rs: same public handler/trampoline registration.
        let action = Sigaction {
            sa_handler: 7,
            sa_flags: 0,
            sa_mask: Sigset { sig: [0; 2] },
        };
        block_on(write_value(
            caller.memory(),
            GuestPtrMut::new(0x100),
            &action,
        ))
        .unwrap();
        assert_eq!(
            block_on(self.kernel.signal_callback_register(caller, 3)),
            Ok(0)
        );
        assert_eq!(
            block_on(self.kernel.rt_sigaction(
                caller,
                sig,
                GuestPtr::new(0x100),
                GuestPtrMut::NULL,
                8
            )),
            Ok(0)
        );
        assert_eq!(self.kernel.take_pending(caller.task()), None);
    }
    fn termios(&self, caller: &Caller<'_>) -> Termios {
        assert_eq!(
            block_on(self.kernel.ioctl(caller, Fd(0), IoctlRequest::TCGETS, 0x40)),
            Ok(0)
        );
        block_on(read_value(caller.memory(), GuestPtr::new(0x40))).unwrap()
    }
    fn set_termios(&self, caller: &Caller<'_>, termios: Termios) {
        block_on(write_value(
            caller.memory(),
            GuestPtrMut::new(0x40),
            &termios,
        ))
        .unwrap();
        assert_eq!(
            block_on(self.kernel.ioctl(caller, Fd(0), IoctlRequest::TCSETS, 0x40)),
            Ok(0)
        );
    }
    fn read(&self, caller: &Caller<'_>) -> Vec<u8> {
        let n = bounded(
            self.kernel
                .read(caller, Fd(0), UserSliceMut::new(0x200, 32)),
        )
        .unwrap();
        let mut bytes = vec![0; n as usize];
        block_on(caller.memory().read(0x200, &mut bytes)).unwrap();
        bytes
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        self.session.abort();
        self.kernel.console_hangup();
    }
}

fn bounded<T>(future: impl Future<Output = T>) -> T {
    let mut future = std::pin::pin!(future);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Poll::Ready(value) = future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "TTY worker did not make progress"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn eventually(mut predicate: impl FnMut() -> bool) {
    bounded(std::future::poll_fn(|_| {
        if predicate() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }));
}

#[test]
fn kernel_canonical_editing_echo_and_ctrl_d_are_not_transport_eof() {
    let tty = Terminal::new();
    let memory = Memory::default();
    let caller = Caller::new(tty.task, &memory);
    tty.streams.stdin.try_write(b"ab\x7fc\r").unwrap();
    assert_eq!(tty.read(&caller), b"ac\n");
    let mut screen = Vec::new();
    eventually(|| {
        let mut bytes = [0; 64];
        if let Ok(n) = tty.streams.stdout.try_read(&mut bytes) {
            screen.extend_from_slice(&bytes[..n]);
        }
        screen.ends_with(b"c\r\n")
    });
    assert_eq!(screen, b"ab\x08 \x08c\r\n");
    tty.streams.stdin.try_write(b"\x04").unwrap();
    assert!(tty.read(&caller).is_empty());
    tty.streams.stdin.try_write(b"next\n").unwrap();
    assert_eq!(tty.read(&caller), b"next\n");
}

#[test]
fn raw_termios_delivers_control_bytes_without_echo_or_signals() {
    let tty = Terminal::new();
    let memory = Memory::default();
    let caller = Caller::new(tty.task, &memory);
    let mut termios = tty.termios(&caller);
    termios.c_lflag.0 &= !(TermiosLflag::ICANON.0 | TermiosLflag::ECHO.0 | TermiosLflag::ISIG.0);
    termios.c_cc[TermiosCcIndex::VMIN.0 as usize] = 1;
    tty.set_termios(&caller, termios);
    tty.streams.stdin.try_write(b"\x03\x04\x7f").unwrap();
    assert_eq!(tty.read(&caller), b"\x03\x04\x7f");
    assert_eq!(
        tty.streams.stdout.try_read(&mut [0]).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    assert_eq!(tty.kernel.take_pending(tty.task), None);
}

#[test]
fn ctrl_c_reaches_busy_task_through_work_flag_without_a_guest_read() {
    let tty = Terminal::new();
    let memory = Memory::default();
    let caller = Caller::new(tty.task, &memory);
    tty.handle(&caller, Signal::SIGINT);
    let flag = tty.kernel.work_flag(tty.task);
    assert!(!flag.is_raised());
    tty.streams.stdin.try_write(b"\x03").unwrap();
    // Model the exec preemption hook of a busy guest. Only the independent TTY pump
    // reads input; the task does not call read/poll. Linux kernel/signal.c:complete_signal.
    eventually(|| flag.is_raised());
    assert!(
        matches!(tty.kernel.take_pending(tty.task), Some(GuestAction::Signal(frame)) if frame.signo() == Signal::SIGINT.0)
    );
}

#[test]
fn resize_uses_existing_sigwinch_path_and_preserves_all_winsize_fields() {
    let tty = Terminal::new();
    let memory = Memory::default();
    let caller = Caller::new(tty.task, &memory);
    tty.handle(&caller, Signal::SIGWINCH);
    let size = WindowSize {
        rows: 50,
        cols: 120,
        xpixel: 800,
        ypixel: 600,
    };
    tty.kernel.console_resize(size);
    assert!(
        matches!(tty.kernel.take_pending(tty.task), Some(GuestAction::Signal(frame)) if frame.signo() == Signal::SIGWINCH.0)
    );
    tty.kernel.signal_return(tty.task, Some(0));
    tty.kernel.console_resize(size);
    assert_eq!(tty.kernel.take_pending(tty.task), None);
    assert_eq!(
        block_on(
            tty.kernel
                .ioctl(&caller, Fd(0), IoctlRequest::TIOCGWINSZ, 0x20)
        ),
        Ok(0)
    );
    let got: Winsize = block_on(read_value(&memory, GuestPtr::new(0x20))).unwrap();
    assert_eq!(
        (got.ws_row, got.ws_col, got.ws_xpixel, got.ws_ypixel),
        (50, 120, 800, 600)
    );
}

#[test]
fn foreground_job_control_selects_ctrl_c_target() {
    let tty = Terminal::new();
    let memory = Memory::default();
    let caller = Caller::new(tty.task, &memory);
    tty.handle(&caller, Signal::SIGINT);
    let child = TaskId(
        block_on(tty.kernel.clone(
            &caller,
            Signal::SIGCHLD.0 as u32,
            0,
            GuestPtrMut::NULL,
            0,
            GuestPtrMut::NULL,
        ))
        .unwrap() as u32,
    );
    assert!(matches!(
        tty.kernel.take_pending(tty.task),
        Some(GuestAction::Fork { .. })
    ));
    let child_memory = Memory::default();
    let child_caller = Caller::new(child, &child_memory);
    assert_eq!(
        block_on(tty.kernel.setpgid(&caller, child.0 as i32, child.0 as i32)),
        Ok(0)
    );
    assert_eq!(
        bounded(
            tty.kernel
                .read(&child_caller, Fd(0), UserSliceMut::new(0x200, 16))
        ),
        Err(SysErr::Restart)
    );
    assert_eq!(tty.kernel.take_pending(child), Some(GuestAction::Stopped));
    // Continue, then move job to foreground using the public terminal ioctl.
    assert_eq!(
        block_on(Syscalls::kill(
            &tty.kernel,
            &caller,
            child.0 as i32,
            Signal::SIGCONT
        )),
        Ok(0)
    );
    while tty.kernel.take_pending(child).is_some() {}
    block_on(write_value(
        &memory,
        GuestPtrMut::new(0x10),
        &(child.0 as i32),
    ))
    .unwrap();
    assert_eq!(
        block_on(
            tty.kernel
                .ioctl(&caller, Fd(0), IoctlRequest::TIOCSPGRP, 0x10)
        ),
        Ok(0)
    );
    tty.streams.stdin.try_write(b"\x03").unwrap();
    let flag = tty.kernel.work_flag(child);
    eventually(|| flag.is_raised());
    assert!(
        matches!(tty.kernel.take_pending(child), Some(GuestAction::Signal(frame)) if frame.signo() == Signal::SIGINT.0)
    );
    assert_eq!(tty.kernel.take_pending(tty.task), None);
}

#[test]
fn stdin_close_hangs_up_console_and_completes_a_blocked_read() {
    let tty = Terminal::new();
    let memory = Memory::default();
    let caller = Caller::new(tty.task, &memory);
    tty.handle(&caller, Signal::SIGHUP);
    tty.streams.stdin.close();
    eventually(|| tty.kernel.work_flag(tty.task).is_raised());
    assert!(
        matches!(tty.kernel.take_pending(tty.task), Some(GuestAction::Signal(frame)) if frame.signo() == Signal::SIGHUP.0)
    );
    tty.kernel.signal_return(tty.task, Some(0));
    assert!(tty.read(&caller).is_empty());
}
