//! Terminals: a device's bytes through the line discipline to readers, writers' bytes through
//! output processing to the device, and the job control a controlling terminal adds (Linux's
//! `drivers/tty`: `tty_io.c`, `tty_ioctl.c`, `tty_jobctrl.c`, `n_tty.c`).
//!
//! The devices are the host's console ([`console`]), a terminal like a serial line or a
//! virtual console, with job control, not Linux's `/dev/console` redirection, which has none;
//! and pseudo-terminals ([`pty`]), whose slaves are such terminals too and whose masters play
//! their devices.

mod console;
mod devpts;
mod ioctl;
mod jobctl;
pub(crate) mod ldisc;
mod pty;

use std::{
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

pub(crate) use console::{Console, HVC_MAJOR};
pub(crate) use devpts::Ptys;
pub(crate) use ioctl::encode_dev;
pub(crate) use jobctl::Jobs;
pub(crate) use pty::{PTMX, PTY_SLAVE_MAJOR};
use pty::{Pty, Side};
use wasixv2_abi::{SiCode, Signal, Termios, TermiosCflag, TiocpktFlags, Winsize};
use wasixv2_host::Host;
use wasixv2_syscalls::{Errno, LocalFuture, SysErr, SysResult};
use wasixv2_vfs::Device;
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, Waiter, Wakeup};

use crate::{
    fasync::{Fasync, FasyncList, OwnerType},
    file::{FileObject, Io},
    pid::PidRef,
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
    /// The open files that get I/O signals (`tty->fasync`).
    fasync: FasyncList,
    /// A write left with bytes unwritten while files were on [`Tty::fasync`], so room for
    /// more is worth a `POLL_OUT` (`TTY_DO_WRITE_WAKEUP`).
    do_write_wakeup: AtomicBool,
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

/// The session the terminal controls and its foreground process group (`tty->ctrl`), held as
/// Linux holds their `struct pid`s: a group that emptied still reads as its number. The
/// innermost lock: taken under a process's identity, never the other way round.
#[derive(Clone, Default)]
pub(crate) struct Ctrl {
    pub(crate) session: Option<PidRef>,
    pub(crate) pgrp: Option<PidRef>,
}

/// The device under the terminal (`tty_driver` and `tty_operations`).
pub(crate) enum Driver {
    Console(Console),
    /// A pty's master: its device is the slave's discipline.
    PtyMaster(Arc<Pty>),
    /// A pty's slave: its device is the master's discipline.
    PtySlave(Arc<Pty>),
}

impl Driver {
    fn pty(&self) -> Option<(&Pty, Side)> {
        match self {
            Driver::Console(_) => None,
            Driver::PtyMaster(pty) => Some((pty, Side::Master)),
            Driver::PtySlave(pty) => Some((pty, Side::Slave)),
        }
    }

    /// `tty->ops->write`, all of `data`.
    async fn write(&self, data: &[u8]) -> io::Result<()> {
        match self {
            Driver::Console(console) => console.write(data).await,
            Driver::PtyMaster(pty) => {
                pty.write(Side::Master, data).await;
                Ok(())
            }
            Driver::PtySlave(pty) => {
                pty.write(Side::Slave, data).await;
                Ok(())
            }
        }
    }

    /// `tty_write_room` is not 0: the device takes a write step now. The console's does
    /// whenever the host does.
    fn write_room(&self) -> bool {
        self.pty().is_none_or(|(pty, side)| pty.write_room(side))
    }

    /// The discipline took input, so the device may hand it more (`tty_unthrottle`).
    fn unthrottle(&self) {
        match self {
            Driver::Console(console) => console.unthrottle(),
            Driver::PtyMaster(pty) => pty.unthrottle(Side::Master),
            Driver::PtySlave(pty) => pty.unthrottle(Side::Slave),
        }
    }

    /// `tty_driver_flush_buffer`: what was written and the device has not sent on is gone.
    fn flush_buffer(&self) {
        if let Some((pty, side)) = self.pty() {
            pty.flush_buffer(side);
        }
    }

    /// `tty_buffer_flush`: what the device received and the discipline has not taken is gone.
    fn flush_incoming(&self) {
        if let Some((pty, side)) = self.pty() {
            pty.flush_incoming(side);
        }
    }

    /// The discipline dropped its unread input (`n_tty_packet_mode_flush`).
    fn input_flushed(&self) {
        if let Some((pty, side)) = self.pty() {
            pty.input_flushed(side);
        }
    }

    /// Output stopped or started (`tty->ops->stop`, `start`).
    fn flow(&self, stopped: bool) {
        if let Some((pty, side)) = self.pty() {
            pty.flow(side, stopped);
        }
    }

    /// `tty->ops->set_termios` after the termios changed from `old` to `new`.
    fn termios_changed(&self, old: &Termios, new: &Termios) {
        if let Some((pty, Side::Slave)) = self.pty() {
            pty.termios_changed(old, new);
        }
    }

    /// Whether the device keeps its own hardware settings in `c_cflag`, as a driver without
    /// `set_termios` does (`tty_termios_copy_hw`).
    fn copies_hw(&self) -> bool {
        self.pty().is_none()
    }

    /// Whether a session leader's exit hangs the terminal up (`disassociate_ctty`): a pty
    /// slave's only signals its foreground group.
    fn hangs_up_on_exit(&self) -> bool {
        self.pty().is_none()
    }

    /// `TTY_OTHER_CLOSED`: the other side of a pty closed, so a read that finds no input
    /// fails with `EIO`.
    fn other_closed(&self) -> bool {
        self.pty().is_some_and(|(pty, side)| pty.other_closed(side))
    }

    /// `tty->link && !tty->link->count`: a pty slave whose master closed takes no writes.
    /// (The master holds a count of its slave, so its own writes go on.)
    fn link_gone(&self) -> bool {
        match self {
            Driver::PtySlave(pty) => pty.other_closed(Side::Slave),
            _ => false,
        }
    }

    /// `tty->ops->open`, for each new file.
    fn open(&self) -> Result<(), Errno> {
        self.pty().map_or(Ok(()), |(pty, side)| pty.open(side))
    }

    /// `tty->ops->close`, for each file that goes.
    fn release(&self) {
        if let Some((pty, side)) = self.pty() {
            pty.release(side);
        }
    }

    /// A pty master's pair, whose packet mode its reads and polls follow.
    fn packet(&self) -> Option<&Pty> {
        match self {
            Driver::PtyMaster(pty) => Some(pty),
            _ => None,
        }
    }
}

impl Tty {
    fn new(
        device: Device,
        termios: Termios,
        winsize: Winsize,
        driver: Driver,
        jobs: Jobs,
        host: Arc<dyn Host>,
    ) -> Self {
        Self {
            device,
            ldisc: Mutex::new(Ldisc::new(termios)),
            winsize: Mutex::new(winsize),
            ctrl: Mutex::default(),
            hangups: AtomicU64::new(0),
            hupped: AtomicBool::new(false),
            opens: AtomicUsize::new(0),
            exclusive: AtomicBool::new(false),
            read_wait: WaitQueue::new(),
            write_wait: WaitQueue::new(),
            fasync: FasyncList::default(),
            do_write_wakeup: AtomicBool::new(false),
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
    pub(crate) fn pgrp(&self) -> Option<PidRef> {
        self.ctrl.lock_or_recover().pgrp.clone()
    }

    /// `tty_open` once the device is found: a new open file, unless the terminal is exclusive
    /// and the opener is not `admin` (`tty_reopen`'s `EBUSY`).
    pub(crate) fn open(self: &Arc<Self>, admin: bool) -> Result<TtyFile, Errno> {
        let opens = self.opens.load(Ordering::Relaxed);
        if opens > 0 && self.exclusive.load(Ordering::Relaxed) && !admin {
            return Err(Errno::EBUSY);
        }
        self.driver.open()?;
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

    /// Acts on what the discipline did: signals (`isig`) with their flush of what waits to be
    /// sent, flow control, and wakeups.
    fn received(&self, received: Received) {
        if received.flushed {
            self.driver.flush_buffer();
            self.driver.input_flushed();
        }
        for sig in received.signals {
            self.signal_pgrp(sig);
        }
        if received.stopped || received.started {
            let stopped = self.ldisc.lock_or_recover().stopped();
            self.driver.flow(stopped);
        }
        if received.readable {
            self.fasync.kill(SiCode::POLL_IN);
            self.read_wait.wake(PollMask::READABLE);
        }
        if received.started {
            self.wakeup();
        }
    }

    /// `tty_wakeup`: the device has room for more, or output went on. A write that left
    /// bytes unwritten has the owners of the files hear of it (`n_tty_write_wakeup`).
    pub(crate) fn wakeup(&self) {
        if self.do_write_wakeup.swap(false, Ordering::Relaxed) {
            self.fasync.kill(SiCode::POLL_OUT);
        }
        self.write_wait.wake(PollMask::WRITABLE);
    }

    /// `n_tty_flush_buffer` with `tty_buffer_flush`: the unread input is gone, and what the
    /// device received but the discipline had not taken yet.
    fn flush_input(&self) {
        self.driver.flush_incoming();
        self.ldisc.lock_or_recover().flush_input();
        self.driver.input_flushed();
        self.driver.unthrottle();
    }

    /// `__isig`: `sig` to the foreground process group, from the kernel. Returns whether some
    /// process has it pending now.
    pub(crate) fn signal_pgrp(&self, sig: i32) -> bool {
        self.pgrp()
            .is_some_and(|pgrp| self.jobs.kill_pgrp(pgrp.id(), sig))
    }

    /// Writes the pending echoes, unless output is stopped (`process_echoes`).
    async fn flush_echoes(&self) {
        if !self.ldisc.lock_or_recover().has_echoes() {
            return;
        }
        let _output = self.output.lock().await;
        self.write_echoes().await;
    }

    /// [`Tty::flush_echoes`] for a caller that holds `output`.
    async fn write_echoes(&self) {
        let mut out = Vec::new();
        {
            let mut ldisc = self.ldisc.lock_or_recover();
            if ldisc.stopped() || !ldisc.has_echoes() {
                return;
            }
            ldisc.take_echoes(&mut out);
        }
        // Echo has nobody to report a failing device to.
        let _ = self.driver.write(&out).await;
    }

    /// `tty_wait_until_sent`: waits, at most `timeout`, until nothing the terminal holds for the
    /// device is unsent: the step under way (a write, or echoes the device's input queued) ends,
    /// and echoes still pending are written. Output that is stopped stays where it is.
    pub(crate) async fn wait_until_sent(&self, timeout: Duration) {
        let deadline = self.host.monotonic() + timeout;
        let sent = async {
            let _output = self.output.lock().await;
            self.write_echoes().await;
        };
        let timer = self.host.sleep_until(deadline);
        futures_util::future::select(std::pin::pin!(sent), timer).await;
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
        // A master in packet mode: each read is a status byte or `TIOCPKT_DATA` and data.
        let packet = self.driver.packet().filter(|pty| pty.packet());
        let waiter = Waiter::new();
        let _registration = waiter.register(&self.read_wait, PollMask::READABLE);
        let mut done = 0;
        while done < buf.len() {
            if let Some(pty) = packet
                && pty.has_status()
            {
                if done > 0 {
                    break;
                }
                if let Some(status) = pty.take_status() {
                    buf[0] = status;
                    done = 1;
                    break;
                }
            }
            let copied = {
                let mut ldisc = self.ldisc.lock_or_recover();
                ldisc.input_available(false).then(|| {
                    let mut n = 0;
                    if packet.is_some() && done == 0 {
                        buf[0] = TiocpktFlags::TIOCPKT_DATA.0 as u8;
                        n = 1;
                    }
                    n + ldisc.read(&mut buf[done + n..])
                })
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
            if self.driver.other_closed() {
                return match done {
                    0 => Err(Errno::EIO.into()),
                    _ => Ok(done),
                };
            }
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
        // Taking more of a pty's buffered input may have left echoes to write.
        self.flush_echoes().await;
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
            if file.hung_up() || self.driver.link_gone() {
                break;
            }
            // Not held while output is stopped, so the input that starts it can still echo.
            let output = self.output.lock().await;
            // Checked before taking the discipline's lock: a pty's buffer is taken before the
            // discipline that receives from it.
            let room = self.driver.write_room();
            let step = {
                let mut ldisc = self.ldisc.lock_or_recover();
                (room && !ldisc.stopped()).then(|| {
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
            // Output is stopped, or the device has no room: wait for it to go on.
            if io.nonblocking() || io.interrupt.pending() {
                break;
            }
            waiter.sleep(io.interrupt, None).await;
        }
        if !self.fasync.is_empty() {
            self.do_write_wakeup.store(true, Ordering::Relaxed);
        }
        match done {
            0 if file.hung_up() || self.driver.link_gone() => Err(Errno::EIO.into()),
            0 if io.nonblocking() => Err(Errno::EAGAIN.into()),
            0 => Err(SysErr::Restart),
            _ => Ok(done),
        }
    }

    // ---- termios ----

    pub(crate) fn termios(&self) -> Termios {
        self.ldisc.lock_or_recover().termios
    }

    /// `tty_set_termios`: a device without its own `set_termios` keeps its hardware settings;
    /// a pty is always 8 bits without parity, receiving (`pty_set_termios`).
    fn set_termios(&self, mut termios: Termios) {
        let mut ldisc = self.ldisc.lock_or_recover();
        let old = ldisc.termios;
        if self.driver.pty().is_some() {
            termios.c_cflag.0 &= !(TermiosCflag::CSIZE.0 | TermiosCflag::PARENB.0);
            termios.c_cflag.0 |= TermiosCflag::CS8.0 | TermiosCflag::CREAD.0;
        }
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
        self.driver.termios_changed(&old, &termios);
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
        // `tty_ldisc_hangup`: the discipline's input and the device's output are flushed.
        self.driver.flush_incoming();
        {
            let mut ldisc = self.ldisc.lock_or_recover();
            ldisc.flush_input();
            ldisc.start();
        }
        self.driver.input_flushed();
        self.driver.flush_buffer();
        *self.ctrl.lock_or_recover() = Ctrl::default();
        // The files hung up signal nobody any more.
        self.fasync.clear();
        let hup = PollMask(PollMask::HUP.0 | PollMask::ERR.0);
        self.read_wait.wake(hup);
        self.write_wait.wake(hup);
        // Input arriving from now on signals no group of the old session.
        self.driver.unthrottle();
    }

    /// Called as an open file goes (`tty_release`); the last one resets what lasts only while
    /// the terminal is open: exclusivity, and unread input, unless the other side of a pty
    /// keeps the pair, and so the slave's discipline, alive.
    fn release(&self) {
        if self.opens.fetch_sub(1, Ordering::Relaxed) == 1 {
            self.exclusive.store(false, Ordering::Relaxed);
            if self.driver.pty().is_none() {
                self.flush_input();
            }
        }
        self.driver.release();
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
    /// `n_tty_poll`: readable once a read would find input, and `POLLPRI` too while a pty
    /// master in packet mode has a status to read; writable unless output is stopped or a
    /// pty's buffer is full; `POLLHUP` once the other side of a pty closed. A hung-up file is
    /// everything at once (`hung_up_tty_poll`).
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        wasixv2_wait::poll_wait(&mut table, &self.tty.read_wait);
        wasixv2_wait::poll_wait(&mut table, &self.tty.write_wait);
        if self.hung_up() {
            return PollMask::DEFAULT | PollMask::ERR | PollMask::HUP;
        }
        let driver = &self.tty.driver;
        let mut mask = PollMask::empty();
        let stopped = {
            let ldisc = self.tty.ldisc.lock_or_recover();
            if ldisc.input_available(true) {
                mask |= PollMask::READABLE;
            }
            ldisc.stopped()
        };
        if driver.packet().is_some_and(|pty| pty.has_status()) {
            mask |= PollMask::PRI | PollMask::READABLE;
        }
        if driver.other_closed() {
            mask |= PollMask::HUP;
        }
        if !stopped && driver.write_room() {
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

    /// `tty_fasync`: `ENOTTY` once hung up. A file put on the list gets the foreground process
    /// group as its owner, or else the caller's process, unless it has one.
    fn fasync(&self, fasync: &Fasync<'_>) -> Option<Result<(), Errno>> {
        if self.hung_up() {
            return Some(Err(Errno::ENOTTY));
        }
        let added = self.tty.fasync.helper(fasync);
        if let Some(on) = &fasync.on
            && added
        {
            let (pid, ty) = match self.tty.pgrp() {
                Some(pgrp) => (pgrp.id(), OwnerType::Group),
                None => (on.task.0, OwnerType::Process),
            };
            fasync.file.set_owner(Some(pid), ty, on.cred, false);
        }
        Some(Ok(()))
    }
}
