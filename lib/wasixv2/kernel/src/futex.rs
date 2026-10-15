//! Futexes (`kernel/futex`): wait queues keyed by address, for the tasks of one address space.
//!
//! A waiter sits on the queue of its key until a wake takes it off, so a wait never misses a
//! wake that follows a change of the futex word: the waiter queues first and compares the word
//! after, as Linux does under the hash bucket lock.

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use wasixv2_abi::{ConstKernelTimespecPtr, FutexOp, GuestPtr, GuestPtrMut, U32Ptr, Ulong};
use wasixv2_syscalls::{Caller, Errno, SysErr, SysResult, read_value, write_value};
use wasixv2_wait::{PollMask, WaitQueue, Waiter, Wakeup};

use crate::{Kernel, time};

/// `FUTEX_BITSET_MATCH_ANY`.
const MATCH_ANY: u32 = u32::MAX;
const FUTEX_WAITERS: u32 = 0x8000_0000;
const FUTEX_OWNER_DIED: u32 = 0x4000_0000;
const FUTEX_TID_MASK: u32 = 0x3fff_ffff;
/// `ROBUST_LIST_LIMIT`: how many entries a robust list walk visits at most.
const ROBUST_LIST_LIMIT: usize = 2048;

/// A futex: the address space (named by its thread group's pid; address spaces are never shared
/// between thread groups) and the address of the word.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Key {
    pub(crate) mm: u32,
    pub(crate) addr: u32,
}

/// One blocked `FUTEX_WAIT`.
struct Entry {
    bitset: u32,
    /// Set by the wake that took the entry off its queue.
    woken: AtomicBool,
    /// The waiter sleeps on this; a wake wakes it.
    queue: WaitQueue,
}

#[derive(Default)]
pub(crate) struct Futexes {
    queues: Mutex<HashMap<Key, VecDeque<Arc<Entry>>>>,
}

impl Futexes {
    fn enqueue(&self, key: Key, entry: Arc<Entry>) {
        self.queues
            .lock()
            .unwrap()
            .entry(key)
            .or_default()
            .push_back(entry);
    }

    /// Takes `entry` off whatever queue it is on. False if a wake took it first.
    fn dequeue(&self, entry: &Arc<Entry>) -> bool {
        let mut queues = self.queues.lock().unwrap();
        let mut found = None;
        for (key, queue) in queues.iter_mut() {
            if let Some(at) = queue.iter().position(|e| Arc::ptr_eq(e, entry)) {
                queue.remove(at);
                found = Some(*key);
                break;
            }
        }
        match found {
            Some(key) => {
                if queues.get(&key).is_some_and(VecDeque::is_empty) {
                    queues.remove(&key);
                }
                true
            }
            None => false,
        }
    }

    /// `futex_wake`: wakes up to `n` waiters on `key` whose bitset intersects `bitset`. As on
    /// Linux, a count of 0 still wakes one.
    pub(crate) fn wake(&self, key: Key, n: u32, bitset: u32) -> u32 {
        let mut queues = self.queues.lock().unwrap();
        let Some(queue) = queues.get_mut(&key) else {
            return 0;
        };
        let mut woken = 0;
        let mut at = 0;
        while at < queue.len() {
            if queue[at].bitset & bitset == 0 {
                at += 1;
                continue;
            }
            let entry = queue.remove(at).expect("index in range");
            entry.woken.store(true, Ordering::Release);
            entry.queue.wake(PollMask::empty());
            woken += 1;
            if woken >= n.max(1) {
                break;
            }
        }
        if queue.is_empty() {
            queues.remove(&key);
        }
        woken
    }

    /// `futex_requeue`: wakes up to `n_wake` waiters on `from` and moves up to `n_requeue` of the
    /// rest to `to`. Returns the number woken plus the number moved.
    fn requeue(&self, from: Key, to: Key, n_wake: u32, n_requeue: u32) -> u32 {
        let woken = match n_wake {
            0 => 0,
            n => self.wake(from, n, MATCH_ANY),
        };
        let mut queues = self.queues.lock().unwrap();
        let Some(queue) = queues.get_mut(&from) else {
            return woken;
        };
        let take = (n_requeue as usize).min(queue.len());
        let moved: Vec<_> = queue.drain(..take).collect();
        if queue.is_empty() {
            queues.remove(&from);
        }
        if !moved.is_empty() {
            queues.entry(to).or_default().extend(moved);
        }
        woken + take as u32
    }
}

/// How long a `FUTEX_WAIT` may block.
enum Timeout {
    Never,
    /// Relative, on the monotonic clock.
    After(Duration),
    /// Absolute, on the monotonic clock.
    Until(Duration),
}

impl Kernel {
    fn futex_key(&self, caller: &Caller<'_>, addr: u32) -> Key {
        Key {
            mm: self.process(caller.task()).pid.0,
            addr,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn sys_futex(
        &self,
        caller: &Caller<'_>,
        uaddr: U32Ptr,
        op: FutexOp,
        val: u32,
        utime: Ulong,
        uaddr2: U32Ptr,
        val3: u32,
    ) -> SysResult<i32> {
        let cmd = FutexOp(op.0 & FutexOp::FUTEX_CMD_MASK.0);
        let realtime = op.0 & FutexOp::FUTEX_CLOCK_REALTIME.0 != 0;
        if realtime && cmd != FutexOp::FUTEX_WAIT_BITSET {
            return Err(Errno::ENOSYS.into());
        }
        if !uaddr.addr().is_multiple_of(4) {
            return Err(Errno::EINVAL.into());
        }
        let key = self.futex_key(caller, uaddr.addr());
        match cmd {
            FutexOp::FUTEX_WAIT | FutexOp::FUTEX_WAIT_BITSET => {
                let bitset = match cmd {
                    FutexOp::FUTEX_WAIT => MATCH_ANY,
                    _ => val3,
                };
                if bitset == 0 {
                    return Err(Errno::EINVAL.into());
                }
                let timeout = match utime {
                    0 => Timeout::Never,
                    ptr => {
                        let ts =
                            read_value(caller.memory(), ConstKernelTimespecPtr::new(ptr)).await?;
                        let ts = time::from_timespec(ts)?;
                        match (cmd, realtime) {
                            (FutexOp::FUTEX_WAIT, _) => Timeout::After(ts),
                            (_, false) => Timeout::Until(ts),
                            (_, true) => Timeout::After(ts.saturating_sub(self.host.realtime())),
                        }
                    }
                };
                self.futex_wait(caller, key, val, bitset, timeout).await
            }
            FutexOp::FUTEX_WAKE => Ok(self.futexes.wake(key, val, MATCH_ANY) as i32),
            FutexOp::FUTEX_WAKE_BITSET => match val3 {
                0 => Err(Errno::EINVAL.into()),
                bitset => Ok(self.futexes.wake(key, val, bitset) as i32),
            },
            FutexOp::FUTEX_REQUEUE | FutexOp::FUTEX_CMP_REQUEUE => {
                // `val2` arrives in the timeout slot.
                let (n_wake, n_requeue) = (val as i32, utime as i32);
                if n_wake < 0 || n_requeue < 0 {
                    return Err(Errno::EINVAL.into());
                }
                if !uaddr2.addr().is_multiple_of(4) {
                    return Err(Errno::EINVAL.into());
                }
                if cmd == FutexOp::FUTEX_CMP_REQUEUE {
                    let current: u32 =
                        read_value(caller.memory(), GuestPtr::new(uaddr.addr())).await?;
                    if current != val3 {
                        return Err(Errno::EAGAIN.into());
                    }
                }
                let to = self.futex_key(caller, uaddr2.addr());
                Ok(self
                    .futexes
                    .requeue(key, to, n_wake as u32, n_requeue as u32) as i32)
            }
            FutexOp::FUTEX_WAKE_OP => {
                if !uaddr2.addr().is_multiple_of(4) {
                    return Err(Errno::EINVAL.into());
                }
                // TODO: the read-modify-write of `uaddr2` is not atomic against guest atomics;
                // `UserMemory` has no atomic access yet.
                let target = GuestPtrMut::<u32>::new(uaddr2.addr());
                let old: u32 = read_value(caller.memory(), GuestPtr::new(uaddr2.addr())).await?;
                let (new, matches) = wake_op(val3, old)?;
                write_value(caller.memory(), target, &new).await?;
                let n_wake2 = utime;
                let mut woken = self.futexes.wake(key, val, MATCH_ANY);
                if matches {
                    let key2 = self.futex_key(caller, uaddr2.addr());
                    woken += self.futexes.wake(key2, n_wake2, MATCH_ANY);
                }
                Ok(woken as i32)
            }
            // Priority inheritance and FUTEX_FD (gone since 2.6.26).
            _ => Err(Errno::ENOSYS.into()),
        }
    }

    async fn futex_wait(
        &self,
        caller: &Caller<'_>,
        key: Key,
        val: u32,
        bitset: u32,
        timeout: Timeout,
    ) -> SysResult<i32> {
        let entry = Arc::new(Entry {
            bitset,
            woken: AtomicBool::new(false),
            queue: WaitQueue::new(),
        });
        let waiter = Waiter::new();
        let _woken = waiter.register(&entry.queue, PollMask::empty());
        let process = self.process(caller.task());
        let group = &process.group;
        let _exiting = waiter.register(&group.exited, PollMask::empty());
        self.futexes.enqueue(key, entry.clone());
        let current = read_value(caller.memory(), GuestPtr::<u32>::new(key.addr)).await;
        let current = match current {
            Ok(current) => current,
            Err(fault) => {
                self.futexes.dequeue(&entry);
                return Err(fault.into());
            }
        };
        if current != val {
            return match self.futexes.dequeue(&entry) {
                true => Err(Errno::EAGAIN.into()),
                false => Ok(0),
            };
        }
        let mut timer = match timeout {
            Timeout::Never => None,
            Timeout::After(d) => Some(self.host.sleep(d)),
            Timeout::Until(at) => Some(self.host.sleep_until(at)),
        };
        let interrupt = self.interrupt(caller);
        loop {
            if entry.woken.load(Ordering::Acquire) {
                return Ok(0);
            }
            if group.exiting() {
                // The task is about to go; its pending exit unwinds it on the way out.
                self.futexes.dequeue(&entry);
                return Ok(0);
            }
            let wakeup = waiter
                .sleep(&*interrupt, timer.as_mut().map(|t| t.as_mut()))
                .await;
            match wakeup {
                Wakeup::Woken(_) => {}
                Wakeup::TimedOut => {
                    return match self.futexes.dequeue(&entry) {
                        true => Err(Errno::ETIMEDOUT.into()),
                        false => Ok(0),
                    };
                }
                Wakeup::Interrupted => {
                    if !self.futexes.dequeue(&entry) {
                        return Ok(0);
                    }
                    // Linux restarts an untimed wait and fails a timed one with EINTR.
                    return match timer {
                        None => Err(SysErr::Restart),
                        Some(_) => Err(Errno::EINTR.into()),
                    };
                }
            }
        }
    }

    /// `handle_futex_death`: the owner `tid` of the robust futex at `addr` died. Marks it
    /// `FUTEX_OWNER_DIED` and wakes one waiter, unless someone else owns it.
    pub(crate) async fn futex_owner_died(&self, caller: &Caller<'_>, addr: u32, tid: u32) {
        if !addr.is_multiple_of(4) {
            return;
        }
        let Ok(word) = read_value(caller.memory(), GuestPtr::<u32>::new(addr)).await else {
            return;
        };
        if word & FUTEX_TID_MASK != tid {
            return;
        }
        let new = (word & FUTEX_WAITERS) | FUTEX_OWNER_DIED;
        if write_value(caller.memory(), GuestPtrMut::new(addr), &new)
            .await
            .is_err()
        {
            return;
        }
        if word & FUTEX_WAITERS != 0 {
            self.futexes
                .wake(self.futex_key(caller, addr), 1, MATCH_ANY);
        }
    }

    /// `exit_robust_list`: releases the robust futexes the exiting task `tid` still holds. Faults
    /// end the walk silently.
    pub(crate) async fn exit_robust_list(&self, caller: &Caller<'_>, head: u32, tid: u32) {
        let read = |addr: u32| read_value(caller.memory(), GuestPtr::<u32>::new(addr));
        let (Ok(mut entry), Ok(offset), Ok(pending)) =
            (read(head).await, read(head + 4).await, read(head + 8).await)
        else {
            return;
        };
        let offset = offset as i32;
        // Bit 0 of a list pointer flags a PI futex; the address is the rest.
        let pending = pending & !1;
        for _ in 0..ROBUST_LIST_LIMIT {
            entry &= !1;
            if entry == head {
                break;
            }
            let Ok(next) = read(entry).await else {
                return;
            };
            // The entry an operation is in the middle of is handled last.
            if entry != pending {
                self.futex_owner_died(caller, entry.wrapping_add_signed(offset), tid)
                    .await;
            }
            entry = next;
        }
        if pending != 0 {
            self.futex_owner_died(caller, pending.wrapping_add_signed(offset), tid)
                .await;
        }
    }
}

/// `futex_atomic_op_inuser` minus the atomicity: the new value of the `FUTEX_WAKE_OP` target
/// and whether its old value passes the comparison.
fn wake_op(encoded: u32, old: u32) -> Result<(u32, bool), Errno> {
    let op = (encoded >> 28) & 7;
    let cmp = (encoded >> 24) & 15;
    let mut oparg = (encoded << 8) as i32 >> 20;
    let cmparg = (encoded << 20) as i32 >> 20;
    // FUTEX_OP_OPARG_SHIFT
    if encoded & (8 << 28) != 0 {
        if !(0..=31).contains(&oparg) {
            return Err(Errno::EINVAL);
        }
        oparg = 1 << oparg;
    }
    let oparg = oparg as u32;
    let new = match op {
        0 => oparg,
        1 => old.wrapping_add(oparg),
        2 => old | oparg,
        3 => old & !oparg,
        4 => old ^ oparg,
        _ => return Err(Errno::ENOSYS),
    };
    let (old, cmparg) = (old as i32, cmparg);
    let matches = match cmp {
        0 => old == cmparg,
        1 => old != cmparg,
        2 => old < cmparg,
        3 => old <= cmparg,
        4 => old > cmparg,
        5 => old >= cmparg,
        _ => return Err(Errno::ENOSYS),
    };
    Ok((new, matches))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wake_op_decodes_linux_encoding() {
        // FUTEX_OP(FUTEX_OP_ADD, 1, FUTEX_OP_CMP_GT, 0)
        let encoded = (1 << 28) | (4 << 24) | (1 << 12);
        assert_eq!(wake_op(encoded, 5), Ok((6, true)));
        assert_eq!(wake_op(encoded, 0), Ok((1, false)));
        // FUTEX_OP_OPARG_SHIFT with FUTEX_OP_OR: sets bit 3.
        let encoded = (8 << 28) | (2 << 28) | (3 << 12);
        assert_eq!(wake_op(encoded, 0), Ok((8, true)));
    }

    #[test]
    fn wake_counts_and_requeue_moves() {
        let futexes = Futexes::default();
        let a = Key { mm: 1, addr: 4 };
        let b = Key { mm: 1, addr: 8 };
        let entries: Vec<_> = (0..4)
            .map(|_| {
                let entry = Arc::new(Entry {
                    bitset: MATCH_ANY,
                    woken: AtomicBool::new(false),
                    queue: WaitQueue::new(),
                });
                futexes.enqueue(a, entry.clone());
                entry
            })
            .collect();
        // A count of 0 wakes one, as on Linux.
        assert_eq!(futexes.wake(a, 0, MATCH_ANY), 1);
        assert!(entries[0].woken.load(Ordering::Acquire));
        assert_eq!(futexes.requeue(a, b, 1, 1), 2);
        assert!(entries[1].woken.load(Ordering::Acquire));
        assert_eq!(futexes.wake(b, 10, MATCH_ANY), 1);
        assert!(entries[2].woken.load(Ordering::Acquire));
        assert!(futexes.dequeue(&entries[3]));
        assert!(!futexes.dequeue(&entries[3]));
    }
}
