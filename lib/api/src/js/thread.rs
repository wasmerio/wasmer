use std::{sync::{Arc, mpsc, Mutex}, time::Duration};

/// Contains controls for the instance
#[derive(Debug, Clone)]
pub struct ThreadControl {
    id: u32,
    /// Signalers used to tell joiners that the thread has exited
    exit: Arc<Mutex<Option<mpsc::Sender<()>>>>,
    /// Event to wait on for the thread to join
    join: Arc<Mutex<mpsc::Receiver<()>>>,
}

impl ThreadControl {
    /// Creates a thread control object the a specific unique identifier
    pub fn new(id: u32) -> ThreadControl {
        let (tx, rx) = mpsc::channel();
        ThreadControl {
            id,
            exit: Arc::new(Mutex::new(Some(tx))),
            join: Arc::new(Mutex::new(rx)),
        }
    }

    /// Returns the unique ID of this thread
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Makes the instance thread that its exited
    pub fn mark_exited(&self) {
        let mut guard = self.exit.lock().unwrap();
        guard.take();
    }
    
    /// Waits for the thread to exit (false = timeout)
    pub fn join(&self, timeout: Duration) -> bool {
        let guard = self.join.lock().unwrap();
        match guard.recv_timeout(timeout) {
            Ok(_) => true,
            Err(mpsc::RecvTimeoutError::Disconnected) => true,
            Err(mpsc::RecvTimeoutError::Timeout) => false,
        }
    }
}