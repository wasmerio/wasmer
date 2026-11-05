//! Process accounting between the execution layer and the kernel: the CPU time each task is
//! charged with, split into user and system time, and the thresholds past which the kernel has
//! CPU timers to run.
//!
//! The execution layer samples the host thread's CPU clock as a guest runs and charges the
//! task's [`CpuAccount`], as Linux's tick accounting charges `task_struct::utime` and `stime`
//! (`kernel/sched/cputime.c`). Every charge also goes to the thread group's [`GroupCpu`], so a
//! process's CPU clock reads one sum that includes its dead threads, as Linux's
//! `thread_group_cputimer` does while process timers run. A charge that takes either past an
//! [`Expiry`] tells the kernel to run the timers ([`CpuTime::cpu_timers`]), as
//! `run_posix_cpu_timers` runs only once `fastpath_timer_check` sees an expiry cache passed.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use crate::{TaskId, generated::Layered};

/// CPU time split as `struct task_cputime` splits it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CpuTimes {
    /// Time the guest ran its own code (`utime`).
    pub user: Duration,
    /// Time the kernel ran for it, inside syscalls (`stime`).
    pub system: Duration,
}

impl CpuTimes {
    /// User and system time together (`CPUCLOCK_PROF`, and `CPUCLOCK_SCHED` here).
    pub fn total(&self) -> Duration {
        self.user.saturating_add(self.system)
    }

    /// What the clock `base` reads.
    pub fn on(&self, base: CpuBase) -> Duration {
        match base {
            CpuBase::Prof => self.total(),
            CpuBase::Virt => self.user,
        }
    }

    pub fn saturating_add(self, other: Self) -> Self {
        Self {
            user: self.user.saturating_add(other.user),
            system: self.system.saturating_add(other.system),
        }
    }
}

/// The clocks a CPU timer runs on (`CPUCLOCK_PROF`, `CPUCLOCK_VIRT`). `CPUCLOCK_SCHED`, which
/// Linux counts in nanoseconds rather than ticks, counts the same time as `CPUCLOCK_PROF` here,
/// as both come from the same samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuBase {
    /// User and system time.
    Prof = 0,
    /// User time only.
    Virt = 1,
}

impl CpuBase {
    pub const ALL: [CpuBase; 2] = [CpuBase::Prof, CpuBase::Virt];
}

/// User and system time in nanoseconds.
#[derive(Debug, Default)]
struct Counters {
    user: AtomicU64,
    system: AtomicU64,
}

impl Counters {
    fn add(&self, user: Duration, system: Duration) -> CpuTimes {
        let add = |counter: &AtomicU64, d: Duration| {
            let d = nanos(d);
            from_nanos(counter.fetch_add(d, Ordering::Relaxed).saturating_add(d))
        };
        CpuTimes {
            user: add(&self.user, user),
            system: add(&self.system, system),
        }
    }

    fn read(&self) -> CpuTimes {
        CpuTimes {
            user: from_nanos(self.user.load(Ordering::Relaxed)),
            system: from_nanos(self.system.load(Ordering::Relaxed)),
        }
    }
}

fn nanos(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

fn from_nanos(n: u64) -> Duration {
    Duration::from_nanos(n)
}

/// The earliest expiry of the CPU timers on each [`CpuBase`] of a task or thread group
/// (`posix_cputimer_base::nextevt`), which the charges compare against.
#[derive(Debug)]
pub struct Expiry([AtomicU64; 2]);

impl Default for Expiry {
    fn default() -> Self {
        Self([AtomicU64::new(u64::MAX), AtomicU64::new(u64::MAX)])
    }
}

impl Expiry {
    /// The earliest expiry on `base`, `None` while no timer is on it.
    pub fn get(&self, base: CpuBase) -> Option<Duration> {
        let at = self.0[base as usize].load(Ordering::Relaxed);
        (at != u64::MAX).then(|| from_nanos(at))
    }

    /// Makes `at` the earliest expiry on `base`.
    pub fn set(&self, base: CpuBase, at: Option<Duration>) {
        let at = at.map_or(u64::MAX, |at| nanos(at).min(u64::MAX - 1));
        self.0[base as usize].store(at, Ordering::Relaxed);
    }

    /// Lowers the expiry on `base` to `at` if that is earlier, as arming a timer does.
    pub fn lower(&self, base: CpuBase, at: Duration) {
        let at = nanos(at).min(u64::MAX - 1);
        self.0[base as usize].fetch_min(at, Ordering::Relaxed);
    }

    /// Whether `times` reached an expiry (`task_cputimers_expired`).
    pub fn reached(&self, times: CpuTimes) -> bool {
        CpuBase::ALL
            .iter()
            .any(|&base| nanos(times.on(base)) >= self.0[base as usize].load(Ordering::Relaxed))
    }
}

/// The CPU time of a thread group: every charge to one of its tasks, so the dead ones count on
/// (`signal_struct`'s sums and its `thread_group_cputimer`), and the expiry cache of its
/// process-wide CPU timers (`signal_struct::posix_cputimers`).
#[derive(Debug, Default)]
pub struct GroupCpu {
    times: Counters,
    expiry: Expiry,
}

impl GroupCpu {
    pub fn times(&self) -> CpuTimes {
        self.times.read()
    }

    pub fn expiry(&self) -> &Expiry {
        &self.expiry
    }
}

/// The CPU time of one task (`task_struct::utime`, `stime`), the expiry cache of the timers on
/// its own CPU clock (`task_struct::posix_cputimers`), and its thread group's account.
#[derive(Debug)]
pub struct CpuAccount {
    times: Counters,
    expiry: Expiry,
    /// The host thread's CPU clock (in nanoseconds) at the last charge, from which the task's
    /// own syscalls can count the time not charged yet; `u64::MAX` while unknown.
    sampled: AtomicU64,
    group: Arc<GroupCpu>,
}

impl CpuAccount {
    /// A task's account in the thread group whose account is `group`.
    pub fn new(group: Arc<GroupCpu>) -> Self {
        Self {
            times: Counters::default(),
            expiry: Expiry::default(),
            sampled: AtomicU64::new(u64::MAX),
            group,
        }
    }

    /// An account of its own that nobody reads, for implementations that keep none.
    pub fn detached() -> Self {
        Self::new(Arc::default())
    }

    /// Charges `user` and `system` time to the task and its thread group
    /// (`account_user_time`, `account_system_time`). True when that took the task or the group
    /// past an expiry: [`CpuTime::cpu_timers`] has timers to run.
    pub fn charge(&self, user: Duration, system: Duration) -> bool {
        let task = self.times.add(user, system);
        let group = self.group.times.add(user, system);
        self.expiry.reached(task) || self.group.expiry.reached(group)
    }

    pub fn times(&self) -> CpuTimes {
        self.times.read()
    }

    pub fn expiry(&self) -> &Expiry {
        &self.expiry
    }

    pub fn group(&self) -> &Arc<GroupCpu> {
        &self.group
    }

    /// Records `at`, the host thread's CPU clock as of the last charge.
    pub fn set_sampled(&self, at: Duration) {
        self.sampled
            .store(nanos(at).min(u64::MAX - 1), Ordering::Relaxed);
    }

    /// The host thread's CPU clock as of the last charge, if the task was charged on a host
    /// with one.
    pub fn sampled(&self) -> Option<Duration> {
        let at = self.sampled.load(Ordering::Relaxed);
        (at != u64::MAX).then(|| from_nanos(at))
    }
}

/// The kernel's side of process accounting.
pub trait CpuTime {
    /// The account the execution layer charges `task`'s CPU time to. By default one nobody
    /// reads, for implementations that keep no accounts.
    fn cpu_account(&self, task: TaskId) -> Arc<CpuAccount> {
        let _ = task;
        Arc::new(CpuAccount::detached())
    }

    /// A charge to `task`'s account took it or its thread group past an expiry: runs the CPU
    /// timers of both that expired (`run_posix_cpu_timers`). What they send is work for `task`
    /// or its group, which the return to the guest that follows the charge takes.
    fn cpu_timers(&self, task: TaskId) {
        let _ = task;
    }
}

// By hand rather than generated with the rest of `Layered`: the layers see syscalls, and this
// is not one.
impl<L, S: CpuTime> CpuTime for Layered<L, S> {
    fn cpu_account(&self, task: TaskId) -> Arc<CpuAccount> {
        self.inner.cpu_account(task)
    }

    fn cpu_timers(&self, task: TaskId) {
        self.inner.cpu_timers(task)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn charges_add_up_per_task_and_per_group() {
        let group = Arc::new(GroupCpu::default());
        let (a, b) = (
            CpuAccount::new(group.clone()),
            CpuAccount::new(group.clone()),
        );
        assert!(!a.charge(2 * MS, MS));
        assert!(!b.charge(3 * MS, Duration::ZERO));
        assert_eq!(
            a.times(),
            CpuTimes {
                user: 2 * MS,
                system: MS
            }
        );
        assert_eq!(b.times().total(), 3 * MS);
        assert_eq!(
            group.times(),
            CpuTimes {
                user: 5 * MS,
                system: MS
            }
        );
        assert_eq!(group.times().on(CpuBase::Virt), 5 * MS);
        assert_eq!(group.times().on(CpuBase::Prof), 6 * MS);
    }

    #[test]
    fn a_charge_reports_the_expiry_it_reaches() {
        let group = Arc::new(GroupCpu::default());
        let (a, b) = (
            CpuAccount::new(group.clone()),
            CpuAccount::new(group.clone()),
        );
        // The group's user time reaches 4 ms only with both tasks' charges.
        group.expiry().set(CpuBase::Virt, Some(4 * MS));
        assert!(!a.charge(2 * MS, 5 * MS));
        assert!(b.charge(2 * MS, Duration::ZERO));
        group.expiry().set(CpuBase::Virt, None);
        assert!(!b.charge(MS, Duration::ZERO));
        // A task's own expiry counts its own time only.
        a.expiry().set(CpuBase::Prof, Some(10 * MS));
        assert!(!a.charge(Duration::ZERO, 2 * MS));
        assert!(a.charge(Duration::ZERO, MS));
        a.expiry().lower(CpuBase::Prof, MS);
        assert_eq!(a.expiry().get(CpuBase::Prof), Some(MS));
        a.expiry().lower(CpuBase::Prof, 5 * MS);
        assert_eq!(a.expiry().get(CpuBase::Prof), Some(MS));
    }

    #[test]
    fn sampled_is_unknown_until_set() {
        let account = CpuAccount::detached();
        assert_eq!(account.sampled(), None);
        account.set_sampled(7 * MS);
        assert_eq!(account.sampled(), Some(7 * MS));
    }
}
