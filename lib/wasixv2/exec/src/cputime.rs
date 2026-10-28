//! Process accounting: charges the task an instance runs as with the CPU time its host thread
//! uses, split into user and system time.
//!
//! Reading the host thread's CPU clock costs a host system call, which a fast guest syscall's
//! whole path makes none of: about 0.1 µs, and more in what it disturbs after it (two of them
//! every 32 guest syscalls made `write` 8% slower), while even the monotonic clock costs a tenth
//! of a guest syscall. So neither is read on every syscall. The CPU clock is read at every
//! preemption point, when the instance switches tasks or its task exits, and around one syscall
//! in [`SAMPLE_EVERY`]: each read charges what the clock advanced since the last. The syscall measured so says how much CPU time a syscall costs
//! the kernel; a charge counts that much system time for each syscall since the last charge, at
//! most all of the charge, and the rest as user time. Time a syscall spends blocked is no CPU
//! time and never shows, and neither does time the host does not run the thread.
//!
//! This is how Linux's tick accounting splits a task's time, by sampling: every tick charges a
//! whole tick to user or system time by the mode the tick found the task in
//! (`account_process_tick` in `kernel/sched/cputime.c`).
//!
//! On a host without a thread CPU clock ([`Host::thread_cpu_time`]), every syscall reads the
//! monotonic clock as it enters and returns, and the time the guest ran its own code between
//! them is charged as user time, nothing as system time.

use std::{cell::Cell, sync::Arc, time::Duration};

use wasixv2_host::Host;
use wasixv2_syscalls::CpuAccount;

/// One syscall in this many is measured, and charges the time since the last charge. A busy
/// syscall loop is charged every 0.1 to 0.3 ms so, much finer than Linux's tick of 4 ms.
pub(crate) const SAMPLE_EVERY: u32 = 256;

/// The sampling state of one instance. Cells: the entry hook only reads the environment.
pub(crate) struct CpuSample {
    host: Arc<dyn Host>,
    /// The account of the task the instance runs as.
    account: Arc<CpuAccount>,
    /// Whether the host has a thread CPU clock to read.
    cpu_clock: bool,
    /// The clock at the last charge: the thread CPU clock, or without one the monotonic clock.
    last: Cell<Duration>,
    /// Syscalls since the last charge.
    calls: Cell<u32>,
    /// Syscalls until the next measured one.
    countdown: Cell<u32>,
    /// The thread CPU clock as the measured syscall entered.
    entered: Cell<Option<Duration>>,
    /// The CPU time the last measured syscall took.
    per_call: Cell<Duration>,
    /// Without a thread CPU clock: when the guest last got control back, on the monotonic clock,
    /// and how long it ran its own code since the last charge.
    resumed: Cell<Duration>,
    user: Cell<Duration>,
}

impl CpuSample {
    /// Sampling from now, on the host thread that runs the instance, for the task whose
    /// account is `account`.
    pub(crate) fn new(host: Arc<dyn Host>, account: Arc<CpuAccount>) -> Self {
        let cpu = host.thread_cpu_time();
        let now = host.monotonic();
        if let Some(cpu) = cpu {
            account.set_sampled(cpu);
        }
        Self {
            account,
            cpu_clock: cpu.is_some(),
            last: Cell::new(cpu.unwrap_or(now)),
            calls: Cell::new(0),
            countdown: Cell::new(SAMPLE_EVERY),
            entered: Cell::new(None),
            per_call: Cell::new(Duration::ZERO),
            resumed: Cell::new(now),
            user: Cell::new(Duration::ZERO),
            host,
        }
    }

    /// A syscall enters.
    pub(crate) fn enter(&self) {
        self.calls.set(self.calls.get() + 1);
        if !self.cpu_clock {
            let now = self.host.monotonic();
            let ran = now.saturating_sub(self.resumed.get());
            self.user.set(self.user.get().saturating_add(ran));
            return;
        }
        let countdown = self.countdown.get() - 1;
        if countdown == 0 {
            self.countdown.set(SAMPLE_EVERY);
            self.entered.set(self.host.thread_cpu_time());
        } else {
            self.countdown.set(countdown);
        }
    }

    /// A syscall returns, before the return-to-guest path runs. True when the measured one did
    /// and its charge took the task or its group past a CPU timer's expiry
    /// ([`CpuAccount::charge`]).
    pub(crate) fn exit(&self) -> bool {
        if !self.cpu_clock {
            self.resumed.set(self.host.monotonic());
            return self.calls.get() >= SAMPLE_EVERY && self.sample();
        }
        let Some(entered) = self.entered.take() else {
            return false;
        };
        let Some(now) = self.host.thread_cpu_time() else {
            return false;
        };
        self.per_call.set(now.saturating_sub(entered));
        self.charge(now)
    }

    /// A preemption point, where the guest ran its own code until now. Samples.
    pub(crate) fn preempt(&self) -> bool {
        if !self.cpu_clock {
            let now = self.host.monotonic();
            let ran = now.saturating_sub(self.resumed.replace(now));
            self.user.set(self.user.get().saturating_add(ran));
        }
        self.sample()
    }

    /// Charges the time since the last charge to the task. True when that took the task or its
    /// group past a CPU timer's expiry.
    pub(crate) fn sample(&self) -> bool {
        match self.cpu_clock {
            true => match self.host.thread_cpu_time() {
                Some(now) => self.charge(now),
                None => false,
            },
            false => {
                self.calls.set(0);
                let user = self.user.replace(Duration::ZERO);
                self.account.charge(user, Duration::ZERO)
            }
        }
    }

    /// Charges what the thread CPU clock, now at `now`, advanced since the last charge: as
    /// system time, the measured cost of a syscall for each syscall since, and the rest as user
    /// time.
    fn charge(&self, now: Duration) -> bool {
        let used = now.saturating_sub(self.last.replace(now));
        let calls = self.calls.replace(0);
        let system = self.per_call.get().saturating_mul(calls).min(used);
        self.account.set_sampled(now);
        self.account.charge(used - system, system)
    }

    /// The instance runs as the task whose account is `account` from now on. Returns whether
    /// the last charge to the previous task took it past an expiry.
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
        fn advance(&self, wall: Duration, cpu: Duration) {
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

    fn setup(cpu: bool) -> (Arc<Clocks>, Arc<CpuAccount>, CpuSample) {
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
    fn user_time_is_the_guests_own_and_system_time_the_kernels() {
        let (clocks, account, sample) = setup(true);
        for _ in 0..SAMPLE_EVERY - 1 {
            syscall(&clocks, &sample, 3 * US, US);
        }
        // Nothing is charged until the measured syscall returns.
        assert_eq!(account.times(), CpuTimes::default());
        syscall(&clocks, &sample, 3 * US, US);
        let n = SAMPLE_EVERY;
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
        // The next measured call says what the calls since cost.
        for _ in 0..SAMPLE_EVERY {
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
        for _ in 0..SAMPLE_EVERY {
            syscall(&clocks, &sample, 3 * US, US);
        }
        let before = account.times();
        // Two syscalls, each charged with the cost the last measured one had, then a busy
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
        for _ in 0..SAMPLE_EVERY {
            syscall(&clocks, &sample, 3 * US, US);
        }
        assert_eq!(account.times().user, (13 + 3 * SAMPLE_EVERY) * US);
    }

    #[test]
    fn a_switch_charges_the_task_that_ran() {
        let (clocks, parent, mut sample) = setup(true);
        syscall(&clocks, &sample, 3 * US, US);
        let child = Arc::new(CpuAccount::detached());
        sample.switch(child.clone());
        assert_eq!(parent.times().total(), 4 * US);
        assert_eq!(child.sampled(), parent.sampled());
        syscall(&clocks, &sample, 5 * US, 2 * US);
        clocks.advance(US, US);
        sample.preempt();
        assert_eq!(parent.times().total(), 4 * US);
        assert_eq!(child.times().total(), 8 * US);
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
