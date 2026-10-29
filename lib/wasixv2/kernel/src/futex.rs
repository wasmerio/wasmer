//! Futexes (`kernel/futex`): wait queues keyed by the futex word, as `get_futex_key` names it.
//!
//! A word in private memory is named by its address space and address, so the tasks of one
//! address space find each other there. A word in a mapping of a shared object
//! ([`Mapped`](crate::mm::Mapped)) is named, unless the call says `FUTEX_PRIVATE_FLAG`, by the
//! object and the word's offset in it, so every process that maps the object finds the same
//! waiters wherever it mapped it: process-shared mutexes, condition variables and semaphores in
//! `MAP_SHARED` memory or a SysV segment work across processes as on Linux. A shared futex in
//! private memory is the private one here (Linux marks it `FUT_OFF_MMSHARED`, which keeps it
//! apart from a private futex on the same word), so `memory.atomic.wait` and `notify`, which say
//! neither, meet `futex` calls of both kinds there.
//!
//! As on Linux, waiters sit on the chains of a fixed table of hash buckets, each with its own
//! lock, and a waiter's entry records its key, so a wait, a wake and a waiter leaving only lock
//! and walk one bucket. A waiter queues first and compares the futex word after, so it never
//! misses a wake that follows a change of the word, as Linux's comparison under the bucket lock
//! guarantees. The operations that compare or change the word on the waker's side
//! (`FUTEX_CMP_REQUEUE`, `FUTEX_WAKE_OP`) do so holding the bucket locks, as Linux does.
//!
//! Every wait is cancellation-safe: its entry leaves the chain when the wait's future is
//! dropped, and a priority-inheritance waiter is skipped from then on, so neither a wake nor a
//! lock handover goes to a task that stopped waiting.
//!
//! Priority-inheritance futexes (`kernel/futex/pi.c`) follow Linux's protocol on the futex word
//! exactly: the owner's TID, `FUTEX_WAITERS` while the kernel has waiters, and `FUTEX_OWNER_DIED`
//! from robust list cleanup. The kernel keeps, per contended PI futex, the owner it believes in
//! and its waiters in arrival order; unlocking hands the lock straight to the first waiter.
//! Every task runs at the same priority here, so inheritance itself changes nothing.
//!
//! The memory's low guard ([`UserMemory::low_guard`]) stands for the pages below
//! `mmap_min_addr` that Linux never maps, so futexes there fault as Linux's do: `EFAULT` where
//! Linux reads or changes the word (`futex_get_value_locked`, `get_user`) or finds the page of a
//! futex without `FUTEX_PRIVATE_FLAG` (`get_futex_key`'s `get_user_pages_fast`). A private
//! futex's key is its address alone, which `access_ok` allows down to 0, so a private
//! `FUTEX_WAKE` or `FUTEX_REQUEUE` there wakes nobody without a fault. Wasm's atomic wait and
//! notify are guest accesses rather than syscalls, and read the guard as the guest's loads do.

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
    time::Duration,
};

use futures_util::FutureExt as _;
use wasixv2_abi::{ConstKernelTimespecPtr, FutexOp, GuestPtr, U32Ptr, Ulong};
use wasixv2_syscalls::{
    AtomicWord, Caller, Errno, SysErr, SysResult, TaskId, UserMemory, read_value,
};
use wasixv2_wait::{Interrupt, PollMask, WaitRegistration, Waiter, WakeHandle, Wakeup, Watched};

use crate::{Kernel, Process, mm::MMAP_MIN_ADDR, restart::Restart, sync::MutexExt, time};

mod syscalls;

/// `FUTEX_BITSET_MATCH_ANY`.
const MATCH_ANY: u32 = u32::MAX;
const FUTEX_WAITERS: u32 = 0x8000_0000;
const FUTEX_OWNER_DIED: u32 = 0x4000_0000;
const FUTEX_TID_MASK: u32 = 0x3fff_ffff;
/// `ROBUST_LIST_LIMIT`: how many entries a robust list walk visits at most.
const ROBUST_LIST_LIMIT: usize = 2048;

/// A futex (`union futex_key`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Key {
    /// A word of an address space's own memory: the address space
    /// ([`Mm::id`](crate::mm::Mm::id)) and the word's address. As Linux keys private futexes by
    /// `mm`, every task on the same memory, a vfork child included, finds the same queue.
    Private { mm: u64, addr: u32 },
    /// A word of a shared object ([`SharedPages::id`](wasixv2_pages::SharedPages::id)), at
    /// `offset` in it (Linux's inode-based key).
    Shared { object: u64, offset: u64 },
}

/// The bit of [`Key::parts`]'s first half that tells a shared key from a private one.
const SHARED_KEY: u64 = 1 << 63;

impl Key {
    /// The key as two words, as an [`Entry`] keeps it.
    fn parts(self) -> (u64, u64) {
        match self {
            Key::Private { mm, addr } => (mm, u64::from(addr)),
            Key::Shared { object, offset } => (object | SHARED_KEY, offset),
        }
    }

    fn from_parts(space: u64, word: u64) -> Self {
        match space & SHARED_KEY {
            0 => Key::Private {
                mm: space,
                addr: word as u32,
            },
            _ => Key::Shared {
                object: space & !SHARED_KEY,
                offset: word,
            },
        }
    }
}

/// A futex word as an operation finds it: its key, and its address in the caller's memory.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Word {
    pub(crate) key: Key,
    pub(crate) addr: u32,
}

/// One blocked `FUTEX_WAIT` or `FUTEX_WAIT_REQUEUE_PI` (`futex_q`), on the chain of its key's
/// bucket while `queued`.
struct Entry {
    /// The key, as [`Key::parts`]. A requeue changes it holding the locks of both buckets, so it
    /// is stable under the lock of the bucket it names (`futex_q::lock_ptr`); read without that
    /// lock, its halves may come from two keys.
    space: AtomicU64,
    word: AtomicU64,
    bitset: u32,
    /// On a chain. Whoever takes the entry off clears it under the bucket lock: a wake, a requeue
    /// onto a PI futex, or the waiter leaving (`plist_node_empty`).
    queued: AtomicBool,
    /// The sleeping task (`futex_q::task`).
    task: WakeTarget,
    /// For `FUTEX_WAIT_REQUEUE_PI`: the PI futex it may be moved to (`rt_waiter`).
    requeue: Option<Arc<Requeue>>,
}

impl Entry {
    fn new(key: Key, bitset: u32, task: WakeHandle, requeue: Option<Arc<Requeue>>) -> Arc<Self> {
        Self::with_target(key, bitset, WakeTarget::Task(task), requeue)
    }

    fn with_target(
        key: Key,
        bitset: u32,
        task: WakeTarget,
        requeue: Option<Arc<Requeue>>,
    ) -> Arc<Self> {
        let (space, word) = key.parts();
        Arc::new(Self {
            space: AtomicU64::new(space),
            word: AtomicU64::new(word),
            bitset,
            queued: AtomicBool::new(true),
            task,
            requeue,
        })
    }

    /// The key; stable while the caller holds the lock of the entry's bucket.
    fn key(&self) -> Key {
        let space = self.space.load(Ordering::Acquire);
        Key::from_parts(space, self.word.load(Ordering::Acquire))
    }

    /// `requeue_futex`: moves the entry to `key`. The caller holds the locks of the buckets of
    /// both keys.
    fn set_key(&self, key: Key) {
        let (space, word) = key.parts();
        self.space.store(space, Ordering::Release);
        self.word.store(word, Ordering::Release);
    }

    /// `futex_wake_mark`: the caller took the entry off its chain; it was woken.
    fn wake(&self) {
        self.queued.store(false, Ordering::Release);
        self.task.wake();
    }
}

/// io_uring/futex.c:io_futex_wake_fn/io_futex_wakev_fn use the same hash queues.
enum WakeTarget {
    Task(WakeHandle),
    Uring {
        ctx: std::sync::Weak<crate::io_uring::Ctx>,
        id: u64,
        result: Arc<std::sync::atomic::AtomicI32>,
        index: i32,
    },
}
impl WakeTarget {
    fn wake(&self) {
        match self {
            Self::Task(task) => {
                task.wake();
            }
            Self::Uring {
                ctx,
                id,
                result,
                index,
            } => {
                if result
                    .compare_exchange(-1, *index, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                    && let Some(ctx) = ctx.upgrade()
                {
                    ctx.woken(*id);
                }
            }
        }
    }
}

pub(crate) struct UringWait {
    futexes: Arc<Futexes>,
    entries: Vec<Arc<Entry>>,
    result: Arc<std::sync::atomic::AtomicI32>,
}
impl UringWait {
    fn new(futexes: Arc<Futexes>) -> Self {
        Self {
            futexes,
            entries: Vec::new(),
            result: Arc::new(std::sync::atomic::AtomicI32::new(-1)),
        }
    }
    fn queue(
        &mut self,
        key: Key,
        mask: u32,
        ctx: std::sync::Weak<crate::io_uring::Ctx>,
        id: u64,
        index: i32,
    ) {
        let task = WakeTarget::Uring {
            ctx,
            id,
            result: self.result.clone(),
            index,
        };
        let entry = Entry::with_target(key, mask, task, None);
        self.futexes.enqueue(&entry);
        self.entries.push(entry);
    }
    pub(crate) fn ready(&self) -> Option<i32> {
        let result = self.result.load(Ordering::Acquire);
        (result >= 0).then_some(result)
    }
}
impl Drop for UringWait {
    fn drop(&mut self) {
        for entry in &self.entries {
            self.futexes.dequeue(entry);
        }
    }
}

/// What became of a `FUTEX_WAIT_REQUEUE_PI` waiter.
struct Requeue {
    target: Key,
    pi: Arc<PiWaiter>,
    /// Set when `FUTEX_CMP_REQUEUE_PI` moved the waiter onto the PI futex, or took the lock
    /// for it.
    moved: AtomicBool,
}

/// [`PiWaiter::state`] values.
const WAITING: u8 = 0;
const GRANTED: u8 = 1;
const GONE: u8 = 2;

/// A task blocked on a PI futex (`rt_mutex_waiter`).
struct PiWaiter {
    tid: u32,
    /// `WAITING`, then `GRANTED` once the lock was handed to it, or `GONE` once it stopped
    /// waiting without the lock. Changes only from `WAITING`.
    state: AtomicU8,
    task: WakeHandle,
}

impl PiWaiter {
    fn new(tid: u32, task: WakeHandle) -> Arc<Self> {
        Arc::new(Self {
            tid,
            state: AtomicU8::new(WAITING),
            task,
        })
    }

    fn waiting(&self) -> bool {
        self.state.load(Ordering::Acquire) == WAITING
    }

    fn granted(&self) -> bool {
        self.state.load(Ordering::Acquire) == GRANTED
    }

    /// Hands the lock to the waiter and wakes it; false if it stopped waiting first.
    fn grant(&self) -> bool {
        let granted = self
            .state
            .compare_exchange(WAITING, GRANTED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok();
        if granted {
            self.task.wake();
        }
        granted
    }

    /// Stops waiting; false if the lock was handed over first (`rt_mutex_cleanup_proxy_lock`).
    fn leave(&self) -> bool {
        let state = self
            .state
            .compare_exchange(WAITING, GONE, Ordering::AcqRel, Ordering::Acquire);
        state.unwrap_or_else(|state| state) != GRANTED
    }
}

/// A PI wait: when it is dropped unfinished, its waiter is skipped by every later handover.
struct PiGuard(Arc<PiWaiter>);

impl Drop for PiGuard {
    fn drop(&mut self) {
        self.0.leave();
    }
}

/// `futex_pi_state`: the kernel's side of a contended PI futex.
struct PiState {
    /// The owner's TID, as the kernel knows it.
    owner: u32,
    /// In arrival order; waiters that left are dropped when they reach the front.
    waiters: VecDeque<Arc<PiWaiter>>,
}

impl PiState {
    /// The first waiter still waiting, forgetting those before it that left.
    fn first(&mut self) -> Option<Arc<PiWaiter>> {
        while let Some(front) = self.waiters.front() {
            if front.waiting() {
                return Some(front.clone());
            }
            self.waiters.pop_front();
        }
        None
    }
}

/// The waiters of the keys that hash to one bucket, in arrival order (`futex_hash_bucket`).
type Chain = Vec<Arc<Entry>>;

/// The futex word could not be read without blocking while the bucket locks were held
/// (`futex_get_value_locked` faulted): the caller reads it unlocked and tries again.
struct WouldBlock;

/// What a bucket-locked operation that reads the futex word found.
type Locked<T> = Result<Result<T, Errno>, WouldBlock>;

pub(crate) struct Futexes {
    buckets: Box<[Mutex<Chain>]>,
    /// Held across every PI operation, from the first look at the futex word to the last update
    /// of the state (the hash bucket and `pi_mutex.wait_lock` together).
    pi: futures_util::lock::Mutex<HashMap<Key, PiState>>,
}

impl Default for Futexes {
    /// `futex_init`: 256 buckets per CPU, rounded up to a power of two.
    fn default() -> Self {
        let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
        let buckets = (256 * cpus).next_power_of_two();
        Self {
            buckets: (0..buckets).map(|_| Mutex::default()).collect(),
            pi: Default::default(),
        }
    }
}

impl Futexes {
    fn index(&self, key: Key) -> usize {
        // Fibonacci hashing of both halves of the key (Linux hashes with `jhash2`).
        const MULTIPLIER: u64 = 0x9e37_79b9_7f4a_7c15;
        let (space, word) = key.parts();
        let hash = (space.wrapping_mul(MULTIPLIER) ^ word).wrapping_mul(MULTIPLIER);
        (hash >> 32) as usize & (self.buckets.len() - 1)
    }

    fn lock(&self, key: Key) -> MutexGuard<'_, Chain> {
        self.buckets[self.index(key)].lock_or_recover()
    }

    /// `double_lock_hb`: the buckets of `a` and `b`, locked in a fixed order. The second is
    /// `None` when both keys hash to the same bucket.
    fn lock_pair(&self, a: Key, b: Key) -> (MutexGuard<'_, Chain>, Option<MutexGuard<'_, Chain>>) {
        let (i, j) = (self.index(a), self.index(b));
        let lock = |index: usize| self.buckets[index].lock_or_recover();
        match i.cmp(&j) {
            std::cmp::Ordering::Equal => (lock(i), None),
            std::cmp::Ordering::Less => {
                let first = lock(i);
                (first, Some(lock(j)))
            }
            std::cmp::Ordering::Greater => {
                let second = lock(j);
                (lock(i), Some(second))
            }
        }
    }

    fn enqueue(&self, entry: &Arc<Entry>) {
        self.lock(entry.key()).push(entry.clone());
    }

    /// `futex_unqueue`: takes `entry` off its chain. False if somebody else took it first.
    fn dequeue(&self, entry: &Arc<Entry>) -> bool {
        loop {
            if !entry.queued.load(Ordering::Acquire) {
                return false;
            }
            let mut chain = self.lock(entry.key());
            // Not there while queued: a requeue moved it before the lock was ours, or is moving
            // it (whose key read half old, half new). Its key names its chain once it is done.
            // A wake takes it off under the lock of the chain it is on, and clears `queued`.
            let Some(at) = chain.iter().position(|e| Arc::ptr_eq(e, entry)) else {
                continue;
            };
            chain.remove(at);
            entry.queued.store(false, Ordering::Release);
            return true;
        }
    }

    /// The waiters on `key`, in arrival order.
    fn waiters(&self, key: Key) -> Vec<Arc<Entry>> {
        let chain = self.lock(key);
        chain.iter().filter(|e| e.key() == key).cloned().collect()
    }

    /// `futex_wake`: wakes up to `n` waiters on `key` whose bitset intersects `bitset`. As on
    /// Linux, a count of 0 still wakes one. A `FUTEX_WAIT_REQUEUE_PI` waiter makes it `EINVAL`.
    pub(crate) fn wake(&self, key: Key, n: u32, bitset: u32) -> Result<u32, Errno> {
        wake_chain(&mut self.lock(key), key, n, bitset)
    }

    /// `futex_requeue` for plain futexes, holding both buckets: wakes up to `n_wake` waiters on
    /// `from` and moves up to `n_requeue` of the rest to `to`. With `expected`, the word `from`
    /// must hold it (`FUTEX_CMP_REQUEUE`), or nothing happens and the result is `EAGAIN`.
    /// Returns the number woken plus the number moved.
    fn requeue(
        &self,
        memory: &dyn UserMemory,
        from: Word,
        to: Key,
        (n_wake, n_requeue): (u32, u32),
        expected: Option<u32>,
    ) -> Locked<u32> {
        let (addr, from) = (from.addr, from.key);
        let (mut chain, mut other) = self.lock_pair(from, to);
        if let Some(expected) = expected {
            match get_word(memory, addr).now_or_never() {
                None => return Err(WouldBlock),
                Some(Err(errno)) => return Ok(Err(errno)),
                Some(Ok(word)) if word != expected => return Ok(Err(Errno::EAGAIN)),
                Some(Ok(_)) => {}
            }
        }
        if chain.iter().any(|e| e.key() == from && e.requeue.is_some()) {
            return Ok(Err(Errno::EINVAL));
        }
        let (mut woken, mut moved) = (0, 0);
        let mut at = 0;
        while at < chain.len() && (woken < n_wake || moved < n_requeue) {
            if chain[at].key() != from {
                at += 1;
                continue;
            }
            if woken < n_wake {
                chain.remove(at).wake();
                woken += 1;
                continue;
            }
            chain[at].set_key(to);
            match &mut other {
                Some(other) => other.push(chain.remove(at)),
                None => at += 1,
            }
            moved += 1;
        }
        Ok(Ok(woken + moved))
    }

    /// `futex_wake_op`, holding both buckets: applies the operation `encoded` to the word
    /// `word2`, wakes up to `n` waiters on `key`, and if the word's old value passed the
    /// operation's comparison, up to `n2` on `word2`.
    fn wake_op(
        &self,
        memory: &dyn UserMemory,
        (key, n): (Key, u32),
        (word2, n2): (Word, u32),
        encoded: u32,
    ) -> Locked<u32> {
        let key2 = word2.key;
        let (mut chain, mut other) = self.lock_pair(key, key2);
        // `futex_atomic_op_inuser`: retried until the word did not change under us.
        let matches = loop {
            let old = match get_word(memory, word2.addr).now_or_never() {
                None => return Err(WouldBlock),
                Some(Err(errno)) => return Ok(Err(errno)),
                Some(Ok(old)) => old,
            };
            let (new, matches) = match wake_op(encoded, old) {
                Ok(op) => op,
                Err(errno) => return Ok(Err(errno)),
            };
            match cmpxchg(memory, word2.addr, old, new).now_or_never() {
                None => return Err(WouldBlock),
                Some(Err(errno)) => return Ok(Err(errno)),
                Some(Ok(current)) if current == old => break matches,
                Some(Ok(_)) => {}
            }
        };
        let mut woken = match wake_chain(&mut chain, key, n, MATCH_ANY) {
            Ok(woken) => woken,
            Err(errno) => return Ok(Err(errno)),
        };
        if matches {
            let chain2 = other.as_deref_mut().unwrap_or(&mut chain);
            match wake_chain(chain2, key2, n2, MATCH_ANY) {
                Ok(n) => woken += n,
                Err(errno) => return Ok(Err(errno)),
            }
        }
        Ok(Ok(woken))
    }
}

/// `futex_wake` on a locked chain.
fn wake_chain(chain: &mut Chain, key: Key, n: u32, bitset: u32) -> Result<u32, Errno> {
    let mut woken = 0;
    let mut at = 0;
    while at < chain.len() {
        let entry = &chain[at];
        // `futex_match`: other keys share the bucket.
        if entry.key() != key {
            at += 1;
            continue;
        }
        if entry.requeue.is_some() {
            return Err(Errno::EINVAL);
        }
        if entry.bitset & bitset == 0 {
            at += 1;
            continue;
        }
        chain.remove(at).wake();
        woken += 1;
        if woken >= n.max(1) {
            break;
        }
    }
    Ok(woken)
}

/// A waiter on its futex's chain until a wake takes it or this is dropped, so a wait whose
/// future is dropped leaves no entry behind to absorb a later wake.
struct Queued<'a> {
    futexes: &'a Futexes,
    entry: Arc<Entry>,
}

impl<'a> Queued<'a> {
    /// `futex_queue`.
    fn new(futexes: &'a Futexes, entry: Arc<Entry>) -> Self {
        futexes.enqueue(&entry);
        Self { futexes, entry }
    }

    /// Whether a wake (or a requeue onto a PI futex) took the entry off its chain.
    fn taken(&self) -> bool {
        !self.entry.queued.load(Ordering::Acquire)
    }

    /// Leaves the chain; false if a wake took the entry first.
    fn leave(&self) -> bool {
        self.futexes.dequeue(&self.entry)
    }
}

impl Drop for Queued<'_> {
    fn drop(&mut self) {
        self.futexes.dequeue(&self.entry);
    }
}

/// How a futex wait sleeps (`futex_do_wait`): until its own wakeup, a signal, the group's exit
/// or its timer. The group's exit and the signals are registered once for the whole wait.
struct Sleeper<'a> {
    waiter: &'a Waiter,
    process: &'a Process,
    interrupt: Watched<'a>,
    _exiting: WaitRegistration,
}

impl<'a> Sleeper<'a> {
    fn new(waiter: &'a Waiter, process: &'a Process, interrupt: &'a dyn Interrupt) -> Self {
        Self {
            waiter,
            process,
            interrupt: waiter.watch(interrupt),
            _exiting: waiter.register(&process.group.exited, PollMask::empty()),
        }
    }

    /// The task is about to go; its pending exit unwinds it on the way out.
    fn exiting(&self) -> bool {
        self.process.group.exiting()
    }

    async fn sleep(&self, timer: &mut Option<wasixv2_host::LocalFuture<'_, ()>>) -> Wakeup {
        let timer = timer.as_mut().map(|t| t.as_mut());
        self.waiter.sleep(&self.interrupt, timer).await
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

/// Whether the word at `addr` lies in the memory's low guard, which Linux leaves unmapped. The
/// guard never reaches past `mmap_min_addr`, so only words below it ask the memory.
async fn in_low_guard(memory: &dyn UserMemory, addr: u32) -> bool {
    u64::from(addr) < MMAP_MIN_ADDR && addr < memory.low_guard().await
}

/// The faults Linux raises for the futex at `addr` where nothing is mapped: always for a
/// futex whose word the operation reads or changes (`touched`), and for any futex without
/// `private`, whose page `get_futex_key` looks up.
async fn check_mapped(
    memory: &dyn UserMemory,
    addr: u32,
    private: bool,
    touched: bool,
) -> Result<(), Errno> {
    match (!private || touched) && in_low_guard(memory, addr).await {
        true => Err(Errno::EFAULT),
        false => Ok(()),
    }
}

/// `get_user` of the futex word or robust list entry at `addr`, which faults in the low guard.
async fn get_user_word(memory: &dyn UserMemory, addr: u32) -> Result<u32, Errno> {
    if in_low_guard(memory, addr).await {
        return Err(Errno::EFAULT);
    }
    get_word(memory, addr).await
}

/// Whether the word at `addr` holds `expected`, read at its size. A torn 64-bit read only mixes
/// in writes made after the waiter queued, whose wakes still reach it.
async fn holds(memory: &dyn UserMemory, addr: u32, expected: AtomicWord) -> Result<bool, Errno> {
    Ok(match expected {
        AtomicWord::U32(val) => get_word(memory, addr).await? == val,
        AtomicWord::U64(val) => {
            let word = read_value(memory, GuestPtr::<u64>::new(addr))
                .await
                .map_err(|_| Errno::EFAULT)?;
            word == val
        }
    })
}

/// What `futex_lock_pi_atomic` found.
enum Attempt {
    /// The caller owns the lock now.
    Acquired,
    /// Somebody else owns it; `FUTEX_WAITERS` is set and the kernel state exists.
    Wait,
}

impl Kernel {
    /// `get_futex_key` for the word at `addr` of `task`'s memory: in a mapping of a shared
    /// object, unless `private` (`FUTEX_PRIVATE_FLAG`), the object and the word's offset in it;
    /// otherwise the address space and the address.
    pub(crate) fn futex_key(&self, task: TaskId, addr: u32, private: bool) -> Key {
        let mm = self.mm(task);
        let shared = match private {
            true => None,
            false => mm.shared_futex_key(addr),
        };
        shared.unwrap_or(Key::Private { mm: mm.id(), addr })
    }

    /// The futex word at `addr` of the caller's memory.
    fn futex_word(&self, caller: &Caller<'_>, addr: u32, private: bool) -> Word {
        Word {
            key: self.futex_key(caller.task(), addr, private),
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
        let private = op.0 & FutexOp::FUTEX_PRIVATE_FLAG.0 != 0;
        let timeout = match utime {
            ptr if ptr != 0 && has_timeout(cmd) => {
                let ts = read_value(caller.memory(), ConstKernelTimespecPtr::new(ptr)).await?;
                let ts = time::from_timespec(ts)?;
                match cmd {
                    FutexOp::FUTEX_WAIT => Timeout::Until(self.host.monotonic().saturating_add(ts)),
                    FutexOp::FUTEX_LOCK_PI => Timeout::UntilRealtime(ts),
                    _ if realtime => Timeout::UntilRealtime(ts),
                    // An absolute monotonic timeout is in the caller's time namespace.
                    _ => Timeout::Until(self.timens_to_host(caller, time::Clock::Monotonic, ts)),
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
        let word = self.futex_word(caller, uaddr.addr(), private);
        let key = word.key;
        let tid = self.own_tid(caller.task()) as u32;
        let memory = caller.memory();
        match cmd {
            FutexOp::FUTEX_WAIT | FutexOp::FUTEX_WAIT_BITSET => {
                let bitset = match cmd {
                    FutexOp::FUTEX_WAIT => MATCH_ANY,
                    _ => val3,
                };
                if bitset == 0 {
                    return Err(Errno::EINVAL.into());
                }
                check_mapped(memory, word.addr, private, true).await?;
                let val = AtomicWord::U32(val);
                self.futex_wait(caller, word, private, val, bitset, timeout)
                    .await
            }
            FutexOp::FUTEX_WAKE | FutexOp::FUTEX_WAKE_BITSET => {
                let bitset = match cmd {
                    FutexOp::FUTEX_WAKE => MATCH_ANY,
                    _ => val3,
                };
                if bitset == 0 {
                    return Err(Errno::EINVAL.into());
                }
                check_mapped(memory, word.addr, private, false).await?;
                Ok(self.futexes.wake(key, val, bitset)? as i32)
            }
            FutexOp::FUTEX_REQUEUE | FutexOp::FUTEX_CMP_REQUEUE => {
                // `val2` arrives in the timeout slot.
                let (n_wake, n_requeue) = (val as i32, utime as i32);
                if n_wake < 0 || n_requeue < 0 {
                    return Err(Errno::EINVAL.into());
                }
                let compare = cmd == FutexOp::FUTEX_CMP_REQUEUE;
                check_mapped(memory, word.addr, private, compare).await?;
                if !uaddr2.addr().is_multiple_of(4) {
                    return Err(Errno::EINVAL.into());
                }
                check_mapped(memory, uaddr2.addr(), private, false).await?;
                let to = self.futex_key(caller.task(), uaddr2.addr(), private);
                let expected = compare.then_some(val3);
                let counts = (n_wake as u32, n_requeue as u32);
                let requeue = || self.futexes.requeue(memory, word, to, counts, expected);
                Ok(with_word_locked(memory, word.addr, requeue).await? as i32)
            }
            FutexOp::FUTEX_WAKE_OP => {
                check_mapped(memory, word.addr, private, false).await?;
                if !uaddr2.addr().is_multiple_of(4) {
                    return Err(Errno::EINVAL.into());
                }
                // The operation changes the second word.
                check_mapped(memory, uaddr2.addr(), private, true).await?;
                let word2 = self.futex_word(caller, uaddr2.addr(), private);
                let wake_op = || {
                    self.futexes
                        .wake_op(memory, (key, val), (word2, utime), val3)
                };
                Ok(with_word_locked(memory, word2.addr, wake_op).await? as i32)
            }
            FutexOp::FUTEX_LOCK_PI | FutexOp::FUTEX_LOCK_PI2 => {
                check_mapped(memory, word.addr, private, true).await?;
                self.futex_lock_pi(caller, word, tid, timeout).await
            }
            FutexOp::FUTEX_TRYLOCK_PI => {
                check_mapped(memory, word.addr, private, true).await?;
                self.futex_trylock_pi(caller, word, tid).await
            }
            FutexOp::FUTEX_UNLOCK_PI => {
                check_mapped(memory, word.addr, private, true).await?;
                self.futex_unlock_pi(caller, word, tid).await
            }
            FutexOp::FUTEX_WAIT_REQUEUE_PI => {
                if !uaddr2.addr().is_multiple_of(4) {
                    return Err(Errno::EINVAL.into());
                }
                let target = self.futex_key(caller.task(), uaddr2.addr(), private);
                if target == key {
                    return Err(Errno::EINVAL.into());
                }
                check_mapped(memory, uaddr2.addr(), private, false).await?;
                check_mapped(memory, word.addr, private, true).await?;
                self.futex_wait_requeue_pi(caller, word, val, timeout, target, tid)
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
                let target = self.futex_word(caller, uaddr2.addr(), private);
                if target.key == key || n_wake != 1 {
                    return Err(Errno::EINVAL.into());
                }
                // The lock may be taken for the waiter on the second word.
                check_mapped(memory, word.addr, private, true).await?;
                check_mapped(memory, target.addr, private, true).await?;
                self.futex_cmp_requeue_pi(caller, word, target, n_requeue as u32, val3)
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
            Timeout::UntilRealtime(at) => Some(self.time.sleep_until_realtime(at)),
        }
    }

    /// `memory.atomic.wait32`/`wait64`: a `FUTEX_WAIT` on a word of `expected`'s size, shared
    /// where the word lies in a shared object, since the instruction cannot say.
    pub(crate) async fn atomic_wait(
        &self,
        caller: &Caller<'_>,
        addr: u32,
        expected: AtomicWord,
        timeout: Option<Duration>,
    ) -> SysResult<i32> {
        let timeout = match timeout {
            None => Timeout::Never,
            Some(timeout) => Timeout::Until(self.host.monotonic().saturating_add(timeout)),
        };
        let word = self.futex_word(caller, addr, false);
        let waited = self.futex_wait(caller, word, false, expected, MATCH_ANY, timeout);
        // The instruction has no `EINTR`: an untimed wait goes on after any handler.
        waited.await.map_err(|err| match err {
            SysErr::Restart => SysErr::RestartNoIntr,
            err => err,
        })
    }

    /// `memory.atomic.notify`: a `FUTEX_WAKE`, shared as [`Kernel::atomic_wait`] is, except
    /// that a count of 0 wakes none.
    pub(crate) fn atomic_notify(&self, task: TaskId, addr: u32, count: u32) -> u32 {
        if count == 0 {
            return 0;
        }
        let key = self.futex_key(task, addr, false);
        // A `FUTEX_WAIT_REQUEUE_PI` waiter makes `FUTEX_WAKE` fail; the instruction cannot,
        // and wakes none, as the syscall does.
        self.futexes.wake(key, count, MATCH_ANY).unwrap_or(0)
    }

    /// `futex_wait_restart`: a timed wait a signal interrupted, going on to its deadline.
    pub(crate) async fn futex_wait_restart(
        &self,
        caller: &Caller<'_>,
        (addr, private): (u32, bool),
        val: AtomicWord,
        bitset: u32,
        timeout: Timeout,
    ) -> SysResult<i32> {
        let word = self.futex_word(caller, addr, private);
        self.futex_wait(caller, word, private, val, bitset, timeout)
            .await
    }

    /// `futex_wait`, on a word found with `private`.
    async fn futex_wait(
        &self,
        caller: &Caller<'_>,
        word: Word,
        private: bool,
        val: AtomicWord,
        bitset: u32,
        timeout: Timeout,
    ) -> SysResult<i32> {
        match self.do_futex_wait(caller, word, val, bitset, timeout).await {
            // An untimed wait reruns; a timed one goes on to the same deadline.
            Err(SysErr::Restart) if !matches!(timeout, Timeout::Never) => Err(self.restart_with(
                caller,
                Restart::FutexWait {
                    addr: word.addr,
                    private,
                    val,
                    bitset,
                    timeout,
                },
            )),
            ret => ret,
        }
    }

    /// `__futex_wait`: queues on `word` while it holds `val`, then sleeps until a wake, the
    /// timeout (`ETIMEDOUT`) or a signal (`SysErr::Restart`).
    async fn do_futex_wait(
        &self,
        caller: &Caller<'_>,
        word: Word,
        val: AtomicWord,
        bitset: u32,
        timeout: Timeout,
    ) -> SysResult<i32> {
        let waiter = Waiter::new();
        let (process, signals) = (self.process(caller.task()), self.interrupt(caller));
        let sleeper = Sleeper::new(&waiter, &process, &*signals);
        let queued = Queued::new(
            &self.futexes,
            Entry::new(word.key, bitset, waiter.handle(), None),
        );
        match holds(caller.memory(), word.addr, val).await {
            Ok(true) => {}
            Ok(false) => {
                return match queued.leave() {
                    true => Err(Errno::EAGAIN.into()),
                    false => Ok(0),
                };
            }
            Err(fault) => return Err(fault.into()),
        }
        let mut timer = self.futex_timer(timeout);
        loop {
            // Woken: taken off the chain by the wake.
            if queued.taken() || sleeper.exiting() {
                return Ok(0);
            }
            match sleeper.sleep(&mut timer).await {
                Wakeup::Woken(_) => {}
                Wakeup::TimedOut => {
                    return match queued.leave() {
                        true => Err(Errno::ETIMEDOUT.into()),
                        false => Ok(0),
                    };
                }
                Wakeup::Interrupted => {
                    return match queued.leave() {
                        true => Err(SysErr::Restart),
                        false => Ok(0),
                    };
                }
            }
        }
    }

    /// Whether the caller's `tid` is a task that has not exited (`find_get_task_by_vpid`).
    fn task_alive(&self, caller: &Caller<'_>, tid: u32) -> bool {
        let Some(id) = self.find_vpid(caller, tid as i32) else {
            return false;
        };
        let state = self.state.lock_or_recover();
        state
            .tasks
            .get(&wasixv2_syscalls::TaskId(id))
            .is_some_and(|task| !task.thread.exited())
    }

    /// `futex_lock_pi_atomic` for `tid`, with the PI table held: takes a free lock, or sets
    /// `FUTEX_WAITERS` and makes sure the kernel state names the owner.
    async fn futex_lock_pi_atomic(
        &self,
        caller: &Caller<'_>,
        table: &mut HashMap<Key, PiState>,
        futex: Word,
        tid: u32,
    ) -> Result<Attempt, Errno> {
        let (key, addr) = (futex.key, futex.addr);
        let memory = caller.memory();
        loop {
            let word = get_word(memory, addr).await?;
            let owner = word & FUTEX_TID_MASK;
            if owner == tid {
                return Err(Errno::EDEADLK);
            }
            let queued = table
                .get_mut(&key)
                .is_some_and(|state| state.first().is_some());
            if queued {
                // `attach_to_pi_state`: the word must name the kernel's owner, or no owner once
                // the robust list marked it dead.
                let known = table[&key].owner;
                let agrees = owner == known || (owner == 0 && word & FUTEX_OWNER_DIED != 0);
                if !agrees {
                    return Err(Errno::EINVAL);
                }
                if word & FUTEX_WAITERS == 0
                    && cmpxchg(memory, addr, word, word | FUTEX_WAITERS).await? != word
                {
                    continue;
                }
                return Ok(Attempt::Wait);
            }
            if owner == 0 {
                // Free, perhaps after its owner died: take it, keeping `FUTEX_OWNER_DIED`.
                let new = (word & FUTEX_OWNER_DIED) | tid;
                if cmpxchg(memory, addr, word, new).await? != word {
                    continue;
                }
                return Ok(Attempt::Acquired);
            }
            if cmpxchg(memory, addr, word, word | FUTEX_WAITERS).await? != word {
                continue;
            }
            // `attach_to_pi_owner`: the owner must exist (`handle_exit_race`).
            if !self.task_alive(caller, owner) {
                let now = get_word(memory, addr).await?;
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

    async fn futex_trylock_pi(&self, caller: &Caller<'_>, word: Word, tid: u32) -> SysResult<i32> {
        let mut table = self.futexes.pi.lock().await;
        let attempt = self
            .futex_lock_pi_atomic(caller, &mut table, word, tid)
            .await?;
        drop_idle(&mut table, word.key);
        match attempt {
            Attempt::Acquired => Ok(0),
            Attempt::Wait => Err(Errno::EAGAIN.into()),
        }
    }

    /// `futex_lock_pi`.
    async fn futex_lock_pi(
        &self,
        caller: &Caller<'_>,
        word: Word,
        tid: u32,
        timeout: Timeout,
    ) -> SysResult<i32> {
        let key = word.key;
        let waiter = Waiter::new();
        let pi = PiGuard(PiWaiter::new(tid, waiter.handle()));
        {
            let mut table = self.futexes.pi.lock().await;
            let attempt = self
                .futex_lock_pi_atomic(caller, &mut table, word, tid)
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
                    state.waiters.push_back(pi.0.clone());
                }
            }
        }
        let (process, signals) = (self.process(caller.task()), self.interrupt(caller));
        let sleeper = Sleeper::new(&waiter, &process, &*signals);
        match self.pi_wait(&sleeper, key, &pi.0, timeout).await {
            Ok(()) => Ok(0),
            Err(PiInterrupt::TimedOut) => Err(Errno::ETIMEDOUT.into()),
            Err(PiInterrupt::Signal) => Err(SysErr::RestartNoIntr),
        }
    }

    /// `rt_mutex_wait_proxy_lock` and its cleanup: sleeps until the lock is handed to `pi`.
    /// On a timeout or a signal the waiter leaves, unless the lock reached it first.
    async fn pi_wait(
        &self,
        sleeper: &Sleeper<'_>,
        key: Key,
        pi: &Arc<PiWaiter>,
        timeout: Timeout,
    ) -> Result<(), PiInterrupt> {
        let mut timer = self.futex_timer(timeout);
        let why = loop {
            // Granted only once the futex word names the waiter.
            if pi.granted() {
                return Ok(());
            }
            if sleeper.exiting() {
                break PiInterrupt::Signal;
            }
            match sleeper.sleep(&mut timer).await {
                Wakeup::Woken(_) => {}
                Wakeup::TimedOut => break PiInterrupt::TimedOut,
                Wakeup::Interrupted => break PiInterrupt::Signal,
            }
        };
        let mut table = self.futexes.pi.lock().await;
        if !pi.leave() {
            return Ok(());
        }
        if let Some(state) = table.get_mut(&key) {
            state.waiters.retain(|w| !Arc::ptr_eq(w, pi));
        }
        drop_idle(&mut table, key);
        Err(why)
    }

    /// `futex_unlock_pi`.
    async fn futex_unlock_pi(&self, caller: &Caller<'_>, futex: Word, tid: u32) -> SysResult<i32> {
        let (key, addr) = (futex.key, futex.addr);
        let memory = caller.memory();
        let mut table = self.futexes.pi.lock().await;
        loop {
            let word = get_word(memory, addr).await?;
            if word & FUTEX_TID_MASK != tid {
                return Err(Errno::EPERM.into());
            }
            match pass_on(memory, &mut table, futex, word, 0).await? {
                Handover::Granted => return Ok(0),
                Handover::Changed(current) if current & FUTEX_TID_MASK == tid => continue,
                Handover::Changed(_) => return Err(Errno::EINVAL.into()),
                Handover::Nobody(word) => {
                    // No waiters: the word may still say there are, from a waiter that left.
                    if cmpxchg(memory, addr, word, 0).await? != word {
                        continue;
                    }
                    drop_idle(&mut table, key);
                    return Ok(0);
                }
            }
        }
    }

    /// `futex_wait_requeue_pi`: waits on `key` like `FUTEX_WAIT` until `FUTEX_CMP_REQUEUE_PI`
    /// either takes the PI futex `target` for the caller or moves it there, then waits for the
    /// lock.
    async fn futex_wait_requeue_pi(
        &self,
        caller: &Caller<'_>,
        word: Word,
        val: u32,
        timeout: Timeout,
        target: Key,
        tid: u32,
    ) -> SysResult<i32> {
        let waiter = Waiter::new();
        let (process, signals) = (self.process(caller.task()), self.interrupt(caller));
        let sleeper = Sleeper::new(&waiter, &process, &*signals);
        let pi = PiGuard(PiWaiter::new(tid, waiter.handle()));
        let requeue = Arc::new(Requeue {
            target,
            pi: pi.0.clone(),
            moved: AtomicBool::new(false),
        });
        let entry = Entry::new(word.key, MATCH_ANY, waiter.handle(), Some(requeue.clone()));
        // The PI table orders this against a requeue that already looked at the queue.
        let queued = {
            let _table = self.futexes.pi.lock().await;
            Queued::new(&self.futexes, entry)
        };
        let early = match get_word(caller.memory(), word.addr).await {
            Ok(current) if current == val => None,
            Ok(_) => Some(SysErr::from(Errno::EAGAIN)),
            Err(errno) => Some(errno.into()),
        };
        let early = match early {
            Some(err) => Some(err),
            None => self.requeue_pi_sleep(&sleeper, &requeue, timeout).await,
        };
        if let Some(err) = early
            && self.leave_requeue_queue(&queued).await
        {
            return Err(err);
        }
        // On the PI futex now, perhaps already its owner. A deadline that passed, or a signal,
        // ends the wait at once unless the lock is there.
        match self.pi_wait(&sleeper, target, &pi.0, timeout).await {
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
        sleeper: &Sleeper<'_>,
        requeue: &Requeue,
        timeout: Timeout,
    ) -> Option<SysErr> {
        let mut timer = self.futex_timer(timeout);
        loop {
            if requeue.moved.load(Ordering::Acquire) {
                return None;
            }
            if sleeper.exiting() {
                return Some(SysErr::RestartNoIntr);
            }
            match sleeper.sleep(&mut timer).await {
                Wakeup::Woken(_) => {}
                Wakeup::TimedOut => return Some(Errno::ETIMEDOUT.into()),
                Wakeup::Interrupted => return Some(SysErr::RestartNoIntr),
            }
        }
    }

    /// Takes a `FUTEX_WAIT_REQUEUE_PI` waiter off its first queue. False if a requeue moved it
    /// first, so it now waits on the PI futex.
    async fn leave_requeue_queue(&self, queued: &Queued<'_>) -> bool {
        let _table = self.futexes.pi.lock().await;
        queued.leave()
    }

    /// `futex_requeue` with `requeue_pi`: takes the PI futex `futex` for the first waiter on
    /// `word` if it is free, and moves up to `n_requeue` waiters (one more if the lock was not
    /// free) onto it.
    async fn futex_cmp_requeue_pi(
        &self,
        caller: &Caller<'_>,
        word: Word,
        futex: Word,
        n_requeue: u32,
        val3: u32,
    ) -> SysResult<i32> {
        let (key, target) = (word.key, futex.key);
        let mut table = self.futexes.pi.lock().await;
        let current = get_word(caller.memory(), word.addr).await?;
        if current != val3 {
            return Err(Errno::EAGAIN.into());
        }
        let mut count: i64 = 0;
        let mut first = true;
        for entry in self.futexes.waiters(key) {
            if count > i64::from(n_requeue) {
                break;
            }
            let Some(requeue) = entry.requeue.clone() else {
                return Err(Errno::EINVAL.into());
            };
            if requeue.target != target {
                return Err(Errno::EINVAL.into());
            }
            // A waiter whose wait was dropped takes nothing.
            if !requeue.pi.waiting() {
                continue;
            }
            let attempt = self
                .futex_lock_pi_atomic(caller, &mut table, futex, requeue.pi.tid)
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
            if !self.futexes.dequeue(&entry) {
                continue;
            }
            requeue.moved.store(true, Ordering::Release);
            match acquired {
                true => {
                    requeue.pi.grant();
                }
                false => {
                    let state = table.get_mut(&target).expect("attached by the attempt");
                    state.waiters.push_back(requeue.pi.clone());
                }
            }
            entry.task.wake();
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
            let Ok(word) = get_user_word(memory, addr).await else {
                return;
            };
            let owner = word & FUTEX_TID_MASK;
            if owner != tid {
                if pending && !pi && (owner == 0 || word & FUTEX_WAITERS == 0) {
                    let _ =
                        self.futexes
                            .wake(self.futex_key(caller.task(), addr, false), 1, MATCH_ANY);
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
                    .wake(self.futex_key(caller.task(), addr, false), 1, MATCH_ANY);
            }
            return;
        }
    }

    /// `exit_robust_list`: releases the robust futexes the exiting task `tid` still holds. Faults
    /// end the walk silently.
    pub(crate) async fn exit_robust_list(&self, caller: &Caller<'_>, head: u32, tid: u32) {
        // A head at the top of memory faults, as reading past it would.
        let read = |addr: Option<u32>| async move {
            get_user_word(caller.memory(), addr.ok_or(Errno::EFAULT)?).await
        };
        let (Ok(mut entry), Ok(offset), Ok(pending)) = (
            read(Some(head)).await,
            read(head.checked_add(4)).await,
            read(head.checked_add(8)).await,
        ) else {
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
            let Ok(next) = read(Some(entry)).await else {
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
    /// first waiter, which sees `FUTEX_OWNER_DIED` if the robust list set it. A shared futex's
    /// word is found where the caller's memory maps its object; where it no longer does, as for a
    /// word that faults, the waiter gets the lock without the word.
    pub(crate) async fn exit_pi_state_list(&self, caller: &Caller<'_>, tid: u32) {
        let memory = caller.memory();
        let mm = self.mm(caller.task());
        let mut table = self.futexes.pi.lock().await;
        let owned: Vec<(Key, Option<u32>)> = table
            .iter()
            .filter(|(_, state)| state.owner == tid)
            .filter_map(|(&key, _)| match key {
                Key::Private { mm: id, addr } => (id == mm.id()).then_some((key, Some(addr))),
                Key::Shared { object, offset } => Some((key, mm.address_of(object, offset))),
            })
            .collect();
        for (key, addr) in owned {
            loop {
                let word = match addr {
                    Some(addr) => get_word(memory, addr).await.ok().map(|word| (addr, word)),
                    None => None,
                };
                let Some((addr, word)) = word else {
                    // The word is gone; the waiter still gets the lock rather than wait forever.
                    hand_over(&mut table, key);
                    break;
                };
                let futex = Word { key, addr };
                match pass_on(memory, &mut table, futex, word, FUTEX_OWNER_DIED).await {
                    Ok(Handover::Changed(_)) => continue,
                    Ok(Handover::Granted | Handover::Nobody(_)) => break,
                    Err(_) => {
                        hand_over(&mut table, key);
                        break;
                    }
                }
            }
            drop_idle(&mut table, key);
        }
    }
}

/// Runs `op`, a futex operation that reads the futex word holding bucket locks, until the read
/// did not have to block (`futex_get_value_locked`). When it had to, the word at `addr` is read
/// with no lock held, as Linux faults the page in, and the operation starts over.
async fn with_word_locked<T>(
    memory: &dyn UserMemory,
    addr: u32,
    mut op: impl FnMut() -> Locked<T>,
) -> Result<T, Errno> {
    loop {
        match op() {
            Ok(result) => return result,
            Err(WouldBlock) => {
                get_word(memory, addr).await?;
            }
        }
    }
}

/// Why a PI wait ended without the lock.
enum PiInterrupt {
    TimedOut,
    Signal,
}

/// How [`pass_on`] ended.
enum Handover {
    /// The first waiter owns the lock now, and the word names it.
    Granted,
    /// Nobody waits any more; the word holds this, unchanged by the handover unless it named
    /// waiters that left meanwhile.
    Nobody(u32),
    /// The word did not hold what the caller read, but this.
    Changed(u32),
}

/// `wake_futex_pi`, with the PI table held: the first waiter of `futex` becomes the owner. The
/// word, which held `word`, names it with `FUTEX_WAITERS`, keeping the bits of `word` in
/// `keep`, before the waiter learns it owns the lock. A waiter that leaves in between is
/// skipped.
async fn pass_on(
    memory: &dyn UserMemory,
    table: &mut HashMap<Key, PiState>,
    futex: Word,
    mut word: u32,
    keep: u32,
) -> Result<Handover, Errno> {
    let key = futex.key;
    loop {
        let Some(next) = table.get_mut(&key).and_then(PiState::first) else {
            return Ok(Handover::Nobody(word));
        };
        let new = (word & keep) | FUTEX_WAITERS | next.tid;
        let current = cmpxchg(memory, futex.addr, word, new).await?;
        if current != word {
            return Ok(Handover::Changed(current));
        }
        let state = table.get_mut(&key).expect("looked up above");
        state.waiters.pop_front();
        if next.grant() {
            state.owner = next.tid;
            drop_idle(table, key);
            return Ok(Handover::Granted);
        }
        word = new;
    }
}

/// Gives the lock at `key` to its first waiter without touching the futex word.
fn hand_over(table: &mut HashMap<Key, PiState>, key: Key) {
    let Some(state) = table.get_mut(&key) else {
        return;
    };
    while let Some(next) = state.first() {
        state.waiters.pop_front();
        if next.grant() {
            state.owner = next.tid;
            break;
        }
    }
}

/// Forgets the kernel state of a PI futex nobody waits on.
fn drop_idle(table: &mut HashMap<Key, PiState>, key: Key) {
    if table
        .get_mut(&key)
        .is_some_and(|state| state.first().is_none())
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
    // FUTEX_OP_OPARG_SHIFT. Linux masks a shift out of range rather than fail.
    if encoded & (8 << 28) != 0 {
        oparg = 1 << (oparg & 31);
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
        // A shift of 35 is one of 3.
        let encoded = (8 << 28) | (2 << 28) | (35 << 12);
        assert_eq!(wake_op(encoded, 0), Ok((8, true)));
    }

    use wasixv2_syscalls::{Fault, LocalFuture};

    /// One futex word, at address 0.
    struct One(std::cell::Cell<u32>);

    impl UserMemory for One {
        fn read<'a>(&'a self, addr: u32, buf: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
            let result = match (addr, buf.len()) {
                (0, 4) => {
                    buf.copy_from_slice(&self.0.get().to_le_bytes());
                    Ok(())
                }
                _ => Err(Fault),
            };
            Box::pin(async move { result })
        }

        fn write<'a>(&'a self, _: u32, _: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
            Box::pin(async { Err(Fault) })
        }

        fn size(&self) -> LocalFuture<'_, u64> {
            Box::pin(async { 4 })
        }

        fn grow(&self, _: u32) -> LocalFuture<'_, Option<u64>> {
            Box::pin(async { None })
        }
    }

    fn entry(key: Key, waiter: &Waiter) -> Arc<Entry> {
        Entry::new(key, MATCH_ANY, waiter.handle(), None)
    }

    #[test]
    fn wake_counts_and_requeue_moves() {
        let futexes = Futexes::default();
        let memory = One(std::cell::Cell::new(7));
        let a = Key::Private { mm: 1, addr: 0 };
        let b = Key::Private { mm: 1, addr: 8 };
        let waiters: Vec<_> = (0..4).map(|_| Waiter::new()).collect();
        let entries: Vec<_> = waiters
            .iter()
            .map(|waiter| {
                let entry = entry(a, waiter);
                futexes.enqueue(&entry);
                entry
            })
            .collect();
        // A count of 0 wakes one, as on Linux.
        assert_eq!(futexes.wake(a, 0, MATCH_ANY), Ok(1));
        assert!(!entries[0].queued.load(Ordering::Acquire));
        assert!(waiters[0].take_wakeup().is_some());
        // `FUTEX_CMP_REQUEUE` compares under the bucket locks, and changes nothing on a mismatch.
        let from = Word { key: a, addr: 0 };
        let requeue = |expected| futexes.requeue(&memory, from, b, (1, 1), expected);
        assert!(matches!(requeue(Some(8)), Ok(Err(Errno::EAGAIN))));
        assert!(waiters[1].take_wakeup().is_none());
        assert!(matches!(requeue(Some(7)), Ok(Ok(2))));
        assert!(waiters[1].take_wakeup().is_some());
        assert_eq!(entries[2].key(), b);
        assert_eq!(futexes.wake(b, 10, MATCH_ANY), Ok(1));
        assert!(waiters[2].take_wakeup().is_some());
        assert!(futexes.dequeue(&entries[3]));
        assert!(!futexes.dequeue(&entries[3]));
    }

    #[test]
    fn waiters_of_keys_sharing_a_bucket_stay_apart() {
        let futexes = Futexes {
            buckets: vec![Mutex::default()].into_boxed_slice(),
            pi: Default::default(),
        };
        let (a, b) = (
            Key::Private { mm: 1, addr: 0 },
            Key::Private { mm: 2, addr: 0 },
        );
        let (on_a, on_b) = (Waiter::new(), Waiter::new());
        let (entry_a, entry_b) = (entry(a, &on_a), entry(b, &on_b));
        futexes.enqueue(&entry_a);
        futexes.enqueue(&entry_b);

        assert_eq!(futexes.wake(b, 1, MATCH_ANY), Ok(1));
        assert!(on_b.take_wakeup().is_some() && on_a.take_wakeup().is_none());
        // A requeue within the bucket only renames the key.
        let c = Key::Private { mm: 1, addr: 4 };
        assert!(matches!(
            futexes.requeue(
                &One(Default::default()),
                Word { key: a, addr: 0 },
                c,
                (0, 1),
                None
            ),
            Ok(Ok(1))
        ));
        assert_eq!(futexes.wake(a, 1, MATCH_ANY), Ok(0));
        assert_eq!(futexes.wake(c, 1, MATCH_ANY), Ok(1));
        assert!(on_a.take_wakeup().is_some());
    }

    #[test]
    fn a_dropped_wait_leaves_its_chain() {
        let futexes = Futexes::default();
        let key = Key::Private { mm: 1, addr: 0 };
        let (dropped, stays) = (Waiter::new(), Waiter::new());
        let queued = Queued::new(&futexes, entry(key, &dropped));
        let _stays = Queued::new(&futexes, entry(key, &stays));
        drop(queued);

        // The wake goes to the waiter that is still there.
        assert_eq!(futexes.wake(key, 1, MATCH_ANY), Ok(1));
        assert!(stays.take_wakeup().is_some());
        assert!(dropped.take_wakeup().is_none());
    }
}
