//! Native host timers. Linux's kernel/time/hrtimer.c orders timers by hard expiry and
//! `__hrtimer_run_queues` expires that queue's soft-expired prefix at each wakeup.

use std::{
    collections::BTreeMap,
    future::Future,
    io,
    pin::Pin,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    task::{Context, Poll, Waker},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

// The sequence breaks ties without relying on a future's address staying put.
type Key = (Instant, u64);

struct Entry {
    soft: Instant,
    fired: Arc<AtomicBool>,
    waker: Waker,
}

#[derive(Default)]
struct Queue {
    timers: BTreeMap<Key, Entry>,
    sequence: u64,
    closed: bool,
}

impl Queue {
    /// `__hrtimer_run_queues`: stop at the first timer whose soft expiry is in the future,
    /// even if another timer farther right has an earlier soft expiry.
    fn expire(&mut self, now: Instant) -> Vec<Waker> {
        let mut wakers = Vec::new();
        while self
            .timers
            .first_key_value()
            .is_some_and(|(_, e)| e.soft <= now)
        {
            let (_, entry) = self.timers.pop_first().unwrap();
            entry.fired.store(true, Ordering::Release);
            wakers.push(entry.waker);
        }
        wakers
    }
}

#[derive(Default)]
struct Shared {
    queue: Mutex<Queue>,
    changed: Condvar,
}

pub(super) struct Timers {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

impl Timers {
    pub(super) fn new() -> Self {
        let shared = Arc::new(Shared::default());
        let run = shared.clone();
        let (ready, started) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("wasixv2-timer".into())
            .spawn(move || {
                let configured = configure_worker();
                let failed = configured.is_err();
                if ready.send(configured).is_ok() && !failed {
                    run.run();
                }
            })
            .expect("could not start the host timer thread");
        // Surface setup failure on the caller, rather than leaving sleeps on a dead worker.
        if let Err(error) = started.recv().expect("host timer worker did not start") {
            worker.join().expect("host timer worker panicked");
            panic!("could not configure the host timer thread: {error}");
        }
        Self {
            shared,
            worker: Some(worker),
        }
    }

    pub(super) fn sleep(&self, soft: Instant, slack: Duration) -> impl Future<Output = ()> + '_ {
        // Instant has no MAX. An unrepresentable hard expiry may use the soft expiry,
        // which is within the allowed range and still never early.
        let hard = soft.checked_add(slack).unwrap_or(soft);
        Sleep {
            timers: self,
            soft,
            hard,
            key: None,
            fired: Arc::default(),
        }
    }
}

impl Drop for Timers {
    fn drop(&mut self) {
        self.shared.queue.lock().unwrap().closed = true;
        self.shared.changed.notify_one();
        let worker = self.worker.take().unwrap();
        // A waker may drop its host on this worker. In that case it exits on its own.
        if worker.thread().id() != thread::current().id() {
            worker.join().expect("host timer thread panicked");
        }
    }
}

fn configure_worker() -> io::Result<()> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        // kernel/sys.c PR_SET_TIMERSLACK: do not add the native thread's inherited
        // slack to the guest expiry range. This changes only this dedicated worker.
        // SAFETY: PR_SET_TIMERSLACK takes an unsigned long value and no pointers.
        if unsafe { libc::prctl(libc::PR_SET_TIMERSLACK, 1 as libc::c_ulong, 0, 0, 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

impl Shared {
    fn run(&self) {
        let mut queue = self.queue.lock().unwrap();
        loop {
            if queue.closed {
                return;
            }
            let wakers = queue.expire(Instant::now());
            if !wakers.is_empty() {
                drop(queue);
                for waker in wakers {
                    waker.wake();
                }
                queue = self.queue.lock().unwrap();
                continue;
            }
            queue = match queue.timers.first_key_value() {
                Some(((hard, _), _)) => {
                    let left = hard.saturating_duration_since(Instant::now());
                    self.changed.wait_timeout(queue, left).unwrap().0
                }
                None => self.changed.wait(queue).unwrap(),
            };
        }
    }
}

struct Sleep<'a> {
    timers: &'a Timers,
    soft: Instant,
    hard: Instant,
    key: Option<Key>,
    fired: Arc<AtomicBool>,
}

impl Future for Sleep<'_> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.fired.load(Ordering::Acquire) {
            return Poll::Ready(());
        }
        let timers = self.timers;
        let shared = &timers.shared;
        let mut queue = shared.queue.lock().unwrap();
        if let Some(key) = self.key {
            if let Some(entry) = queue.timers.get_mut(&key) {
                let previous = std::mem::replace(&mut entry.waker, cx.waker().clone());
                drop(queue);
                drop(previous);
                return Poll::Pending;
            }
            return Poll::Ready(());
        }
        if self.hard <= Instant::now() {
            return Poll::Ready(());
        }
        let key = (self.hard, queue.sequence);
        queue.sequence = queue
            .sequence
            .checked_add(1)
            .expect("host timer id exhausted");
        let rearm = queue
            .timers
            .first_key_value()
            .is_none_or(|(first, _)| key < *first);
        queue.timers.insert(
            key,
            Entry {
                soft: self.soft,
                fired: self.fired.clone(),
                waker: cx.waker().clone(),
            },
        );
        self.key = Some(key);
        if rearm {
            shared.changed.notify_one();
        }
        Poll::Pending
    }
}

impl Drop for Sleep<'_> {
    fn drop(&mut self) {
        if let Some(key) = self.key {
            let shared = &self.timers.shared;
            let mut queue = shared.queue.lock().unwrap();
            let rearm = queue
                .timers
                .first_key_value()
                .is_some_and(|(first, _)| *first == key);
            let removed = queue.timers.remove(&key);
            if rearm {
                shared.changed.notify_one();
            }
            drop(queue);
            drop(removed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add(queue: &mut Queue, base: Instant, soft: u64, hard: u64) -> Arc<AtomicBool> {
        let fired = Arc::new(AtomicBool::new(false));
        queue.timers.insert(
            (base + Duration::from_millis(hard), hard),
            Entry {
                soft: base + Duration::from_millis(soft),
                fired: fired.clone(),
                waker: Waker::noop().clone(),
            },
        );
        fired
    }

    #[test]
    fn expiry_coalesces_in_hard_expiry_order_without_firing_early() {
        let base = Instant::now();
        let mut queue = Queue::default();
        let first = add(&mut queue, base, 10, 20);
        let second = add(&mut queue, base, 15, 25);
        let blocked = add(&mut queue, base, 22, 30);
        let later = add(&mut queue, base, 5, 40);
        assert!(queue.expire(base + Duration::from_millis(9)).is_empty());
        assert_eq!(queue.expire(base + Duration::from_millis(20)).len(), 2);
        assert!(first.load(Ordering::Acquire));
        assert!(second.load(Ordering::Acquire));
        assert!(!blocked.load(Ordering::Acquire));
        assert!(!later.load(Ordering::Acquire));
        assert_eq!(queue.expire(base + Duration::from_millis(30)).len(), 2);
    }

    #[tokio::test]
    async fn dropping_a_sleep_removes_it_and_earlier_insert_rearms_the_worker() {
        let timers = Timers::new();
        let mut far =
            Box::pin(timers.sleep(Instant::now() + Duration::from_secs(3600), Duration::ZERO));
        assert!(
            far.as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        assert_eq!(timers.shared.queue.lock().unwrap().timers.len(), 1);
        tokio::time::timeout(
            Duration::from_secs(1),
            timers.sleep(Instant::now() + Duration::from_millis(1), Duration::ZERO),
        )
        .await
        .unwrap();
        drop(far);
        assert!(timers.shared.queue.lock().unwrap().timers.is_empty());
    }

    #[test]
    fn replacing_and_cancelling_a_timer_drop_wakers_outside_the_queue_lock() {
        struct CheckDrop(Arc<Shared>, Arc<AtomicBool>);
        #[allow(
            clippy::manual_noop_waker,
            reason = "this test observes the destruction of owned waker state"
        )]
        impl std::task::Wake for CheckDrop {
            fn wake(self: Arc<Self>) {}
        }
        impl Drop for CheckDrop {
            fn drop(&mut self) {
                assert!(self.0.queue.try_lock().is_ok());
                self.1.store(true, Ordering::Release);
            }
        }
        // This unit checks poll/drop's lock ownership. A live expiry worker could
        // hold the mutex independently and make CheckDrop report a false failure.
        let timers = Timers {
            shared: Arc::default(),
            worker: Some(thread::spawn(|| {})),
        };
        let mut sleep =
            Box::pin(timers.sleep(Instant::now() + Duration::from_secs(3600), Duration::ZERO));
        let replaced = Arc::new(AtomicBool::new(false));
        let waker = Waker::from(Arc::new(CheckDrop(timers.shared.clone(), replaced.clone())));
        assert!(
            sleep
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        drop(waker);
        assert!(
            sleep
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        assert!(replaced.load(Ordering::Acquire));
        let cancelled = Arc::new(AtomicBool::new(false));
        let waker = Waker::from(Arc::new(CheckDrop(
            timers.shared.clone(),
            cancelled.clone(),
        )));
        assert!(
            sleep
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        drop(waker);
        drop(sleep);
        assert!(cancelled.load(Ordering::Acquire));
    }

    #[test]
    fn dropping_the_owner_joins_an_idle_worker() {
        let timers = Timers::new();
        let shared = timers.shared.clone();
        drop(timers);
        assert_eq!(Arc::strong_count(&shared), 1);
    }
}
