//! Timer descriptors, as Linux's `fs/timerfd.c`.
//!
//! Expirations are counted from the clock whenever the timer is looked at, so reads and polls
//! are exact. A kernel timer wakes the descriptor's queue at the next expiry, so blocked readers
//! and pollers notice without polling. As on Linux, it stays disarmed once it fired until a read
//! takes the expirations, so a periodic timer nobody reads costs nothing.
//!
//! `TFD_TIMER_CANCEL_ON_SET` works as Linux's: a set of the realtime clock cancels an absolute
//! realtime timer with the flag, whose read then fails with `ECANCELED`. The wall clock is the
//! host's, so the kernel is not told of a set but sees one when the clock's offset from the
//! monotonic clock moved ([`ClockOffset`]): when the timer fires, is read, polled or set. A
//! read or an expiry that finds a set also re-derives the monotonic deadlines of the machine's
//! other realtime timers ([`Timer::clock_was_set`]).

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
    sync::MutexExt,
    time::{self, Clock, ClockOffset},
    timer::{self, Timer},
};

struct State {
    /// The next expiry on the timer's clock; `None` while disarmed.
    deadline: Option<Duration>,
    interval: Duration,
    /// Expirations not read yet.
    ticks: u64,
    /// The setting is an absolute one of a realtime timer with `TFD_TIMER_CANCEL_ON_SET`
    /// (`might_cancel`): a set of the clock cancels it.
    might_cancel: bool,
    moffs: Moffs,
}

/// What the timer knows of sets of the realtime clock (`timerfd_ctx::moffs`).
#[derive(Clone, Copy)]
enum Moffs {
    /// The clock's offset as last seen.
    At(ClockOffset),
    /// A set was seen and not reported yet (`KTIME_MAX`).
    Set,
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
        time::read(&*self.host, self.clock, None)
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

    /// `timerfd_clock_was_set` for this timer, at a look at it: when the realtime clock was set
    /// since the timer last saw its offset, a timer that cancels on a set is marked, and stays
    /// readable until a read reports the set. Returns whether this look found the set.
    fn clock_was_set(&self, state: &mut State) -> bool {
        let Moffs::At(seen) = state.moffs else {
            return false;
        };
        if !state.might_cancel || !seen.moved(&ClockOffset::read(&*self.host)) {
            return false;
        }
        state.moffs = Moffs::Set;
        true
    }

    /// `timerfd_canceled`: whether a set marked the timer. The mark goes, and the clock's
    /// offset is seen anew.
    fn canceled(&self, state: &mut State) -> bool {
        if !state.might_cancel || !matches!(state.moffs, Moffs::Set) {
            return false;
        }
        state.moffs = Moffs::At(ClockOffset::read(&*self.host));
        true
    }

    /// Unread expirations, or a set to report.
    fn readable(&self, state: &mut State) -> bool {
        let ticks = self.update(state);
        ticks || (state.might_cancel && matches!(state.moffs, Moffs::Set))
    }

    /// `timerfd_tmrproc`: wakes readers once an expiration is due, or a set of the clock
    /// canceled the timer. Until then (a realtime deadline the clock has not reached yet) the
    /// timer goes on.
    fn expired(&self) -> Option<Duration> {
        let mut state = self.state.lock_or_recover();
        let set = self.clock_was_set(&mut state);
        let next = match self.readable(&mut state) {
            true => None,
            false => state.deadline.map(|deadline| self.monotonic(deadline)),
        };
        drop(state);
        if next.is_none() {
            self.wait.wake(PollMask::READABLE);
        }
        if set {
            self.timer.clock_was_set();
        }
        next
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
            let (taken, set) = {
                let mut state = self.0.state.lock_or_recover();
                let set = self.0.clock_was_set(&mut state);
                (self.take_ticks(&mut state, io.nonblocking()), set)
            };
            if set {
                self.0.timer.clock_was_set();
            }
            taken
        };
        let ticks = wait_event(&self.0.wait, take, io.interrupt)
            .await
            .map_err(|_| SysErr::Restart)??;
        buf[..8].copy_from_slice(&ticks.to_ne_bytes());
        Ok(8)
    }

    /// The expirations a read takes, if any: `ECANCELED` once for a set of the clock, after
    /// which the ticks are gone and a timer that had fired is not re-armed, as the caller must
    /// look at the clock again anyway; else `EAGAIN` for a non-blocking read with nothing to
    /// take, or `None` to wait.
    fn take_ticks(&self, state: &mut State, nonblocking: bool) -> Option<Result<u64, Errno>> {
        if self.0.canceled(state) {
            if self.0.update(state) {
                state.deadline = None;
            }
            state.ticks = 0;
            self.0.rearm(state);
            return Some(Err(Errno::ECANCELED));
        }
        if !self.0.update(state) {
            return nonblocking.then_some(Err(Errno::EAGAIN));
        }
        let ticks = std::mem::take(&mut state.ticks);
        // `hrtimer_forward_now` and `hrtimer_restart`: on to the next expiry.
        self.0.rearm(state);
        Some(Ok(ticks))
    }
}

impl Pollable for TimerFd {
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, &self.0.wait);
        let mut state = self.0.state.lock_or_recover();
        // Polls run under other objects' locks; a set found here re-derives no other timer.
        self.0.clock_was_set(&mut state);
        match self.0.readable(&mut state) {
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
            // The alarm clocks need CAP_WAKE_ALARM. A machine never suspends, so they run as
            // their plain clocks do.
            ClockId::CLOCK_REALTIME_ALARM | ClockId::CLOCK_BOOTTIME_ALARM
                if !self.cred(caller).capable_init(wasixv2_vfs::Cap::WakeAlarm) =>
            {
                return Err(Errno::EPERM.into());
            }
            ClockId::CLOCK_REALTIME_ALARM => Clock::Realtime,
            ClockId::CLOCK_BOOTTIME_ALARM => Clock::Monotonic,
            _ => return Err(Errno::EINVAL.into()),
        };
        let object = TimerFd(Arc::new_cyclic(|inner: &Weak<Inner>| {
            let inner = inner.clone();
            let expired: timer::Callback =
                Box::new(move || inner.upgrade().and_then(|inner| inner.expired()));
            Inner {
                clock,
                host: self.host.clone(),
                state: Mutex::new(State {
                    deadline: None,
                    interval: Duration::ZERO,
                    ticks: 0,
                    might_cancel: false,
                    moffs: Moffs::At(ClockOffset::read(&*self.host)),
                }),
                wait: WaitQueue::new(),
                timer: match clock {
                    Clock::Realtime => self.timers.realtime_timer(expired),
                    _ => self.timers.timer(expired),
                },
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
        let absolute = flags.contains(TimerfdSettimeFlags::TFD_TIMER_ABSTIME);
        let (previous, set, canceled) = {
            let mut state = timer.state.lock_or_recover();
            // A set the old setting cancels on marks the timer, as it did when it happened.
            let set = timer.clock_was_set(&mut state);
            let previous = timer.setting(&mut state);
            // `timerfd_setup_cancel`. A set seen once the timer cancels on it is one after
            // this call (Linux keeps the offset of the timer's creation or its last canceled
            // read, and compares at the next set).
            state.might_cancel = timer.clock == Clock::Realtime
                && absolute
                && flags.contains(TimerfdSettimeFlags::TFD_TIMER_CANCEL_ON_SET);
            if state.might_cancel && matches!(state.moffs, Moffs::At(_)) {
                state.moffs = Moffs::At(ClockOffset::read(&*timer.host));
            }
            state.ticks = 0;
            state.interval = interval;
            state.deadline = match (value.is_zero(), absolute) {
                (true, _) => None,
                (false, true) => Some(value),
                (false, false) => timer.now().checked_add(value),
            };
            // `hrtimer_cancel`, then `hrtimer_start` for the new setting.
            timer.rearm(&state);
            // An unreported set fails the arming, which stands (`timerfd_setup`).
            let canceled = !value.is_zero() && timer.canceled(&mut state);
            (previous, set, canceled)
        };
        if set {
            timer.timer.clock_was_set();
        }
        if canceled {
            return Err(Errno::ECANCELED.into());
        }
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
        let setting = timer.setting(&mut timer.state.lock_or_recover());
        write_value(caller.memory(), out, &setting).await?;
        Ok(0)
    }
}
