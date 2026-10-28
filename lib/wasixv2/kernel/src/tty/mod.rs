//! Terminals: a device's bytes through the line discipline to readers, writers' bytes through
//! output processing to the device, and the job control a controlling terminal adds (Linux's
//! `drivers/tty`: `tty_io.c`, `tty_ioctl.c`, `tty_jobctrl.c`, `n_tty.c`).
//!
//! The only device so far is the host's console ([`console`]); it is a terminal like a serial
//! line or a virtual console, with job control, not Linux's `/dev/console` redirection, which
//! has none.

mod console;
mod ioctl;
mod jobctl;
pub(crate) mod ldisc;

use std::{
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

pub(crate) use console::{Console, HVC_MAJOR};
pub(crate) use ioctl::encode_dev;
pub(crate) use jobctl::Jobs;
use wasixv2_abi::{Signal, Termios, Winsize};
use wasixv2_host::Host;
use wasixv2_syscalls::{Errno, LocalFuture, SysErr, SysResult};
use wasixv2_vfs::Device;
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, Waiter, Wakeup};

use crate::{
    file::{FileObject, Io},
    sync::MutexExt,
};
use ldisc::{Ldisc, Received};

/// Most bytes one step of a write processes for the device.
const WRITE_CHUNK: usize = 4096;

/// A terminal (`tty_struct`).
pub(crate) struct Tty {
    device: Device,
    /// The termios and the line discipline's state (`termios_rwsem` and `n_tty_data`).
    ldisc: Mutex<Ldisc>,
    winsize: Mutex<Winsize>,
    ctrl: Mutex<Ctrl>,
    /// Bumped by each hangup: the files opened before it are hung up (`hung_up_tty_fops`).
    hangups: AtomicU64,
    /// Hung up, and not opened since (`TTY_HUPPED`).
    hupped: AtomicBool,
    /// Open files (`tty->count`).
    opens: AtomicUsize,
    /// `TIOCEXCL`: no more opens but by `CAP_SYS_ADMIN`.
    exclusive: AtomicBool,
    /// Readers and pollers wait here, and the device for room to receive more.
    read_wait: WaitQueue,
    /// Writers wait here for stopped output to go on.
    write_wait: WaitQueue,
    /// Serialises reads (`atomic_read_lock`).
    read_lock: futures_util::lock::Mutex<()>,
    /// Serialises writers, through their waits for stopped output (`atomic_write_lock`).
    write_lock: futures_util::lock::Mutex<()>,
    /// Serialises what goes to the device, writes and echoes alike, for as long as one step
    /// takes (`output_lock`), so the discipline's column follows what the device got.
    output: futures_util::lock::Mutex<()>,
    driver: Driver,
    jobs: Jobs,
    host: Arc<dyn Host>,
}

/// The session the terminal controls and its foreground process group (`tty->ctrl`). The
/// innermost lock: taken under a process's identity, never the other way round.
#[derive(Clone, Copy, Default)]
pub(crate) struct Ctrl {
    pub(crate) session: Option<u32>,
    pub(crate) pgrp: Option<u32>,
}

/// The device under the terminal (`tty_driver` and `tty_operations`).
pub(crate) enum Driver {
    Console(Console),
}

impl Driver {
    /// `tty->ops->write`, all of `data`.
    async fn write(&self, data: &[u8]) -> io::Result<()> {
        match self {
            Driver::Console(console) => console.write(data).await,
        }
    }

    /// The discipline took input, so the device may hand it more (`tty_unthrottle`).
    fn unthrottle(&self) {
        match self {
            Driver::Console(console) => console.unthrottle(),
        }
    }

    /// Whether the device keeps its own hardware settings in `c_cflag`, as a driver without
    /// `set_termios` does (`tty_termios_copy_hw`).
    fn copies_hw(&self) -> bool {
        match self {
            Driver::Console(_) => true,
        }
    }

    /// Whether a session leader's exit hangs the terminal up (`disassociate_ctty`): a pty
    /// slave's only signals its foreground group.
    fn hangs_up_on_exit(&self) -> bool {
        match self {
            Driver::Console(_) => true,
        }
    }
}

impl Tty {
    fn new(
        device: Device,
        winsize: Winsize,
        driver: Driver,
        jobs: Jobs,
        host: Arc<dyn Host>,
    ) -> Self {
        Self {
            device,
            ldisc: Mutex::new(Ldisc::new(ldisc::std_termios())),
            winsize: Mutex::new(winsize),
            ctrl: Mutex::default(),
            hangups: AtomicU64::new(0),
            hupped: AtomicBool::new(false),
            opens: AtomicUsize::new(0),
            exclusive: AtomicBool::new(false),
            read_wait: WaitQueue::new(),
            write_wait: WaitQueue::new(),
            read_lock: futures_util::lock::Mutex::new(()),
            write_lock: futures_util::lock::Mutex::new(()),
            output: futures_util::lock::Mutex::new(()),
            driver,
            jobs,
            host,
        }
    }

    pub(crate) fn device(&self) -> Device {
        self.device
    }

    /// The foreground process group, if any (`tty_get_pgrp`).
    pub(crate) fn pgrp(&self) -> Option<u32> {
        self.ctrl.lock_or_recover().pgrp
    }

    /// `tty_open` once the device is found: a new open file, unless the terminal is exclusive
    /// and the opener is not `admin` (`tty_reopen`'s `EBUSY`).
    pub(crate) fn open(self: &Arc<Self>, admin: bool) -> Result<TtyFile, Errno> {
        let opens = self.opens.load(Ordering::Relaxed);
        if opens > 0 && self.exclusive.load(Ordering::Relaxed) && !admin {
            return Err(Errno::EBUSY);
        }
        self.opens.fetch_add(1, Ordering::Relaxed);
        self.hupped.store(false, Ordering::Relaxed);
        Ok(TtyFile {
            tty: self.clone(),
            generation: self.hangups.load(Ordering::Acquire),
        })
    }

    // ---- input ----

    /// `receive_buf`: the device's input `data` through the discipline. Returns how many bytes
    /// it took; the rest wait until a reader makes room. Signals go to the foreground process
    /// group, and echoes to the device.
    pub(crate) async fn receive(&self, data: &[u8]) -> usize {
        let received = self.ldisc.lock_or_recover().receive(data);
        let taken = received.taken;
        self.received(received);
        self.flush_echoes().await;
        taken
    }

    /// Acts on what the discipline did: signals (`isig`) and wakeups.
    fn received(&self, received: Received) {
        for sig in received.signals {
            self.signal_pgrp(sig);
        }
        if received.readable {
            self.read_wait.wake(PollMask::READABLE);
        }
        if received.started {
            self.write_wait.wake(PollMask::WRITABLE);
        }
    }

    /// `__isig`: `sig` to the foreground process group, from the kernel. Returns whether some
    /// process has it pending now.
    pub(crate) fn signal_pgrp(&self, sig: i32) -> bool {
        self.pgrp()
            .is_some_and(|pgrp| self.jobs.kill_pgrp(pgrp, sig))
    }

    /// Writes the pending echoes, unless output is stopped (`process_echoes`).
    async fn flush_echoes(&self) {
        if !self.ldisc.lock_or_recover().has_echoes() {
            return;
        }
        let _output = self.output.lock().await;
        let mut out = Vec::new();
        {
            let mut ldisc = self.ldisc.lock_or_recover();
            if ldisc.stopped() {
                return;
            }
            ldisc.take_echoes(&mut out);
        }
        // Echo has nobody to report a failing device to.
        let _ = self.driver.write(&out).await;
    }

    // ---- reading and writing ----

    /// `n_tty_read`.
    async fn read(&self, file: &TtyFile, io: Io<'_>, buf: &mut [u8]) -> SysResult<usize> {
        if file.hung_up() {
            return Ok(0);
        }
        self.check_change(io.task, Signal::SIGTTIN)?;
        let _reading = match io.nonblocking() {
            true => self.read_lock.try_lock().ok_or(Errno::EAGAIN)?,
            false => wasixv2_wait::interruptible(self.read_lock.lock(), io.interrupt)
                .await
                .map_err(|_| SysErr::Restart)?,
        };
        // Without canonical mode, `VMIN` bytes are enough, and `VTIME` tenths of a second is how
        // long to wait: for the first byte when `VMIN` is 0, between bytes otherwise.
        let (icanon, minimum, time) = {
            let ldisc = self.ldisc.lock_or_recover();
            let time = Duration::from_millis(100 * u64::from(ldisc.vtime()));
            (ldisc.icanon(), usize::from(ldisc.vmin()), time)
        };
        let (minimum, mut deadline, inter_byte) = match (icanon, minimum, time.is_zero()) {
            (true, ..) => (0, None, None),
            (false, 0, _) => (1, Some(self.host.monotonic() + time), None),
            (false, minimum, true) => (minimum, None, None),
            (false, minimum, false) => (minimum, None, Some(time)),
        };
        let waiter = Waiter::new();
        let _registration = waiter.register(&self.read_wait, PollMask::READABLE);
        let mut done = 0;
        while done < buf.len() {
            let copied = {
                let mut ldisc = self.ldisc.lock_or_recover();
                ldisc
                    .input_available(false)
                    .then(|| ldisc.read(&mut buf[done..]))
            };
            if let Some(n) = copied {
                done += n;
                self.driver.unthrottle();
                if done >= minimum {
                    break;
                }
                if let Some(time) = inter_byte {
                    deadline = Some(self.host.monotonic() + time);
                }
                continue;
            }
            // `n_tty_wait_for_input`.
            if file.hung_up() || deadline.is_some_and(|at| self.host.monotonic() >= at) {
                break;
            }
            if io.nonblocking() {
                return match done {
                    0 => Err(Errno::EAGAIN.into()),
                    _ => Ok(done),
                };
            }
            if io.interrupt.pending() {
                return match done {
                    0 => Err(SysErr::Restart),
                    _ => Ok(done),
                };
            }
            let mut timer = deadline.map(|at| self.host.sleep_until(at));
            let timer = timer.as_mut().map(|timer| timer.as_mut());
            if waiter.sleep(io.interrupt, timer).await == Wakeup::TimedOut {
                deadline = Some(Duration::ZERO);
            }
        }
        Ok(done)
    }

    /// `n_tty_write`.
    async fn write(&self, file: &TtyFile, io: Io<'_>, data: &[u8]) -> SysResult<usize> {
        if file.hung_up() {
            return Err(Errno::EIO.into());
        }
        if self.ldisc.lock_or_recover().tostop() {
            self.check_change(io.task, Signal::SIGTTOU)?;
        }
        let _writing = match io.nonblocking() {
            true => self.write_lock.try_lock().ok_or(Errno::EAGAIN)?,
            false => wasixv2_wait::interruptible(self.write_lock.lock(), io.interrupt)
                .await
                .map_err(|_| SysErr::Restart)?,
        };
        if io.interrupt.pending() {
            return Err(SysErr::Restart);
        }
        let waiter = Waiter::new();
        let _registration = waiter.register(&self.write_wait, PollMask::WRITABLE);
        let mut done = 0;
        let mut out = Vec::new();
        loop {
            if file.hung_up() {
                break;
            }
            // Not held while output is stopped, so the input that starts it can still echo.
            let output = self.output.lock().await;
            let step = {
                let mut ldisc = self.ldisc.lock_or_recover();
                (!ldisc.stopped()).then(|| {
                    out.clear();
                    ldisc.take_echoes(&mut out);
                    let n = (data.len() - done).min(WRITE_CHUNK);
                    ldisc.output(&data[done..done + n], &mut out);
                    n
                })
            };
            if let Some(n) = step {
                if let Err(err) = self.driver.write(&out).await {
                    return match done {
                        0 => Err(crate::stdio::io_errno(err).into()),
                        _ => Ok(done),
                    };
                }
                done += n;
                if done == data.len() {
                    return Ok(done);
                }
                continue;
            }
            drop(output);
            // Output is stopped: wait for it to start again.
            if io.nonblocking() || io.interrupt.pending() {
                break;
            }
            waiter.sleep(io.interrupt, None).await;
        }
        match done {
            0 if file.hung_up() => Err(Errno::EIO.into()),
            0 if io.nonblocking() => Err(Errno::EAGAIN.into()),
            0 => Err(SysErr::Restart),
            _ => Ok(done),
        }
    }

    // ---- termios ----

    pub(crate) fn termios(&self) -> Termios {
        self.ldisc.lock_or_recover().termios
    }

    /// `tty_set_termios`: a device without its own `set_termios` keeps its hardware settings.
    fn set_termios(&self, mut termios: Termios) {
        let mut ldisc = self.ldisc.lock_or_recover();
        if self.driver.copies_hw() {
            let old = ldisc.termios.c_cflag.0;
            // `tty_termios_copy_hw`: only HUPCL, CREAD and CLOCAL may change.
            let soft = wasixv2_abi::TermiosCflag::HUPCL.0
                | wasixv2_abi::TermiosCflag::CREAD.0
                | wasixv2_abi::TermiosCflag::CLOCAL.0;
            termios.c_cflag.0 = (termios.c_cflag.0 & soft) | (old & !soft);
        }
        let received = ldisc.set_termios(termios);
        drop(ldisc);
        self.received(received);
        self.write_wait.wake(PollMask::WRITABLE);
    }

    // ---- window size ----

    pub(crate) fn winsize(&self) -> Winsize {
        *self.winsize.lock_or_recover()
    }

    /// `tty_do_resize`: a new size tells the foreground process group with SIGWINCH.
    pub(crate) fn resize(&self, winsize: Winsize) {
        {
            let mut current = self.winsize.lock_or_recover();
            if *current == winsize {
                return;
            }
            *current = winsize;
        }
        self.signal_pgrp(Signal::SIGWINCH.0);
    }

    // ---- hangup ----

    /// `__tty_hangup`: the files open now are hung up (reads end at once with 0, writes fail
    /// with `EIO`), the session loses its controlling terminal, and its leader gets SIGHUP and
    /// SIGCONT; with `exit_session` (the leader is exiting) the foreground process group gets
    /// SIGHUP too. Input waiting to be read is gone. Opening the terminal again works.
    pub(crate) fn hangup(self: &Arc<Self>, exit_session: bool) {
        if self.hupped.swap(true, Ordering::Relaxed) {
            return;
        }
        self.hangups.fetch_add(1, Ordering::Release);
        let pgrp = self.signal_session_leader();
        if exit_session && let Some(pgrp) = pgrp {
            self.jobs.kill_pgrp(pgrp, Signal::SIGHUP.0);
        }
        {
            let mut ldisc = self.ldisc.lock_or_recover();
            ldisc.flush_input();
            ldisc.start();
        }
        *self.ctrl.lock_or_recover() = Ctrl::default();
        let hup = PollMask(PollMask::HUP.0 | PollMask::ERR.0);
        self.read_wait.wake(hup);
        self.write_wait.wake(hup);
        self.driver.unthrottle();
    }

    /// Called as an open file goes (`tty_release`); the last one resets what lasts only while
    /// the terminal is open: exclusivity and unread input.
    fn release(&self) {
        if self.opens.fetch_sub(1, Ordering::Relaxed) == 1 {
            self.exclusive.store(false, Ordering::Relaxed);
            self.ldisc.lock_or_recover().flush_input();
            self.driver.unthrottle();
        }
    }
}

/// An open file of a terminal (`struct file` with `tty_fops`).
pub(crate) struct TtyFile {
    tty: Arc<Tty>,
    /// The hangups there were when it was opened.
    generation: u64,
}

impl TtyFile {
    pub(crate) fn tty(&self) -> &Arc<Tty> {
        &self.tty
    }

    /// `tty_hung_up_p`: a hangup came since the file was opened.
    pub(crate) fn hung_up(&self) -> bool {
        self.tty.hangups.load(Ordering::Acquire) != self.generation
    }
}

impl Drop for TtyFile {
    fn drop(&mut self) {
        self.tty.release();
    }
}

impl Pollable for TtyFile {
    /// `n_tty_poll`: readable once a read would find input; writable unless output is
    /// stopped. A hung-up file is everything at once (`hung_up_tty_poll`).
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        wasixv2_wait::poll_wait(&mut table, &self.tty.read_wait);
        wasixv2_wait::poll_wait(&mut table, &self.tty.write_wait);
        if self.hung_up() {
            return PollMask::DEFAULT | PollMask::ERR | PollMask::HUP;
        }
        let ldisc = self.tty.ldisc.lock_or_recover();
        let mut mask = PollMask::empty();
        if ldisc.input_available(true) {
            mask |= PollMask::READABLE;
        }
        if !ldisc.stopped() {
            mask |= PollMask::WRITABLE;
        }
        mask
    }
}

impl FileObject for TtyFile {
    fn read<'a>(&'a self, io: Io<'a>, buf: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(self.tty.read(self, io, buf))
    }

    fn write<'a>(&'a self, io: Io<'a>, data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(self.tty.write(self, io, data))
    }
}
