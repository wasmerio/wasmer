//! Timer descriptors, as Linux's `fs/timerfd.c`.
//!
//! Expirations are counted from the clock whenever the timer is looked at, so reads and polls
//! are exact. A kernel timer wakes the descriptor's queue at the next expiry, so blocked readers
//! and pollers notice without polling. As on Linux, it stays disarmed once it fired until a read
//! takes the expirations, so a periodic timer nobody reads costs nothing.

use std::{
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

use wasixv2_abi::{
    ClockId, ConstKernelItimerspecPtr, KernelItimerspec, KernelItimerspecPtr, OpenFlags,
    TimerfdFlags, TimerfdSettimeFlags,
};
use wasixv2_host::Host;
use wasixv2_syscalls::{
    Caller, Errno, Fd, LocalFuture, SysErr, SysResult, read_value, write_value,
};
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, poll_wait, wait_event};

use crate::{
    Kernel,
    file::{Backing, FileObject, Io, OpenFile},
    time::{self, Clock},
    timer::{self, Timer},
};

#[derive(Default)]
struct State {
    /// The next expiry on the timer's clock; `None` while disarmed.
    deadline: Option<Duration>,
    interval: Duration,
    /// Expirations not read yet.
    ticks: u64,
}

struct Inner {
    clock: Clock,
    host: Arc<dyn Host>,
    state: Mutex<State>,
    wait: WaitQueue,
    /// Fires at `deadline` while no expiration is unread.
    timer: Timer,
}

pub(crate) struct TimerFd(Arc<Inner>);

impl Inner {
    fn now(&self) -> Duration {
        time::read(&*self.host, self.clock, Duration::ZERO)
    }

    /// Counts the expirations up to now; returns whether there are unread ones.
    fn update(&self, state: &mut State) -> bool {
        let now = self.now();
        if let Some(deadline) = state.deadline
            && deadline <= now
        {
            if state.interval.is_zero() {
                state.ticks = state.ticks.saturating_add(1);
                state.deadline = None;
            } else {
                let (ticks, next) = timer::forward(deadline, state.interval, now);
                state.ticks = state.ticks.saturating_add(ticks);
                state.deadline = next;
            }
        }
        state.ticks > 0
    }

    /// `deadline` on the host's monotonic clock, which kernel timers run on. A realtime
    /// deadline is checked again when the timer fires, since that clock can move.
    fn monotonic(&self, deadline: Duration) -> Duration {
        match self.clock {
            Clock::Realtime => {
                let left = deadline.saturating_sub(self.host.realtime());
                self.host.monotonic().saturating_add(left)
            }
            _ => deadline,
        }
    }

    /// Arms the kernel timer for the next expiry while none is unread.
    fn rearm(&self, state: &State) {
        match state.deadline {
            Some(deadline) if state.ticks == 0 => self.timer.start(self.monotonic(deadline)),
            _ => self.timer.cancel(),
        }
    }

    /// `timerfd_tmrproc`: wakes readers once an expiration is due. Until then (a realtime
    /// deadline the clock has not reached yet) the timer goes on.
    fn expired(&self) -> Option<Duration> {
        let mut state = self.state.lock().unwrap();
        if self.update(&mut state) {
            drop(state);
            self.wait.wake(PollMask::READABLE);
            return None;
        }
        state.deadline.map(|deadline| self.monotonic(deadline))
    }

    /// The current setting, as `timerfd_gettime` reports it.
    fn setting(&self, state: &mut State) -> KernelItimerspec {
        self.update(state);
        let left = state.deadline.map_or(Duration::ZERO, |d| {
            d.saturating_sub(self.now()).max(Duration::from_nanos(1))
        });
        KernelItimerspec {
            it_interval: time::to_timespec(state.interval),
            it_value: time::to_timespec(left),
        }
    }
}

impl TimerFd {
    async fn read_ticks(&self, io: Io<'_>, buf: &mut [u8]) -> SysResult<usize> {
        if buf.len() < 8 {
            return Err(Errno::EINVAL.into());
        }
        let take = || {
            let mut state = self.0.state.lock().unwrap();
            if !self.0.update(&mut state) {
                return io.nonblocking().then_some(Err(Errno::EAGAIN));
            }
            let ticks = std::mem::take(&mut state.ticks);
            // `hrtimer_forward_now` and `hrtimer_restart`: on to the next expiry.
            self.0.rearm(&state);
            Some(Ok(ticks))
        };
        let ticks = wait_event(&self.0.wait, take, io.interrupt)
            .await
            .map_err(|_| SysErr::Restart)??;
        buf[..8].copy_from_slice(&ticks.to_ne_bytes());
        Ok(8)
    }
}

impl Pollable for TimerFd {
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, &self.0.wait);
        let mut state = self.0.state.lock().unwrap();
        match self.0.update(&mut state) {
            true => PollMask::READABLE,
            false => PollMask::empty(),
        }
    }
}

impl FileObject for TimerFd {
    fn read<'a>(&'a self, io: Io<'a>, buf: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(self.read_ticks(io, buf))
    }

    fn write<'a>(&'a self, _: Io<'a>, _: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async { Err(Errno::EINVAL.into()) })
    }
}

impl Kernel {
    pub(crate) async fn sys_timerfd_create(
        &self,
        caller: &Caller<'_>,
        clockid: ClockId,
        flags: TimerfdFlags,
    ) -> SysResult<Fd> {
        let allowed = TimerfdFlags::TFD_NONBLOCK | TimerfdFlags::TFD_CLOEXEC;
        if !(flags & !allowed).is_empty() {
            return Err(Errno::EINVAL.into());
        }
        let clock = match clockid {
            ClockId::CLOCK_REALTIME => Clock::Realtime,
            ClockId::CLOCK_MONOTONIC | ClockId::CLOCK_BOOTTIME => Clock::Monotonic,
            // The alarm clocks need CAP_WAKE_ALARM.
            ClockId::CLOCK_REALTIME_ALARM | ClockId::CLOCK_BOOTTIME_ALARM => {
                return Err(Errno::EPERM.into());
            }
            _ => return Err(Errno::EINVAL.into()),
        };
        let object = TimerFd(Arc::new_cyclic(|inner: &Weak<Inner>| {
            let inner = inner.clone();
            let expired = move || inner.upgrade().and_then(|inner| inner.expired());
            Inner {
                clock,
                host: self.host.clone(),
                state: Mutex::default(),
                wait: WaitQueue::new(),
                timer: self.timers.timer(Box::new(expired)),
            }
        }));
        let mut status = OpenFlags::O_RDWR;
        if flags.contains(TimerfdFlags::TFD_NONBLOCK) {
            status |= OpenFlags::O_NONBLOCK;
        }
        let file = Arc::new(OpenFile::new(
            Backing::Stream(Arc::new(object), self.pseudo.anon("[timerfd]")),
            status,
        ));
        self.install(caller, file, flags.contains(TimerfdFlags::TFD_CLOEXEC))
    }

    fn timerfd(&self, caller: &Caller<'_>, fd: Fd) -> Result<Arc<OpenFile>, Errno> {
        let file = self.file(caller.task(), fd)?;
        match file.downcast::<TimerFd>() {
            Some(_) => Ok(file),
            None => Err(Errno::EINVAL),
        }
    }

    pub(crate) async fn sys_timerfd_settime(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        flags: TimerfdSettimeFlags,
        new: ConstKernelItimerspecPtr,
        old: KernelItimerspecPtr,
    ) -> SysResult<i32> {
        let new = read_value(caller.memory(), new).await?;
        let allowed =
            TimerfdSettimeFlags::TFD_TIMER_ABSTIME | TimerfdSettimeFlags::TFD_TIMER_CANCEL_ON_SET;
        if !(flags & !allowed).is_empty() {
            return Err(Errno::EINVAL.into());
        }
        let value = time::from_timespec(new.it_value)?;
        let interval = time::from_timespec(new.it_interval)?;
        let file = self.timerfd(caller, fd)?;
        let timer = &file.downcast::<TimerFd>().unwrap().0;
        let previous = {
            let mut state = timer.state.lock().unwrap();
            let previous = timer.setting(&mut state);
            state.ticks = 0;
            state.interval = interval;
            state.deadline = match (
                value.is_zero(),
                flags.contains(TimerfdSettimeFlags::TFD_TIMER_ABSTIME),
            ) {
                (true, _) => None,
                (false, true) => Some(value),
                (false, false) => timer.now().checked_add(value),
            };
            // `hrtimer_cancel`, then `hrtimer_start` for the new setting.
            timer.rearm(&state);
            previous
        };
        if !old.is_null() {
            write_value(caller.memory(), old, &previous).await?;
        }
        Ok(0)
    }

    pub(crate) async fn sys_timerfd_gettime(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        out: KernelItimerspecPtr,
    ) -> SysResult<i32> {
        let file = self.timerfd(caller, fd)?;
        let timer = &file.downcast::<TimerFd>().unwrap().0;
        let setting = timer.setting(&mut timer.state.lock().unwrap());
        write_value(caller.memory(), out, &setting).await?;
        Ok(0)
    }
}
