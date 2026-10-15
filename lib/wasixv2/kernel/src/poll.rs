//! `ppoll` and `pselect6`, as Linux's `fs/select.c`, plus the timeout and temporary signal
//! mask handling that epoll shares.

use std::time::Duration;

use wasixv2_abi::{
    ConstSigsetPtr, GuestLayout, KernelFdSetPtr, KernelTimespec, KernelTimespecPtr, PollEvents,
    Pollfd, PollfdArray, Sigset,
};
use wasixv2_syscalls::{Caller, Errno, Fd, SysErr, SysResult, read_value, write_value};
use wasixv2_wait::{Interrupt, PollMask, PollWaiter, Wakeup};

use crate::{Kernel, sigset_bits, time};

/// `RLIMIT_NOFILE`'s default soft limit, which bounds `nfds` for poll.
// TODO: the task's real limit once rlimits exist.
const NOFILE: u32 = 1024;

/// When a wait gives up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Deadline {
    Never,
    /// A zero timeout: poll once, never sleep.
    Now,
    /// On the monotonic clock.
    At(Duration),
}

impl Kernel {
    /// `poll_select_set_timeout`.
    pub(crate) fn deadline(&self, ts: KernelTimespec) -> Result<Deadline, Errno> {
        let timeout = time::from_timespec(ts)?;
        if timeout.is_zero() {
            return Ok(Deadline::Now);
        }
        Ok(Deadline::At(self.host.monotonic().saturating_add(timeout)))
    }

    /// A sleep until `deadline` the waiter can race against; `None` for `Never`.
    pub(crate) fn timer(&self, deadline: Deadline) -> Option<wasixv2_host::LocalFuture<'_, ()>> {
        match deadline {
            Deadline::Never => None,
            Deadline::Now => Some(Box::pin(async {})),
            Deadline::At(at) => Some(self.host.sleep_until(at)),
        }
    }

    /// `set_user_sigmask`: installs a temporary signal mask, returning the one to restore.
    pub(crate) async fn set_user_sigmask(
        &self,
        caller: &Caller<'_>,
        mask: ConstSigsetPtr,
        size: u32,
    ) -> SysResult<Option<u64>> {
        if mask.is_null() {
            return Ok(None);
        }
        if size as usize != Sigset::SIZE {
            return Err(Errno::EINVAL.into());
        }
        let set = sigset_bits(read_value(caller.memory(), mask).await?);
        Ok(Some(self.task_signals(caller.task()).replace_blocked(set)))
    }

    /// `restore_saved_sigmask_unless`. Nothing delivers signals yet, so the mask is restored
    /// at once even when the call was interrupted.
    // TODO: keep the temporary mask until delivery when interrupted (TIF_RESTORE_SIGMASK).
    pub(crate) fn restore_sigmask(
        &self,
        caller: &Caller<'_>,
        saved: Option<u64>,
        interrupted: bool,
    ) {
        if let Some(mask) = saved {
            self.task_signals(caller.task())
                .restore_blocked(mask, interrupted);
        }
    }

    /// `poll_select_finish`: restores the signal mask and writes the time left back to the
    /// guest's timespec.
    async fn poll_select_finish(
        &self,
        caller: &Caller<'_>,
        deadline: Deadline,
        tsp: KernelTimespecPtr,
        saved: Option<u64>,
        ret: SysResult<i32>,
    ) -> SysResult<i32> {
        let interrupted = matches!(ret, Err(SysErr::Restart | SysErr::RestartNoHand));
        self.restore_sigmask(caller, saved, interrupted);
        let Deadline::At(at) = deadline else {
            return ret;
        };
        let left = at.saturating_sub(self.host.monotonic());
        let written = write_value(caller.memory(), tsp, &time::to_timespec(left)).await;
        match (written, ret) {
            // The timespec could not be updated, so the call cannot restart.
            (Err(_), Err(SysErr::Restart | SysErr::RestartNoHand)) => Err(Errno::EINTR.into()),
            (_, ret) => ret,
        }
    }

    async fn read_deadline(
        &self,
        caller: &Caller<'_>,
        tsp: KernelTimespecPtr,
    ) -> SysResult<Deadline> {
        if tsp.is_null() {
            return Ok(Deadline::Never);
        }
        let ts = read_value(caller.memory(), wasixv2_abi::GuestPtr::new(tsp.addr())).await?;
        Ok(self.deadline(ts)?)
    }

    /// `ppoll`.
    pub(crate) async fn sys_ppoll(
        &self,
        caller: &Caller<'_>,
        ufds: PollfdArray,
        nfds: u32,
        tsp: KernelTimespecPtr,
        sigmask: ConstSigsetPtr,
        sigsetsize: u32,
    ) -> SysResult<i32> {
        let deadline = self.read_deadline(caller, tsp).await?;
        let saved = self.set_user_sigmask(caller, sigmask, sigsetsize).await?;
        let ret = self.do_sys_poll(caller, ufds, nfds, deadline).await;
        self.poll_select_finish(caller, deadline, tsp, saved, ret)
            .await
    }

    /// `do_sys_poll` and `do_poll`.
    async fn do_sys_poll(
        &self,
        caller: &Caller<'_>,
        ufds: PollfdArray,
        nfds: u32,
        deadline: Deadline,
    ) -> SysResult<i32> {
        if nfds > NOFILE {
            return Err(Errno::EINVAL.into());
        }
        let memory = caller.memory();
        let mut raw = vec![0; nfds as usize * Pollfd::SIZE];
        memory.read(ufds.addr(), &mut raw).await?;
        let mut fds: Vec<Pollfd> = raw
            .chunks_exact(Pollfd::SIZE)
            .filter_map(Pollfd::read_from)
            .collect();

        let interrupt = self.interrupt(caller);
        let mut waiter = PollWaiter::new();
        let mut timer = self.timer(deadline);
        let mut timed_out = deadline == Deadline::Now;
        let mut register = !timed_out;
        let count = loop {
            let mut count = 0;
            for pollfd in &mut fds {
                let mask = self.do_pollfd(caller, pollfd, &mut waiter, register);
                if !mask.is_empty() {
                    count += 1;
                    register = false;
                }
            }
            register = false;
            if count == 0 && interrupt.pending() {
                return Err(SysErr::RestartNoHand);
            }
            if count > 0 || timed_out {
                break count;
            }
            let timer = timer.as_mut().map(|timer| timer.as_mut());
            match waiter.sleep(&*interrupt, timer).await {
                Wakeup::Woken(_) => {}
                Wakeup::TimedOut => timed_out = true,
                Wakeup::Interrupted => return Err(SysErr::RestartNoHand),
            }
        };
        drop(waiter);

        // Only `revents` is written back, as Linux does.
        for (i, pollfd) in fds.iter().enumerate() {
            let at = ufds.addr() + (i * Pollfd::SIZE) as u32 + 6;
            memory.write(at, &pollfd.revents.0.to_le_bytes()).await?;
        }
        Ok(count)
    }

    /// `do_pollfd`: negative descriptors are skipped, closed ones report `POLLNVAL`, and
    /// `POLLERR` and `POLLHUP` are always reported.
    fn do_pollfd(
        &self,
        caller: &Caller<'_>,
        pollfd: &mut Pollfd,
        waiter: &mut PollWaiter,
        register: bool,
    ) -> PollMask {
        pollfd.revents = PollEvents::empty();
        if pollfd.fd.0 < 0 {
            return PollMask::empty();
        }
        let mask = match self.file(caller.task(), pollfd.fd) {
            Err(_) => PollMask::NVAL,
            Ok(file) => {
                let filter = PollMask::from(pollfd.events) | PollMask::ERR | PollMask::HUP;
                waiter.poll(&*file, filter, register) & filter
            }
        };
        pollfd.revents = mask.into();
        mask
    }

    /// `pselect6`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn sys_pselect6(
        &self,
        caller: &Caller<'_>,
        n: i32,
        inp: KernelFdSetPtr,
        outp: KernelFdSetPtr,
        exp: KernelFdSetPtr,
        tsp: KernelTimespecPtr,
        sig: wasixv2_abi::ConstSigsetArgpackPtr,
    ) -> SysResult<i32> {
        let deadline = self.read_deadline(caller, tsp).await?;
        let saved = match sig.is_null() {
            true => None,
            false => {
                let pack = read_value(caller.memory(), sig).await?;
                self.set_user_sigmask(caller, pack.p, pack.size).await?
            }
        };
        let ret = self
            .core_sys_select(caller, n, [inp, outp, exp], deadline)
            .await;
        self.poll_select_finish(caller, deadline, tsp, saved, ret)
            .await
    }

    /// `core_sys_select` and `do_select`.
    async fn core_sys_select(
        &self,
        caller: &Caller<'_>,
        n: i32,
        sets: [KernelFdSetPtr; 3],
        deadline: Deadline,
    ) -> SysResult<i32> {
        let n = usize::try_from(n).map_err(|_| Errno::EINVAL)?;
        let n = n.min(self.files(caller.task()).lock().unwrap().max_fds());
        let bytes = n.div_ceil(32) * 4;
        let mut input = [vec![0u8; bytes], vec![0u8; bytes], vec![0u8; bytes]];
        for (set, ptr) in input.iter_mut().zip(sets) {
            if !ptr.is_null() {
                caller.memory().read(ptr.addr(), set).await?;
            }
        }
        let bit = |set: &[u8], fd: usize| set[fd / 8] & (1 << (fd % 8)) != 0;
        let wanted: Vec<(usize, [bool; 3])> = (0..n)
            .map(|fd| (fd, [0, 1, 2].map(|i| bit(&input[i], fd))))
            .filter(|(_, want)| want.iter().any(|&w| w))
            .collect();

        // `max_select_fd`: every descriptor asked about must be open.
        for (fd, _) in &wanted {
            self.file(caller.task(), Fd(*fd as i32))?;
        }

        let in_set = PollMask::READABLE | PollMask::RDBAND | PollMask::HUP | PollMask::ERR;
        let out_set = PollMask::WRITABLE | PollMask::WRBAND | PollMask::ERR;
        let ex_set = PollMask::PRI;
        let keys = [in_set, out_set, ex_set];

        let interrupt = self.interrupt(caller);
        let mut waiter = PollWaiter::new();
        let mut timer = self.timer(deadline);
        let mut timed_out = deadline == Deadline::Now;
        let mut register = !timed_out;
        let mut result = [vec![0u8; bytes], vec![0u8; bytes], vec![0u8; bytes]];
        let count = loop {
            let mut count = 0;
            // Looked up again on every pass, as Linux does.
            for (fd, want) in &wanted {
                let Ok(file) = self.file(caller.task(), Fd(*fd as i32)) else {
                    continue;
                };
                let key = (0..3)
                    .filter(|&i| want[i])
                    .fold(PollMask::empty(), |key, i| key | keys[i]);
                let mask = waiter.poll(&*file, key, register);
                for i in 0..3 {
                    if want[i] && mask.intersects(keys[i]) {
                        result[i][fd / 8] |= 1 << (fd % 8);
                        count += 1;
                        register = false;
                    }
                }
            }
            register = false;
            if count > 0 || timed_out {
                break count;
            }
            if interrupt.pending() {
                return Err(SysErr::RestartNoHand);
            }
            let timer = timer.as_mut().map(|timer| timer.as_mut());
            match waiter.sleep(&*interrupt, timer).await {
                Wakeup::Woken(_) => {}
                Wakeup::TimedOut => timed_out = true,
                Wakeup::Interrupted => return Err(SysErr::RestartNoHand),
            }
        };
        drop(waiter);

        for (set, ptr) in result.iter().zip(sets) {
            if !ptr.is_null() {
                caller.memory().write(ptr.addr(), set).await?;
            }
        }
        Ok(count)
    }
}
