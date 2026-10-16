//! Timer descriptors, as Linux's `fs/timerfd.c`.
//!
//! Expirations are counted from the clock whenever the timer is looked at, so reads and polls
//! are exact. A host timer task wakes the timer's queue at each expiry, so blocked readers and
//! pollers notice without polling.

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
};

#[derive(Default)]
struct State {
    /// The next expiry on the timer's clock; `None` while disarmed.
    deadline: Option<Duration>,
    interval: Duration,
    /// Expirations not read yet.
    ticks: u64,
    /// Bumped on every `settime`, retiring the previous host timer task.
    generation: u64,
}

struct Inner {
    clock: Clock,
    host: Arc<dyn Host>,
    state: Mutex<State>,
    wait: WaitQueue,
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
                state.ticks += 1;
                state.deadline = None;
            } else {
                let missed = (now - deadline).as_nanos() / state.interval.as_nanos();
                state.ticks += 1 + missed as u64;
                state.deadline = Some(deadline + state.interval * (missed as u32 + 1));
            }
        }
        state.ticks > 0
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

/// Wakes the timer's readers at each expiry until the timer is re-armed or closed.
async fn run_timer(inner: Weak<Inner>, host: Arc<dyn Host>, generation: u64) {
    loop {
        let (clock, deadline) = {
            let Some(inner) = inner.upgrade() else { return };
            let state = inner.state.lock().unwrap();
            match state.deadline {
                Some(deadline) if state.generation == generation => (inner.clock, deadline),
                _ => return,
            }
        };
        match clock {
            Clock::Realtime => host.sleep(deadline.saturating_sub(host.realtime())).await,
            _ => host.sleep_until(deadline).await,
        }
        let Some(inner) = inner.upgrade() else { return };
        let ready = {
            let mut state = inner.state.lock().unwrap();
            if state.generation != generation {
                return;
            }
            inner.update(&mut state)
        };
        if ready {
            inner.wait.wake(PollMask::READABLE);
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
            Some(Ok(std::mem::take(&mut state.ticks)))
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
        let object = TimerFd(Arc::new(Inner {
            clock,
            host: self.host.clone(),
            state: Mutex::default(),
            wait: WaitQueue::new(),
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
        let (previous, generation, armed) = {
            let mut state = timer.state.lock().unwrap();
            let previous = timer.setting(&mut state);
            state.generation += 1;
            state.ticks = 0;
            state.interval = interval;
            state.deadline = match (
                value.is_zero(),
                flags.contains(TimerfdSettimeFlags::TFD_TIMER_ABSTIME),
            ) {
                (true, _) => None,
                (false, true) => Some(value),
                (false, false) => Some(timer.now() + value),
            };
            (previous, state.generation, state.deadline.is_some())
        };
        if armed {
            let task = run_timer(Arc::downgrade(timer), self.host.clone(), generation);
            self.host.executor().spawn_local(Box::pin(task));
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
        let setting = timer.setting(&mut timer.state.lock().unwrap());
        write_value(caller.memory(), out, &setting).await?;
        Ok(0)
    }
}
