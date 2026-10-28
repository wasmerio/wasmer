//! A terminal's `ioctl`s (`tty_ioctl`, `tty_jobctrl_ioctl`, `n_tty_ioctl` and
//! `tty_mode_ioctl`). The kernel answers them itself, as they act on processes and sessions,
//! not only on the terminal.

use wasixv2_abi::{
    GuestPtr, GuestPtrMut, IoctlRequest, Signal, TcflowAction, TcflushQueue, Termios, Termios2,
    TermiosCflag, Winsize,
};
use wasixv2_syscalls::{Caller, Errno, SysErr, SysResult, read_value, write_value};
use wasixv2_vfs::Cap;
use wasixv2_wait::Interrupt;

use super::{TtyFile, ldisc};
use crate::{Kernel, file::OpenFile, sync::MutexExt};

/// `N_TTY`, the only line discipline.
const N_TTY: i32 = 0;

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
        let memory = caller.memory();
        let task = caller.task();
        let process = self.process(task);
        let admin = || self.cred(caller).capable(Cap::SysAdmin);
        let put_int = async |value: i32| -> SysResult<i32> {
            write_value(memory, GuestPtrMut::new(arg), &value).await?;
            Ok(0)
        };
        match request {
            // ---- termios (`tty_mode_ioctl`) ----
            IoctlRequest::TCGETS => {
                write_value(memory, GuestPtrMut::new(arg), &tty.termios()).await?;
                Ok(0)
            }
            IoctlRequest::TCGETS2 => {
                let termios = tty.termios();
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
                self.tty_set_termios(caller, tty_file, termios, wait, flush)
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
                self.tty_set_termios(caller, tty_file, termios, wait, flush)
                    .await
            }
            IoctlRequest::TIOCGSOFTCAR => {
                let clocal = tty.termios().c_cflag.0 & TermiosCflag::CLOCAL.0 != 0;
                put_int(i32::from(clocal)).await
            }
            IoctlRequest::TIOCSSOFTCAR => {
                let on: i32 = read_value(memory, GuestPtr::new(arg)).await?;
                let mut termios = tty.termios();
                match on {
                    0 => termios.c_cflag.0 &= !TermiosCflag::CLOCAL.0,
                    _ => termios.c_cflag.0 |= TermiosCflag::CLOCAL.0,
                }
                tty.set_termios(termios);
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
                match TcflushQueue(arg as i32) {
                    TcflushQueue::TCIFLUSH | TcflushQueue::TCIOFLUSH => {
                        tty.ldisc.lock_or_recover().flush_input();
                        tty.driver.unthrottle();
                        Ok(0)
                    }
                    TcflushQueue::TCOFLUSH => Ok(0),
                    _ => Err(Errno::EINVAL.into()),
                }
            }
            IoctlRequest::TCXONC => {
                tty.check_change(task, Signal::SIGTTOU)?;
                let started = match TcflowAction(arg as i32) {
                    TcflowAction::TCOOFF => tty.ldisc.lock_or_recover().tco_stop(true),
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
                    tty.write_wait.wake(wasixv2_wait::PollMask::WRITABLE);
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
                write_value(memory, GuestPtrMut::new(arg), &tty.winsize()).await?;
                Ok(0)
            }
            IoctlRequest::TIOCSWINSZ => {
                let winsize: Winsize = read_value(memory, GuestPtr::new(arg)).await?;
                tty.resize(winsize);
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
                let device = tty.device();
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
                tty.set_ctty_ioctl(&process, readable, arg == 1, admin())?;
                Ok(0)
            }
            IoctlRequest::TIOCGPGRP => {
                if !tty.is_ctty_of(&process) {
                    return Err(Errno::ENOTTY.into());
                }
                put_int(tty.pgrp().map_or(0, |pgrp| pgrp as i32)).await
            }
            IoctlRequest::TIOCSPGRP => {
                match tty.check_change(task, Signal::SIGTTOU) {
                    Err(SysErr::Errno(Errno::EIO)) => return Err(Errno::ENOTTY.into()),
                    result => result?,
                }
                let pgrp: i32 = read_value(memory, GuestPtr::new(arg)).await?;
                tty.set_pgrp(&process, pgrp)?;
                Ok(0)
            }
            IoctlRequest::TIOCGSID => {
                if !tty.is_ctty_of(&process) {
                    return Err(Errno::ENOTTY.into());
                }
                let sid = tty.session().ok_or(Errno::ENOTTY)?;
                put_int(sid as i32).await
            }
            _ => Err(Errno::ENOTTY.into()),
        }
    }

    /// `set_termios`: from a background group only as `tty_check_change` allows; `wait` lets
    /// the output under way finish first, and `flush` drops unread input too.
    async fn tty_set_termios(
        &self,
        caller: &Caller<'_>,
        tty_file: &TtyFile,
        termios: Termios,
        wait: bool,
        flush: bool,
    ) -> SysResult<i32> {
        let tty = tty_file.tty();
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
            tty.ldisc.lock_or_recover().flush_input();
            tty.driver.unthrottle();
        }
        tty.set_termios(termios);
        Ok(0)
    }
}
