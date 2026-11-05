//! fs/fuse/file.c:fuse_file_poll and fuse_notify_poll_wakeup.
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
};
use wasixv2_wait::{PollMask, WaitQueue};
pub(super) struct PollState {
    pub(super) key: u64,
    pub(super) ready: AtomicU32,
    pub(super) epoch: AtomicU64,
    pub(super) wait: WaitQueue,
    pub(super) completed: AtomicU64,
    pub(super) in_flight: AtomicBool,
    pub(super) events: AtomicU32,
}
impl PollState {
    pub(super) fn new(key: u64) -> Arc<Self> {
        Arc::new(Self {
            key,
            ready: AtomicU32::new(0),
            epoch: AtomicU64::new(1),
            completed: AtomicU64::new(0),
            in_flight: AtomicBool::new(false),
            events: AtomicU32::new(0),
            wait: WaitQueue::new(),
        })
    }
    pub(super) fn interest(&self, events: u32) {
        if self.events.fetch_or(events, Ordering::AcqRel) & events != events {
            self.epoch.fetch_add(1, Ordering::Release);
        }
    }
    pub(super) fn stale(&self) -> bool {
        self.epoch.load(Ordering::Acquire) != self.completed.load(Ordering::Acquire)
    }
    pub(super) fn complete(&self, epoch: u64, mask: u32) {
        self.ready.store(mask, Ordering::Release);
        self.completed.store(epoch, Ordering::Release);
        self.in_flight.store(false, Ordering::Release);
        self.wait.wake(PollMask::empty());
    }
    pub(super) fn notify(&self) {
        self.epoch.fetch_add(1, Ordering::Release);
        self.wait.wake(PollMask::empty());
    }
}
