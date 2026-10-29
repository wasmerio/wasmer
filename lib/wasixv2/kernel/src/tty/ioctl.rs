//! A terminal's `ioctl`s (`tty_ioctl`, `tty_jobctrl_ioctl`, `n_tty_ioctl`, `tty_mode_ioctl`
//! and a pty master's `pty_unix98_ioctl`). The kernel answers them itself, as they act on
//! processes and sessions, not only on the terminal. On a pty master, the termios, window and
//! job control requests act on its slave (`tty_pair_get_tty`).

use std::sync::Arc;

use wasixv2_abi::{
    GuestPtr, GuestPtrMut, IoctlRequest, OpenFlags, Signal, TcflowAction, TcflushQueue, Termios,
    Termios2, TermiosCflag, Winsize,
};
use wasixv2_syscalls::{Caller, Errno, SysErr, SysResult, read_value, write_value};
use wasixv2_vfs::Cap;
use wasixv2_wait::Interrupt;

use super::{Driver, Tty, TtyFile, ldisc};
use crate::{Kernel, file::OpenFile, sync::MutexExt};

/// `N_TTY`, the only line discipline.
const N_TTY: i32 = 0;
/// `NCC`: the control characters `struct termio` carries.
const NCC: usize = 8;
/// `struct termio` up to its padding: four 16-bit flag words, `c_line` and `c_cc[NCC]`.
const TERMIO_SIZE: usize = 8 + 1 + NCC;

/// `new_encode_dev`: a device number as `stat` and `TIOCGDEV` report it.
pub(crate) fn encode_dev(major: u32, minor: u32) -> u32 {
    (minor & 0xff) | (major << 8) | ((minor & !0xff) << 12)
}

impl Kernel {
    pub(crate) async fn tty_ioctl(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        tty_file: &TtyFile,
        request: IoctlRequest,
        arg: u32,
    ) -> SysResult<i32> {
        if tty_file.hung_up() {
            // `hung_up_tty_ioctl`.
            return Err(match request {
                IoctlRequest::TIOCSPGRP => Errno::ENOTTY,
                _ => Errno::EIO,
            }
            .into());
        }
        let tty = tty_file.tty();
        let real = tty.real_tty()?;
        let real = &real;
        let memory = caller.memory();
        let task = caller.task();
        let process = self.process(task);
        // `TIOCSTI`, `TIOCCONS`, `TIOCSCTTY`'s steal: `capable(CAP_SYS_ADMIN)`, never a user
        // namespace's root.
        let admin = || self.cred(caller).capable_init(Cap::SysAdmin);
        let put_int = async |value: i32| -> SysResult<i32> {
            write_value(memory, GuestPtrMut::new(arg), &value).await?;
            Ok(0)
        };
        match request {
            // ---- termios (`tty_mode_ioctl`) ----
            IoctlRequest::TCGETS => {
                write_value(memory, GuestPtrMut::new(arg), &real.termios()).await?;
                Ok(0)
            }
            IoctlRequest::TCGETS2 => {
                let termios = real.termios();
                let speed = ldisc::baud_rate(termios.c_cflag);
                let termios2 = Termios2 {
                    c_iflag: termios.c_iflag,
                    c_oflag: termios.c_oflag,
                    c_cflag: termios.c_cflag,
                    c_lflag: termios.c_lflag,
                    c_line: termios.c_line,
                    c_cc: termios.c_cc,
                    c_ispeed: speed,
                    c_ospeed: speed,
                };
                write_value(memory, GuestPtrMut::new(arg), &termios2).await?;
                Ok(0)
            }
            IoctlRequest::TCSETS | IoctlRequest::TCSETSW | IoctlRequest::TCSETSF => {
                let termios: Termios = read_value(memory, GuestPtr::new(arg)).await?;
                let flush = request == IoctlRequest::TCSETSF;
                let wait = request != IoctlRequest::TCSETS;
                self.tty_set_termios(caller, real, termios, wait, flush)
                    .await
            }
            IoctlRequest::TCSETS2 | IoctlRequest::TCSETSW2 | IoctlRequest::TCSETSF2 => {
                let termios2: Termios2 = read_value(memory, GuestPtr::new(arg)).await?;
                let termios = Termios {
                    c_iflag: termios2.c_iflag,
                    c_oflag: termios2.c_oflag,
                    c_cflag: termios2.c_cflag,
                    c_lflag: termios2.c_lflag,
                    c_line: termios2.c_line,
                    c_cc: termios2.c_cc,
                };
                let flush = request == IoctlRequest::TCSETSF2;
                let wait = request != IoctlRequest::TCSETS2;
                self.tty_set_termios(caller, real, termios, wait, flush)
                    .await
            }
            IoctlRequest::TCGETA => {
                let termio = termio(&real.termios());
                memory
                    .write(arg, &termio)
                    .await
                    .map_err(|_| Errno::EFAULT)?;
                Ok(0)
            }
            IoctlRequest::TCSETA | IoctlRequest::TCSETAW | IoctlRequest::TCSETAF => {
                let mut termio = [0; TERMIO_SIZE];
                memory
                    .read(arg, &mut termio)
                    .await
                    .map_err(|_| Errno::EFAULT)?;
                let termios = from_termio(real.termios(), &termio);
                let flush = request == IoctlRequest::TCSETAF;
                let wait = request != IoctlRequest::TCSETA;
                self.tty_set_termios(caller, real, termios, wait, flush)
                    .await
            }
            IoctlRequest::TIOCGSOFTCAR => {
                let clocal = real.termios().c_cflag.0 & TermiosCflag::CLOCAL.0 != 0;
                put_int(i32::from(clocal)).await
            }
            IoctlRequest::TIOCSSOFTCAR => {
                let on: i32 = read_value(memory, GuestPtr::new(arg)).await?;
                let mut termios = real.termios();
                match on {
                    0 => termios.c_cflag.0 &= !TermiosCflag::CLOCAL.0,
                    _ => termios.c_cflag.0 |= TermiosCflag::CLOCAL.0,
                }
                real.set_termios(termios);
                Ok(0)
            }
            // ---- the discipline (`n_tty_ioctl`, `n_tty_ioctl_helper`) ----
            IoctlRequest::TIOCINQ => {
                let inq = tty.ldisc.lock_or_recover().inq();
                put_int(inq as i32).await
            }
            // Output goes to the device as it is written; nothing waits in between.
            IoctlRequest::TIOCOUTQ => put_int(0).await,
            IoctlRequest::TCFLSH => {
                tty.check_change(task, Signal::SIGTTOU)?;
                // `tty_perform_flush`: input in the discipline, output in the device.
                let (input, output) = match TcflushQueue(arg as i32) {
                    TcflushQueue::TCIFLUSH => (true, false),
                    TcflushQueue::TCOFLUSH => (false, true),
                    TcflushQueue::TCIOFLUSH => (true, true),
                    _ => return Err(Errno::EINVAL.into()),
                };
                if input {
                    tty.flush_input();
                }
                if output {
                    tty.driver.flush_buffer();
                }
                Ok(0)
            }
            IoctlRequest::TCXONC => {
                tty.check_change(task, Signal::SIGTTOU)?;
                let started = match TcflowAction(arg as i32) {
                    TcflowAction::TCOOFF => {
                        let stopped = {
                            let mut ldisc = tty.ldisc.lock_or_recover();
                            let was = ldisc.stopped();
                            ldisc.tco_stop(true);
                            !was && ldisc.stopped()
                        };
                        if stopped {
                            tty.driver.flow(true);
                        }
                        false
                    }
                    TcflowAction::TCOON => tty.ldisc.lock_or_recover().tco_stop(false),
                    action @ (TcflowAction::TCIOFF | TcflowAction::TCION) => {
                        // `tty_send_xchar`: the flow character goes to the device.
                        let index = match action {
                            TcflowAction::TCIOFF => wasixv2_abi::TermiosCcIndex::VSTOP,
                            _ => wasixv2_abi::TermiosCcIndex::VSTART,
                        };
                        let c = tty.termios().c_cc[index.0 as usize];
                        if c != 0 {
                            let _output = tty.output.lock().await;
                            tty.driver
                                .write(&[c])
                                .await
                                .map_err(crate::stdio::io_errno)?;
                        }
                        false
                    }
                    _ => return Err(Errno::EINVAL.into()),
                };
                if started {
                    tty.driver.flow(false);
                    tty.wakeup();
                    tty.flush_echoes().await;
                }
                Ok(0)
            }
            // ---- breaks: no device here has a line to send one on ----
            IoctlRequest::TCSBRK | IoctlRequest::TCSBRKP => {
                tty.check_change(task, Signal::SIGTTOU)?;
                // `tty_wait_until_sent`: what was written reached the device once no step of
                // a write is under way.
                drop(tty.output.lock().await);
                if self.interrupt(caller).pending() {
                    return Err(Errno::EINTR.into());
                }
                Ok(0)
            }
            IoctlRequest::TIOCSBRK | IoctlRequest::TIOCCBRK => {
                if request == IoctlRequest::TIOCSBRK {
                    tty.check_change(task, Signal::SIGTTOU)?;
                }
                Ok(0)
            }
            // ---- the terminal (`tty_ioctl`) ----
            IoctlRequest::TIOCGWINSZ => {
                write_value(memory, GuestPtrMut::new(arg), &real.winsize()).await?;
                Ok(0)
            }
            IoctlRequest::TIOCSWINSZ => {
                let winsize: Winsize = read_value(memory, GuestPtr::new(arg)).await?;
                real.resize(winsize);
                Ok(0)
            }
            IoctlRequest::TIOCSTI => {
                if !tty.is_ctty_of(&process) && !admin() {
                    return Err(Errno::EPERM.into());
                }
                let c: u8 = read_value(memory, GuestPtr::new(arg)).await?;
                tty.receive(&[c]).await;
                Ok(0)
            }
            IoctlRequest::TIOCEXCL | IoctlRequest::TIOCNXCL => {
                let exclusive = request == IoctlRequest::TIOCEXCL;
                tty.exclusive
                    .store(exclusive, std::sync::atomic::Ordering::Relaxed);
                Ok(0)
            }
            IoctlRequest::TIOCGEXCL => {
                let exclusive = tty.exclusive.load(std::sync::atomic::Ordering::Relaxed);
                put_int(i32::from(exclusive)).await
            }
            IoctlRequest::TIOCGETD => put_int(N_TTY).await,
            IoctlRequest::TIOCSETD => {
                tty.check_change(task, Signal::SIGTTOU)?;
                let ldisc: i32 = read_value(memory, GuestPtr::new(arg)).await?;
                match ldisc {
                    N_TTY => Ok(0),
                    // `tty_ldisc_get`: no other discipline is registered.
                    _ => Err(Errno::EINVAL.into()),
                }
            }
            IoctlRequest::TIOCGDEV => {
                let device = real.device();
                let encoded = encode_dev(device.major, device.minor);
                write_value(memory, GuestPtrMut::new(arg), &encoded).await?;
                Ok(0)
            }
            IoctlRequest::TIOCVHANGUP => {
                if !admin() {
                    return Err(Errno::EPERM.into());
                }
                tty.hangup(false);
                Ok(0)
            }
            // ---- job control (`tty_jobctrl_ioctl`) ----
            IoctlRequest::TIOCNOTTY => {
                if !tty.is_ctty_of(&process) {
                    return Err(Errno::ENOTTY.into());
                }
                self.no_tty(&process);
                Ok(0)
            }
            IoctlRequest::TIOCSCTTY => {
                let readable = file.check_readable().is_ok();
                real.set_ctty_ioctl(&process, readable, arg == 1, admin())?;
                Ok(0)
            }
            // A master asks about its slave whether or not the slave is the caller's.
            IoctlRequest::TIOCGPGRP => {
                if Arc::ptr_eq(tty, real) && !tty.is_ctty_of(&process) {
                    return Err(Errno::ENOTTY.into());
                }
                let ns = self.active_pid_ns(task);
                put_int(real.pgrp().map_or(0, |pgrp| pgrp.nr(&ns))).await
            }
            IoctlRequest::TIOCSPGRP => {
                match real.check_change(task, Signal::SIGTTOU) {
                    Err(SysErr::Errno(Errno::EIO)) => return Err(Errno::ENOTTY.into()),
                    result => result?,
                }
                let pgrp: i32 = read_value(memory, GuestPtr::new(arg)).await?;
                if pgrp < 0 {
                    return Err(Errno::EINVAL.into());
                }
                // `find_vpid`: a group outside the caller's pid namespace does not exist for it.
                real.set_pgrp(&process, self.find_vpid(caller, pgrp))?;
                Ok(0)
            }
            IoctlRequest::TIOCGSID => {
                if Arc::ptr_eq(tty, real) && !tty.is_ctty_of(&process) {
                    return Err(Errno::ENOTTY.into());
                }
                let sid = real.session().ok_or(Errno::ENOTTY)?;
                put_int(sid.nr(&self.active_pid_ns(task))).await
            }
            // ---- a pty master's (`pty_unix98_ioctl`, `ptm_open_peer`) ----
            IoctlRequest::TIOCGPTPEER => match &tty.driver {
                Driver::PtyMaster(pty) => {
                    let flags = OpenFlags(arg);
                    self.open_pty_peer(caller, file, pty, flags).await
                }
                _ => Err(Errno::EIO.into()),
            },
            IoctlRequest::TIOCSPTLCK
            | IoctlRequest::TIOCGPTLCK
            | IoctlRequest::TIOCPKT
            | IoctlRequest::TIOCGPKT
            | IoctlRequest::TIOCGPTN
            | IoctlRequest::TIOCSIG => {
                let Driver::PtyMaster(pty) = &tty.driver else {
                    return Err(Errno::ENOTTY.into());
                };
                match request {
                    IoctlRequest::TIOCSPTLCK => {
                        let locked: i32 = read_value(memory, GuestPtr::new(arg)).await?;
                        pty.set_lock(locked != 0);
                        Ok(0)
                    }
                    IoctlRequest::TIOCGPTLCK => put_int(i32::from(pty.locked())).await,
                    IoctlRequest::TIOCPKT => {
                        let on: i32 = read_value(memory, GuestPtr::new(arg)).await?;
                        pty.set_packet(on != 0);
                        Ok(0)
                    }
                    IoctlRequest::TIOCGPKT => put_int(i32::from(pty.packet())).await,
                    IoctlRequest::TIOCGPTN => put_int(pty.index() as i32).await,
                    _ => {
                        pty.signal(arg as i32)?;
                        Ok(0)
                    }
                }
            }
            _ => Err(Errno::ENOTTY.into()),
        }
    }

    /// `set_termios`: from a background group only as `tty_check_change` allows; `wait` lets
    /// the output under way finish first, and `flush` drops unread input too.
    async fn tty_set_termios(
        &self,
        caller: &Caller<'_>,
        tty: &Arc<Tty>,
        termios: Termios,
        wait: bool,
        flush: bool,
    ) -> SysResult<i32> {
        tty.check_change(caller.task(), Signal::SIGTTOU)?;
        let _writing = match wait {
            true => {
                let interrupt = self.interrupt(caller);
                Some(
                    wasixv2_wait::interruptible(tty.write_lock.lock(), &*interrupt)
                        .await
                        .map_err(|_| SysErr::Restart)?,
                )
            }
            false => None,
        };
        if flush {
            tty.flush_input();
        }
        tty.set_termios(termios);
        Ok(0)
    }
}

/// `kernel_termios_to_user_termio`: the low 16 bits of each flag word, the line discipline
/// and the first `NCC` control characters, little-endian.
fn termio(termios: &Termios) -> [u8; TERMIO_SIZE] {
    let mut out = [0; TERMIO_SIZE];
    let flags = [
        termios.c_iflag.0,
        termios.c_oflag.0,
        termios.c_cflag.0,
        termios.c_lflag.0,
    ];
    for (i, flag) in flags.into_iter().enumerate() {
        out[2 * i..2 * i + 2].copy_from_slice(&(flag as u16).to_le_bytes());
    }
    out[8] = termios.c_line;
    out[9..].copy_from_slice(&termios.c_cc[..NCC]);
    out
}

/// `user_termio_to_kernel_termios`: `termio` replaces the low 16 bits of each flag word of
/// `termios`, its line discipline and its first `NCC` control characters.
fn from_termio(mut termios: Termios, termio: &[u8; TERMIO_SIZE]) -> Termios {
    let low = |i: usize| u32::from(u16::from_le_bytes([termio[2 * i], termio[2 * i + 1]]));
    let set = |flag: u32, i| (flag & 0xffff_0000) | low(i);
    termios.c_iflag.0 = set(termios.c_iflag.0, 0);
    termios.c_oflag.0 = set(termios.c_oflag.0, 1);
    termios.c_cflag.0 = set(termios.c_cflag.0, 2);
    termios.c_lflag.0 = set(termios.c_lflag.0, 3);
    termios.c_line = termio[8];
    termios.c_cc[..NCC].copy_from_slice(&termio[9..]);
    termios
}
