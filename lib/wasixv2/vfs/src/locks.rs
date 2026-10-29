//! Advisory file locks (Linux's `fs/locks.c`): `flock` locks, which belong to an open file;
//! POSIX record locks, which belong to a process and go away when it closes *any* descriptor for
//! the file; and open file description (OFD) record locks, which belong to an open file.
//!
//! Record locks of both kinds conflict with each other, even within one process. `flock` locks
//! and record locks never conflict.
//!
//! Leases (`F_SETLEASE`) belong to an open file too. An open or truncation that conflicts with
//! one tells its holder ([`LeaseHolder`]) and waits until the holder gave way or its time
//! (`lease-break-time`) is up (`__break_lease`); see [`crate::Vfs`]'s `break_lease`.

use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
    time::Duration,
};

use async_trait::async_trait;
use wasixv2_abi::Errno;

use crate::{inode::Inode, types::FsResult};

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
    leases: Vec<Lease>,
}

impl ObjectLocks {
    fn is_empty(&self) -> bool {
        self.flocks.is_empty() && self.ranges.is_empty() && self.leases.is_empty()
    }
}

/// The holder of a lease, which hears when it must give way (`lease_manager_operations`).
/// Called with none of the lock manager's locks held.
pub trait LeaseHolder: Send + Sync {
    /// `lm_break`: an open or a truncation that conflicts with the lease began.
    fn lease_break(&self);
    /// The lease went, as its holder asked or once its break time was up (`lease_modify` to
    /// `F_UNLCK`); not when its file's last close takes it.
    fn lease_removed(&self);
}

/// How a call that breaks leases waits for them (`__break_lease`'s
/// `wait_event_interruptible_timeout`): the caller's clock and signals, and the machine's
/// `lease-break-time`.
#[async_trait(?Send)]
pub trait LeaseWait {
    /// The monotonic time the leases' deadlines are kept in.
    fn now(&self) -> Duration;
    /// `lease-break-time`: how long a holder has to give way, or `None` for as long as it
    /// takes.
    fn break_time(&self) -> Option<Duration>;
    /// Waits until `changed` completes or the clock reaches `deadline`. `EINTR` when a signal
    /// ends the wait; the kernel restarts the call.
    async fn wait<'a>(
        &'a self,
        changed: Pin<Box<dyn Future<Output = ()> + 'a>>,
        deadline: Option<Duration>,
    ) -> FsResult<()>;
    /// Waits for `answer` (a permission event's), ended early only by a fatal signal
    /// (`TASK_KILLABLE`), which restarts the call. By default it is not ended at all.
    async fn wait_killable<'a>(
        &'a self,
        answer: Pin<Box<dyn Future<Output = FsResult<()>> + 'a>>,
    ) -> FsResult<()> {
        answer.await
    }
}

/// One lease (`file_lease` with `FL_LEASE`), held by an open file.
struct Lease {
    file: u64,
    kind: LockKind,
    /// `FL_UNLOCK_PENDING`: an open for writing or a truncation waits for it to go, until
    /// `break_time`.
    unlock_pending: bool,
    /// `FL_DOWNGRADE_PENDING`: an open for reading waits for it to become a read lease, until
    /// `downgrade_time`.
    downgrade_pending: bool,
    break_time: Option<Duration>,
    downgrade_time: Option<Duration>,
    holder: Arc<dyn LeaseHolder>,
}

impl Lease {
    /// `lease_modify` to `kind` (not `F_UNLCK`), and what it no longer waits for
    /// (`lease_clear_pending`).
    fn modify(&mut self, kind: LockKind) {
        self.kind = kind;
        if kind == LockKind::Read {
            self.downgrade_pending = false;
        }
    }

    /// `target_leasetype`: what `F_GETLEASE` reports, `None` for `F_UNLCK`.
    fn target(&self) -> Option<LockKind> {
        match (self.unlock_pending, self.downgrade_pending) {
            (true, _) => None,
            (false, true) => Some(LockKind::Read),
            (false, false) => Some(self.kind),
        }
    }

    /// `leases_conflict`: either the lease or the breaker writes.
    fn conflicts(&self, write: bool) -> bool {
        write || self.kind == LockKind::Write
    }
}

/// The holders to tell, once the manager's lock is released (`lease_dispose_list`).
#[derive(Default)]
struct Calls(Vec<(Arc<dyn LeaseHolder>, bool)>);

impl Calls {
    fn broken(&mut self, lease: &Lease) {
        self.0.push((lease.holder.clone(), true));
    }

    fn removed(&mut self, lease: &Lease) {
        self.0.push((lease.holder.clone(), false));
    }

    fn run(self) {
        for (holder, broken) in self.0 {
            match broken {
                true => holder.lease_break(),
                false => holder.lease_removed(),
            }
        }
    }
}

/// What a lease request asks for (`F_SETLEASE` with `F_RDLCK` or `F_WRLCK`).
pub(crate) struct LeaseRequest<'a> {
    pub(crate) file: u64,
    pub(crate) kind: LockKind,
    /// The file's own share of the inode's open counts (`self_wcount`, `self_rcount`).
    pub(crate) own: (i32, i32),
    pub(crate) inode: &'a Inode,
    pub(crate) holder: Arc<dyn LeaseHolder>,
    /// `fs.leases-enable`.
    pub(crate) enabled: bool,
    pub(crate) now: Duration,
}

/// Where breaking an object's leases stands.
pub(crate) enum Break {
    /// No lease conflicts any more.
    Done,
    /// The first lease still in the way has until `deadline`; the leases changed last at
    /// `generation`.
    Wait {
        deadline: Option<Duration>,
        generation: u64,
    },
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
    /// How many leases exist, so that an open of a file without any looks no further
    /// (`break_lease`'s lockless check).
    leases: AtomicUsize,
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

    /// Drops the `flock` and OFD locks and the lease of an open file on its last close
    /// (`locks_remove_file`).
    pub fn release_file(&self, key: LockKey, file: u64) {
        self.edit(key, |locks| {
            locks.flocks.retain(|(f, _)| *f != file);
            locks.ranges.retain(|l| l.owner != LockOwner::File(file));
            let had = locks.leases.len();
            locks.leases.retain(|l| l.file != file);
            self.leases
                .fetch_sub(had - locks.leases.len(), Ordering::SeqCst);
        });
    }

    fn edit(&self, key: LockKey, f: impl FnOnce(&mut ObjectLocks)) {
        let mut state = self.state.lock().unwrap();
        let Some(locks) = state.objects.get_mut(&key) else {
            return;
        };
        f(locks);
        if locks.is_empty() {
            state.objects.remove(&key);
        }
        wake(&mut state, key);
    }

    /// Runs `f` on `key`'s locks, with the leases whose time is up gone first
    /// (`time_out_leases`), then wakes the object's waiters and tells the holders what `f`
    /// and the time did to their leases. Returns what `f` did and the generation it left.
    fn leases<R>(
        &self,
        key: LockKey,
        now: Duration,
        f: impl FnOnce(&mut ObjectLocks, &mut Calls, &AtomicUsize) -> R,
    ) -> (R, u64) {
        let mut calls = Calls::default();
        let done = {
            let mut state = self.state.lock().unwrap();
            let locks = state.objects.entry(key).or_default();
            self.time_out(locks, now, &mut calls);
            let result = f(locks, &mut calls, &self.leases);
            if locks.is_empty() {
                state.objects.remove(&key);
            }
            wake(&mut state, key);
            (result, state.generation)
        };
        calls.run();
        done
    }

    /// `time_out_leases`: a lease past its downgrade time becomes a read lease, one past its
    /// break time goes.
    fn time_out(&self, locks: &mut ObjectLocks, now: Duration, calls: &mut Calls) {
        let past = |t: Option<Duration>| t.is_some_and(|t| now > t);
        locks.leases.retain_mut(|lease| {
            if past(lease.downgrade_time) {
                lease.modify(LockKind::Read);
            }
            if past(lease.break_time) {
                calls.removed(lease);
                self.leases.fetch_sub(1, Ordering::SeqCst);
                return false;
            }
            true
        });
    }

    /// Whether any object has a lease (`break_lease`'s check, without the lock).
    pub(crate) fn any_leases(&self) -> bool {
        self.leases.load(Ordering::SeqCst) > 0
    }

    /// `generic_add_lease`: takes or changes `request.file`'s lease on `key`. `EAGAIN` while
    /// the file is open in a way the lease would hide (`lease_open_conflict`), or for another
    /// lease in the way: any for a write lease, one being broken for a new lease. A new lease
    /// needs `fs.leases-enable` (`EINVAL`).
    pub(crate) fn add_lease(&self, key: LockKey, request: LeaseRequest<'_>) -> FsResult<()> {
        let open_conflict = || match request.kind {
            LockKind::Read => request.inode.open_for_write(),
            LockKind::Write => request.inode.open_counts() != request.own,
        };
        self.leases(key, request.now, |locks, _, count| {
            if open_conflict() {
                return Err(Errno::EAGAIN);
            }
            let others = locks.leases.iter().filter(|l| l.file != request.file);
            for other in others {
                if request.kind == LockKind::Write || other.unlock_pending {
                    return Err(Errno::EAGAIN);
                }
            }
            if let Some(mine) = locks.leases.iter_mut().find(|l| l.file == request.file) {
                mine.modify(request.kind);
                mine.holder = request.holder;
                return Ok(());
            }
            if !request.enabled {
                return Err(Errno::EINVAL);
            }
            locks.leases.push(Lease {
                file: request.file,
                kind: request.kind,
                unlock_pending: false,
                downgrade_pending: false,
                break_time: None,
                downgrade_time: None,
                holder: request.holder,
            });
            // An open that raced past the check before the lease was in place.
            if open_conflict() {
                locks.leases.pop();
                return Err(Errno::EAGAIN);
            }
            count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .0
    }

    /// `generic_delete_lease`: removes `file`'s lease on `key`; `EAGAIN` without one.
    pub(crate) fn remove_lease(&self, key: LockKey, file: u64, now: Duration) -> FsResult<()> {
        self.leases(key, now, |locks, calls, count| {
            let at = locks.leases.iter().position(|l| l.file == file);
            let lease = locks.leases.remove(at.ok_or(Errno::EAGAIN)?);
            count.fetch_sub(1, Ordering::SeqCst);
            calls.removed(&lease);
            Ok(())
        })
        .0
    }

    /// `__fcntl_getlease`: `file`'s lease on `key` as it is to become, `None` for `F_UNLCK`.
    pub(crate) fn lease(&self, key: LockKey, file: u64, now: Duration) -> Option<LockKind> {
        if !self.any_leases() {
            return None;
        }
        self.leases(key, now, |locks, _, _| {
            let lease = locks.leases.iter().find(|l| l.file == file)?;
            lease.target()
        })
        .0
    }

    /// `__break_lease`'s step: an open (for writing with `write`) or a truncation tells the
    /// holders of the leases in its way that they must go, or become read leases for an open
    /// for reading, by `break_time` from `now`, unless they were told already; then says
    /// whether one is still in the way.
    pub(crate) fn break_leases(
        &self,
        key: LockKey,
        write: bool,
        now: Duration,
        break_time: Option<Duration>,
    ) -> Break {
        let deadline = break_time.map(|t| now + t);
        let (wait, generation) = self.leases(key, now, |locks, calls, _| {
            for lease in locks.leases.iter_mut().filter(|l| l.conflicts(write)) {
                if write {
                    if lease.unlock_pending {
                        continue;
                    }
                    lease.unlock_pending = true;
                    lease.break_time = deadline;
                } else {
                    if lease.unlock_pending || lease.downgrade_pending {
                        continue;
                    }
                    lease.downgrade_pending = true;
                    lease.downgrade_time = deadline;
                }
                calls.broken(lease);
            }
            let first = locks.leases.iter().find(|l| l.conflicts(write))?;
            Some(match first.unlock_pending {
                true => first.break_time,
                false => first.downgrade_time,
            })
        });
        match wait {
            None => Break::Done,
            Some(deadline) => Break::Wait {
                deadline,
                generation,
            },
        }
    }

    /// Completes once the locks of `key` changed after `generation`.
    pub(crate) fn changed_since(
        &self,
        key: LockKey,
        generation: u64,
    ) -> impl Future<Output = ()> + '_ {
        self.changed(key, generation)
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
    // Linux fs/locks.c:posix_lock_inode keeps each owner's ranges contiguous
    // and ordered by start; posix_test_lock reports the first conflicting one.
    let owner_start = ranges
        .iter()
        .position(|lock| lock.owner == new.owner)
        .unwrap_or(ranges.len());
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
    let mut at = owner_start;
    while at < ranges.len() && ranges[at].owner == new.owner && ranges[at].start < new.start {
        at += 1;
    }
    ranges.insert(at, new);
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
    fn getlk_reports_the_first_conflicting_range_of_an_owner() {
        for (later, earlier) in [
            (LockKind::Write, LockKind::Read),
            (LockKind::Read, LockKind::Write),
        ] {
            let locks = LockManager::default();
            let owner = LockOwner::Process(1);
            block_on(locks.set(KEY, lock(owner, later, 10, 14), false)).unwrap();
            let first = lock(owner, earlier, 1, 5);
            block_on(locks.set(KEY, first, false)).unwrap();
            // fs/locks.c:posix_test_lock walks each owner's ranges by start,
            // regardless of the order in which posix_lock_inode received them.
            let request = lock(LockOwner::Process(2), LockKind::Write, 0, u64::MAX);
            assert_eq!(locks.test(KEY, &request), Some(first));
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
