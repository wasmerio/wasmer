//! Process accounting and the timers on CPU clocks, after Linux's `kernel/sched/cputime.c`,
//! `kernel/time/posix-cpu-timers.c` and `kernel/time/itimer.c`.
//!
//! The execution layer charges every task's [`CpuAccount`] with the CPU time its host thread
//! uses, and the account adds it to the thread group's [`GroupCpu`] too, so a process's time
//! counts its dead threads on. Reaping a child adds its time, and its children's, to the
//! parent's `cutime` and `cstime` ([`Children`], `wait_task_zombie`).
//!
//! Timers on CPU clocks (`ITIMER_VIRTUAL`, `ITIMER_PROF`, `RLIMIT_CPU`, POSIX timers and
//! `clock_nanosleep` on a CPU clock) are kept with the process whose clock they are on
//! ([`CpuTimers`], `posix_cputimers`), and fire as a charge passes their expiry: the charges
//! compare against the expiry caches the timers leave in the accounts, and a charge past one
//! runs [`Kernel::run_cpu_timers`] (`run_posix_cpu_timers`). No host timer is involved, so a
//! CPU timer fires at the first sample of a running task past its expiry.

use std::{
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

use wasixv2_abi::{RlimitResource, Signal};
use wasixv2_syscalls::{CpuAccount, CpuBase, CpuTimes, TaskId};
use wasixv2_wait::{PollMask, WaitQueue};

use crate::{Kernel, Process, posix_timer::PosixTimer};

/// One tick at `HZ=250`, which Linux adds to an interval timer's first expiry so it counts at
/// least one tick (`TICK_NSEC` in `set_cpu_itimer`) and reports as left of one about to fire.
pub(crate) const TICK: Duration = Duration::from_millis(4);

/// A CPU clock: whose time it counts, and which part of it.
#[derive(Clone)]
pub(crate) struct CpuClock {
    /// The process it counts the time of, or the process of the thread it counts.
    pub(crate) process: Arc<Process>,
    /// For a thread clock, the thread and its account.
    pub(crate) thread: Option<(TaskId, Arc<CpuAccount>)>,
    pub(crate) base: CpuBase,
}

impl CpuClock {
    /// What the clock reads, as of the last charges.
    pub(crate) fn now(&self) -> Duration {
        self.times().on(self.base)
    }

    fn times(&self) -> CpuTimes {
        match &self.thread {
            Some((_, account)) => account.times(),
            None => self.process.cpu.times(),
        }
    }

    fn account(&self) -> Option<&Arc<CpuAccount>> {
        self.thread.as_ref().map(|(_, account)| account)
    }
}

/// What the reaped children of a process used, themselves and their own reaped children
/// (`signal_struct`'s `cutime`, `cstime` and `cmaxrss`).
#[derive(Clone, Copy, Default)]
pub(crate) struct Children {
    pub(crate) times: CpuTimes,
    /// In KiB.
    pub(crate) maxrss: u64,
}

/// An interval timer on the process CPU clock (`struct cpu_itimer`).
#[derive(Clone, Copy, Default)]
struct Itimer {
    /// On the clock; zero while disarmed.
    expires: Duration,
    incr: Duration,
}

/// The timers on one process's CPU clocks and on its threads' (`posix_cputimers` of the
/// `signal_struct` and of each task), and the sleeps on them.
#[derive(Default)]
pub(crate) struct CpuTimers {
    state: Mutex<Armed>,
    /// Woken when a sleep on one of the clocks may be over.
    pub(crate) woken: WaitQueue,
}

#[derive(Default)]
struct Armed {
    entries: Vec<Entry>,
    /// `signal_struct::it`, indexed by [`CpuBase`]: `ITIMER_PROF` and `ITIMER_VIRTUAL`.
    itimers: [Itimer; 2],
    last_id: u64,
}

/// One timer or sleep on a CPU clock.
struct Entry {
    id: u64,
    /// The thread whose clock it is on, by its account; `None` for the process clock.
    thread: Option<Arc<CpuAccount>>,
    base: CpuBase,
    expires: Duration,
    what: What,
}

enum What {
    Timer(Weak<PosixTimer>),
    Sleep,
}

/// What a check of the timers found expired, to act on once the timers are unlocked.
#[derive(Default)]
struct Fired {
    signals: Vec<i32>,
    timers: Vec<(Arc<PosixTimer>, u64)>,
    wake: bool,
}

impl Entry {
    fn on(&self, thread: Option<&Arc<CpuAccount>>) -> bool {
        match (&self.thread, thread) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Process {
    /// Arms a timer or sleep on `clock` that expires when it reads `expires`; returns its id.
    fn arm_cpu(&self, clock: &CpuClock, expires: Duration, what: What) -> u64 {
        let mut armed = self.cpu_timers.state.lock().unwrap();
        armed.last_id += 1;
        let id = armed.last_id;
        let thread = clock.account().cloned();
        let cache = match &thread {
            Some(account) => account.expiry(),
            None => self.cpu.expiry(),
        };
        cache.lower(clock.base, expires);
        armed.entries.push(Entry {
            id,
            thread,
            base: clock.base,
            expires,
            what,
        });
        id
    }

    /// Arms `timer` on `clock` (`arm_timer`); returns the id [`Process::disarm_cpu`] takes.
    pub(crate) fn arm_cpu_timer(
        &self,
        clock: &CpuClock,
        expires: Duration,
        timer: Weak<PosixTimer>,
    ) -> u64 {
        self.arm_cpu(clock, expires, What::Timer(timer))
    }

    /// Takes the timer or sleep `id` off its clock, if it is still on it.
    pub(crate) fn disarm_cpu(&self, id: u64) {
        let mut armed = self.cpu_timers.state.lock().unwrap();
        let Some(at) = armed.entries.iter().position(|e| e.id == id) else {
            return;
        };
        let entry = armed.entries.swap_remove(at);
        self.update_expiry(&armed, entry.thread.as_ref());
    }

    /// Recomputes the expiry cache of the process clock, and of `thread`'s clock if given, from
    /// what is armed on them: the earliest expiry of each base (`posix_cputimers_*`), and on the
    /// process's `CPUCLOCK_PROF`, the interval timer and `RLIMIT_CPU`'s soft limit.
    fn update_expiry(&self, armed: &Armed, thread: Option<&Arc<CpuAccount>>) {
        let earliest = |on: Option<&Arc<CpuAccount>>, base: CpuBase| {
            armed
                .entries
                .iter()
                .filter(|e| e.base == base && e.on(on))
                .map(|e| e.expires)
                .min()
        };
        if let Some(account) = thread {
            for base in CpuBase::ALL {
                account.expiry().set(base, earliest(Some(account), base));
            }
        }
        let soft = self
            .resources
            .lock()
            .unwrap()
            .soft(RlimitResource::RLIMIT_CPU);
        let limit = (soft != u64::MAX).then(|| Duration::from_secs(soft));
        for base in CpuBase::ALL {
            let itimer = armed.itimers[base as usize].expires;
            let itimer = (!itimer.is_zero()).then_some(itimer);
            let rlimit = limit.filter(|_| base == CpuBase::Prof);
            let next = [earliest(None, base), itimer, rlimit]
                .into_iter()
                .flatten()
                .min();
            self.cpu.expiry().set(base, next);
        }
    }

    /// `update_rlimit_cpu`: `RLIMIT_CPU` changed, and with it the earliest expiry of the
    /// process's `CPUCLOCK_PROF`. Also for a new process, which inherits the limit
    /// (`posix_cputimers_group_init`).
    pub(crate) fn rlimit_cpu_changed(&self) {
        let armed = self.cpu_timers.state.lock().unwrap();
        self.update_expiry(&armed, None);
    }

    /// `set_cpu_itimer`: arms `ITIMER_PROF` or `ITIMER_VIRTUAL` (`base`) for `value` from now
    /// on the process clock, repeating every `interval`, or disarms it for a zero value.
    /// Returns the old setting as `get_cpu_itimer` reports it.
    pub(crate) fn set_itimer(
        &self,
        base: CpuBase,
        value: Duration,
        interval: Duration,
    ) -> (Duration, Duration) {
        let mut armed = self.cpu_timers.state.lock().unwrap();
        let now = self.cpu.times().on(base);
        let old = itimer_value(armed.itimers[base as usize], now);
        // A timer counts at least one tick (`nval += TICK_NSEC`).
        let expires = match value.is_zero() {
            true => Duration::ZERO,
            false => now.saturating_add(value).saturating_add(TICK),
        };
        armed.itimers[base as usize] = Itimer {
            expires,
            incr: interval,
        };
        self.update_expiry(&armed, None);
        old
    }

    /// `get_cpu_itimer`: the time left of `ITIMER_PROF` or `ITIMER_VIRTUAL` and its interval.
    pub(crate) fn get_itimer(&self, base: CpuBase) -> (Duration, Duration) {
        let armed = self.cpu_timers.state.lock().unwrap();
        itimer_value(armed.itimers[base as usize], self.cpu.times().on(base))
    }

    /// The timers of the process clock and of `thread`'s clock that the charges took past
    /// their expiry (`check_thread_timers`, `check_process_timers`), off their clocks, and the
    /// signals the interval timers and `RLIMIT_CPU` send; the expiry caches move on to what is
    /// left.
    fn expire_cpu(&self, thread: &Arc<CpuAccount>) -> Fired {
        let mut fired = Fired::default();
        let mut armed = self.cpu_timers.state.lock().unwrap();
        let (own, group) = (thread.times(), self.cpu.times());
        armed.entries.retain(|entry| {
            let times = match &entry.thread {
                None => group,
                Some(account) if Arc::ptr_eq(account, thread) => own,
                Some(_) => return true,
            };
            if times.on(entry.base) < entry.expires {
                return true;
            }
            match &entry.what {
                What::Timer(timer) => {
                    if let Some(timer) = timer.upgrade() {
                        fired.timers.push((timer, entry.id));
                    }
                }
                What::Sleep => fired.wake = true,
            }
            false
        });
        // `check_cpu_itimer`
        for (base, signo) in [
            (CpuBase::Prof, Signal::SIGPROF.0),
            (CpuBase::Virt, Signal::SIGVTALRM.0),
        ] {
            let it = &mut armed.itimers[base as usize];
            if !it.expires.is_zero() && group.on(base) >= it.expires {
                it.expires = match it.incr.is_zero() {
                    true => Duration::ZERO,
                    false => it.expires.saturating_add(it.incr),
                };
                fired.signals.push(signo);
            }
        }
        // `RLIMIT_CPU`, in seconds of `CPUCLOCK_PROF`: SIGKILL at the hard limit, and at the
        // soft limit SIGXCPU, once more each second the soft limit is then raised by.
        let mut resources = self.resources.lock().unwrap();
        let limit = resources.limits()[RlimitResource::RLIMIT_CPU.0 as usize];
        if limit.rlim_cur != u64::MAX {
            let reached = |secs: u64| group.total() >= Duration::from_secs(secs);
            if limit.rlim_max != u64::MAX && reached(limit.rlim_max) {
                fired.signals.push(Signal::SIGKILL.0);
            } else if reached(limit.rlim_cur) {
                fired.signals.push(Signal::SIGXCPU.0);
                resources.set_soft(RlimitResource::RLIMIT_CPU, limit.rlim_cur + 1);
            }
        }
        drop(resources);
        self.update_expiry(&armed, Some(thread));
        fired
    }
}

/// `get_cpu_itimer` of `it` at `now` on its clock: the time left, a tick for a timer about to
/// fire, and the interval.
fn itimer_value(it: Itimer, now: Duration) -> (Duration, Duration) {
    let left = match it.expires.is_zero() {
        true => Duration::ZERO,
        false if it.expires <= now => TICK,
        false => it.expires - now,
    };
    (left, it.incr)
}

/// A sleep on a CPU clock: on its clock while it lives.
pub(crate) struct CpuSleep<'a> {
    process: &'a Process,
    id: u64,
}

impl<'a> CpuSleep<'a> {
    /// Puts a sleep until `clock` reads `expires` on the clock; [`CpuTimers::woken`] wakes once
    /// it may be over.
    pub(crate) fn new(clock: &'a CpuClock, expires: Duration) -> Self {
        let process = &*clock.process;
        Self {
            id: process.arm_cpu(clock, expires, What::Sleep),
            process,
        }
    }
}

impl Drop for CpuSleep<'_> {
    fn drop(&mut self) {
        self.process.disarm_cpu(self.id);
    }
}

impl Kernel {
    /// The CPU clock of `clock`'s owner as the task of `caller` reads it: past the last charge,
    /// by the time its host thread used since, when it reads its own thread's clock or its
    /// process's on `CPUCLOCK_PROF`, as Linux's `cpu_clock_sample` includes the running task's
    /// time not yet accounted. Others' time is counted as of their last charge.
    pub(crate) fn cpu_clock_now(&self, task: TaskId, clock: &CpuClock) -> Duration {
        let charged = clock.now();
        if clock.base != CpuBase::Prof {
            return charged;
        }
        let account = match &clock.thread {
            Some((thread, account)) if *thread == task => account.clone(),
            Some(_) => return charged,
            None => match self.task_ref(task, |t| {
                Arc::ptr_eq(&t.process, &clock.process).then(|| t.cpu.clone())
            }) {
                Some(account) => account,
                None => return charged,
            },
        };
        match (self.host.thread_cpu_time(), account.sampled()) {
            (Some(now), Some(sampled)) => charged.saturating_add(now.saturating_sub(sampled)),
            _ => charged,
        }
    }

    /// `run_posix_cpu_timers` for `task`, whose last charge took it or its thread group past an
    /// expiry: fires the timers of both that expired, sends the signals of the interval timers
    /// and `RLIMIT_CPU`, and ends the sleeps that are over.
    pub(crate) fn run_cpu_timers(&self, task: TaskId) {
        let found = {
            let state = self.state.read_or_recover();
            let entry = state.tasks.get(&task);
            entry.map(|t| (t.process.clone(), t.cpu.clone(), t.signals.group().clone()))
        };
        let Some((process, account, signals)) = found else {
            return;
        };
        let fired = process.expire_cpu(&account);
        for signo in fired.signals {
            // `send_signal_locked(.., SEND_SIG_PRIV, .., PIDTYPE_TGID)`
            signals.send_kernel(signo);
        }
        for (timer, id) in fired.timers {
            timer.cpu_expired(id);
        }
        if fired.wake {
            process.cpu_timers.woken.wake(PollMask::empty());
        }
    }
}
