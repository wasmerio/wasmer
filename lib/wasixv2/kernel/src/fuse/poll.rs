//! fs/fuse/file.c:fuse_file_poll and fuse_notify_poll_wakeup.
use std::sync::{
    Arc,
    atomic::{AtomicU32, AtomicU64, Ordering},
};
use wasixv2_wait::{PollMask, WaitQueue};
pub(super) struct PollState {
    pub(super) key: u64,
    pub(super) ready: AtomicU32,
    pub(super) epoch: AtomicU64,
    pub(super) wait: WaitQueue,
}
impl PollState {
    pub(super) fn new(key: u64) -> Arc<Self> {
        Arc::new(Self {
            key,
            ready: AtomicU32::new(0),
            epoch: AtomicU64::new(0),
            wait: WaitQueue::new(),
        })
    }
    pub(super) fn notify(&self) {
        self.epoch.fetch_add(1, Ordering::Release);
        self.wait.wake(PollMask::empty());
    }
}
