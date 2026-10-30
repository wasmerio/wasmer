//! Browser deadline policy, shared by every worker of one runtime heap. JS owns only the
//! alarm; Rust owns registrations and Wakers. Linux kernel/time/hrtimer.c:
//! __hrtimer_run_queues expires the soft-ready prefix in hard-expiry order.

use std::{sync::Mutex, time::Duration};

pub(crate) const MAX_TIMEOUT_MS: f64 = i32::MAX as f64;
pub(crate) const MAX_RANDOM_CHUNK: usize = 65536;

/// Round up and split rather than overflowing setTimeout's signed 32-bit delay.
pub(crate) fn delay_ms(left: Duration) -> f64 {
    (left.as_secs_f64() * 1000.0).ceil().min(MAX_TIMEOUT_MS)
}

pub(crate) fn random_chunks(buf: &mut [u8], mut fill: impl FnMut(&mut [u8])) {
    for chunk in buf.chunks_mut(MAX_RANDOM_CHUNK) {
        fill(chunk);
    }
}

#[derive(Default)]
pub(crate) struct Clock {
    origin: Option<f64>,
    last: Duration,
}

impl Clock {
    pub(crate) fn sample(&mut self, origin_ms: f64, now_ms: f64) -> Duration {
        let epoch = *self.origin.get_or_insert(origin_ms);
        // Subtract epochs before adding now, retaining sub-millisecond precision.
        let millis = ((origin_ms - epoch) + now_ms).max(0.0);
        let sample = Duration::from_secs_f64(millis / 1000.0);
        self.last = self.last.max(sample);
        self.last
    }
}

pub(crate) static CLOCK: Mutex<Clock> = Mutex::new(Clock {
    origin: None,
    last: Duration::ZERO,
});

#[cfg(any(feature = "browser", test))]
mod queue {
    use super::*;
    use std::{
        collections::BTreeMap,
        future::Future,
        pin::Pin,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        task::{Context, Poll, Waker},
    };

    type Key = (Duration, u64);
    struct Entry {
        soft: Duration,
        fired: Arc<AtomicBool>,
        waker: Waker,
    }
    #[derive(Default)]
    struct Queue {
        entries: BTreeMap<Key, Entry>,
        sequence: u64,
    }

    pub(crate) struct Timers {
        queue: Mutex<Queue>,
        rearm: fn(),
        notified: AtomicBool,
    }

    impl Timers {
        pub(crate) fn new(rearm: fn()) -> Self {
            Self {
                queue: Mutex::default(),
                rearm,
                notified: AtomicBool::new(false),
            }
        }

        fn notify(&self) {
            if !self.notified.swap(true, Ordering::AcqRel) {
                (self.rearm)();
            }
        }

        pub(crate) fn sleep(&self, soft: Duration, slack: Duration) -> Sleep<'_> {
            Sleep {
                timers: self,
                soft,
                hard: soft.saturating_add(slack),
                key: None,
                fired: Arc::default(),
            }
        }

        /// Alarm callbacks always inspect the current queue, never a JS-carried Rust pointer.
        /// Canceled registrations cannot be revived by a delayed message or old callback.
        #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
        pub(crate) fn tick(&self, now: Duration) -> Option<Duration> {
            self.notified.store(false, Ordering::Release);
            let (wakers, next) = {
                let mut queue = self.queue.lock().unwrap();
                let mut wakers = Vec::new();
                while queue
                    .entries
                    .first_key_value()
                    .is_some_and(|(_, e)| e.soft <= now)
                {
                    let (_, entry) = queue.entries.pop_first().unwrap();
                    entry.fired.store(true, Ordering::Release);
                    wakers.push(entry.waker);
                }
                (
                    wakers,
                    queue.entries.first_key_value().map(|((hard, _), _)| *hard),
                )
            };
            for waker in wakers {
                waker.wake();
            }
            next
        }

        #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
        pub(crate) fn pending(&self) -> usize {
            self.queue.lock().unwrap().entries.len()
        }
    }

    pub(crate) struct Sleep<'a> {
        timers: &'a Timers,
        soft: Duration,
        hard: Duration,
        key: Option<Key>,
        fired: Arc<AtomicBool>,
    }

    impl Sleep<'_> {
        fn poll_at(&mut self, cx: &mut Context<'_>, now: Duration) -> Poll<()> {
            if self.fired.load(Ordering::Acquire) {
                return Poll::Ready(());
            }
            let mut queue = self.timers.queue.lock().unwrap();
            if let Some(key) = self.key {
                let previous = queue
                    .entries
                    .get_mut(&key)
                    .map(|entry| std::mem::replace(&mut entry.waker, cx.waker().clone()));
                let fired = previous.is_none();
                drop(queue);
                drop(previous);
                return if fired {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                };
            }
            if now >= self.hard {
                return Poll::Ready(());
            }
            assert!(
                queue.entries.len() < 65536,
                "browser timer capacity exhausted"
            );
            queue.sequence = queue
                .sequence
                .checked_add(1)
                .expect("browser timer id exhausted");
            let key = (self.hard, queue.sequence);
            let rearm = queue
                .entries
                .first_key_value()
                .is_none_or(|(first, _)| key < *first);
            queue.entries.insert(
                key,
                Entry {
                    soft: self.soft,
                    fired: self.fired.clone(),
                    waker: cx.waker().clone(),
                },
            );
            self.key = Some(key);
            drop(queue);
            if rearm {
                self.timers.notify();
            }
            Poll::Pending
        }
    }

    impl Future for Sleep<'_> {
        type Output = ();
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            self.poll_at(cx, crate::web_host::monotonic())
        }
    }

    impl Drop for Sleep<'_> {
        fn drop(&mut self) {
            if let Some(key) = self.key {
                let mut queue = self.timers.queue.lock().unwrap();
                let rearm = queue
                    .entries
                    .first_key_value()
                    .is_some_and(|(first, _)| *first == key);
                let removed = queue.entries.remove(&key);
                drop(queue);
                drop(removed);
                if rearm {
                    self.timers.notify();
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        fn ms(n: u64) -> Duration {
            Duration::from_millis(n)
        }
        fn poll(sleep: &mut Sleep<'_>, now: u64) -> Poll<()> {
            sleep.poll_at(&mut Context::from_waker(Waker::noop()), ms(now))
        }

        #[test]
        fn fake_clock_expires_soft_ready_prefix_in_hard_order() {
            let timers = Timers::new(|| {});
            let mut first = timers.sleep(ms(10), ms(10));
            let mut second = timers.sleep(ms(15), ms(10));
            let mut blocked = timers.sleep(ms(22), ms(8));
            let mut later = timers.sleep(ms(5), ms(35));
            for sleep in [&mut first, &mut second, &mut blocked, &mut later] {
                assert!(poll(sleep, 0).is_pending());
            }
            assert_eq!(timers.tick(ms(9)), Some(ms(20)));
            assert!(poll(&mut first, 9).is_pending());
            assert_eq!(timers.tick(ms(20)), Some(ms(30)));
            assert!(poll(&mut first, 20).is_ready());
            assert!(poll(&mut second, 20).is_ready());
            assert!(poll(&mut blocked, 20).is_pending());
            assert!(poll(&mut later, 20).is_pending());
            assert_eq!(timers.tick(ms(30)), None);
            assert!(poll(&mut blocked, 30).is_ready());
            assert!(poll(&mut later, 30).is_ready());
        }

        #[test]
        fn cancel_then_late_alarm_does_not_wake_or_remove_new_sleep() {
            let timers = Timers::new(|| {});
            let mut old = timers.sleep(ms(10), Duration::ZERO);
            assert!(poll(&mut old, 0).is_pending());
            let old_id = old.key.unwrap().1;
            drop(old);
            assert_eq!(timers.pending(), 0);
            let mut new = timers.sleep(ms(20), Duration::ZERO);
            assert!(poll(&mut new, 0).is_pending());
            assert_ne!(new.key.unwrap().1, old_id);
            assert_eq!(timers.tick(ms(10)), Some(ms(20)));
            assert!(poll(&mut new, 10).is_pending());
            drop(new);
            assert_eq!(timers.tick(ms(100)), None);
        }

        #[test]
        fn insert_cancel_storm_has_one_pending_rearm_notification() {
            static POSTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let timers = Timers::new(|| {
                POSTS.fetch_add(1, Ordering::Relaxed);
            });
            for _ in 0..1000 {
                let mut sleep = timers.sleep(ms(10), Duration::ZERO);
                assert!(poll(&mut sleep, 0).is_pending());
                drop(sleep);
            }
            assert_eq!(POSTS.load(Ordering::Relaxed), 1);
            assert_eq!(timers.tick(ms(0)), None);
            let mut sleep = timers.sleep(ms(5), Duration::ZERO);
            assert!(poll(&mut sleep, 0).is_pending());
            assert_eq!(POSTS.load(Ordering::Relaxed), 2);
            assert_eq!(timers.tick(ms(4)), Some(ms(5)));
            drop(sleep);
            assert_eq!(POSTS.load(Ordering::Relaxed), 3);
            assert_eq!(timers.tick(ms(10)), None);
        }

        #[test]
        fn wake_and_waker_drop_can_reenter_the_queue() {
            struct Check(Arc<Timers>);
            impl std::task::Wake for Check {
                fn wake(self: Arc<Self>) {
                    assert!(self.0.queue.try_lock().is_ok());
                }
            }
            impl Drop for Check {
                fn drop(&mut self) {
                    assert!(self.0.queue.try_lock().is_ok());
                }
            }
            let timers = Arc::new(Timers::new(|| {}));
            let mut sleep = timers.sleep(ms(10), Duration::ZERO);
            for _ in 0..2 {
                let waker = Waker::from(Arc::new(Check(timers.clone())));
                assert!(
                    sleep
                        .poll_at(&mut Context::from_waker(&waker), ms(0))
                        .is_pending()
                );
            }
            assert_eq!(timers.tick(ms(10)), None);
            let mut canceled = timers.sleep(ms(20), Duration::ZERO);
            let waker = Waker::from(Arc::new(Check(timers.clone())));
            assert!(
                canceled
                    .poll_at(&mut Context::from_waker(&waker), ms(0))
                    .is_pending()
            );
            drop(waker);
            drop(canceled);
        }
    }
}

#[cfg(feature = "browser")]
pub(crate) use queue::Timers;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worker_origins_share_one_epoch_and_backward_rounding_is_clamped() {
        let mut clock = Clock::default();
        assert_eq!(clock.sample(1000.0, 20.0), Duration::from_millis(20));
        assert_eq!(clock.sample(1015.0, 10.0), Duration::from_millis(25));
        assert_eq!(clock.sample(1000.0, 24.0), Duration::from_millis(25));
        assert_eq!(clock.sample(990.0, 40.0), Duration::from_millis(30));
    }
    #[test]
    fn timeout_rounds_up_and_splits_long_or_maximum_deadlines() {
        assert_eq!(delay_ms(Duration::from_nanos(1)), 1.0);
        assert_eq!(delay_ms(Duration::from_millis(10)), 10.0);
        assert_eq!(delay_ms(Duration::from_secs(3_000_000)), MAX_TIMEOUT_MS);
        assert_eq!(delay_ms(Duration::MAX), MAX_TIMEOUT_MS);
    }
    #[test]
    fn random_chunks_never_exceed_the_web_crypto_quota() {
        let mut data = vec![0; MAX_RANDOM_CHUNK * 2 + 17];
        let mut sizes = Vec::new();
        random_chunks(&mut data, |chunk| {
            chunk.fill(42);
            sizes.push(chunk.len());
        });
        assert_eq!(sizes, [65536, 65536, 17]);
        assert!(data.iter().all(|byte| *byte == 42));
    }
}
