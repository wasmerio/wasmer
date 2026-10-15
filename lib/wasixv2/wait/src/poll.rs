use crate::{PollMask, WaitQueue};

/// Linux's `poll_table`: what `Pollable::poll` registers its wait queues with.
///
/// The poller (poll, select, epoll) supplies the callback that puts its own entry on each queue,
/// plus the key: the events it is interested in, which objects may use to skip queues that can
/// never report them.
pub struct PollTable<'a> {
    key: PollMask,
    queue_proc: &'a mut dyn FnMut(&WaitQueue, PollMask),
}

impl<'a> PollTable<'a> {
    pub fn new(key: PollMask, queue_proc: &'a mut dyn FnMut(&WaitQueue, PollMask)) -> Self {
        Self { key, queue_proc }
    }

    /// `poll_wait`: registers the poller on `queue`. Objects call this for every queue whose
    /// wakeups can change the mask they report.
    pub fn wait_on(&mut self, queue: &WaitQueue) {
        (self.queue_proc)(queue, self.key);
    }

    /// `poll_requested_events`: the events the poller cares about. Empty means all.
    pub fn key(&self) -> PollMask {
        self.key
    }
}

/// An object whose readiness can be polled: Linux's `f_op->poll`.
pub trait Pollable: Send + Sync {
    /// Registers on every relevant queue through `table` (when given) and returns the current
    /// readiness. Must not block. A `None` table only queries the mask, like `vfs_poll` with a
    /// NULL table.
    ///
    /// The result may contain bits the poller did not ask for; pollers filter. `ERR` and `HUP`
    /// are always reported to pollers, whatever they asked for.
    fn poll(&self, table: Option<&mut PollTable<'_>>) -> PollMask;
}

/// `poll_wait` for an optional table, which is how objects usually hold it.
pub fn poll_wait(table: &mut Option<&mut PollTable<'_>>, queue: &WaitQueue) {
    if let Some(table) = table {
        table.wait_on(queue);
    }
}
