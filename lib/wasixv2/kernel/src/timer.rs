//! Kernel timers, after Linux's hrtimers: every armed timer of the machine sits in one queue
//! ordered by deadline, and one driver sleeps on the host until the earliest deadline, then
//! runs the callbacks of the timers that expired.
//!
//! Arming, re-arming and cancelling only update the queue, so a timer that is re-armed or
//! dropped leaves no host sleep behind. The driver runs on a thread of its own when the host
//! can start one, so a timer outlives the guest thread that armed it.

use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

use crate::sync::MutexExt;
use wasixv2_host::Host;
use wasixv2_wait::{NoInterrupt, PollMask, WaitQueue, Waiter};

/// What a timer does when it expires, on the driver's thread. It must not block. It returns the
/// next monotonic deadline to re-arm the timer at (`HRTIMER_RESTART`), or `None` to leave it
/// disarmed (`HRTIMER_NORESTART`).
pub(crate) type Callback = Box<dyn Fn() -> Option<Duration> + Send + Sync>;

/// The machine's timer queue.
#[derive(Clone)]
pub(crate) struct Timers {
    shared: Arc<Shared>,
}

struct Shared {
    host: Arc<dyn Host>,
    state: Mutex<State>,
    /// Woken when the earliest deadline moves earlier, and when the kernel goes away.
    changed: WaitQueue,
}

#[derive(Default)]
struct State {
    /// Armed timers by monotonic deadline, then id.
    queue: BTreeSet<(Duration, u64)>,
    timers: HashMap<u64, Entry>,
    last_id: u64,
    driver_started: bool,
    /// The clock was set while it was read; the driver runs the realtime timers again
    /// (`clock_was_set_delayed`).
    was_set: bool,
}

struct Entry {
    callback: Weak<Callback>,
    /// The timer runs on the realtime clock, so its monotonic deadline is only an estimate
    /// that a set of the clock changes ([`Timer::clock_was_set`]).
    realtime: bool,
    deadline: Option<Duration>,
    /// Bumped by every `start` and `cancel`, so a callback that returns after its timer was
    /// re-armed or cancelled does not re-arm it.
    generation: u64,
}

impl Timers {
    pub(crate) fn new(host: Arc<dyn Host>) -> Self {
        Self {
            shared: Arc::new(Shared {
                host,
                state: Mutex::default(),
                changed: WaitQueue::new(),
            }),
        }
    }

    /// A new, disarmed timer (`hrtimer_setup`).
    pub(crate) fn timer(&self, callback: Callback) -> Timer {
        self.new_timer(callback, false)
    }

    /// A new, disarmed timer for a deadline on the realtime clock (`CLOCK_REALTIME` bases),
    /// whose callback checks that clock again when it runs.
    pub(crate) fn realtime_timer(&self, callback: Callback) -> Timer {
        self.new_timer(callback, true)
    }

    /// `clock_was_set`, after a set of the realtime clock: the machine's armed realtime timers
    /// run their callbacks now ([`Timer::clock_was_set`]).
    pub(crate) fn clock_was_set(&self) {
        self.shared.clock_was_set();
    }

    /// What has the driver run [`Timers::clock_was_set`] on its own thread, for a set found
    /// where the timers' callbacks cannot run: a leap second seen by a reading of the clock.
    pub(crate) fn clock_was_set_delayed_hook(&self) -> Box<dyn Fn() + Send + Sync> {
        let shared = Arc::downgrade(&self.shared);
        Box::new(move || {
            let Some(shared) = shared.upgrade() else {
                return;
            };
            let mut state = shared.state.lock_or_recover();
            // Without a driver no timer was ever armed.
            if state.driver_started {
                state.was_set = true;
                drop(state);
                shared.changed.wake(PollMask::empty());
            }
        })
    }

    fn new_timer(&self, callback: Callback, realtime: bool) -> Timer {
        let callback = Arc::new(callback);
        let mut state = self.shared.state.lock_or_recover();
        state.last_id += 1;
        let id = state.last_id;
        state.timers.insert(
            id,
            Entry {
                callback: Arc::downgrade(&callback),
                realtime,
                deadline: None,
                generation: 0,
            },
        );
        Timer {
            shared: Arc::downgrade(&self.shared),
            id,
            _callback: callback,
        }
    }
}

impl Drop for Shared {
    /// Lets the driver see that the kernel is gone.
    fn drop(&mut self) {
        self.changed.wake(PollMask::empty());
    }
}

/// One timer. Dropping it cancels it.
pub(crate) struct Timer {
    shared: Weak<Shared>,
    id: u64,
    /// Owned here; the queue only holds it weakly.
    _callback: Arc<Callback>,
}

impl Timer {
    /// `hrtimer_start`: arms the timer for the monotonic `deadline`, replacing any earlier
    /// arming. A deadline already past fires at once.
    pub(crate) fn start(&self, deadline: Duration) {
        let Some(shared) = self.shared.upgrade() else {
            return;
        };
        let (earliest, start_driver) = {
            let mut state = shared.state.lock_or_recover();
            state.disarm(self.id);
            let earliest = state.arm(self.id, deadline);
            let start_driver = !std::mem::replace(&mut state.driver_started, true);
            (earliest, start_driver)
        };
        if start_driver {
            spawn_driver(&shared);
        } else if earliest {
            shared.changed.wake(PollMask::empty());
        }
    }

    /// `hrtimer_try_to_cancel`. A callback already running still finishes, but cannot re-arm
    /// the timer.
    pub(crate) fn cancel(&self) {
        if let Some(shared) = self.shared.upgrade() {
            shared.state.lock_or_recover().disarm(self.id);
        }
    }

    /// `clock_was_set`, from whoever noticed that the realtime clock was set: the machine's
    /// armed realtime timers run their callbacks now, which fire those whose deadline the clock
    /// passed and re-arm the others for their new monotonic deadline
    /// (`retrigger_next_event`). Their callbacks must not block, and may call this again.
    pub(crate) fn clock_was_set(&self) {
        if let Some(shared) = self.shared.upgrade() {
            shared.clock_was_set();
        }
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        if let Some(shared) = self.shared.upgrade() {
            let mut state = shared.state.lock_or_recover();
            state.disarm(self.id);
            state.timers.remove(&self.id);
        }
    }
}

impl State {
    /// Queues timer `id` for `deadline`; returns whether it is now the earliest.
    fn arm(&mut self, id: u64, deadline: Duration) -> bool {
        let Some(entry) = self.timers.get_mut(&id) else {
            return false;
        };
        entry.deadline = Some(deadline);
        self.queue.insert((deadline, id));
        self.queue.first() == Some(&(deadline, id))
    }

    fn disarm(&mut self, id: u64) {
        if let Some(entry) = self.timers.get_mut(&id) {
            entry.generation += 1;
            if let Some(deadline) = entry.deadline.take() {
                self.queue.remove(&(deadline, id));
            }
        }
    }
}

impl Shared {
    /// [`Timer::clock_was_set`]. The timers leave the queue before their callbacks run, so a
    /// nested call does not run them again.
    fn clock_was_set(&self) {
        let armed: Vec<_> = {
            let mut state = self.state.lock_or_recover();
            let ids: Vec<u64> = state
                .timers
                .iter()
                .filter(|(_, entry)| entry.realtime && entry.deadline.is_some())
                .map(|(&id, _)| id)
                .collect();
            ids.into_iter()
                .filter_map(|id| {
                    state.disarm(id);
                    let entry = &state.timers[&id];
                    let callback = entry.callback.upgrade()?;
                    Some((id, entry.generation, callback))
                })
                .collect()
        };
        for (id, generation, callback) in armed {
            let Some(next) = callback() else { continue };
            let earliest = {
                let mut state = self.state.lock_or_recover();
                let current = state.timers.get(&id).map(|entry| entry.generation);
                current == Some(generation) && state.arm(id, next)
            };
            if earliest {
                self.changed.wake(PollMask::empty());
            }
        }
    }

    /// Runs every timer whose deadline has passed and returns the next deadline.
    fn run_expired(&self) -> Option<Duration> {
        loop {
            let now = self.host.monotonic();
            let expired: Vec<_> = {
                let mut state = self.state.lock_or_recover();
                let mut expired = Vec::new();
                while let Some(&(deadline, id)) = state.queue.first()
                    && deadline <= now
                {
                    state.queue.pop_first();
                    let entry = state.timers.get_mut(&id).expect("queued timers exist");
                    entry.deadline = None;
                    if let Some(callback) = entry.callback.upgrade() {
                        expired.push((id, entry.generation, callback));
                    }
                }
                if expired.is_empty() {
                    return state.queue.first().map(|&(deadline, _)| deadline);
                }
                expired
            };
            for (id, generation, callback) in expired {
                let Some(next) = callback() else { continue };
                let mut state = self.state.lock_or_recover();
                let current = state.timers.get(&id).map(|entry| entry.generation);
                if current == Some(generation) {
                    state.arm(id, next);
                }
            }
        }
    }
}

/// Starts the driver: on a host thread of its own when the host has threads, so it does not
/// end with the guest thread that armed the first timer, else on the current executor.
fn spawn_driver(shared: &Arc<Shared>) {
    let weak = Arc::downgrade(shared);
    let start: wasixv2_host::ThreadStart = Box::new(move || Box::pin(drive(weak)));
    let executor = shared.host.executor();
    if executor.spawn_thread(start).is_err() {
        executor.spawn_local(Box::pin(drive(Arc::downgrade(shared))));
    }
}

async fn drive(shared: Weak<Shared>) {
    let waiter = Waiter::new();
    let _registration = match shared.upgrade() {
        Some(shared) => waiter.register(&shared.changed, PollMask::empty()),
        None => return,
    };
    loop {
        let Some(live) = shared.upgrade() else { return };
        if std::mem::take(&mut live.state.lock_or_recover().was_set) {
            live.clock_was_set();
        }
        let next = live.run_expired();
        let host = live.host.clone();
        drop(live);
        let mut sleep = next.map(|deadline| host.sleep_until(deadline));
        let sleep = sleep.as_mut().map(|sleep| sleep.as_mut());
        waiter.sleep(&NoInterrupt, sleep).await;
    }
}

/// `hrtimer_forward`: for a periodic timer whose `deadline` has passed at `now`, how many
/// expirations went by (at least one) and the first deadline after `now`. `None` for the next
/// deadline once it is past what a `Duration` holds.
pub(crate) fn forward(
    deadline: Duration,
    interval: Duration,
    now: Duration,
) -> (u64, Option<Duration>) {
    debug_assert!(deadline <= now && !interval.is_zero());
    let interval = interval.as_nanos();
    let periods = (now - deadline).as_nanos() / interval + 1;
    let advance = periods.saturating_mul(interval);
    let next = u64::try_from(advance / 1_000_000_000)
        .ok()
        .map(|secs| Duration::new(secs, (advance % 1_000_000_000) as u32))
        .and_then(|advance| deadline.checked_add(advance));
    (u64::try_from(periods).unwrap_or(u64::MAX), next)
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Instant,
    };

    use wasixv2_host::{Executor, LocalFuture, ThreadStart};

    use super::*;

    /// Real clocks and sleeps, and threads, so the driver runs as it would on a real host.
    struct ThreadHost(Instant);

    impl Host for ThreadHost {
        fn realtime(&self) -> Duration {
            self.0.elapsed()
        }

        fn monotonic(&self) -> Duration {
            self.0.elapsed()
        }

        fn sleep(&self, duration: Duration) -> LocalFuture<'_, ()> {
            let (send, done) = futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                std::thread::sleep(duration);
                let _ = send.send(());
            });
            Box::pin(async move {
                let _ = done.await;
            })
        }

        fn random(&self, _: &mut [u8]) {}

        fn executor(&self) -> &dyn Executor {
            self
        }
    }

    impl Executor for ThreadHost {
        fn spawn_local(&self, _: LocalFuture<'static, ()>) {
            unreachable!("the driver gets a thread")
        }

        fn spawn_blocking(&self, work: Box<dyn FnOnce() + Send>) -> LocalFuture<'static, ()> {
            work();
            Box::pin(async {})
        }

        fn spawn_thread(&self, start: ThreadStart) -> std::io::Result<()> {
            std::thread::spawn(move || futures::executor::block_on(start()));
            Ok(())
        }
    }

    fn timers() -> (Timers, Arc<dyn Host>) {
        let host: Arc<dyn Host> = Arc::new(ThreadHost(Instant::now()));
        (Timers::new(host.clone()), host)
    }

    fn counter() -> (Arc<AtomicUsize>, Callback) {
        let count = Arc::new(AtomicUsize::new(0));
        let hits = count.clone();
        let callback: Callback = Box::new(move || {
            hits.fetch_add(1, Ordering::SeqCst);
            None
        });
        (count, callback)
    }

    fn wait_for(count: &AtomicUsize, n: usize) {
        let start = Instant::now();
        while count.load(Ordering::SeqCst) < n {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "timer never fired"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn a_timer_fires_once_at_its_deadline() {
        let (timers, host) = timers();
        let (count, callback) = counter();
        let timer = timers.timer(callback);
        timer.start(host.monotonic() + Duration::from_millis(10));
        wait_for(&count, 1);
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn re_arming_replaces_the_deadline_and_leaves_one_entry() {
        let (timers, host) = timers();
        let (count, callback) = counter();
        let timer = timers.timer(callback);
        // A re-arm storm, as an idle timeout reset on every request: only the last arming
        // stays queued.
        for _ in 0..10_000 {
            timer.start(host.monotonic() + Duration::from_secs(30));
        }
        assert_eq!(timers.shared.state.lock_or_recover().queue.len(), 1);
        timer.start(host.monotonic() + Duration::from_millis(5));
        wait_for(&count, 1);
        assert!(timers.shared.state.lock_or_recover().queue.is_empty());
    }

    #[test]
    fn cancelled_and_dropped_timers_do_not_fire() {
        let (timers, host) = timers();
        let (cancelled, callback) = counter();
        let timer = timers.timer(callback);
        timer.start(host.monotonic() + Duration::from_millis(5));
        timer.cancel();
        let (dropped, callback) = counter();
        let gone = timers.timer(callback);
        gone.start(host.monotonic() + Duration::from_millis(5));
        drop(gone);
        let (witness, callback) = counter();
        let later = timers.timer(callback);
        later.start(host.monotonic() + Duration::from_millis(20));

        wait_for(&witness, 1);
        assert_eq!(cancelled.load(Ordering::SeqCst), 0);
        assert_eq!(dropped.load(Ordering::SeqCst), 0);
        assert!(timers.shared.state.lock_or_recover().timers.len() == 2);
    }

    #[test]
    fn a_restarting_callback_re_arms_its_timer() {
        let (timers, host) = timers();
        let count = Arc::new(AtomicUsize::new(0));
        let hits = count.clone();
        let clock = host.clone();
        let timer = timers.timer(Box::new(move || {
            let n = hits.fetch_add(1, Ordering::SeqCst) + 1;
            (n < 3).then(|| clock.monotonic() + Duration::from_millis(2))
        }));
        timer.start(host.monotonic());
        wait_for(&count, 3);
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(count.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn forward_counts_missed_periods_without_truncation() {
        let ns = Duration::from_nanos;
        assert_eq!(forward(ns(10), ns(5), ns(10)), (1, Some(ns(15))));
        assert_eq!(forward(ns(10), ns(5), ns(24)), (3, Some(ns(25))));
        // More than 2^32 periods of 1 ns missed: counted exactly, next deadline past now.
        let now = Duration::from_secs(10);
        let (ticks, next) = forward(ns(0), ns(1), now);
        assert_eq!(ticks, 10_000_000_001);
        assert_eq!(next, Some(now + ns(1)));
        // A deadline that cannot be represented is never reached.
        assert_eq!(
            forward(Duration::MAX - ns(5), ns(10), Duration::MAX).1,
            None
        );
    }
}
