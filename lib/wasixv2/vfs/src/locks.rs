//! Advisory file locks (Linux's `fs/locks.c`): `flock` locks, which belong to an open file;
//! POSIX record locks, which belong to a process and go away when it closes *any* descriptor for
//! the file; and open file description (OFD) record locks, which belong to an open file.
//!
//! Record locks of both kinds conflict with each other, even within one process. `flock` locks
//! and record locks never conflict.

use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::Mutex,
    task::{Context, Poll, Waker},
};

use wasixv2_abi::Errno;

use crate::types::FsResult;

/// Identifies a locked object: superblock id and inode number.
pub type LockKey = (u64, u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlockOp {
    Shared,
    Exclusive,
    Unlock,
}

/// Who holds a record lock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LockOwner {
    /// A process (POSIX `F_SETLK`), by its file table's identity.
    Process(u64),
    /// An open file description (`F_OFD_SETLK`), by the file's id.
    File(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockKind {
    Read,
    Write,
}

/// A byte-range lock over `start..=end`; `end == u64::MAX` reaches past any end of file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RangeLock {
    pub owner: LockOwner,
    /// What `F_GETLK` reports as `l_pid`.
    pub pid: i32,
    pub kind: LockKind,
    pub start: u64,
    pub end: u64,
}

impl RangeLock {
    fn overlaps(&self, other: &RangeLock) -> bool {
        self.start <= other.end && other.start <= self.end
    }

    fn conflicts(&self, other: &RangeLock) -> bool {
        self.owner != other.owner
            && self.overlaps(other)
            && (self.kind == LockKind::Write || other.kind == LockKind::Write)
    }
}

#[derive(Default)]
struct ObjectLocks {
    /// `(file id, exclusive)`.
    flocks: Vec<(u64, bool)>,
    ranges: Vec<RangeLock>,
}

#[derive(Default)]
struct State {
    objects: HashMap<LockKey, ObjectLocks>,
    /// Process owners blocked in `F_SETLKW`, and on which lock, for deadlock detection.
    waiting: HashMap<LockOwner, RangeLock>,
    /// Bumped by every change; a waiter that saw an older one looks again.
    generation: u64,
    /// Waiters, by the object whose locks they wait on: a change wakes only its object's.
    wakers: HashMap<LockKey, Vec<Waker>>,
}

/// The lock tables of one VFS.
#[derive(Default)]
pub struct LockManager {
    state: Mutex<State>,
}

/// Linux's `MAX_DEADLK_ITERATIONS`.
const MAX_DEADLOCK_STEPS: usize = 10;

impl LockManager {
    /// `flock`: converts or takes a whole-file lock for `file`, waiting unless `nonblock`.
    /// Conversion drops the old lock first, so it is not atomic, as on Linux.
    pub async fn flock(
        &self,
        key: LockKey,
        file: u64,
        op: FlockOp,
        nonblock: bool,
    ) -> FsResult<()> {
        let exclusive = match op {
            FlockOp::Unlock => {
                self.edit(key, |locks| locks.flocks.retain(|(f, _)| *f != file));
                return Ok(());
            }
            FlockOp::Shared => false,
            FlockOp::Exclusive => true,
        };
        {
            let mut state = self.state.lock().unwrap();
            let locks = state.objects.entry(key).or_default();
            if locks.flocks.contains(&(file, exclusive)) {
                return Ok(());
            }
            let had = locks.flocks.len();
            locks.flocks.retain(|(f, _)| *f != file);
            if locks.flocks.len() != had {
                wake(&mut state, key);
            }
        }
        loop {
            let generation = {
                let mut state = self.state.lock().unwrap();
                let locks = state.objects.entry(key).or_default();
                let conflict = locks
                    .flocks
                    .iter()
                    .any(|&(f, ex)| f != file && (ex || exclusive));
                if !conflict {
                    locks.flocks.push((file, exclusive));
                    return Ok(());
                }
                if nonblock {
                    return Err(Errno::EWOULDBLOCK);
                }
                state.generation
            };
            self.changed(key, generation).await;
        }
    }

    /// `F_GETLK`: the first lock that would block `request`.
    pub fn test(&self, key: LockKey, request: &RangeLock) -> Option<RangeLock> {
        let state = self.state.lock().unwrap();
        let locks = state.objects.get(&key)?;
        locks.ranges.iter().find(|l| l.conflicts(request)).copied()
    }

    /// `F_SETLK`/`F_SETLKW`: sets `request`, waiting for conflicting locks if `wait`.
    pub async fn set(&self, key: LockKey, request: RangeLock, wait: bool) -> FsResult<()> {
        loop {
            let generation = {
                let mut state = self.state.lock().unwrap();
                let locks = state.objects.entry(key).or_default();
                match locks.ranges.iter().find(|l| l.conflicts(&request)).copied() {
                    None => {
                        insert_range(&mut locks.ranges, request);
                        state.waiting.remove(&request.owner);
                        wake(&mut state, key);
                        return Ok(());
                    }
                    Some(_) if !wait => return Err(Errno::EAGAIN),
                    Some(blocker) => {
                        if matches!(request.owner, LockOwner::Process(_))
                            && deadlocks(&state, request.owner, blocker.owner)
                        {
                            state.waiting.remove(&request.owner);
                            return Err(Errno::EDEADLK);
                        }
                        state.waiting.insert(request.owner, blocker);
                    }
                }
                state.generation
            };
            let guard = WaitingGuard(self, request.owner);
            self.changed(key, generation).await;
            drop(guard);
        }
    }

    /// Removes `owner`'s locks over `start..=end` (`F_UNLCK`).
    pub fn unlock(&self, key: LockKey, owner: LockOwner, start: u64, end: u64) {
        self.edit(key, |locks| {
            remove_range(&mut locks.ranges, owner, start, end);
        });
    }

    /// Drops every record lock `owner` holds on `key`: what closing any descriptor does to a
    /// process's POSIX locks (`locks_remove_posix`).
    pub fn release_owner(&self, key: LockKey, owner: LockOwner) {
        self.unlock(key, owner, 0, u64::MAX);
    }

    /// Drops the `flock` and OFD locks of an open file on its last close (`locks_remove_file`).
    pub fn release_file(&self, key: LockKey, file: u64) {
        self.edit(key, |locks| {
            locks.flocks.retain(|(f, _)| *f != file);
            locks.ranges.retain(|l| l.owner != LockOwner::File(file));
        });
    }

    fn edit(&self, key: LockKey, f: impl FnOnce(&mut ObjectLocks)) {
        let mut state = self.state.lock().unwrap();
        let Some(locks) = state.objects.get_mut(&key) else {
            return;
        };
        f(locks);
        if locks.flocks.is_empty() && locks.ranges.is_empty() {
            state.objects.remove(&key);
        }
        wake(&mut state, key);
    }

    /// Completes once `key`'s locks changed after `generation` (or, spuriously, others').
    fn changed(&self, key: LockKey, generation: u64) -> Changed<'_> {
        Changed {
            manager: self,
            key,
            generation,
        }
    }
}

fn wake(state: &mut State, key: LockKey) {
    state.generation += 1;
    for waker in state.wakers.remove(&key).into_iter().flatten() {
        waker.wake();
    }
}

/// `posix_locks_deadlock`: whether `blocker` is, through a chain of waits, waiting for `owner`.
fn deadlocks(state: &State, owner: LockOwner, mut blocker: LockOwner) -> bool {
    for _ in 0..MAX_DEADLOCK_STEPS {
        if blocker == owner {
            return true;
        }
        match state.waiting.get(&blocker) {
            Some(lock) => blocker = lock.owner,
            None => return false,
        }
    }
    false
}

/// Replaces `request.owner`'s locks in the range with `request`, merging with adjacent or
/// overlapping locks of the same kind (`posix_lock_inode`).
fn insert_range(ranges: &mut Vec<RangeLock>, request: RangeLock) {
    let mut new = request;
    ranges.retain(|l| {
        let adjacent =
            l.end.checked_add(1) == Some(new.start) || new.end.checked_add(1) == Some(l.start);
        if l.owner == new.owner && l.kind == new.kind && (l.overlaps(&new) || adjacent) {
            new.start = new.start.min(l.start);
            new.end = new.end.max(l.end);
            false
        } else {
            true
        }
    });
    remove_range(ranges, new.owner, new.start, new.end);
    ranges.push(new);
}

/// Cuts `start..=end` out of `owner`'s locks, splitting any that straddle it.
fn remove_range(ranges: &mut Vec<RangeLock>, owner: LockOwner, start: u64, end: u64) {
    let mut kept = Vec::with_capacity(ranges.len());
    for lock in ranges.drain(..) {
        if lock.owner != owner || lock.end < start || lock.start > end {
            kept.push(lock);
            continue;
        }
        if lock.start < start {
            kept.push(RangeLock {
                end: start - 1,
                ..lock
            });
        }
        if lock.end > end {
            kept.push(RangeLock {
                start: end + 1,
                ..lock
            });
        }
    }
    *ranges = kept;
}

struct WaitingGuard<'a>(&'a LockManager, LockOwner);

impl Drop for WaitingGuard<'_> {
    fn drop(&mut self) {
        self.0.state.lock().unwrap().waiting.remove(&self.1);
    }
}

struct Changed<'a> {
    manager: &'a LockManager,
    key: LockKey,
    generation: u64,
}

impl Future for Changed<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut state = self.manager.state.lock().unwrap();
        if state.generation != self.generation {
            return Poll::Ready(());
        }
        let key = self.key;
        let wakers = state.wakers.entry(key).or_default();
        if !wakers.iter().any(|w| w.will_wake(cx.waker())) {
            wakers.push(cx.waker().clone());
        }
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use futures::{FutureExt, executor::block_on};

    use super::*;

    const KEY: LockKey = (1, 1);

    fn lock(owner: LockOwner, kind: LockKind, start: u64, end: u64) -> RangeLock {
        RangeLock {
            owner,
            pid: 1,
            kind,
            start,
            end,
        }
    }

    #[test]
    fn posix_locks_split_and_conflict_across_owners() {
        let locks = LockManager::default();
        let (a, b) = (LockOwner::Process(1), LockOwner::Process(2));
        block_on(locks.set(KEY, lock(a, LockKind::Write, 0, 99), false)).unwrap();
        locks.unlock(KEY, a, 10, 19);

        assert_eq!(locks.test(KEY, &lock(b, LockKind::Read, 10, 19)), None);
        assert_eq!(
            locks
                .test(KEY, &lock(b, LockKind::Read, 15, 25))
                .map(|l| l.start),
            Some(20)
        );
        assert_eq!(
            block_on(locks.set(KEY, lock(b, LockKind::Read, 0, 0), false)),
            Err(Errno::EAGAIN)
        );
        locks.release_owner(KEY, a);
        assert_eq!(
            block_on(locks.set(KEY, lock(b, LockKind::Write, 0, u64::MAX), false)),
            Ok(())
        );
    }

    #[test]
    fn ofd_and_posix_locks_conflict_but_flock_is_separate() {
        let locks = LockManager::default();
        block_on(locks.set(KEY, lock(LockOwner::File(7), LockKind::Write, 0, 9), false)).unwrap();

        assert!(
            locks
                .test(KEY, &lock(LockOwner::Process(1), LockKind::Read, 5, 5))
                .is_some()
        );
        assert_eq!(
            block_on(locks.flock(KEY, 7, FlockOp::Exclusive, true)),
            Ok(())
        );
        assert_eq!(
            block_on(locks.flock(KEY, 8, FlockOp::Shared, true)),
            Err(Errno::EWOULDBLOCK)
        );
        locks.release_file(KEY, 7);
        assert_eq!(block_on(locks.flock(KEY, 8, FlockOp::Shared, true)), Ok(()));
    }

    #[test]
    fn a_change_wakes_only_the_waiters_of_its_object() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        struct Count(AtomicUsize);
        impl futures::task::ArcWake for Count {
            fn wake_by_ref(this: &Arc<Self>) {
                this.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        const OTHER: LockKey = (1, 2);
        let locks = LockManager::default();
        let (a, b) = (LockOwner::Process(1), LockOwner::Process(2));
        for key in [KEY, OTHER] {
            block_on(locks.set(key, lock(a, LockKind::Write, 0, 0), false)).unwrap();
        }
        let count = Arc::new(Count(AtomicUsize::new(0)));
        let waker = futures::task::waker(count.clone());
        let mut waiter = Box::pin(locks.set(KEY, lock(b, LockKind::Write, 0, 0), true));
        let mut cx = Context::from_waker(&waker);
        assert!(waiter.as_mut().poll(&mut cx).is_pending());

        locks.release_owner(OTHER, a);
        assert_eq!(count.0.load(Ordering::SeqCst), 0);
        locks.release_owner(KEY, a);
        assert_eq!(count.0.load(Ordering::SeqCst), 1);
        assert_eq!(waiter.as_mut().poll(&mut cx), Poll::Ready(Ok(())));
    }

    #[test]
    fn blocked_lock_is_granted_on_release_and_deadlock_is_detected() {
        let locks = LockManager::default();
        let (a, b) = (LockOwner::Process(1), LockOwner::Process(2));
        block_on(locks.set(KEY, lock(a, LockKind::Write, 0, 0), false)).unwrap();
        block_on(locks.set(KEY, lock(b, LockKind::Write, 1, 1), false)).unwrap();
        let mut waiter = Box::pin(locks.set(KEY, lock(b, LockKind::Write, 0, 0), true));
        assert!((&mut waiter).now_or_never().is_none());

        // `a` now waiting for `b` would close a cycle.
        assert_eq!(
            block_on(locks.set(KEY, lock(a, LockKind::Write, 1, 1), true)),
            Err(Errno::EDEADLK)
        );
        locks.release_owner(KEY, a);
        assert_eq!(waiter.now_or_never(), Some(Ok(())));
    }
}
