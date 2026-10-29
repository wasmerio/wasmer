//! CPU accounting by the mode observed at a periodic tick, as Linux's
//! `kernel/sched/cputime.c:account_process_tick` and `irqtime_account_process_tick` do.
//! A captured host thread CPU clock excludes blocked, descheduled and interrupt time. One
//! shared sampler reads it at 1000 Hz; syscall boundaries only store the execution mode.
//!
//! Ticks accumulate pending deltas. The guest thread publishes them and their timestamp at
//! its next return/preemption hook, so the kernel's live CPU-clock extrapolation cannot race
//! background counter publication. Hosts without cross-thread clocks retain boundary accounting.

use std::{cell::Cell, sync::Arc, time::Duration};

#[cfg(not(target_family = "wasm"))]
use std::sync::{
    Mutex, PoisonError,
    atomic::{AtomicBool, Ordering},
};

use wasixv2_host::Host;
#[cfg(not(target_family = "wasm"))]
use wasixv2_host::ThreadCpuClock;
use wasixv2_syscalls::CpuAccount;
#[cfg(not(target_family = "wasm"))]
use wasixv2_syscalls::CpuTimes;

/// CONFIG_HZ_1000: leaves headroom for host wake-up latency within Linux's coarse
/// accounting resolution, independently of syscall count or cost.
#[cfg(not(target_family = "wasm"))]
const TICK: Duration = Duration::from_millis(1);

pub(crate) enum CpuSample {
    #[cfg(not(target_family = "wasm"))]
    Tick(Arc<TickSample>),
    Boundary(BoundarySample),
}

impl CpuSample {
    pub(crate) fn new(host: Arc<dyn Host>, account: Arc<CpuAccount>) -> Self {
        #[cfg(not(target_family = "wasm"))]
        if let Some(clock) = host.thread_cpu_clock()
            && let Some(sample) = TickSample::new(clock, account.clone())
            && let Some(sampler) = ticks::Sampler::global()
        {
            let sample = Arc::new(sample);
            sampler.register(&sample);
            return Self::Tick(sample);
        }
        Self::Boundary(BoundarySample::new(host, account))
    }

    #[inline]
    pub(crate) fn enter(&self) {
        match self {
            #[cfg(not(target_family = "wasm"))]
            Self::Tick(sample) => sample.mode.system.store(true, Ordering::Relaxed),
            Self::Boundary(sample) => sample.enter(),
        }
    }

    #[inline]
    pub(crate) fn exit(&self) -> bool {
        match self {
            #[cfg(not(target_family = "wasm"))]
            Self::Tick(sample) => {
                sample.mode.system.store(false, Ordering::Relaxed);
                sample.publish_pending()
            }
            Self::Boundary(sample) => sample.exit(),
        }
    }

    pub(crate) fn preempt(&self) -> bool {
        match self {
            #[cfg(not(target_family = "wasm"))]
            Self::Tick(sample) => sample.publish_pending(),
            Self::Boundary(sample) => sample.sample(),
        }
    }

    /// Flush the final partial tick before the task's end becomes visible.
    pub(crate) fn sample(&self) -> bool {
        match self {
            #[cfg(not(target_family = "wasm"))]
            Self::Tick(sample) => sample.flush(),
            Self::Boundary(sample) => sample.sample(),
        }
    }

    pub(crate) fn switch(&mut self, account: Arc<CpuAccount>) -> bool {
        match self {
            #[cfg(not(target_family = "wasm"))]
            Self::Tick(sample) => sample.switch(account),
            Self::Boundary(sample) => sample.switch(account),
        }
    }
}

#[cfg(not(target_family = "wasm"))]
impl Drop for CpuSample {
    fn drop(&mut self) {
        if let Self::Tick(sample) = self {
            sample.close();
        }
    }
}

// Keep the hot flag apart from the state the sampler modifies on another CPU.
#[cfg(not(target_family = "wasm"))]
#[repr(align(64))]
struct Mode {
    system: AtomicBool,
    pending: AtomicBool,
}

#[cfg(not(target_family = "wasm"))]
pub(crate) struct TickSample {
    mode: Mode,
    clock: Arc<dyn ThreadCpuClock>,
    state: Mutex<TickState>,
}

#[cfg(not(target_family = "wasm"))]
struct TickState {
    account: Arc<CpuAccount>,
    last: Duration,
    closed: bool,
    pending: CpuTimes,
}

#[cfg(not(target_family = "wasm"))]
impl TickSample {
    fn new(clock: Arc<dyn ThreadCpuClock>, account: Arc<CpuAccount>) -> Option<Self> {
        let last = clock.read()?;
        account.set_sampled(last);
        Some(Self {
            mode: Mode {
                system: AtomicBool::new(false),
                pending: AtomicBool::new(false),
            },
            clock,
            state: Mutex::new(TickState {
                account,
                last,
                closed: false,
                pending: CpuTimes::default(),
            }),
        })
    }

    /// The sampler never publishes to CpuAccount: its owner can be reading the account and
    /// timestamp separately inside cpu_clock_now. Only the owner publishes, under this lock.
    fn tick(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.closed {
            return false;
        }
        self.accumulate(&mut state);
        true
    }

    fn accumulate(&self, state: &mut TickState) {
        let Some(now) = self.clock.read() else { return };
        let used = now.saturating_sub(std::mem::replace(&mut state.last, now));
        if used.is_zero() {
            return;
        }
        // account_process_tick's user_tick: the mode at sampling time, not an estimate of
        // syscall costs. CPU-clock deltas already remove irq/steal and blocked time.
        let counter = if self.mode.system.load(Ordering::Relaxed) {
            &mut state.pending.system
        } else {
            &mut state.pending.user
        };
        *counter = counter.saturating_add(used);
        self.mode.pending.store(true, Ordering::Relaxed);
    }

    #[inline]
    fn publish_pending(&self) -> bool {
        if !self.mode.pending.load(Ordering::Relaxed) {
            return false;
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        self.publish(&mut state)
    }

    fn publish(&self, state: &mut TickState) -> bool {
        if state.closed {
            return false;
        }
        let pending = std::mem::take(&mut state.pending);
        state.account.set_sampled(state.last);
        // The flag is just a hint; the mutex orders both the data and clearing the hint
        // against a new tick, without a read-modify-write on the ordinary syscall path.
        self.mode.pending.store(false, Ordering::Relaxed);
        state.account.charge(pending.user, pending.system)
    }

    fn flush(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.closed {
            return false;
        }
        self.accumulate(&mut state);
        self.publish(&mut state)
    }

    fn switch(&self, account: Arc<CpuAccount>) -> bool {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.closed {
            return false;
        }
        self.accumulate(&mut state);
        let expired = self.publish(&mut state);
        account.set_sampled(state.last);
        state.account = account;
        expired
    }

    fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        // The final explicit flush preceded the task's visible exit. Stop a sampler holding
        // a strong snapshot before the owner can leave and its clock id can be reused.
        state.closed = true;
    }
}

/// Publish accumulated deltas after this many syscalls, or at a preemption point.
const CHARGE_EVERY: u32 = 256;

/// The accounting state of one instance. Cells: the entry hook only reads the environment.
pub(crate) struct BoundarySample {
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

impl BoundarySample {
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

#[cfg(not(target_family = "wasm"))]
mod ticks {
    use super::*;
    use std::{
        sync::{Condvar, OnceLock, Weak},
        time::Instant,
    };

    /// One sampler for all guest instances, parked while none is registered.
    pub(super) struct Sampler {
        entries: Mutex<Vec<Weak<TickSample>>>,
        wake: Condvar,
    }

    impl Sampler {
        pub(super) fn global() -> Option<&'static Arc<Self>> {
            static SAMPLER: OnceLock<Option<Arc<Sampler>>> = OnceLock::new();
            SAMPLER
                .get_or_init(|| {
                    let sampler = Arc::new(Self {
                        entries: Mutex::new(Vec::new()),
                        wake: Condvar::new(),
                    });
                    let driver = sampler.clone();
                    std::thread::Builder::new()
                        .name("wasixv2-cpu".into())
                        .spawn(move || driver.run())
                        .ok()?;
                    Some(sampler)
                })
                .as_ref()
        }

        pub(super) fn register(&self, sample: &Arc<TickSample>) {
            self.entries
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(Arc::downgrade(sample));
            self.wake.notify_one();
        }

        fn run(&self) {
            let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
            loop {
                while entries.is_empty() {
                    entries = self
                        .wake
                        .wait(entries)
                        .unwrap_or_else(PoisonError::into_inner);
                }
                let mut deadline = Instant::now() + TICK;
                while !entries.is_empty() {
                    while Instant::now() < deadline {
                        (entries, _) = self
                            .wake
                            .wait_timeout(
                                entries,
                                deadline.saturating_duration_since(Instant::now()),
                            )
                            .unwrap_or_else(PoisonError::into_inner);
                    }
                    Self::tick(&mut entries);
                    deadline += TICK;
                    if deadline <= Instant::now() {
                        // A delayed sampler observes the current mode once, never replays
                        // missed ticks using one stale observation.
                        deadline = Instant::now() + TICK;
                    }
                }
            }
        }

        pub(super) fn tick(entries: &mut Vec<Weak<TickSample>>) {
            entries.retain(|entry| entry.upgrade().is_some_and(|sample| sample.tick()));
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    use wasixv2_host::{Executor, LocalFuture};
    use wasixv2_syscalls::CpuTimes;

    use super::*;

    const US: Duration = Duration::from_micros(1);

    /// Clocks the test moves by hand.
    #[derive(Default)]
    pub(crate) struct Clocks {
        wall: Mutex<Duration>,
        cpu: Arc<Mutex<Option<Duration>>>,
        external: AtomicBool,
        self_reads: AtomicUsize,
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
            self.self_reads.fetch_add(1, Ordering::Relaxed);
            *self.cpu.lock().unwrap()
        }
        fn thread_cpu_clock(&self) -> Option<Arc<dyn wasixv2_host::ThreadCpuClock>> {
            self.external.load(Ordering::Relaxed).then(|| {
                Arc::new(ExternalClock(self.cpu.clone())) as Arc<dyn wasixv2_host::ThreadCpuClock>
            })
        }
        fn sleep(&self, _: Duration) -> LocalFuture<'_, ()> {
            Box::pin(std::future::pending())
        }
        fn random(&self, _: &mut [u8]) {}
        fn executor(&self) -> &dyn Executor {
            unimplemented!()
        }
    }

    struct ExternalClock(Arc<Mutex<Option<Duration>>>);

    impl wasixv2_host::ThreadCpuClock for ExternalClock {
        fn read(&self) -> Option<Duration> {
            *self.0.lock().unwrap()
        }
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn external_clocks_do_not_read_cpu_time_at_syscall_boundaries() {
        let clocks = Arc::new(Clocks::default());
        *clocks.cpu.lock().unwrap() = Some(Duration::from_secs(1));
        clocks.external.store(true, Ordering::Relaxed);
        let sample = CpuSample::new(clocks.clone(), Arc::new(CpuAccount::detached()));
        for _ in 0..1000 {
            sample.enter();
            sample.exit();
        }
        assert_eq!(clocks.self_reads.load(Ordering::Relaxed), 0);
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

    #[cfg(not(target_family = "wasm"))]
    fn setup_ticks() -> (Arc<Clocks>, Arc<CpuAccount>, CpuSample, Arc<TickSample>) {
        let (clocks, account, _) = setup(true);
        let clock = Arc::new(ExternalClock(clocks.cpu.clone()));
        let tick = Arc::new(TickSample::new(clock, account.clone()).unwrap());
        (clocks, account, CpuSample::Tick(tick.clone()), tick)
    }

    /// Advance to every scheduled tick, including while blocked. No real sleeps or racing
    /// sampler thread: the CPU clock advances only while this simulated thread runs.
    #[cfg(not(target_family = "wasm"))]
    fn run_for(
        clocks: &Clocks,
        tick: &TickSample,
        next: &mut Duration,
        duration: Duration,
        running: bool,
    ) {
        let mut now = *clocks.wall.lock().unwrap();
        let end = now + duration;
        while *next <= end {
            let step = *next - now;
            clocks.advance(step, if running { step } else { Duration::ZERO });
            tick.tick();
            now = *next;
            *next += TICK;
        }
        let step = end - now;
        clocks.advance(step, if running { step } else { Duration::ZERO });
    }

    #[test]
    #[cfg(not(target_family = "wasm"))]
    fn periodic_syscall_costs_do_not_erase_user_time() {
        let (clocks, account, sample, tick) = setup_ticks();
        let mut next = TICK;
        for _ in 0..4 {
            let before = account.times();
            // Give the user's loop a full tick; 256-call sampling must not choose its mode.
            run_for(&clocks, &tick, &mut next, TICK, true);
            for call in 1..=256 {
                run_for(&clocks, &tick, &mut next, 3 * US, true);
                sample.enter();
                let system = if call % 4 == 0 { 20 * US } else { US };
                run_for(&clocks, &tick, &mut next, system, true);
                run_for(&clocks, &tick, &mut next, 100 * US, false);
                sample.exit();
            }
            sample.sample();
            let after = account.times();
            assert!(after.user > before.user);
            assert_eq!(
                after.total() - before.total(),
                TICK + 3 * US * 256 + 23 * US * 64
            );
            assert_eq!(account.group().times(), after);
            assert_eq!(account.sampled(), *clocks.cpu.lock().unwrap());
        }
    }

    #[test]
    #[cfg(not(target_family = "wasm"))]
    fn ticks_charge_the_observed_mode_and_exclude_blocked_time() {
        let (clocks, account, sample, tick) = setup_ticks();
        let mut next = TICK;
        run_for(&clocks, &tick, &mut next, TICK, true);
        sample.enter();
        run_for(&clocks, &tick, &mut next, TICK, true);
        run_for(&clocks, &tick, &mut next, 10 * TICK, false);
        sample.exit();
        assert_eq!(
            account.times(),
            CpuTimes {
                user: TICK,
                system: TICK
            }
        );
        // Preemption checks must not turn every user-side hook into a biased extra tick.
        clocks.advance(US, US);
        sample.preempt();
        assert_eq!(
            account.times(),
            CpuTimes {
                user: TICK,
                system: TICK
            }
        );
    }

    #[test]
    #[cfg(not(target_family = "wasm"))]
    fn a_tick_expiry_is_consumed_by_the_return_hook_once() {
        let (clocks, account, sample, tick) = setup_ticks();
        account
            .group()
            .expiry()
            .set(wasixv2_syscalls::CpuBase::Virt, Some(TICK));
        clocks.advance(TICK, TICK);
        tick.tick();
        assert_eq!(account.times(), CpuTimes::default());
        assert!(sample.exit());
        assert!(!sample.exit());
    }

    #[test]
    #[cfg(not(target_family = "wasm"))]
    fn ticks_preserve_precise_live_cpu_clock_reads_between_publications() {
        let (clocks, account, sample, tick) = setup_ticks();
        // The existing kernel cpu_clock_now contract: published counters plus the calling
        // thread's CPU since the matching published timestamp (CPUCLOCK_SCHED runtime).
        let live = || {
            account.times().total()
                + clocks
                    .thread_cpu_time()
                    .unwrap()
                    .saturating_sub(account.sampled().unwrap())
        };
        clocks.advance(US, US);
        assert_eq!(live(), US);
        sample.enter();
        clocks.advance(TICK, TICK);
        tick.tick();
        assert_eq!(account.times(), CpuTimes::default());
        assert_eq!(live(), TICK + US);
        sample.exit();
        assert_eq!(account.times().total(), TICK + US);
        assert_eq!(live(), TICK + US);
        clocks.advance(US, US);
        assert_eq!(live(), TICK + 2 * US);
    }

    #[test]
    #[cfg(not(target_family = "wasm"))]
    fn tick_switch_and_close_serialize_with_sampling() {
        let (clocks, parent, mut sample, tick) = setup_ticks();
        clocks.advance(2 * US, 2 * US);
        let child = Arc::new(CpuAccount::detached());
        sample.switch(child.clone());
        assert_eq!(
            parent.times(),
            CpuTimes {
                user: 2 * US,
                system: Duration::ZERO
            }
        );
        sample.enter();
        clocks.advance(TICK, TICK);
        tick.tick();
        sample.exit();
        assert_eq!(
            child.times(),
            CpuTimes {
                user: Duration::ZERO,
                system: TICK
            }
        );
        drop(sample);
        let before = child.times();
        clocks.advance(TICK, TICK);
        assert!(!tick.tick());
        assert!(!tick.flush());
        assert_eq!(child.times(), before);
        // Even a sampler holding a strong snapshot must discard a closed registration.
        #[cfg(not(target_family = "wasm"))]
        {
            let mut entries = vec![Arc::downgrade(&tick)];
            ticks::Sampler::tick(&mut entries);
            assert!(entries.is_empty());
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
