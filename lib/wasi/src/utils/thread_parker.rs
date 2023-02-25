use std::{
    sync::{Arc, Condvar, Mutex},
    task::Waker,
    time::Duration,
};

/// Represents a waker that can be used to put a thread to
/// sleep while it waits for an event to occur
#[derive(Debug)]
pub struct WasiParkingLot {
    waker: Waker,
    #[allow(dead_code)]
    run: Arc<(Mutex<bool>, Condvar)>,
}

impl Default for WasiParkingLot {
    fn default() -> Self {
        Self::new(true)
    }
}

impl WasiParkingLot {
    /// Creates a new parking lot with a specific value
    pub fn new(initial_val: bool) -> Self {
        let run = Arc::new((Mutex::new(initial_val), Condvar::default()));
        let waker = {
            let run = run.clone();
            waker_fn::waker_fn(move || {
                let mut guard = run.0.lock().unwrap();
                *guard = true;
                run.1.notify_one();
            })
        };

        Self { waker, run }
    }

    /// Gets a reference to the waker that can be used for
    /// asynchronous calls
    pub fn get_waker(&self) -> Waker {
        self.waker.clone()
    }

    /// Wakes one of the reactors thats currently waiting
    pub fn wake(&self) {
        self.waker.wake_by_ref();
    }

    /// Will wait until either the reactor is triggered
    /// or the timeout occurs
    // TODO: review allow...
    #[allow(dead_code)]
    pub fn wait(&self, timeout: Duration) -> bool {
        let mut run = self.run.0.lock().unwrap();
        if *run {
            *run = false;
            return true;
        }
        loop {
            let woken = self.run.1.wait_timeout(run, timeout).unwrap();
            if woken.1.timed_out() {
                return false;
            }
            run = woken.0;
            if *run {
                *run = false;
                return true;
            }
        }
    }
}
