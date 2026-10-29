//! Process accounting: charges the task an instance runs as with the CPU time its host thread
//! uses, split into user and system time.
//!
//! Read the host thread CPU clock at each syscall boundary: CPU spent between entry and exit
//! is system time, and CPU spent executing the guest between calls is user time. This follows
//! Linux's CONFIG_VIRT_CPU_ACCOUNTING_GEN boundary accounting (`vtime_user_enter` and
//! `vtime_user_exit` in `kernel/sched/cputime.c`), rather than estimating syscall costs from a
//! periodic sample, which can alias a periodic workload and erase its user time. Blocked time
//! and time the host does not run this thread never advance its CPU clock.
//!
//! Counter publication and timer checks are batched, as Linux's vtime accounting batches its
//! accumulated deltas. Preemption, task switches and exit publish immediately.
//!
//! Without a host thread CPU clock ([`Host::thread_cpu_time`]), only the monotonic time the
//! guest runs between syscalls is charged, as user time.

use std::{cell::Cell, sync::Arc, time::Duration};

use wasixv2_host::Host;
use wasixv2_syscalls::CpuAccount;

/// Publish accumulated deltas after this many syscalls, or at a preemption point.
pub(crate) const CHARGE_EVERY: u32 = 256;

/// The accounting state of one instance. Cells: the entry hook only reads the environment.
pub(crate) struct CpuSample {
    host: Arc<dyn Host>,
    /// The account of the task the instance runs as.
    account: Arc<CpuAccount>,
    /// Whether the host has a thread CPU clock to read.
    cpu_clock: bool,
    /// The clock at the last boundary: thread CPU time, or monotonic time without one.
    last: Cell<Duration>,
    /// Syscalls since the last publication.
    calls: Cell<u32>,
    /// Whether the instance is inside a syscall.
    entered: Cell<bool>,
    /// Actual execution time accumulated since the last publication.
    user: Cell<Duration>,
    system: Cell<Duration>,
}

impl CpuSample {
    /// Accounting from now, on the host thread that runs the instance, for `account`.
    pub(crate) fn new(host: Arc<dyn Host>, account: Arc<CpuAccount>) -> Self {
        let cpu = host.thread_cpu_time();
        let now = cpu.unwrap_or_else(|| host.monotonic());
        if let Some(cpu) = cpu {
            account.set_sampled(cpu);
        }
        Self {
            account,
            cpu_clock: cpu.is_some(),
            last: Cell::new(now),
            calls: Cell::new(0),
            entered: Cell::new(false),
            user: Cell::new(Duration::ZERO),
            system: Cell::new(Duration::ZERO),
            host,
        }
    }

    /// A syscall enters; account for guest execution since the preceding boundary.
    pub(crate) fn enter(&self) {
        self.accumulate();
        self.entered.set(true);
        self.calls.set(self.calls.get() + 1);
    }

    /// A syscall returns, before the return-to-guest path runs. True when publication took
    /// the task or its group past a CPU timer's expiry ([`CpuAccount::charge`]).
    pub(crate) fn exit(&self) -> bool {
        self.accumulate();
        self.entered.set(false);
        self.calls.get() >= CHARGE_EVERY && self.charge()
    }

    /// A preemption point, where the guest ran its own code until now. Publishes.
    pub(crate) fn preempt(&self) -> bool {
        self.sample()
    }

    /// Accounts for execution up to now and publishes accumulated time, including a syscall
    /// in progress. True when the task or group passed a CPU timer's expiry.
    pub(crate) fn sample(&self) -> bool {
        self.accumulate();
        self.charge()
    }

    fn accumulate(&self) {
        let now = if self.cpu_clock {
            let Some(now) = self.host.thread_cpu_time() else {
                return;
            };
            now
        } else {
            self.host.monotonic()
        };
        let used = now.saturating_sub(self.last.replace(now));
        if !self.entered.get() {
            self.user.set(self.user.get().saturating_add(used));
        } else if self.cpu_clock {
            self.system.set(self.system.get().saturating_add(used));
        }
    }

    fn charge(&self) -> bool {
        self.calls.set(0);
        if self.cpu_clock {
            self.account.set_sampled(self.last.get());
        }
        self.account.charge(
            self.user.replace(Duration::ZERO),
            self.system.replace(Duration::ZERO),
        )
    }

    /// The instance runs as `account` from now on. Returns whether the last charge to the
    /// previous task took it past an expiry.
    pub(crate) fn switch(&mut self, account: Arc<CpuAccount>) -> bool {
        let expired = self.sample();
        if self.cpu_clock {
            account.set_sampled(self.last.get());
        }
        self.account = account;
        expired
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use wasixv2_host::{Executor, LocalFuture};
    use wasixv2_syscalls::CpuTimes;

    use super::*;

    const US: Duration = Duration::from_micros(1);

    /// Clocks the test moves by hand.
    #[derive(Default)]
    pub(crate) struct Clocks {
        wall: Mutex<Duration>,
        cpu: Mutex<Option<Duration>>,
    }

    impl Clocks {
        pub(crate) fn advance(&self, wall: Duration, cpu: Duration) {
            *self.wall.lock().unwrap() += wall;
            if let Some(time) = self.cpu.lock().unwrap().as_mut() {
                *time += cpu;
            }
        }
    }

    impl Host for Clocks {
        fn realtime(&self) -> Duration {
            Duration::ZERO
        }
        fn monotonic(&self) -> Duration {
            *self.wall.lock().unwrap()
        }
        fn thread_cpu_time(&self) -> Option<Duration> {
            *self.cpu.lock().unwrap()
        }
        fn sleep(&self, _: Duration) -> LocalFuture<'_, ()> {
            Box::pin(std::future::pending())
        }
        fn random(&self, _: &mut [u8]) {}
        fn executor(&self) -> &dyn Executor {
            unimplemented!()
        }
    }

    pub(crate) fn setup(cpu: bool) -> (Arc<Clocks>, Arc<CpuAccount>, CpuSample) {
        let clocks = Arc::new(Clocks::default());
        if cpu {
            *clocks.cpu.lock().unwrap() = Some(Duration::from_secs(1));
        }
        let account = Arc::new(CpuAccount::detached());
        let sample = CpuSample::new(clocks.clone(), account.clone());
        (clocks, account, sample)
    }

    /// The guest computes `user`, then makes a syscall the kernel runs `system` of and that
    /// blocks for 100 µs besides.
    fn syscall(clocks: &Clocks, sample: &CpuSample, user: Duration, system: Duration) -> bool {
        clocks.advance(user, user);
        sample.enter();
        clocks.advance(system + 100 * US, system);
        sample.exit()
    }

    #[test]
    fn periodic_syscall_costs_do_not_erase_user_time() {
        let (clocks, account, sample) = setup(true);
        // Like open/read/lseek/close of /proc/self/stat: every fourth call costs more.
        // A fixed every-256th-call sample always sees that expensive phase.
        for _ in 0..4 {
            let before = account.times();
            for call in 1..=CHARGE_EVERY {
                let system = if call % 4 == 0 { 20 * US } else { US };
                syscall(&clocks, &sample, US, system);
            }
            let after = account.times();
            assert_eq!(after.user - before.user, US * CHARGE_EVERY);
            assert_eq!(after.system - before.system, 23 * US * (CHARGE_EVERY / 4));
        }
    }

    #[test]
    fn user_time_is_the_guests_own_and_system_time_the_kernels() {
        let (clocks, account, sample) = setup(true);
        for _ in 0..CHARGE_EVERY - 1 {
            syscall(&clocks, &sample, 3 * US, US);
        }
        // Nothing is published until the batch of syscalls returns.
        assert_eq!(account.times(), CpuTimes::default());
        syscall(&clocks, &sample, 3 * US, US);
        let n = CHARGE_EVERY;
        // The blocked time of each call is neither.
        assert_eq!(
            account.times(),
            CpuTimes {
                user: 3 * US * n,
                system: US * n
            }
        );
        assert_eq!(account.sampled(), Some(Duration::from_secs(1) + 4 * US * n));
        assert_eq!(account.group().times(), account.times());
        // A second batch accumulates the actual, different syscall costs.
        for _ in 0..CHARGE_EVERY {
            syscall(&clocks, &sample, 3 * US, 2 * US);
        }
        assert_eq!(
            account.times(),
            CpuTimes {
                user: 6 * US * n,
                system: 3 * US * n
            }
        );
    }

    #[test]
    fn a_preemption_point_samples() {
        let (clocks, account, sample) = setup(true);
        // A guest that only computes runs user time only.
        clocks.advance(1000 * US, 1000 * US);
        sample.preempt();
        assert_eq!(
            account.times(),
            CpuTimes {
                user: 1000 * US,
                system: Duration::ZERO
            }
        );
        for _ in 0..CHARGE_EVERY {
            syscall(&clocks, &sample, 3 * US, US);
        }
        let before = account.times();
        // Two syscalls, then a busy
        // stretch the host ran the thread only half of.
        syscall(&clocks, &sample, 3 * US, US);
        syscall(&clocks, &sample, 3 * US, US);
        clocks.advance(1000 * US, 500 * US);
        sample.preempt();
        let times = account.times();
        assert_eq!(times.system - before.system, 2 * US);
        assert_eq!(times.user - before.user, 506 * US);
    }

    #[test]
    fn without_a_cpu_clock_the_guests_own_time_is_user_time() {
        let (clocks, account, sample) = setup(false);
        syscall(&clocks, &sample, 3 * US, US);
        clocks.advance(10 * US, Duration::ZERO);
        sample.preempt();
        assert_eq!(
            account.times(),
            CpuTimes {
                user: 13 * US,
                system: Duration::ZERO
            }
        );
        for _ in 0..CHARGE_EVERY {
            syscall(&clocks, &sample, 3 * US, US);
        }
        assert_eq!(account.times().user, (13 + 3 * CHARGE_EVERY) * US);
    }

    #[test]
    fn sampling_inside_a_syscall_preserves_its_execution_mode() {
        let (clocks, account, sample) = setup(true);
        clocks.advance(3 * US, 3 * US);
        sample.enter();
        clocks.advance(100 * US, 2 * US);
        sample.sample();
        assert_eq!(
            account.times(),
            CpuTimes {
                user: 3 * US,
                system: 2 * US
            }
        );
        clocks.advance(100 * US, 5 * US);
        sample.exit();
        clocks.advance(7 * US, 7 * US);
        sample.preempt();
        assert_eq!(
            account.times(),
            CpuTimes {
                user: 10 * US,
                system: 7 * US
            }
        );
    }

    #[test]
    fn a_switch_charges_the_task_that_ran() {
        let (clocks, parent, mut sample) = setup(true);
        syscall(&clocks, &sample, 3 * US, US);
        let child = Arc::new(CpuAccount::detached());
        sample.switch(child.clone());
        assert_eq!(parent.times().total(), 4 * US);
        assert_eq!(parent.times().user, 3 * US);
        assert_eq!(parent.times().system, US);
        assert_eq!(child.sampled(), parent.sampled());
        syscall(&clocks, &sample, 5 * US, 2 * US);
        clocks.advance(US, US);
        sample.preempt();
        assert_eq!(parent.times().total(), 4 * US);
        assert_eq!(child.times().total(), 8 * US);
        assert_eq!(child.times().user, 6 * US);
        assert_eq!(child.times().system, 2 * US);
    }

    #[test]
    fn a_charge_past_an_expiry_says_so() {
        let (clocks, account, sample) = setup(true);
        account
            .group()
            .expiry()
            .set(wasixv2_syscalls::CpuBase::Virt, Some(10 * US));
        clocks.advance(9 * US, 9 * US);
        assert!(!sample.preempt());
        clocks.advance(US, US);
        assert!(sample.preempt());
    }
}
