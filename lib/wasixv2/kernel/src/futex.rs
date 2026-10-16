//! Futexes (`kernel/futex`): wait queues keyed by address, for the tasks of one address space.
//!
//! A waiter sits on the queue of its key until a wake takes it off, so a wait never misses a
//! wake that follows a change of the futex word: the waiter queues first and compares the word
//! after, as Linux does under the hash bucket lock.
//!
//! Priority-inheritance futexes (`kernel/futex/pi.c`) follow Linux's protocol on the futex word
//! exactly: the owner's TID, `FUTEX_WAITERS` while the kernel has waiters, and `FUTEX_OWNER_DIED`
//! from robust list cleanup. The kernel keeps, per contended PI futex, the owner it believes in
//! and its waiters in arrival order; unlocking hands the lock straight to the first waiter.
//! Every task runs at the same priority here, so inheritance itself changes nothing.

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use wasixv2_abi::{ConstKernelTimespecPtr, FutexOp, GuestPtr, U32Ptr, Ulong};
use wasixv2_syscalls::{Caller, Errno, SysErr, SysResult, UserMemory, read_value};
use wasixv2_wait::{PollMask, WaitQueue, Waiter, Wakeup};

use crate::{Kernel, restart::Restart, time};

/// `FUTEX_BITSET_MATCH_ANY`.
const MATCH_ANY: u32 = u32::MAX;
const FUTEX_WAITERS: u32 = 0x8000_0000;
const FUTEX_OWNER_DIED: u32 = 0x4000_0000;
const FUTEX_TID_MASK: u32 = 0x3fff_ffff;
/// `ROBUST_LIST_LIMIT`: how many entries a robust list walk visits at most.
const ROBUST_LIST_LIMIT: usize = 2048;

/// A futex: the address space ([`Mm::id`](crate::mm::Mm::id)) and the address of the word. As
/// Linux keys private futexes by `mm`, every task on the same memory, a vfork child included,
/// finds the same queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Key {
    pub(crate) mm: u64,
    pub(crate) addr: u32,
}

/// One blocked `FUTEX_WAIT` or `FUTEX_WAIT_REQUEUE_PI`.
struct Entry {
    bitset: u32,
    /// Set by the wake that took the entry off its queue.
    woken: AtomicBool,
    /// The waiter sleeps on this; a wake wakes it.
    queue: WaitQueue,
    /// For `FUTEX_WAIT_REQUEUE_PI`: the PI futex it may be moved to (`rt_waiter`).
    requeue: Option<Arc<Requeue>>,
}

/// What became of a `FUTEX_WAIT_REQUEUE_PI` waiter.
struct Requeue {
    target: Key,
    pi: Arc<PiWaiter>,
    /// Set when `FUTEX_CMP_REQUEUE_PI` moved the waiter onto the PI futex, or took the lock
    /// for it.
    moved: AtomicBool,
}

/// A task blocked on a PI futex (`rt_mutex_waiter`).
struct PiWaiter {
    tid: u32,
    /// The lock was handed to this task.
    granted: AtomicBool,
    queue: WaitQueue,
}

/// `futex_pi_state`: the kernel's side of a contended PI futex.
struct PiState {
    /// The owner's TID, as the kernel knows it.
    owner: u32,
    waiters: VecDeque<Arc<PiWaiter>>,
}

#[derive(Default)]
pub(crate) struct Futexes {
    queues: Mutex<HashMap<Key, VecDeque<Arc<Entry>>>>,
    /// Held across every PI operation, from the first look at the futex word to the last update
    /// of the state (the hash bucket and `pi_mutex.wait_lock` together).
    pi: futures_util::lock::Mutex<HashMap<Key, PiState>>,
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
    /// Linux, a count of 0 still wakes one. A `FUTEX_WAIT_REQUEUE_PI` waiter makes it `EINVAL`.
    pub(crate) fn wake(&self, key: Key, n: u32, bitset: u32) -> Result<u32, Errno> {
        let mut queues = self.queues.lock().unwrap();
        let Some(queue) = queues.get_mut(&key) else {
            return Ok(0);
        };
        let mut woken = 0;
        let mut at = 0;
        while at < queue.len() {
            if queue[at].requeue.is_some() {
                return Err(Errno::EINVAL);
            }
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
        Ok(woken)
    }

    /// `futex_requeue` for plain futexes: wakes up to `n_wake` waiters on `from` and moves up to
    /// `n_requeue` of the rest to `to`. Returns the number woken plus the number moved.
    fn requeue(&self, from: Key, to: Key, n_wake: u32, n_requeue: u32) -> Result<u32, Errno> {
        let mut queues = self.queues.lock().unwrap();
        let Some(queue) = queues.get_mut(&from) else {
            return Ok(0);
        };
        if queue.iter().any(|entry| entry.requeue.is_some()) {
            return Err(Errno::EINVAL);
        }
        let wake = (n_wake as usize).min(queue.len());
        for entry in queue.drain(..wake) {
            entry.woken.store(true, Ordering::Release);
            entry.queue.wake(PollMask::empty());
        }
        let take = (n_requeue as usize).min(queue.len());
        let moved: Vec<_> = queue.drain(..take).collect();
        if queue.is_empty() {
            queues.remove(&from);
        }
        if !moved.is_empty() {
            queues.entry(to).or_default().extend(moved);
        }
        Ok((wake + take) as u32)
    }
}

/// How long a futex operation may block. A relative timeout is made absolute on entry, so a
/// restarted wait goes on to the same deadline.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Timeout {
    Never,
    /// Absolute, on the monotonic clock.
    Until(Duration),
    /// Absolute, on the realtime clock.
    UntilRealtime(Duration),
}

/// Whether `cmd` takes a timeout in the `utime` slot (`futex_cmd_has_timeout`).
fn has_timeout(cmd: FutexOp) -> bool {
    matches!(
        cmd,
        FutexOp::FUTEX_WAIT
            | FutexOp::FUTEX_LOCK_PI
            | FutexOp::FUTEX_LOCK_PI2
            | FutexOp::FUTEX_WAIT_BITSET
            | FutexOp::FUTEX_WAIT_REQUEUE_PI
    )
}

/// `cmpxchg_futex_value_locked`.
async fn cmpxchg(memory: &dyn UserMemory, addr: u32, old: u32, new: u32) -> Result<u32, Errno> {
    memory
        .compare_exchange_u32(addr, old, new)
        .await
        .map_err(|_| Errno::EFAULT)
}

async fn get_word(memory: &dyn UserMemory, addr: u32) -> Result<u32, Errno> {
    read_value(memory, GuestPtr::<u32>::new(addr))
        .await
        .map_err(|_| Errno::EFAULT)
}

/// What `futex_lock_pi_atomic` found.
enum Attempt {
    /// The caller owns the lock now.
    Acquired,
    /// Somebody else owns it; `FUTEX_WAITERS` is set and the kernel state exists.
    Wait,
}

impl Kernel {
    fn futex_key(&self, caller: &Caller<'_>, addr: u32) -> Key {
        Key {
            mm: self.mm(caller.task()).id(),
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
        let timeout = match utime {
            ptr if ptr != 0 && has_timeout(cmd) => {
                let ts = read_value(caller.memory(), ConstKernelTimespecPtr::new(ptr)).await?;
                let ts = time::from_timespec(ts)?;
                match cmd {
                    FutexOp::FUTEX_WAIT => Timeout::Until(self.host.monotonic().saturating_add(ts)),
                    FutexOp::FUTEX_LOCK_PI => Timeout::UntilRealtime(ts),
                    _ if realtime => Timeout::UntilRealtime(ts),
                    _ => Timeout::Until(ts),
                }
            }
            _ => Timeout::Never,
        };
        let realtime_ok = matches!(
            cmd,
            FutexOp::FUTEX_WAIT_BITSET | FutexOp::FUTEX_WAIT_REQUEUE_PI | FutexOp::FUTEX_LOCK_PI2
        );
        if realtime && !realtime_ok {
            return Err(Errno::ENOSYS.into());
        }
        if !uaddr.addr().is_multiple_of(4) {
            return Err(Errno::EINVAL.into());
        }
        let key = self.futex_key(caller, uaddr.addr());
        let tid = caller.task().0;
        match cmd {
            FutexOp::FUTEX_WAIT | FutexOp::FUTEX_WAIT_BITSET => {
                let bitset = match cmd {
                    FutexOp::FUTEX_WAIT => MATCH_ANY,
                    _ => val3,
                };
                if bitset == 0 {
                    return Err(Errno::EINVAL.into());
                }
                self.futex_wait(caller, key, val, bitset, timeout).await
            }
            FutexOp::FUTEX_WAKE => Ok(self.futexes.wake(key, val, MATCH_ANY)? as i32),
            FutexOp::FUTEX_WAKE_BITSET => match val3 {
                0 => Err(Errno::EINVAL.into()),
                bitset => Ok(self.futexes.wake(key, val, bitset)? as i32),
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
                    let current = get_word(caller.memory(), uaddr.addr()).await?;
                    if current != val3 {
                        return Err(Errno::EAGAIN.into());
                    }
                }
                let to = self.futex_key(caller, uaddr2.addr());
                let n = self
                    .futexes
                    .requeue(key, to, n_wake as u32, n_requeue as u32)?;
                Ok(n as i32)
            }
            FutexOp::FUTEX_WAKE_OP => {
                if !uaddr2.addr().is_multiple_of(4) {
                    return Err(Errno::EINVAL.into());
                }
                let target = uaddr2.addr();
                // `futex_atomic_op_inuser`: retried until the word did not change under us.
                let matches = loop {
                    let old = get_word(caller.memory(), target).await?;
                    let (new, matches) = wake_op(val3, old)?;
                    if cmpxchg(caller.memory(), target, old, new).await? == old {
                        break matches;
                    }
                };
                let n_wake2 = utime;
                let mut woken = self.futexes.wake(key, val, MATCH_ANY)?;
                if matches {
                    let key2 = self.futex_key(caller, target);
                    woken += self.futexes.wake(key2, n_wake2, MATCH_ANY)?;
                }
                Ok(woken as i32)
            }
            FutexOp::FUTEX_LOCK_PI | FutexOp::FUTEX_LOCK_PI2 => {
                self.futex_lock_pi(caller, key, tid, timeout).await
            }
            FutexOp::FUTEX_TRYLOCK_PI => self.futex_trylock_pi(caller, key, tid).await,
            FutexOp::FUTEX_UNLOCK_PI => self.futex_unlock_pi(caller, key, tid).await,
            FutexOp::FUTEX_WAIT_REQUEUE_PI => {
                if !uaddr2.addr().is_multiple_of(4) {
                    return Err(Errno::EINVAL.into());
                }
                let target = self.futex_key(caller, uaddr2.addr());
                if target == key {
                    return Err(Errno::EINVAL.into());
                }
                self.futex_wait_requeue_pi(caller, key, val, timeout, target, tid)
                    .await
            }
            FutexOp::FUTEX_CMP_REQUEUE_PI => {
                let (n_wake, n_requeue) = (val as i32, utime as i32);
                if n_wake < 0 || n_requeue < 0 {
                    return Err(Errno::EINVAL.into());
                }
                if !uaddr2.addr().is_multiple_of(4) {
                    return Err(Errno::EINVAL.into());
                }
                let target = self.futex_key(caller, uaddr2.addr());
                if target == key || n_wake != 1 {
                    return Err(Errno::EINVAL.into());
                }
                self.futex_cmp_requeue_pi(caller, key, target, n_requeue as u32, val3)
                    .await
            }
            // FUTEX_FD is gone since 2.6.26.
            _ => Err(Errno::ENOSYS.into()),
        }
    }

    /// A sleep for `timeout`, or none.
    fn futex_timer(&self, timeout: Timeout) -> Option<wasixv2_host::LocalFuture<'_, ()>> {
        match timeout {
            Timeout::Never => None,
            Timeout::Until(at) => Some(self.host.sleep_until(at)),
            Timeout::UntilRealtime(at) => {
                Some(self.host.sleep(at.saturating_sub(self.host.realtime())))
            }
        }
    }

    /// `futex_wait_restart`: a timed wait a signal interrupted, going on to its deadline.
    pub(crate) async fn futex_wait_restart(
        &self,
        caller: &Caller<'_>,
        addr: u32,
        val: u32,
        bitset: u32,
        timeout: Timeout,
    ) -> SysResult<i32> {
        let key = self.futex_key(caller, addr);
        self.futex_wait(caller, key, val, bitset, timeout).await
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
            requeue: None,
        });
        let waiter = Waiter::new();
        let _woken = waiter.register(&entry.queue, PollMask::empty());
        let process = self.process(caller.task());
        let group = &process.group;
        let _exiting = waiter.register(&group.exited, PollMask::empty());
        self.futexes.enqueue(key, entry.clone());
        let current = match get_word(caller.memory(), key.addr).await {
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
        let mut timer = self.futex_timer(timeout);
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
                    // An untimed wait reruns; a timed one goes on to the same deadline.
                    return Err(match timeout {
                        Timeout::Never => SysErr::Restart,
                        timeout => self.restart_with(
                            caller,
                            Restart::FutexWait {
                                addr: key.addr,
                                val,
                                bitset,
                                timeout,
                            },
                        ),
                    });
                }
            }
        }
    }

    /// Whether `tid` is a task that has not exited (`find_get_task_by_vpid`).
    fn task_alive(&self, tid: u32) -> bool {
        let state = self.state.lock().unwrap();
        state
            .tasks
            .get(&wasixv2_syscalls::TaskId(tid))
            .is_some_and(|task| !task.thread.exited())
    }

    /// `futex_lock_pi_atomic` for `tid`, with the PI table held: takes a free lock, or sets
    /// `FUTEX_WAITERS` and makes sure the kernel state names the owner.
    async fn futex_lock_pi_atomic(
        &self,
        memory: &dyn UserMemory,
        table: &mut HashMap<Key, PiState>,
        key: Key,
        tid: u32,
    ) -> Result<Attempt, Errno> {
        loop {
            let word = get_word(memory, key.addr).await?;
            let owner = word & FUTEX_TID_MASK;
            if owner == tid {
                return Err(Errno::EDEADLK);
            }
            let queued = table
                .get(&key)
                .is_some_and(|state| !state.waiters.is_empty());
            if queued {
                // `attach_to_pi_state`: the word must name the kernel's owner, or no owner once
                // the robust list marked it dead.
                let known = table[&key].owner;
                let agrees = owner == known || (owner == 0 && word & FUTEX_OWNER_DIED != 0);
                if !agrees {
                    return Err(Errno::EINVAL);
                }
                if word & FUTEX_WAITERS == 0
                    && cmpxchg(memory, key.addr, word, word | FUTEX_WAITERS).await? != word
                {
                    continue;
                }
                return Ok(Attempt::Wait);
            }
            if owner == 0 {
                // Free, perhaps after its owner died: take it, keeping `FUTEX_OWNER_DIED`.
                let new = (word & FUTEX_OWNER_DIED) | tid;
                if cmpxchg(memory, key.addr, word, new).await? != word {
                    continue;
                }
                return Ok(Attempt::Acquired);
            }
            if cmpxchg(memory, key.addr, word, word | FUTEX_WAITERS).await? != word {
                continue;
            }
            // `attach_to_pi_owner`: the owner must exist (`handle_exit_race`).
            if !self.task_alive(owner) {
                let now = get_word(memory, key.addr).await?;
                if now != word | FUTEX_WAITERS {
                    continue;
                }
                return Err(Errno::ESRCH);
            }
            table.insert(
                key,
                PiState {
                    owner,
                    waiters: VecDeque::new(),
                },
            );
            return Ok(Attempt::Wait);
        }
    }

    async fn futex_trylock_pi(&self, caller: &Caller<'_>, key: Key, tid: u32) -> SysResult<i32> {
        let mut table = self.futexes.pi.lock().await;
        let attempt = self
            .futex_lock_pi_atomic(caller.memory(), &mut table, key, tid)
            .await?;
        drop_idle(&mut table, key);
        match attempt {
            Attempt::Acquired => Ok(0),
            Attempt::Wait => Err(Errno::EAGAIN.into()),
        }
    }

    /// `futex_lock_pi`.
    async fn futex_lock_pi(
        &self,
        caller: &Caller<'_>,
        key: Key,
        tid: u32,
        timeout: Timeout,
    ) -> SysResult<i32> {
        let pi = Arc::new(PiWaiter {
            tid,
            granted: AtomicBool::new(false),
            queue: WaitQueue::new(),
        });
        {
            let mut table = self.futexes.pi.lock().await;
            let attempt = self
                .futex_lock_pi_atomic(caller.memory(), &mut table, key, tid)
                .await;
            match attempt {
                Ok(Attempt::Acquired) => {
                    drop_idle(&mut table, key);
                    return Ok(0);
                }
                Err(errno) => {
                    drop_idle(&mut table, key);
                    return Err(errno.into());
                }
                Ok(Attempt::Wait) => {
                    let state = table.get_mut(&key).expect("attached above");
                    state.waiters.push_back(pi.clone());
                }
            }
        }
        match self.pi_wait(caller, key, &pi, timeout).await {
            Ok(()) => Ok(0),
            Err(PiInterrupt::TimedOut) => Err(Errno::ETIMEDOUT.into()),
            // `ERESTARTNOINTR`, which the restart machinery knows as `ERESTARTSYS`.
            Err(PiInterrupt::Signal) => Err(SysErr::Restart),
        }
    }

    /// `rt_mutex_wait_proxy_lock` and its cleanup: sleeps until the lock is handed to `pi`.
    /// On a timeout or a signal the waiter leaves, unless the lock reached it first.
    async fn pi_wait(
        &self,
        caller: &Caller<'_>,
        key: Key,
        pi: &Arc<PiWaiter>,
        timeout: Timeout,
    ) -> Result<(), PiInterrupt> {
        let waiter = Waiter::new();
        let _granted = waiter.register(&pi.queue, PollMask::empty());
        let process = self.process(caller.task());
        let _exiting = waiter.register(&process.group.exited, PollMask::empty());
        let mut timer = self.futex_timer(timeout);
        let interrupt = self.interrupt(caller);
        let why = loop {
            if pi.granted.load(Ordering::Acquire) {
                return Ok(());
            }
            if process.group.exiting() {
                break PiInterrupt::Signal;
            }
            match waiter
                .sleep(&*interrupt, timer.as_mut().map(|t| t.as_mut()))
                .await
            {
                Wakeup::Woken(_) => {}
                Wakeup::TimedOut => break PiInterrupt::TimedOut,
                Wakeup::Interrupted => break PiInterrupt::Signal,
            }
        };
        let mut table = self.futexes.pi.lock().await;
        if pi.granted.load(Ordering::Acquire) {
            return Ok(());
        }
        if let Some(state) = table.get_mut(&key) {
            state.waiters.retain(|w| !Arc::ptr_eq(w, pi));
        }
        drop_idle(&mut table, key);
        Err(why)
    }

    /// `futex_unlock_pi`.
    async fn futex_unlock_pi(&self, caller: &Caller<'_>, key: Key, tid: u32) -> SysResult<i32> {
        let mut table = self.futexes.pi.lock().await;
        loop {
            let word = get_word(caller.memory(), key.addr).await?;
            if word & FUTEX_TID_MASK != tid {
                return Err(Errno::EPERM.into());
            }
            let next = table
                .get(&key)
                .and_then(|state| state.waiters.front().cloned());
            let Some(next) = next else {
                // No waiters: the word may still say there are, from a waiter that left.
                if cmpxchg(caller.memory(), key.addr, word, 0).await? != word {
                    continue;
                }
                drop_idle(&mut table, key);
                return Ok(0);
            };
            // `wake_futex_pi`: the first waiter owns the lock now.
            let new = FUTEX_WAITERS | next.tid;
            let current = cmpxchg(caller.memory(), key.addr, word, new).await?;
            if current != word {
                if current & FUTEX_TID_MASK == tid {
                    continue;
                }
                return Err(Errno::EINVAL.into());
            }
            hand_over(&mut table, key);
            return Ok(0);
        }
    }

    /// `futex_wait_requeue_pi`: waits on `key` like `FUTEX_WAIT` until `FUTEX_CMP_REQUEUE_PI`
    /// either takes the PI futex `target` for the caller or moves it there, then waits for the
    /// lock.
    async fn futex_wait_requeue_pi(
        &self,
        caller: &Caller<'_>,
        key: Key,
        val: u32,
        timeout: Timeout,
        target: Key,
        tid: u32,
    ) -> SysResult<i32> {
        let pi = Arc::new(PiWaiter {
            tid,
            granted: AtomicBool::new(false),
            queue: WaitQueue::new(),
        });
        let requeue = Arc::new(Requeue {
            target,
            pi: pi.clone(),
            moved: AtomicBool::new(false),
        });
        let entry = Arc::new(Entry {
            bitset: MATCH_ANY,
            woken: AtomicBool::new(false),
            queue: WaitQueue::new(),
            requeue: Some(requeue.clone()),
        });
        let waiter = Waiter::new();
        let _woken = waiter.register(&entry.queue, PollMask::empty());
        let _granted = waiter.register(&pi.queue, PollMask::empty());
        let process = self.process(caller.task());
        let _exiting = waiter.register(&process.group.exited, PollMask::empty());
        // The PI table orders this against a requeue that already looked at the queue.
        {
            let _table = self.futexes.pi.lock().await;
            self.futexes.enqueue(key, entry.clone());
        }
        let early = match get_word(caller.memory(), key.addr).await {
            Ok(current) if current == val => None,
            Ok(_) => Some(SysErr::from(Errno::EAGAIN)),
            Err(errno) => Some(errno.into()),
        };
        let early = match early {
            Some(err) => Some(err),
            None => {
                self.requeue_pi_sleep(caller, &waiter, &requeue, timeout)
                    .await
            }
        };
        if let Some(err) = early
            && self.leave_requeue_queue(&entry).await
        {
            return Err(err);
        }
        // On the PI futex now, perhaps already its owner. A deadline that passed, or a signal,
        // ends the wait at once unless the lock is there.
        match self.pi_wait(caller, target, &pi, timeout).await {
            Ok(()) => Ok(0),
            Err(PiInterrupt::TimedOut) => Err(Errno::ETIMEDOUT.into()),
            // Linux: "We've already been requeued, but cannot restart by calling
            // futex_lock_pi() directly"; the restart would fail the comparison anyway.
            Err(PiInterrupt::Signal) => Err(Errno::EAGAIN.into()),
        }
    }

    /// The first half of `FUTEX_WAIT_REQUEUE_PI`: sleeps until a requeue moved the caller, or
    /// returns why it stopped first (`handle_early_requeue_pi_wakeup`).
    async fn requeue_pi_sleep(
        &self,
        caller: &Caller<'_>,
        waiter: &Waiter,
        requeue: &Requeue,
        timeout: Timeout,
    ) -> Option<SysErr> {
        let process = self.process(caller.task());
        let mut timer = self.futex_timer(timeout);
        let interrupt = self.interrupt(caller);
        loop {
            if requeue.moved.load(Ordering::Acquire) {
                return None;
            }
            if process.group.exiting() {
                return Some(SysErr::Restart);
            }
            match waiter
                .sleep(&*interrupt, timer.as_mut().map(|t| t.as_mut()))
                .await
            {
                Wakeup::Woken(_) => {}
                Wakeup::TimedOut => return Some(Errno::ETIMEDOUT.into()),
                // `ERESTARTNOINTR`.
                Wakeup::Interrupted => return Some(SysErr::Restart),
            }
        }
    }

    /// Takes a `FUTEX_WAIT_REQUEUE_PI` waiter off its first queue. False if a requeue moved it
    /// first, so it now waits on the PI futex.
    async fn leave_requeue_queue(&self, entry: &Arc<Entry>) -> bool {
        let _table = self.futexes.pi.lock().await;
        self.futexes.dequeue(entry)
    }

    /// `futex_requeue` with `requeue_pi`: takes the PI futex `target` for the first waiter on
    /// `key` if it is free, and moves up to `n_requeue` waiters (one more if the lock was not
    /// free) onto it.
    async fn futex_cmp_requeue_pi(
        &self,
        caller: &Caller<'_>,
        key: Key,
        target: Key,
        n_requeue: u32,
        val3: u32,
    ) -> SysResult<i32> {
        let mut table = self.futexes.pi.lock().await;
        let current = get_word(caller.memory(), key.addr).await?;
        if current != val3 {
            return Err(Errno::EAGAIN.into());
        }
        let waiting: Vec<Arc<Entry>> = {
            let queues = self.futexes.queues.lock().unwrap();
            queues
                .get(&key)
                .map(|q| q.iter().cloned().collect())
                .unwrap_or_default()
        };
        let mut count: i64 = 0;
        let mut first = true;
        for entry in waiting {
            if count > i64::from(n_requeue) {
                break;
            }
            let Some(requeue) = entry.requeue.clone() else {
                return Err(Errno::EINVAL.into());
            };
            if requeue.target != target {
                return Err(Errno::EINVAL.into());
            }
            let attempt = self
                .futex_lock_pi_atomic(caller.memory(), &mut table, target, requeue.pi.tid)
                .await;
            let acquired = match attempt {
                Ok(Attempt::Acquired) => true,
                Ok(Attempt::Wait) => false,
                // `futex_proxy_trylock_atomic` fails the whole call for the top waiter only.
                Err(errno) if first => {
                    drop_idle(&mut table, target);
                    return Err(errno.into());
                }
                Err(_) => break,
            };
            first = false;
            self.futexes.dequeue(&entry);
            if acquired {
                requeue.pi.granted.store(true, Ordering::Release);
            } else {
                let state = table.get_mut(&target).expect("attached by the attempt");
                state.waiters.push_back(requeue.pi.clone());
            }
            requeue.moved.store(true, Ordering::Release);
            entry.queue.wake(PollMask::empty());
            requeue.pi.queue.wake(PollMask::empty());
            count += 1;
        }
        drop_idle(&mut table, target);
        Ok(count as i32)
    }

    /// `handle_futex_death`: the owner `tid` of the robust futex at `addr` died. Marks it
    /// `FUTEX_OWNER_DIED`, and for a plain futex wakes one waiter. A PI futex's lock is handed
    /// on by `exit_pi_state_list`. `pending` is the entry the task was in the middle of locking
    /// or unlocking, whose waiters may need the wake the task never made.
    async fn futex_owner_died(
        &self,
        caller: &Caller<'_>,
        addr: u32,
        tid: u32,
        pi: bool,
        pending: bool,
    ) {
        if !addr.is_multiple_of(4) {
            return;
        }
        let memory = caller.memory();
        loop {
            let Ok(word) = get_word(memory, addr).await else {
                return;
            };
            let owner = word & FUTEX_TID_MASK;
            if owner != tid {
                if pending && !pi && (owner == 0 || word & FUTEX_WAITERS == 0) {
                    let _ = self
                        .futexes
                        .wake(self.futex_key(caller, addr), 1, MATCH_ANY);
                }
                return;
            }
            let new = (word & FUTEX_WAITERS) | FUTEX_OWNER_DIED;
            match cmpxchg(memory, addr, word, new).await {
                Err(_) => return,
                Ok(current) if current != word => continue,
                Ok(_) => {}
            }
            if !pi && word & FUTEX_WAITERS != 0 {
                let _ = self
                    .futexes
                    .wake(self.futex_key(caller, addr), 1, MATCH_ANY);
            }
            return;
        }
    }

    /// `exit_robust_list`: releases the robust futexes the exiting task `tid` still holds. Faults
    /// end the walk silently.
    pub(crate) async fn exit_robust_list(&self, caller: &Caller<'_>, head: u32, tid: u32) {
        let read = |addr: u32| get_word(caller.memory(), addr);
        let (Ok(mut entry), Ok(offset), Ok(pending)) =
            (read(head).await, read(head + 4).await, read(head + 8).await)
        else {
            return;
        };
        let offset = offset as i32;
        // Bit 0 of a list pointer flags a PI futex; the address is the rest.
        let (pending, pending_pi) = (pending & !1, pending & 1 != 0);
        for _ in 0..ROBUST_LIST_LIMIT {
            let pi = entry & 1 != 0;
            entry &= !1;
            if entry == head {
                break;
            }
            let Ok(next) = read(entry).await else {
                return;
            };
            // The entry an operation is in the middle of is handled last.
            if entry != pending {
                self.futex_owner_died(caller, entry.wrapping_add_signed(offset), tid, pi, false)
                    .await;
            }
            entry = next;
        }
        if pending != 0 {
            self.futex_owner_died(
                caller,
                pending.wrapping_add_signed(offset),
                tid,
                pending_pi,
                true,
            )
            .await;
        }
    }

    /// `exit_pi_state_list`: every contended PI futex the exiting task `tid` owns goes to its
    /// first waiter, which sees `FUTEX_OWNER_DIED` if the robust list set it.
    pub(crate) async fn exit_pi_state_list(&self, caller: &Caller<'_>, tid: u32) {
        let mm = self.mm(caller.task()).id();
        let mut table = self.futexes.pi.lock().await;
        let owned: Vec<Key> = table
            .iter()
            .filter(|(key, state)| key.mm == mm && state.owner == tid)
            .map(|(key, _)| *key)
            .collect();
        for key in owned {
            let Some(next) = table[&key].waiters.front().cloned() else {
                table.remove(&key);
                continue;
            };
            loop {
                let Ok(word) = get_word(caller.memory(), key.addr).await else {
                    break;
                };
                let new = (word & FUTEX_OWNER_DIED) | FUTEX_WAITERS | next.tid;
                match cmpxchg(caller.memory(), key.addr, word, new).await {
                    Ok(current) if current != word => continue,
                    _ => break,
                }
            }
            hand_over(&mut table, key);
        }
    }
}

/// Why a PI wait ended without the lock.
enum PiInterrupt {
    TimedOut,
    Signal,
}

/// Gives the lock at `key` to its first waiter, whose word the caller already updated.
fn hand_over(table: &mut HashMap<Key, PiState>, key: Key) {
    let Some(state) = table.get_mut(&key) else {
        return;
    };
    let Some(next) = state.waiters.pop_front() else {
        return;
    };
    state.owner = next.tid;
    next.granted.store(true, Ordering::Release);
    next.queue.wake(PollMask::empty());
    drop_idle(table, key);
}

/// Forgets the kernel state of a PI futex nobody waits on.
fn drop_idle(table: &mut HashMap<Key, PiState>, key: Key) {
    if table
        .get(&key)
        .is_some_and(|state| state.waiters.is_empty())
    {
        table.remove(&key);
    }
}

/// `futex_atomic_op_inuser`'s arithmetic: the new value of the `FUTEX_WAKE_OP` target and
/// whether its old value passes the comparison.
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
                    requeue: None,
                });
                futexes.enqueue(a, entry.clone());
                entry
            })
            .collect();
        // A count of 0 wakes one, as on Linux.
        assert_eq!(futexes.wake(a, 0, MATCH_ANY), Ok(1));
        assert!(entries[0].woken.load(Ordering::Acquire));
        assert_eq!(futexes.requeue(a, b, 1, 1), Ok(2));
        assert!(entries[1].woken.load(Ordering::Acquire));
        assert_eq!(futexes.wake(b, 10, MATCH_ANY), Ok(1));
        assert!(entries[2].woken.load(Ordering::Acquire));
        assert!(futexes.dequeue(&entries[3]));
        assert!(!futexes.dequeue(&entries[3]));
    }
}
