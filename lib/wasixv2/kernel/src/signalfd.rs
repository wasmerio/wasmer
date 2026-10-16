//! Signal descriptors, as Linux's `fs/signalfd.c`: reading one dequeues pending signals in its
//! mask as `signalfd_siginfo` records, and it polls readable while such a signal is pending.
//!
//! Linux reads the signals of whichever task reads or polls. Objects here do not learn the
//! calling task, so a signalfd serves the task that created it and that task's thread group;
//! the two differ only for signals sent to one thread and read from another, and in a child
//! that inherited the descriptor.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use wasixv2_abi::{
    ConstSigsetPtr, GuestLayout, OpenFlags, SiCode, Siginfo, Signal, SignalfdFlags,
    SignalfdSiginfo, Sigset,
};
use wasixv2_syscalls::{Caller, Errno, Fd, LocalFuture, SysErr, SysResult, read_value};
use wasixv2_wait::{PollMask, PollTable, Pollable, poll_wait, wait_event};

use crate::{
    Kernel,
    file::{Backing, FileObject, Io, OpenFile},
    signal::TaskSignals,
    sigset_bits,
};

/// SIGKILL and SIGSTOP are never read through a signalfd.
const UNREADABLE: u64 = (1 << (Signal::SIGKILL.0 - 1)) | (1 << (Signal::SIGSTOP.0 - 1));

pub(crate) struct SignalFd {
    signals: Arc<TaskSignals>,
    /// The signals it reports (`ctx->sigmask`).
    mask: AtomicU64,
}

impl SignalFd {
    fn mask(&self) -> u64 {
        self.mask.load(Ordering::Relaxed)
    }

    /// `signalfd_read`: whole records only; waits for the first unless non-blocking, then takes
    /// whatever else is pending without waiting.
    async fn read_signals(&self, io: Io<'_>, buf: &mut [u8]) -> SysResult<usize> {
        let room = buf.len() / SignalfdSiginfo::SIZE;
        if room == 0 {
            return Err(Errno::EINVAL.into());
        }
        let first = || match self.signals.dequeue_from(self.mask()) {
            Some(info) => Some(Ok(info)),
            None => io.nonblocking().then_some(Err(Errno::EAGAIN)),
        };
        let info = wait_event(self.signals.sent(), first, io.interrupt)
            .await
            .map_err(|_| SysErr::Restart)??;
        let mut records = buf.chunks_exact_mut(SignalfdSiginfo::SIZE);
        copy_info(&info).write_to(records.next().expect("room for one"));
        let mut n = 1;
        for record in records {
            let Some(info) = self.signals.dequeue_from(self.mask()) else {
                break;
            };
            copy_info(&info).write_to(record);
            n += 1;
        }
        Ok(n * SignalfdSiginfo::SIZE)
    }
}

impl Pollable for SignalFd {
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, self.signals.sent());
        match self.signals.has_pending(self.mask()) {
            true => PollMask::READABLE,
            false => PollMask::empty(),
        }
    }
}

impl FileObject for SignalFd {
    fn read<'a>(&'a self, io: Io<'a>, buf: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(self.read_signals(io, buf))
    }

    fn write<'a>(&'a self, _: Io<'a>, _: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async { Err(Errno::EINVAL.into()) })
    }
}

/// `siginfo_layout`: which union member of a siginfo is filled.
enum Layout {
    Kill,
    Timer,
    Poll,
    Fault,
    Child,
    Rt,
    Sys,
}

fn layout(sig: i32, code: i32) -> Layout {
    const NSIGPOLL: i32 = 6;
    let kernel = SiCode::SI_KERNEL.0;
    if code > 0 && code < kernel {
        return match Signal(sig) {
            Signal::SIGILL
            | Signal::SIGFPE
            | Signal::SIGSEGV
            | Signal::SIGBUS
            | Signal::SIGTRAP => Layout::Fault,
            Signal::SIGCHLD => Layout::Child,
            Signal::SIGSYS => Layout::Sys,
            _ if code <= NSIGPOLL => Layout::Poll,
            _ => Layout::Kill,
        };
    }
    match SiCode(code) {
        SiCode::SI_TIMER => Layout::Timer,
        SiCode::SI_SIGIO => Layout::Poll,
        _ if code < 0 => Layout::Rt,
        _ => Layout::Kill,
    }
}

/// `signalfd_copyinfo`: the record for one dequeued signal, from the wasm32 siginfo union.
fn copy_info(info: &Siginfo) -> SignalfdSiginfo {
    let word = |at: usize| u32::from_le_bytes(info.sifields[at..at + 4].try_into().unwrap());
    let mut out = SignalfdSiginfo {
        ssi_signo: info.si_signo as u32,
        ssi_errno: info.si_errno,
        ssi_code: info.si_code,
        ..SignalfdSiginfo::ZERO
    };
    match layout(info.si_signo, info.si_code) {
        Layout::Kill => (out.ssi_pid, out.ssi_uid) = (word(0), word(4)),
        Layout::Timer => {
            (out.ssi_tid, out.ssi_overrun) = (word(0), word(4));
            (out.ssi_ptr, out.ssi_int) = (u64::from(word(8)), word(8) as i32);
        }
        Layout::Poll => (out.ssi_band, out.ssi_fd) = (word(0), word(4) as i32),
        Layout::Fault => out.ssi_addr = u64::from(word(0)),
        Layout::Child => {
            (out.ssi_pid, out.ssi_uid, out.ssi_status) = (word(0), word(4), word(8) as i32);
            (out.ssi_utime, out.ssi_stime) = (u64::from(word(12)), u64::from(word(16)));
        }
        Layout::Rt => {
            (out.ssi_pid, out.ssi_uid) = (word(0), word(4));
            (out.ssi_ptr, out.ssi_int) = (u64::from(word(8)), word(8) as i32);
        }
        Layout::Sys => {
            (out.ssi_call_addr, out.ssi_syscall) = (u64::from(word(0)), word(4) as i32);
            out.ssi_arch = word(8);
        }
    }
    out
}

impl Kernel {
    /// `do_signalfd4`: a new signalfd for `ufd == -1`, else a new mask for an existing one.
    pub(crate) async fn sys_signalfd4(
        &self,
        caller: &Caller<'_>,
        ufd: Fd,
        user_mask: ConstSigsetPtr,
        sizemask: u32,
        flags: SignalfdFlags,
    ) -> SysResult<Fd> {
        let allowed = SignalfdFlags::SFD_CLOEXEC | SignalfdFlags::SFD_NONBLOCK;
        if !(flags & !allowed).is_empty() || sizemask as usize != Sigset::SIZE {
            return Err(Errno::EINVAL.into());
        }
        let mask = sigset_bits(read_value(caller.memory(), user_mask).await?) & !UNREADABLE;
        if ufd.0 != -1 {
            let file = self.file(caller.task(), ufd)?;
            let signalfd = file.downcast::<SignalFd>().ok_or(Errno::EINVAL)?;
            signalfd.mask.store(mask, Ordering::Relaxed);
            // Readers and pollers recheck against the new mask.
            signalfd.signals.sent().wake(PollMask::empty());
            return Ok(ufd);
        }
        let object = SignalFd {
            signals: self.interrupt(caller),
            mask: AtomicU64::new(mask),
        };
        let mut status = OpenFlags::O_RDWR;
        if flags.contains(SignalfdFlags::SFD_NONBLOCK) {
            status |= OpenFlags::O_NONBLOCK;
        }
        let file = OpenFile::new(
            Backing::Stream(Arc::new(object), self.pseudo.anon("[signalfd]")),
            status,
        );
        let [fd] =
            self.install_files(caller, [file], flags.contains(SignalfdFlags::SFD_CLOEXEC))?;
        Ok(fd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(sig: Signal, code: SiCode, words: [u32; 5]) -> Siginfo {
        let mut sifields = [0; 116];
        for (i, word) in words.iter().enumerate() {
            sifields[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        Siginfo {
            si_signo: sig.0,
            si_errno: 0,
            si_code: code.0,
            sifields,
        }
    }

    #[test]
    fn records_follow_the_siginfo_layout() {
        let kill = copy_info(&info(Signal::SIGUSR1, SiCode::SI_USER, [7, 1, 9, 0, 0]));
        assert_eq!((kill.ssi_pid, kill.ssi_uid, kill.ssi_int), (7, 1, 0));

        let queued = copy_info(&info(Signal(34), SiCode::SI_QUEUE, [7, 1, 42, 0, 0]));
        assert_eq!(
            (queued.ssi_pid, queued.ssi_int, queued.ssi_ptr),
            (7, 42, 42)
        );

        // CLD_EXITED is 1.
        let child = copy_info(&info(Signal::SIGCHLD, SiCode(1), [5, 1, 3, 10, 20]));
        assert_eq!((child.ssi_pid, child.ssi_status), (5, 3));
        assert_eq!((child.ssi_utime, child.ssi_stime), (10, 20));
    }
}
