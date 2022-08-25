use std::{
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

/// Represents a running thread which allows a joiner to
/// wait for the thread to exit
#[derive(Debug, Clone)]
pub struct WasiThread {
    finished: Arc<(Mutex<bool>, Condvar)>,
}

#[allow(clippy::mutex_atomic)]
impl Default for WasiThread {
    fn default() -> Self {
        Self {
            finished: Arc::new((Mutex::new(false), Condvar::default())),
        }
    }
}

#[allow(clippy::mutex_atomic)]
impl WasiThread {
    /// Marks the thread as finished (which will cause anyone that
    /// joined on it to wake up)
    pub fn mark_finished(&self) {
        let mut guard = self.finished.0.lock().unwrap();
        *guard = true;
        self.finished.1.notify_all();
    }

    /// Waits until the thread is finished or the timeout is reached
    pub fn join(&self, timeout: Duration) -> bool {
        let mut finished = self.finished.0.lock().unwrap();
        if *finished {
            return true;
        }
        loop {
            let woken = self.finished.1.wait_timeout(finished, timeout).unwrap();
            if woken.1.timed_out() {
                return false;
            }
            finished = woken.0;
            if *finished {
                return true;
            }
        }
    }
}
