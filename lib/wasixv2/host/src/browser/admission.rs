use std::{
    collections::BTreeMap,
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, AtomicUsize, Ordering},
    },
};

use crate::ThreadStart;

pub(super) struct Capacity {
    limit: usize,
    active: AtomicUsize,
}

impl Capacity {
    pub(super) fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            limit,
            active: AtomicUsize::new(0),
        })
    }

    fn acquire(self: &Arc<Self>) -> io::Result<Lease> {
        self.active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < self.limit).then_some(active + 1)
            })
            .map_err(|_| io::Error::from(io::ErrorKind::WouldBlock))?;
        Ok(Lease(self.clone()))
    }
}

pub(super) struct Lease(Arc<Capacity>);

impl Drop for Lease {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::Release);
    }
}

struct Job {
    accepted: bool,
    start: ThreadStart,
    lease: Lease,
}

#[derive(Default)]
pub(super) struct Admissions {
    next: AtomicU32,
    jobs: Mutex<BTreeMap<u32, Job>>,
}

impl Admissions {
    pub(super) fn reserve<'a>(
        &'a self,
        capacity: &Arc<Capacity>,
        start: ThreadStart,
    ) -> io::Result<Ticket<'a>> {
        let lease = capacity.acquire()?;
        // Never recycle an ID: a delayed message cannot claim a later thread.
        let id = self
            .next
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| io::Error::from(io::ErrorKind::OutOfMemory))?;
        self.jobs.lock().unwrap().insert(
            id,
            Job {
                accepted: false,
                start,
                lease,
            },
        );
        Ok(Ticket {
            admissions: self,
            id,
            committed: false,
        })
    }

    pub(super) fn claim(&self, id: u32) -> io::Result<(ThreadStart, Lease)> {
        let mut jobs = self.jobs.lock().unwrap();
        if !jobs.get(&id).is_some_and(|job| job.accepted) {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let Job { start, lease, .. } = jobs.remove(&id).unwrap();
        Ok((start, lease))
    }
}

pub(super) struct Ticket<'a> {
    admissions: &'a Admissions,
    pub(super) id: u32,
    committed: bool,
}

impl Ticket<'_> {
    pub(super) fn accept(&self) {
        self.admissions
            .jobs
            .lock()
            .unwrap()
            .get_mut(&self.id)
            .unwrap()
            .accepted = true;
    }

    pub(super) fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for Ticket<'_> {
    fn drop(&mut self) {
        if !self.committed {
            // Drop user code after releasing the registry lock.
            let job = self.admissions.jobs.lock().unwrap().remove(&self.id);
            drop(job);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DropCount(Arc<AtomicUsize>);
    impl Drop for DropCount {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn job(drops: &Arc<AtomicUsize>) -> ThreadStart {
        let count = DropCount(drops.clone());
        Box::new(move || {
            drop(count);
            Box::pin(async {})
        })
    }

    #[test]
    fn failed_and_canceled_admissions_drop_once_and_release_capacity() {
        let admissions = Admissions::default();
        let capacity = Capacity::new(1);
        let drops = Arc::new(AtomicUsize::new(0));
        let ticket = admissions.reserve(&capacity, job(&drops)).unwrap();
        assert!(admissions.claim(ticket.id).is_err());
        assert!(admissions.reserve(&capacity, job(&drops)).is_err());
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        let old_id = ticket.id;
        drop(ticket);
        assert_eq!(drops.load(Ordering::Relaxed), 2);
        let ticket = admissions.reserve(&capacity, job(&drops)).unwrap();
        assert_ne!(ticket.id, old_id);
        assert!(admissions.claim(old_id).is_err());
        drop(ticket);
        assert_eq!(drops.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn ownership_can_be_claimed_only_once_and_capacity_covers_execution() {
        let admissions = Admissions::default();
        let capacity = Capacity::new(1);
        let drops = Arc::new(AtomicUsize::new(0));
        let ticket = admissions.reserve(&capacity, job(&drops)).unwrap();
        let id = ticket.id;
        ticket.accept();
        ticket.commit();
        let (start, lease) = admissions.claim(id).unwrap();
        assert!(admissions.claim(id).is_err());
        assert!(admissions.reserve(&capacity, job(&drops)).is_err());
        drop(start());
        assert_eq!(drops.load(Ordering::Relaxed), 2);
        drop(lease);
        assert!(admissions.reserve(&capacity, job(&drops)).is_ok());
    }
}
