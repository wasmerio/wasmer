
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use virtual_mio::InterestHandler;

#[derive(Debug)]
pub struct NotificationInner {
    value: AtomicU64,
    is_semaphore: bool,
    handler: Mutex<Option<Box<dyn InterestHandler + Send + Sync>>>,
}

impl NotificationInner {
    pub fn new(initial: u64, is_semaphore: bool) -> Self {
        Self {
            value: AtomicU64::new(initial),
            is_semaphore,
            handler: Mutex::new(None),
        }
    }

    pub fn read(&self) -> Option<u64> {
        if self.is_semaphore {
            let current = self.value.load(Ordering::Acquire);
            if current == 0 {
                return None;
            }
            self.value.fetch_sub(1, Ordering::AcqRel);
            Some(1)
        } else {
            let prev = self.value.swap(0, Ordering::AcqRel);
            if prev == 0 { None } else { Some(prev) }
        }
    }

    pub fn write(&self, val: u64) {
        if val == 0 {
            return;
        }
        self.value.fetch_add(val, Ordering::AcqRel);
        if let Some(handler) = &mut *self.handler.lock().unwrap() {
            handler.push_interest(virtual_mio::InterestType::Readable);
        }
    }

    pub fn add_interest_handler(&self, handler: Box<dyn InterestHandler + Send + Sync>) {
        let mut guard = self.handler.lock().unwrap();
        *guard = Some(handler);
    }

    pub fn remove_interest_handler(&self) {
        let mut guard = self.handler.lock().unwrap();
        *guard = None;
    }
}
