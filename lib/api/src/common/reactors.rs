use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::Arc;
use std::task::Waker;
use std::time::Duration;

/// Reactor pattern implementation that allows for web assembly
/// processes to easily implement asynchronous IO
#[derive(Debug, Clone)]
pub struct Reactors
{
    waker: Waker,
    woken: Arc<AtomicBool>,
    waiting: Arc<Mutex<VecDeque<mpsc::Sender<()>>>>,
}

impl Default
for Reactors {
    fn default() -> Self {
        let woken = Arc::new(AtomicBool::new(false));
        let waiting: Arc<Mutex<VecDeque<mpsc::Sender<()>>>> = Default::default();

        let waker = {
            let woken = woken.clone();
            let waiting = Arc::downgrade(&waiting);
            waker_fn::waker_fn(move || {
                if let Some(waiting) = waiting.upgrade() {
                    let mut guard = waiting.lock().unwrap();
                    woken.store(true, Ordering::Release);
                    if let Some(reactor) = guard.pop_front() {
                        let _ = reactor.send(());
                    }
                }
            })
        };

        Self {
            waker,
            woken,
            waiting,
        }
    }
}

impl Reactors
{
    /// Gets a reference to the waker that can be used for
    /// asynchronous calls
    pub fn get_waker(&self) -> Waker {
        self.waker.clone()
    }

    /// Wakes one of the reactors thats currently waiting
    pub fn wake(&self) {
        self.waker.wake_by_ref();
    }

    /// Wakes all of the reactors thats currently waiting
    pub fn wake_all(&self) {
        let mut guard = self.waiting.lock().unwrap();
        self.woken.store(true, Ordering::Release);
        guard.clear();
    }

    /// Returns true if woken, otherwise false for timeout
    pub fn wait(&self, timeout: Duration) -> bool {
        let rx = {
            let mut guard = self.waiting.lock().unwrap();
            if self.woken.compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                return true;
            }
            if timeout.is_zero() {
                return false;
            }

            let (tx, rx) = mpsc::channel();
            guard.push_back(tx);
            rx
        };
        match rx.recv_timeout(timeout) {
            Ok(_) => true,
            Err(_) => false,
        }
    }
}
