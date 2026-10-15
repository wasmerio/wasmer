use std::{
    future::Future,
    pin::pin,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll},
    thread,
    time::Duration,
};

use futures::{executor::block_on, task::noop_waker};

use super::*;

/// A counter of available "items", like bytes in a pipe, with a queue woken when one arrives.
#[derive(Default)]
struct Tokens {
    count: AtomicUsize,
    queue: WaitQueue,
}

impl Tokens {
    fn put(&self) {
        self.count.fetch_add(1, Ordering::SeqCst);
        self.queue.wake(PollMask::READABLE);
    }

    fn take(&self) -> Option<()> {
        self.count
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .ok()
            .map(drop)
    }
}

impl Pollable for Tokens {
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, &self.queue);
        match self.count.load(Ordering::SeqCst) {
            0 => PollMask::WRITABLE,
            _ => PollMask::READABLE | PollMask::WRITABLE,
        }
    }
}

fn poll_once<F: Future>(future: std::pin::Pin<&mut F>) -> Poll<F::Output> {
    let waker = noop_waker();
    future.poll(&mut Context::from_waker(&waker))
}

#[test]
fn wait_event_is_woken_from_another_thread() {
    let tokens = Arc::new(Tokens::default());
    let producer = {
        let tokens = tokens.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            tokens.put();
        })
    };

    let result = block_on(wait_event(&tokens.queue, || tokens.take(), &NoInterrupt));

    assert_eq!(result, Ok(()));
    producer.join().unwrap();
    assert!(!tokens.queue.is_active());
}

#[test]
fn wait_event_returns_at_once_when_ready() {
    let tokens = Tokens::default();
    tokens.put();
    assert_eq!(
        block_on(wait_event(&tokens.queue, || tokens.take(), &NoInterrupt)),
        Ok(())
    );
}

#[test]
fn dropping_a_wait_removes_it_from_the_queue() {
    let tokens = Tokens::default();
    {
        let mut wait = pin!(wait_event(&tokens.queue, || tokens.take(), &NoInterrupt));
        assert!(poll_once(wait.as_mut()).is_pending());
        assert!(tokens.queue.is_active());
    }
    assert!(!tokens.queue.is_active());
}

#[test]
fn many_threads_each_get_one_token() {
    let tokens = Arc::new(Tokens::default());
    let waiters: Vec<_> = (0..8)
        .map(|_| {
            let tokens = tokens.clone();
            thread::spawn(move || {
                block_on(wait_event_exclusive(
                    &tokens.queue,
                    || tokens.take(),
                    &NoInterrupt,
                ))
            })
        })
        .collect();
    for _ in 0..8 {
        thread::sleep(Duration::from_millis(2));
        tokens.put();
    }
    for waiter in waiters {
        assert_eq!(waiter.join().unwrap(), Ok(()));
    }
    assert_eq!(tokens.count.load(Ordering::SeqCst), 0);
}

#[test]
fn an_exclusive_wake_reaches_one_waiter() {
    let tokens = Tokens::default();
    let mut first = pin!(wait_event_exclusive(
        &tokens.queue,
        || tokens.take(),
        &NoInterrupt
    ));
    let mut second = pin!(wait_event_exclusive(
        &tokens.queue,
        || tokens.take(),
        &NoInterrupt
    ));
    assert!(poll_once(first.as_mut()).is_pending());
    assert!(poll_once(second.as_mut()).is_pending());

    tokens.put();

    assert_eq!(poll_once(first.as_mut()), Poll::Ready(Ok(())));
    assert!(poll_once(second.as_mut()).is_pending());
}

#[test]
fn an_abandoned_exclusive_wakeup_passes_to_the_next_waiter() {
    let tokens = Tokens::default();
    // A non-exclusive spy sees every wake of the queue.
    let spy = Waiter::new();
    let _spy = spy.register(&tokens.queue, PollMask::empty());
    let mut second = pin!(wait_event_exclusive(
        &tokens.queue,
        || tokens.take(),
        &NoInterrupt
    ));
    {
        let mut first = pin!(wait_event_exclusive(
            &tokens.queue,
            || tokens.take(),
            &NoInterrupt
        ));
        assert!(poll_once(first.as_mut()).is_pending());
        assert!(poll_once(second.as_mut()).is_pending());
        tokens.put();
        assert!(spy.take_wakeup().is_some());
        // `first` took the wakeup but is dropped before it runs.
    }

    assert_eq!(spy.take_wakeup(), Some(PollMask::READABLE));
    assert_eq!(poll_once(second.as_mut()), Poll::Ready(Ok(())));
}

#[test]
fn wait_event_timeout_gives_up_when_the_timer_fires() {
    let tokens = Tokens::default();
    let expired = block_on(wait_event_timeout(
        &tokens.queue,
        || tokens.take(),
        &NoInterrupt,
        async {},
    ));
    assert_eq!(expired, Ok(None));

    tokens.put();
    let ready = block_on(wait_event_timeout(
        &tokens.queue,
        || tokens.take(),
        &NoInterrupt,
        async {},
    ));
    assert_eq!(ready, Ok(Some(())));
}

struct Signals {
    pending: AtomicBool,
    queue: WaitQueue,
}

impl Interrupt for Signals {
    fn pending(&self) -> bool {
        self.pending.load(Ordering::SeqCst)
    }

    fn queue(&self) -> Option<&WaitQueue> {
        Some(&self.queue)
    }
}

#[test]
fn a_signal_interrupts_the_wait() {
    let tokens = Tokens::default();
    let signals = Arc::new(Signals {
        pending: AtomicBool::new(false),
        queue: WaitQueue::new(),
    });
    let sender = {
        let signals = signals.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            signals.pending.store(true, Ordering::SeqCst);
            signals.queue.wake(PollMask::empty());
        })
    };

    let result = block_on(wait_event(&tokens.queue, || tokens.take(), &*signals));

    assert_eq!(result, Err(Interrupted));
    sender.join().unwrap();
}

#[test]
fn keyed_registrations_ignore_other_events() {
    let queue = WaitQueue::new();
    let waiter = Waiter::new();
    let _registration = waiter.register(&queue, PollMask::IN);

    queue.wake(PollMask::OUT);
    assert_eq!(waiter.take_wakeup(), None);

    queue.wake(PollMask::IN | PollMask::RDNORM);
    queue.wake(PollMask::empty());
    assert_eq!(waiter.take_wakeup(), Some(PollMask::IN | PollMask::RDNORM));
}

#[test]
fn poll_waiter_registers_once_and_sleeps_until_woken() {
    let tokens = Arc::new(Tokens::default());
    let mut poller = PollWaiter::new();

    assert_eq!(
        poller.poll(&*tokens, PollMask::IN, false),
        PollMask::WRITABLE
    );
    assert!(!tokens.queue.is_active());
    assert_eq!(
        poller.poll(&*tokens, PollMask::IN, true),
        PollMask::WRITABLE
    );
    assert!(tokens.queue.is_active());

    let producer = {
        let tokens = tokens.clone();
        thread::spawn(move || tokens.put())
    };
    let wakeup = block_on(poller.sleep(&NoInterrupt, None));
    producer.join().unwrap();

    assert_eq!(wakeup, Wakeup::Woken(PollMask::READABLE));
    assert!(
        poller
            .poll(&*tokens, PollMask::IN, false)
            .contains(PollMask::IN)
    );
    drop(poller);
    assert!(!tokens.queue.is_active());
}

#[test]
fn sleep_reports_the_timer() {
    let waiter = Waiter::new();
    let mut timer = pin!(async {});
    let wakeup = block_on(waiter.sleep(&NoInterrupt, Some(timer.as_mut())));
    assert_eq!(wakeup, Wakeup::TimedOut);
}
