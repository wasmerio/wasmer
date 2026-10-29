//! The machine's wall clock, after Linux's `kernel/time/timekeeping.c`: `CLOCK_REALTIME` is the
//! host's wall clock plus an offset the machine owns, so setting it (`clock_settime`,
//! `adjtimex`'s `ADJ_SETOFFSET`, a leap second) never touches the host's.
//!
//! A [`Timekeeper`] wraps the embedder's [`Host`] and is a host itself, whose
//! [`Host::realtime`] is the machine's wall clock. The machine hands it, not the embedder's
//! host, to the kernel and to every filesystem clock, so every wall-clock reading, file
//! timestamps included, goes through the offset.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicI32, AtomicI64, AtomicU64, Ordering},
    },
    time::Duration,
};

use wasixv2_abi::{AdjtimexModes, ClockState, KernelTimex};
use wasixv2_host::{Executor, Host, LocalFuture};
use wasixv2_pages::SharedPages;
use wasixv2_syscalls::Errno;
use wasixv2_wait::{NoInterrupt, PollMask, WaitQueue, Waiter};

use crate::{ntp::Ntp, sync::MutexExt};

const NANOS_PER_SEC: i128 = 1_000_000_000;
/// `KTIME_SEC_MAX - TIME_UPTIME_SEC_MAX` (30 years of uptime): the wall clock is set below.
const TIME_SETTOD_SEC_MAX: u64 = 9_223_372_036 - 946_080_000;

/// The machine's timekeeping: the wall clock's offset from the host's, the TAI offset and
/// the NTP state (`struct timekeeper` and `struct ntp_data`).
pub struct Timekeeper {
    host: Arc<dyn Host>,
    /// Nanoseconds the machine's wall clock is ahead of the host's.
    offset: AtomicI64,
    /// `tai_offset`, in seconds: `CLOCK_TAI` is the wall clock plus this.
    tai: AtomicI32,
    /// Counts the sets of the clock (`clock_was_set_seq`).
    seq: AtomicU64,
    ntp: Mutex<Ntp>,
    /// Woken by every set, for the absolute waits on the wall clock.
    was_set: WaitQueue,
    /// Runs the machine's realtime timers again, later and elsewhere, after a set found while
    /// reading the clock (`clock_was_set_delayed`).
    delayed: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
}

/// Whether `ts` may be the wall clock (`timespec64_valid_settod`).
pub(crate) fn valid_settod(ts: Duration) -> bool {
    ts.as_secs() < TIME_SETTOD_SEC_MAX
}

fn nanos(d: Duration) -> i128 {
    d.as_nanos() as i128
}

fn duration(nanos: i128) -> Duration {
    let nanos = nanos.max(0);
    let secs = u64::try_from(nanos / NANOS_PER_SEC).unwrap_or(u64::MAX);
    Duration::new(secs, (nanos % NANOS_PER_SEC) as u32)
}

fn secs(nanos: i128) -> i64 {
    nanos.div_euclid(NANOS_PER_SEC) as i64
}

impl Timekeeper {
    /// A wall clock that reads the host's until it is set.
    pub fn new(host: Arc<dyn Host>) -> Arc<Self> {
        let now = secs(nanos(host.realtime()));
        let mut ntp = Ntp::new(now);
        ntp.start_slew(host.monotonic(), nanos(host.realtime()));
        Arc::new(Self {
            host,
            offset: AtomicI64::new(0),
            tai: AtomicI32::new(0),
            seq: AtomicU64::new(0),
            ntp: Mutex::new(ntp),
            was_set: WaitQueue::new(),
            delayed: Mutex::new(None),
        })
    }

    /// The embedder's host.
    pub fn host(&self) -> &Arc<dyn Host> {
        &self.host
    }

    /// The wall clock in nanoseconds, as the offset makes it, before any pending leap.
    fn now_nanos(&self) -> i128 {
        nanos(self.host.realtime()) + i128::from(self.offset.load(Ordering::Acquire))
    }

    /// `CLOCK_TAI`'s offset from the wall clock, in seconds.
    pub(crate) fn tai_offset(&self) -> i32 {
        self.tai.load(Ordering::Acquire)
    }

    /// How many times the clock was set; a change tells a timer that cancels on a set
    /// (`TFD_TIMER_CANCEL_ON_SET`) that one happened.
    pub(crate) fn seq(&self) -> u64 {
        self.seq.load(Ordering::Acquire)
    }

    /// Installs what runs the realtime timers again after a set found while reading the
    /// clock, a leap second's.
    pub(crate) fn on_clock_was_set_delayed(&self, hook: Box<dyn Fn() + Send + Sync>) {
        *self.delayed.lock_or_recover() = Some(hook);
    }

    /// Moves the wall clock by `delta` nanoseconds.
    fn shift(&self, delta: i128) {
        let offset = i128::from(self.offset.load(Ordering::Acquire)) + delta;
        let offset = offset.clamp(i64::MIN.into(), i64::MAX.into()) as i64;
        self.offset.store(offset, Ordering::Release);
    }

    /// Bumps the set count and wakes the absolute waits on the wall clock; the caller runs
    /// the realtime timers ([`crate::Kernel::clock_was_set`]).
    fn was_set(&self) {
        self.seq.fetch_add(1, Ordering::AcqRel);
        self.was_set.wake(PollMask::empty());
    }

    /// Runs the leap state machine up to the second `now`, the wall clock's under `ntp`;
    /// returns the seconds the clock leapt by.
    fn advance(&self, ntp: &mut Ntp, mut now: i64) -> i64 {
        let mut leapt = 0;
        loop {
            let next = ntp.next_event();
            if next > now {
                break;
            }
            ntp.age(next - ntp.last_sec - 1);
            let leap = ntp.second_overflow(next);
            ntp.last_sec = next + leap;
            if leap != 0 {
                // The second is inserted or deleted, and TAI goes on (`tai_offset - leap`).
                self.shift(i128::from(leap) * NANOS_PER_SEC);
                self.tai.fetch_sub(leap as i32, Ordering::AcqRel);
                now += leap;
                leapt += leap;
            }
        }
        if now > ntp.last_sec {
            ntp.age(now - ntp.last_sec);
            ntp.last_sec = now;
        }
        leapt
    }

    /// The wall clock, with a leap second due taken; one taken here re-runs the realtime
    /// timers later (`clock_was_set_delayed`).
    fn realtime_nanos(&self) -> i128 {
        let leapt = {
            let mut ntp = self.ntp.lock_or_recover();
            self.discipline(&mut ntp)
        };
        if leapt != 0 {
            self.was_set();
            if let Some(hook) = &*self.delayed.lock_or_recover() {
                hook();
            }
        }
        self.now_nanos()
    }

    fn discipline(&self, ntp: &mut Ntp) -> i64 {
        let correction = ntp.slew(self.host.monotonic(), self.now_nanos());
        self.shift(correction);
        self.advance(ntp, secs(self.now_nanos()))
    }

    /// `do_settimeofday64` of a valid time ([`valid_settod`]): the clock cannot go back past
    /// the monotonic clock's reading (`EINVAL`). Clears the NTP state.
    pub(crate) fn settime(&self, ts: Duration) -> Result<(), Errno> {
        if ts < self.host.monotonic() {
            return Err(Errno::EINVAL);
        }
        let mut ntp = self.ntp.lock_or_recover();
        let delta = nanos(ts) - self.now_nanos();
        self.shift(delta);
        ntp.clear(secs(nanos(ts)));
        ntp.start_slew(self.host.monotonic(), nanos(ts));
        drop(ntp);
        self.was_set();
        Ok(())
    }

    /// `__timekeeping_inject_offset` of `delta` nanoseconds, under `ntp`.
    fn inject_offset(&self, ntp: &mut Ntp, delta: i128) -> Result<(), Errno> {
        let new = self.now_nanos() + delta;
        let valid = new >= 0 && valid_settod(duration(new));
        if !valid || new < nanos(self.host.monotonic()) {
            return Err(Errno::EINVAL);
        }
        self.shift(delta);
        ntp.clear(secs(new));
        Ok(())
    }

    /// `__do_adjtimex` for a caller with or without `CAP_SYS_TIME`: validates and applies
    /// `txc`, fills it in, and returns the clock state and whether the clock was set, after
    /// which the caller runs the realtime timers again.
    pub(crate) fn adjtimex(
        &self,
        txc: &mut KernelTimex,
        sys_time: bool,
    ) -> Result<(ClockState, bool), Errno> {
        crate::ntp::validate(txc, sys_time)?;
        let mut ntp = self.ntp.lock_or_recover();
        let leapt = self.discipline(&mut ntp);
        let now = self.now_nanos();
        let ts = (secs(now), now.rem_euclid(NANOS_PER_SEC) as u32);
        let mut clock_set = leapt != 0;
        if txc.modes.0 & AdjtimexModes::ADJ_SETOFFSET.0 != 0 {
            let unit = match txc.modes.0 & AdjtimexModes::ADJ_NANO.0 != 0 {
                true => 1,
                false => 1_000,
            };
            let delta =
                i128::from(txc.time.tv_sec) * NANOS_PER_SEC + i128::from(txc.time.tv_usec) * unit;
            self.inject_offset(&mut ntp, delta)?;
            clock_set = true;
        }
        let orig_tai = self.tai_offset();
        let mut tai = orig_tai;
        let real_secs = secs(self.now_nanos());
        let state = ntp.adjtimex(txc, ts, real_secs, &mut tai);
        if tai != orig_tai {
            self.tai.store(tai, Ordering::Release);
            clock_set = true;
        }
        drop(ntp);
        if clock_set {
            self.was_set();
        }
        Ok((state, clock_set))
    }

    /// `CLOCK_TAI`: the wall clock plus the TAI offset.
    pub(crate) fn tai(&self) -> Duration {
        let tai = i128::from(self.tai_offset()) * NANOS_PER_SEC;
        duration(self.realtime_nanos() + tai)
    }

    /// Completes once the wall clock reaches `at`, however it is set meanwhile: an absolute
    /// wait on `CLOCK_REALTIME`, which a set re-arms (`hrtimer` on a `CLOCK_REALTIME` base).
    pub(crate) fn sleep_until_realtime(&self, at: Duration) -> LocalFuture<'_, ()> {
        self.sleep_until(at, false)
    }

    /// [`Timekeeper::sleep_until_realtime`] on `CLOCK_TAI` with `tai`.
    pub(crate) fn sleep_until(&self, at: Duration, tai: bool) -> LocalFuture<'_, ()> {
        self.sleep_until_with_slack(at, tai, Duration::ZERO)
    }

    /// A wall-clock expiry range, rearmed with the same slack after a clock set.
    pub(crate) fn sleep_until_with_slack(
        &self,
        at: Duration,
        tai: bool,
        slack: Duration,
    ) -> LocalFuture<'_, ()> {
        Box::pin(async move {
            let waiter = Waiter::new();
            let _registration = waiter.register(&self.was_set, PollMask::empty());
            loop {
                let now = match tai {
                    true => self.tai(),
                    false => self.realtime(),
                };
                if now >= at {
                    return;
                }
                let deadline = self.host.monotonic().saturating_add(at - now);
                let mut sleep = self.host.sleep_until_range(deadline, slack);
                waiter.sleep(&NoInterrupt, Some(sleep.as_mut())).await;
            }
        })
    }
}

impl Host for Timekeeper {
    fn realtime(&self) -> Duration {
        duration(self.realtime_nanos())
    }

    fn monotonic(&self) -> Duration {
        self.host.monotonic()
    }

    fn sleep(&self, duration: Duration) -> LocalFuture<'_, ()> {
        self.host.sleep(duration)
    }

    fn sleep_until(&self, deadline: Duration) -> LocalFuture<'_, ()> {
        self.host.sleep_until(deadline)
    }

    fn sleep_until_range(&self, deadline: Duration, slack: Duration) -> LocalFuture<'_, ()> {
        self.host.sleep_until_range(deadline, slack)
    }

    fn thread_cpu_time(&self) -> Option<Duration> {
        self.host.thread_cpu_time()
    }

    fn thread_cpu_clock(&self) -> Option<Arc<dyn wasixv2_host::ThreadCpuClock>> {
        self.host.thread_cpu_clock()
    }

    fn random(&self, buf: &mut [u8]) {
        self.host.random(buf)
    }

    fn executor(&self) -> &dyn Executor {
        self.host.executor()
    }

    fn report(&self, message: &str) {
        self.host.report(message)
    }

    fn shared_pages(&self, size: u64) -> std::io::Result<Arc<dyn SharedPages>> {
        self.host.shared_pages(size)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU64;

    use wasixv2_abi::TimexStatus;

    use super::*;

    /// 23:59:58.5 UTC, two seconds and a half before a midnight (1_700_006_400).
    const BEFORE_MIDNIGHT: Duration = Duration::new(1_700_006_398, 500_000_000);
    const SECOND: Duration = Duration::from_secs(1);

    /// Clocks a test moves by hand; sleeps never end on their own.
    struct ManualHost {
        real: AtomicU64,
        mono: AtomicU64,
    }

    impl ManualHost {
        fn at(real: Duration) -> Arc<Self> {
            Arc::new(Self {
                real: AtomicU64::new(real.as_nanos() as u64),
                mono: AtomicU64::new(100_000_000_000),
            })
        }

        fn advance(&self, by: Duration) {
            let by = by.as_nanos() as u64;
            self.real.fetch_add(by, Ordering::SeqCst);
            self.mono.fetch_add(by, Ordering::SeqCst);
        }
    }

    impl Host for ManualHost {
        fn realtime(&self) -> Duration {
            Duration::from_nanos(self.real.load(Ordering::SeqCst))
        }

        fn monotonic(&self) -> Duration {
            Duration::from_nanos(self.mono.load(Ordering::SeqCst))
        }

        fn thread_cpu_clock(&self) -> Option<Arc<dyn wasixv2_host::ThreadCpuClock>> {
            Some(Arc::new(ManualCpuClock))
        }

        fn sleep(&self, _: Duration) -> LocalFuture<'_, ()> {
            Box::pin(futures_util::future::pending())
        }

        fn random(&self, buf: &mut [u8]) {
            buf.fill(0);
        }

        fn executor(&self) -> &dyn Executor {
            self
        }
    }

    struct ManualCpuClock;

    impl wasixv2_host::ThreadCpuClock for ManualCpuClock {
        fn read(&self) -> Option<Duration> {
            Some(Duration::from_micros(7))
        }
    }

    #[test]
    fn wall_clock_adjustment_preserves_the_captured_thread_cpu_clock() {
        let (host, time) = keeper(BEFORE_MIDNIGHT);
        let clock = time
            .thread_cpu_clock()
            .expect("the host's clock capability");
        time.settime(BEFORE_MIDNIGHT + SECOND).unwrap();
        host.advance(SECOND);
        assert_eq!(clock.read(), Some(Duration::from_micros(7)));
    }

    impl Executor for ManualHost {
        fn spawn_local(&self, _: LocalFuture<'static, ()>) {
            unimplemented!()
        }

        fn spawn_blocking(&self, work: Box<dyn FnOnce() + Send>) -> LocalFuture<'static, ()> {
            Box::pin(async move { work() })
        }
    }

    fn keeper(real: Duration) -> (Arc<ManualHost>, Arc<Timekeeper>) {
        let host = ManualHost::at(real);
        (host.clone(), Timekeeper::new(host))
    }

    fn txc(modes: u32) -> KernelTimex {
        KernelTimex {
            modes: AdjtimexModes(modes),
            ..KernelTimex::default()
        }
    }

    /// `adjtimex` with `txc` as a caller with `CAP_SYS_TIME`: the state and what came back.
    fn adjtimex(time: &Timekeeper, mut txc: KernelTimex) -> (ClockState, KernelTimex) {
        let (state, _) = time.adjtimex(&mut txc, true).unwrap();
        (state, txc)
    }

    /// Sets the status, and `maxerror` to 0 as a synchronised daemon does, so the clock
    /// stays synchronised for a while.
    fn status(time: &Timekeeper, status: TimexStatus) -> ClockState {
        let txc = KernelTimex {
            status,
            ..txc(AdjtimexModes::ADJ_STATUS.0 | AdjtimexModes::ADJ_MAXERROR.0)
        };
        adjtimex(time, txc).0
    }

    #[test]
    fn a_set_moves_the_machines_wall_clock_only() {
        let (host, time) = keeper(BEFORE_MIDNIGHT);
        let new = Duration::from_secs(2_000_000_000);
        assert_eq!(time.settime(new), Ok(()));
        assert_eq!(time.realtime(), new);
        assert_eq!(
            host.realtime(),
            BEFORE_MIDNIGHT,
            "the host's clock is untouched"
        );
        assert_eq!(time.seq(), 1);
        host.advance(SECOND);
        assert_eq!(time.realtime(), new + SECOND);
        // Not before the monotonic clock (`wall_to_monotonic`), which reads 101 s now.
        assert_eq!(time.settime(Duration::from_secs(100)), Err(Errno::EINVAL));
        assert_eq!(time.settime(Duration::from_secs(101)), Ok(()));
        assert!(!valid_settod(Duration::from_secs(TIME_SETTOD_SEC_MAX)));
    }

    #[test]
    fn a_set_clears_the_ntp_state() {
        let (_, time) = keeper(BEFORE_MIDNIGHT);
        let synced = KernelTimex {
            maxerror: 100,
            ..txc(AdjtimexModes::ADJ_STATUS.0 | AdjtimexModes::ADJ_MAXERROR.0)
        };
        assert_eq!(adjtimex(&time, synced).0, ClockState::TIME_OK);
        time.settime(BEFORE_MIDNIGHT).unwrap();
        let (state, now) = adjtimex(&time, txc(0));
        assert_eq!(state, ClockState::TIME_ERROR);
        assert_eq!(
            now.status.0 & TimexStatus::STA_UNSYNC.0,
            TimexStatus::STA_UNSYNC.0
        );
        assert_eq!(now.maxerror, 16_000_000);
    }

    /// As LTP's `clock_adjtime01`: every value a mode sets reads back as set.
    #[test]
    fn what_each_mode_sets_reads_back() {
        let (_, time) = keeper(BEFORE_MIDNIGHT);
        let (state, boot) = adjtimex(&time, txc(0));
        assert_eq!(state, ClockState::TIME_ERROR, "unsynchronised at boot");
        assert_eq!(
            (boot.tick, boot.constant, boot.precision, boot.tolerance),
            (10_000, 2, 1, 32_768_000)
        );
        let set = |modes: AdjtimexModes, f: &dyn Fn(&mut KernelTimex)| {
            let (_, mut got) = adjtimex(&time, txc(0));
            got.modes = modes;
            f(&mut got);
            let (_, set) = adjtimex(&time, got);
            let (_, verify) = adjtimex(&time, txc(0));
            (set, verify)
        };
        let (s, v) = set(AdjtimexModes::ADJ_FREQUENCY, &|t| t.freq += 100);
        assert_eq!((s.freq, v.freq), (100, 100));
        let (s, v) = set(AdjtimexModes::ADJ_MAXERROR, &|t| t.maxerror = 100);
        assert_eq!((s.maxerror, v.maxerror), (100, 100));
        let (s, v) = set(AdjtimexModes::ADJ_ESTERROR, &|t| t.esterror = 100);
        assert_eq!((s.esterror, v.esterror), (100, 100));
        let (s, v) = set(AdjtimexModes::ADJ_TIMECONST, &|t| t.constant = 3);
        // Microsecond mode adds 4 (`process_adjtimex_modes`).
        assert_eq!((s.constant, v.constant), (7, 7));
        let (s, v) = set(AdjtimexModes::ADJ_TICK, &|t| t.tick += 1000);
        assert_eq!((s.tick, v.tick), (11_000, 11_000));
        // A phase offset under the PLL steers the frequency and reads back.
        let (_, v) = set(AdjtimexModes::ADJ_STATUS, &|t| {
            t.status = TimexStatus::STA_PLL
        });
        assert_eq!(v.status, TimexStatus::STA_PLL);
        let (s, v) = set(AdjtimexModes::ADJ_OFFSET, &|t| t.offset = 10_000);
        assert_eq!(s.offset, v.offset);
        assert!((9_999..=10_000).contains(&v.offset), "{}", v.offset);
        // Nanosecond mode reports the offset in nanoseconds.
        let (_, v) = set(AdjtimexModes::ADJ_NANO, &|_| {});
        assert_eq!(
            v.status.0 & TimexStatus::STA_NANO.0,
            TimexStatus::STA_NANO.0
        );
        assert!((9_999_000..=10_000_000).contains(&v.offset), "{}", v.offset);
        // `adjtime`: the old remainder comes back; a read-only one changes nothing.
        let (s, _) = set(AdjtimexModes::ADJ_OFFSET_SINGLESHOT, &|t| t.offset = 500);
        assert_eq!(s.offset, 0);
        let (s, _) = set(AdjtimexModes::ADJ_OFFSET_SS_READ, &|t| t.offset = 7);
        assert_eq!(s.offset, 500);
        let (s, _) = set(AdjtimexModes::ADJ_OFFSET_SS_READ, &|_| {});
        assert_eq!(s.offset, 500);
    }

    #[test]
    fn changing_anything_needs_cap_sys_time_and_sane_values() {
        let (_, time) = keeper(BEFORE_MIDNIGHT);
        let check = |txc: KernelTimex, sys_time| time.adjtimex(&mut txc.clone(), sys_time).err();
        assert_eq!(check(txc(0), false), None);
        assert_eq!(check(txc(AdjtimexModes::ADJ_OFFSET_SS_READ.0), false), None);
        assert_eq!(
            check(txc(AdjtimexModes::ADJ_OFFSET_SINGLESHOT.0), false),
            Some(Errno::EPERM)
        );
        assert_eq!(
            check(txc(AdjtimexModes::ADJ_FREQUENCY.0), false),
            Some(Errno::EPERM)
        );
        let tick = |tick| KernelTimex {
            tick,
            ..txc(AdjtimexModes::ADJ_TICK.0)
        };
        assert_eq!(check(tick(8_999), true), Some(Errno::EINVAL));
        assert_eq!(check(tick(11_001), true), Some(Errno::EINVAL));
        let offset = |usec| KernelTimex {
            time: wasixv2_abi::KernelTimexTimeval {
                tv_sec: 0,
                tv_usec: usec,
            },
            ..txc(AdjtimexModes::ADJ_SETOFFSET.0)
        };
        assert_eq!(check(offset(1_000_000), true), Some(Errno::EINVAL));
        assert_eq!(check(offset(-1), true), Some(Errno::EINVAL));
        assert_eq!(check(offset(0), false), Some(Errno::EPERM));
        let freq = KernelTimex {
            freq: i64::MAX,
            ..txc(AdjtimexModes::ADJ_FREQUENCY.0)
        };
        assert_eq!(check(freq, true), Some(Errno::EINVAL));
    }

    #[test]
    fn adj_setoffset_steps_and_adj_tai_moves_tai() {
        let (_, time) = keeper(BEFORE_MIDNIGHT);
        let step = KernelTimex {
            time: wasixv2_abi::KernelTimexTimeval {
                tv_sec: -2,
                tv_usec: 500_000,
            },
            ..txc(AdjtimexModes::ADJ_SETOFFSET.0)
        };
        let (_, set) = time.adjtimex(&mut step.clone(), true).unwrap();
        assert!(set);
        assert_eq!(
            time.realtime(),
            BEFORE_MIDNIGHT - Duration::from_millis(1500)
        );
        let nano = KernelTimex {
            time: wasixv2_abi::KernelTimexTimeval {
                tv_sec: 1,
                tv_usec: 500_000_000,
            },
            ..txc(AdjtimexModes::ADJ_SETOFFSET.0 | AdjtimexModes::ADJ_NANO.0)
        };
        time.adjtimex(&mut nano.clone(), true).unwrap();
        assert_eq!(time.realtime(), BEFORE_MIDNIGHT);
        let tai = KernelTimex {
            constant: 37,
            ..txc(AdjtimexModes::ADJ_TAI.0)
        };
        let seq = time.seq();
        let (_, got) = adjtimex(&time, tai);
        assert_eq!((got.tai, time.tai_offset()), (37, 37));
        assert_eq!(time.tai(), BEFORE_MIDNIGHT + Duration::from_secs(37));
        assert_eq!(time.seq(), seq + 1, "a TAI change is a set");
        // Out of range: ignored.
        let bad = KernelTimex {
            constant: -1,
            ..txc(AdjtimexModes::ADJ_TAI.0)
        };
        assert_eq!(adjtimex(&time, bad).1.tai, 37);
    }

    /// `second_overflow`: armed with `STA_INS`, the clock repeats the last second of the day,
    /// and TAI goes on.
    #[test]
    fn an_inserted_leap_second_repeats_the_last_second_of_the_day() {
        let (host, time) = keeper(BEFORE_MIDNIGHT);
        assert_eq!(status(&time, TimexStatus::STA_INS), ClockState::TIME_OK);
        host.advance(SECOND);
        assert_eq!(adjtimex(&time, txc(0)).0, ClockState::TIME_INS);
        let seq = time.seq();
        host.advance(SECOND);
        // Midnight on the host; the machine reads 23:59:59.5 again.
        assert_eq!(time.realtime(), BEFORE_MIDNIGHT + SECOND);
        assert_eq!(time.seq(), seq + 1);
        let (state, got) = adjtimex(&time, txc(0));
        assert_eq!((state, got.tai), (ClockState::TIME_OOP, 1));
        assert_eq!(
            time.tai(),
            BEFORE_MIDNIGHT + 2 * SECOND,
            "TAI does not repeat"
        );
        host.advance(SECOND);
        assert_eq!(time.realtime(), BEFORE_MIDNIGHT + 2 * SECOND);
        assert_eq!(adjtimex(&time, txc(0)).0, ClockState::TIME_WAIT);
        assert_eq!(status(&time, TimexStatus(0)), ClockState::TIME_WAIT);
        host.advance(SECOND);
        assert_eq!(adjtimex(&time, txc(0)).0, ClockState::TIME_OK);
    }

    #[test]
    fn a_deleted_leap_second_skips_the_last_second_of_the_day() {
        let (host, time) = keeper(BEFORE_MIDNIGHT - SECOND);
        status(&time, TimexStatus::STA_DEL);
        host.advance(SECOND);
        assert_eq!(adjtimex(&time, txc(0)).0, ClockState::TIME_DEL);
        host.advance(SECOND);
        // 23:59:59.5 on the host is 00:00:00.5 on the machine.
        assert_eq!(time.realtime(), BEFORE_MIDNIGHT + 2 * SECOND);
        let (state, got) = adjtimex(&time, txc(0));
        assert_eq!((state, got.tai), (ClockState::TIME_WAIT, -1));
    }

    #[test]
    fn a_pending_leap_is_dropped_with_its_status_bit() {
        let (host, time) = keeper(BEFORE_MIDNIGHT);
        status(&time, TimexStatus::STA_INS);
        host.advance(SECOND);
        assert_eq!(adjtimex(&time, txc(0)).0, ClockState::TIME_INS);
        status(&time, TimexStatus(0));
        host.advance(SECOND);
        assert_eq!(adjtimex(&time, txc(0)).0, ClockState::TIME_OK);
        host.advance(SECOND);
        assert_eq!(time.realtime(), BEFORE_MIDNIGHT + 3 * SECOND, "no leap");
    }

    #[test]
    fn maxerror_grows_by_500_us_a_second() {
        let (host, time) = keeper(BEFORE_MIDNIGHT);
        let set = KernelTimex {
            maxerror: 1_000,
            ..txc(AdjtimexModes::ADJ_MAXERROR.0)
        };
        adjtimex(&time, set);
        host.advance(10 * SECOND);
        assert_eq!(adjtimex(&time, txc(0)).1.maxerror, 6_000);
    }

    /// An absolute wait on the wall clock ends when a set takes the clock past its deadline.
    #[test]
    fn a_set_past_the_deadline_ends_a_realtime_wait() {
        let (_, time) = keeper(BEFORE_MIDNIGHT);
        let setter = time.clone();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            setter.settime(BEFORE_MIDNIGHT + 60 * SECOND).unwrap();
        });
        futures::executor::block_on(time.sleep_until_realtime(BEFORE_MIDNIGHT + 30 * SECOND));
        thread.join().unwrap();
        assert!(time.realtime() >= BEFORE_MIDNIGHT + 60 * SECOND);
    }
    #[test]
    fn frequency_and_tick_adjust_the_rate_without_setting_the_clock() {
        let (host, time) = keeper(BEFORE_MIDNIGHT);
        adjtimex(
            &time,
            KernelTimex {
                freq: 100 * 65536,
                ..txc(AdjtimexModes::ADJ_FREQUENCY.0)
            },
        );
        let seq = time.seq();
        host.advance(10 * SECOND);
        assert_eq!(time.realtime(), host.realtime() + Duration::from_millis(1));
        assert_eq!(
            time.seq(),
            seq,
            "slewing never cancels CANCEL_ON_SET timers"
        );
        // +1 usec at USER_HZ=100 cancels -100 ppm.
        adjtimex(
            &time,
            KernelTimex {
                freq: -100 * 65536,
                tick: 10_001,
                ..txc(AdjtimexModes::ADJ_FREQUENCY.0 | AdjtimexModes::ADJ_TICK.0)
            },
        );
        host.advance(10 * SECOND);
        assert_eq!(time.realtime(), host.realtime() + Duration::from_millis(1));
    }

    #[test]
    fn adjtime_slews_both_directions_and_drains_the_remainder() {
        for sign in [-1, 1] {
            let (host, time) = keeper(BEFORE_MIDNIGHT);
            adjtimex(
                &time,
                KernelTimex {
                    offset: sign * 1000,
                    ..txc(AdjtimexModes::ADJ_OFFSET_SINGLESHOT.0)
                },
            );
            assert_eq!(time.realtime(), host.realtime(), "no step");
            host.advance(SECOND);
            let correction = nanos(time.realtime()) - nanos(host.realtime());
            assert!((200_000..=300_000).contains(&(correction * i128::from(sign))));
            host.advance(3 * SECOND);
            let delivered = nanos(time.realtime()) - nanos(host.realtime());
            assert!((delivered - i128::from(sign) * 1_000_000).abs() <= 1);
            host.advance(4 * SECOND);
            assert_eq!(
                nanos(time.realtime()) - nanos(host.realtime()),
                i128::from(sign) * 1_000_000
            );
            assert_eq!(
                adjtimex(&time, txc(AdjtimexModes::ADJ_OFFSET_SS_READ.0))
                    .1
                    .offset,
                0
            );
        }
    }

    #[test]
    fn pll_phase_converges_and_matches_the_delivered_correction() {
        for sign in [-1, 1] {
            let (host, time) = keeper(BEFORE_MIDNIGHT);
            adjtimex(
                &time,
                KernelTimex {
                    offset: sign * 16_000_000,
                    status: TimexStatus::STA_PLL | TimexStatus::STA_FREQHOLD,
                    ..txc(AdjtimexModes::ADJ_STATUS.0
                        | AdjtimexModes::ADJ_NANO.0
                        | AdjtimexModes::ADJ_OFFSET.0)
                },
            );
            host.advance(120 * SECOND);
            let now = time.realtime();
            let left = adjtimex(&time, txc(0)).1.offset;
            assert!(left.abs() < 16_000, "phase remainder {left}");
            let total = nanos(now) - nanos(host.realtime()) + i128::from(left);
            assert!((total - i128::from(sign) * 16_000_000).abs() <= 2);
        }
    }

    #[test]
    fn opposing_phase_and_adjtime_slews_both_converge() {
        let (host, time) = keeper(BEFORE_MIDNIGHT);
        adjtimex(
            &time,
            KernelTimex {
                offset: 8_000_000,
                status: TimexStatus::STA_PLL | TimexStatus::STA_FREQHOLD,
                ..txc(AdjtimexModes::ADJ_STATUS.0
                    | AdjtimexModes::ADJ_NANO.0
                    | AdjtimexModes::ADJ_OFFSET.0)
            },
        );
        adjtimex(
            &time,
            KernelTimex {
                offset: -1000,
                ..txc(AdjtimexModes::ADJ_OFFSET_SINGLESHOT.0)
            },
        );
        host.advance(180 * SECOND);
        let delivered = nanos(time.realtime()) - nanos(host.realtime());
        let phase = adjtimex(&time, txc(0)).1.offset;
        assert!((delivered + i128::from(phase) - 7_000_000).abs() <= 2);
        assert!(phase.abs() < 1000);
        assert_eq!(
            adjtimex(&time, txc(AdjtimexModes::ADJ_OFFSET_SS_READ.0))
                .1
                .offset,
            0
        );
    }
}
