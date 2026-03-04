//! Epoll runtime implementation for WASIX.
//!
//! This module centralizes epoll internals behind a small crate-internal API used
//! by `epoll_create`, `epoll_ctl`, and `epoll_wait`.
//!
//! ## Architecture
//!
//! The implementation uses:
//! - `EpollState`: global state for one epoll fd (`subscriptions`, ready queue, notifier).
//! - `EpollSubState`: per-watched-fd state (`pending_bits`, `enqueued`, `generation`, metadata).
//! - `EpollHandler`: producer-side interest handler attached to socket/pipe/notification sources.
//!
//! ## Important flows
//!
//! ### Registration (`epoll_ctl` ADD/MOD/DEL)
//! 1. `epoll_ctl` creates/removes `EpollSubState` entries in `EpollState.subscriptions`.
//! 2. On ADD/MOD, `register_epoll_handler()` installs an `EpollHandler` on the target fd.
//! 3. Guard ownership is stored in `EpollSubState.joins`; dropping a subscription drops guards
//!    and unregisters handlers.
//!
//! ### Producer path (`EpollHandler::push_interest`)
//! 1. Convert `InterestType` to a readiness bit.
//! 2. Set `pending_bits` atomically.
//! 3. Enqueue exactly one `ReadyItem` per subscription while `enqueued == true`.
//! 4. Wake one waiter via `Notify`.
//!
//! ### Consumer path (`drain_ready_events` used by `epoll_wait`)
//! 1. Pop `ReadyItem` from the ready queue.
//! 2. Resolve the current subscription and drop stale/missing entries.
//! 3. Atomically take readiness bits, clear `enqueued`, and map bits to output events.
//! 4. Repair queue state if a race set new bits during draining.
//!
//! ## Correctness model
//!
//! - `pending_bits` is the source of truth for unread readiness.
//! - `enqueued` deduplicates queue presence per subscription.
//! - `generation` prevents stale queued entries from emitting after DEL/MOD.
//! - Consumer logic tolerates stale queue entries and must never await while holding locks.
//!
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
};

use fnv::FnvHashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use virtual_mio::{InterestHandler, InterestType};
use virtual_net::net_error_into_io_err;
use wasmer_wasix_types::wasi::{
    EpollEventCtl, EpollType, Errno, Eventtype, Fd as WasiFd, Subscription,
    SubscriptionFsReadwrite, SubscriptionUnion,
};

use crate::{
    fs::{InodeValFilePollGuard, InodeValFilePollGuardMode},
    state::{PollEvent, PollEventBuilder, WasiState},
    syscalls::poll_fd_guard,
};

const READABLE_BIT: u8 = 1 << 0;
const WRITABLE_BIT: u8 = 1 << 1;
const HUP_BIT: u8 = 1 << 2;
const ERR_BIT: u8 = 1 << 3;

static EPOLL_ENQUEUE_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static EPOLL_ENQUEUE_DEDUPE_HITS: AtomicU64 = AtomicU64::new(0);
static EPOLL_STALE_GENERATION_DROPS: AtomicU64 = AtomicU64::new(0);
static EPOLL_EMPTY_DEQUEUE_ENTRIES: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpollFd {
    /// Event mask configured by the caller (`epoll_ctl`).
    events: EpollType,
    /// Opaque pointer payload from the caller.
    ptr: u64,
    /// Watched file descriptor.
    fd: WasiFd,
    /// Additional user payload.
    data1: u32,
    /// Additional user payload.
    data2: u64,
}

impl EpollFd {
    /// Creates immutable metadata for one epoll subscription.
    pub fn new(events: EpollType, ptr: u64, fd: WasiFd, data1: u32, data2: u64) -> Self {
        Self {
            events,
            ptr,
            fd,
            data1,
            data2,
        }
    }

    /// Converts the syscall control payload into subscription metadata.
    pub fn from_event_ctl(fd: WasiFd, event: &EpollEventCtl) -> Self {
        Self::new(event.events, event.ptr, fd, event.data1, event.data2)
    }

    /// Returns the configured event mask for this subscription.
    pub fn events(&self) -> EpollType {
        self.events
    }

    /// Returns the caller-supplied pointer payload.
    pub fn ptr(&self) -> u64 {
        self.ptr
    }

    /// Returns the watched file descriptor.
    pub fn fd(&self) -> WasiFd {
        self.fd
    }

    /// Returns the first user payload value.
    pub fn data1(&self) -> u32 {
        self.data1
    }

    /// Returns the second user payload value.
    pub fn data2(&self) -> u64 {
        self.data2
    }
}

#[derive(Debug)]
pub struct EpollJoinGuard {
    /// Underlying poll registration guard.
    fd_guard: InodeValFilePollGuard,
}

impl EpollJoinGuard {
    fn new(fd_guard: InodeValFilePollGuard) -> Self {
        Self { fd_guard }
    }
}

impl Drop for EpollJoinGuard {
    fn drop(&mut self) {
        // Dropping a subscription must detach its interest handler from the source.
        match &self.fd_guard.mode {
            InodeValFilePollGuardMode::File(_) => {
                // Intentionally ignored, epoll doesn't work with files
            }
            InodeValFilePollGuardMode::Socket { inner } => {
                let mut inner = inner.protected.write().unwrap();
                inner.remove_handler();
            }
            InodeValFilePollGuardMode::EventNotifications(inner) => {
                inner.remove_interest_handler();
            }
            InodeValFilePollGuardMode::DuplexPipe { pipe } => {
                let inner = pipe.write().unwrap();
                inner.remove_interest_handler();
            }
            InodeValFilePollGuardMode::PipeRx { rx } => {
                let inner = rx.write().unwrap();
                inner.remove_interest_handler();
            }
            InodeValFilePollGuardMode::PipeTx { .. } => {
                // Intentionally ignored, the sending end of a pipe can't have an interest handler
            }
        }
    }
}

#[derive(Debug)]
pub struct EpollState {
    /// Active subscriptions keyed by watched fd.
    subscriptions: StdMutex<FnvHashMap<WasiFd, Arc<EpollSubState>>>,
    /// Ready queue of subscriptions with potentially pending bits.
    ready: StdMutex<VecDeque<ReadyItem>>,
    /// Wake primitive for blocked `epoll_wait`.
    notify: Notify,
}

impl Default for EpollState {
    fn default() -> Self {
        Self::new()
    }
}

impl EpollState {
    /// Creates a fresh epoll runtime state.
    pub fn new() -> Self {
        Self {
            subscriptions: StdMutex::new(FnvHashMap::default()),
            ready: StdMutex::new(VecDeque::new()),
            notify: Notify::new(),
        }
    }

    fn insert_subscription(&self, fd: WasiFd, state: Arc<EpollSubState>) {
        self.subscriptions.lock().unwrap().insert(fd, state);
    }

    fn remove_subscription(&self, fd: WasiFd) -> Option<Arc<EpollSubState>> {
        self.subscriptions.lock().unwrap().remove(&fd)
    }

    fn restore_subscription(&self, fd: WasiFd, previous: Option<Arc<EpollSubState>>) {
        let mut subscriptions = self.subscriptions.lock().unwrap();
        subscriptions.remove(&fd);
        if let Some(previous) = previous {
            subscriptions.insert(fd, previous);
        }
    }

    fn subscription(&self, fd: WasiFd) -> Option<Arc<EpollSubState>> {
        self.subscriptions.lock().unwrap().get(&fd).cloned()
    }

    fn enqueue_ready(&self, fd: WasiFd, generation: u64) {
        self.ready
            .lock()
            .unwrap()
            .push_back(ReadyItem { fd, generation });
        self.notify.notify_one();
    }

    fn dequeue_ready(&self) -> Option<ReadyItem> {
        self.ready.lock().unwrap().pop_front()
    }

    /// Waits until a producer enqueues readiness and notifies.
    pub async fn wait(&self) {
        self.notify.notified().await;
    }

    pub(crate) fn prepare_add(
        &self,
        fd: WasiFd,
        event: &EpollEventCtl,
    ) -> Result<(EpollFd, Arc<EpollSubState>), Errno> {
        if self.subscriptions.lock().unwrap().contains_key(&fd) {
            return Err(Errno::Exist);
        }

        let (epoll_fd, sub_state) = self.build_pending_subscription(fd, event, 1);
        self.insert_subscription(fd, sub_state.clone());
        Ok((epoll_fd, sub_state))
    }

    pub(crate) fn prepare_mod(
        &self,
        fd: WasiFd,
        event: &EpollEventCtl,
    ) -> Result<(EpollFd, Arc<EpollSubState>, Arc<EpollSubState>), Errno> {
        let Some(previous) = self.remove_subscription(fd) else {
            return Err(Errno::Noent);
        };
        tracing::trace!(fd, "unregistering waker");

        let (epoll_fd, sub_state) =
            self.build_pending_subscription(fd, event, previous.next_generation());
        self.insert_subscription(fd, sub_state.clone());
        Ok((epoll_fd, sub_state, previous))
    }

    pub(crate) fn apply_del(&self, fd: WasiFd) -> Result<(), Errno> {
        self.remove_subscription(fd).map(|_| ()).ok_or(Errno::Noent)
    }

    pub(crate) fn rollback_registration(&self, fd: WasiFd, previous: Option<Arc<EpollSubState>>) {
        self.restore_subscription(fd, previous);
    }

    fn build_pending_subscription(
        &self,
        fd: WasiFd,
        event: &EpollEventCtl,
        generation: u64,
    ) -> (EpollFd, Arc<EpollSubState>) {
        let epoll_fd = EpollFd::from_event_ctl(fd, event);
        tracing::trace!(
            peb = ?event.events,
            ptr = ?event.ptr,
            data1 = event.data1,
            data2 = event.data2,
            fd,
            "registering waker"
        );
        let sub_state = Arc::new(EpollSubState::new(epoll_fd.clone(), generation));
        (epoll_fd, sub_state)
    }
}

#[derive(Debug)]
pub struct EpollSubState {
    /// Snapshot of user-visible metadata.
    fd_meta: StdMutex<EpollFd>,
    /// Guard ownership for all attached handlers.
    joins: StdMutex<Vec<EpollJoinGuard>>,
    /// Atomic readiness bitset (EPOLLIN/OUT/HUP/ERR).
    pending_bits: AtomicU8,
    /// Queue dedupe flag: whether this sub already has a ready-queue entry.
    enqueued: AtomicBool,
    /// Generation used to invalidate stale queue entries after DEL/MOD.
    generation: AtomicU64,
}

impl EpollSubState {
    /// Creates a new subscription state with empty readiness and queue state.
    pub fn new(fd_meta: EpollFd, generation: u64) -> Self {
        Self {
            fd_meta: StdMutex::new(fd_meta),
            joins: StdMutex::new(Vec::new()),
            pending_bits: AtomicU8::new(0),
            enqueued: AtomicBool::new(false),
            generation: AtomicU64::new(generation),
        }
    }

    /// Returns `generation + 1` without mutating the current subscription.
    ///
    /// Callers use this to seed the generation of a replacement subscription.
    pub fn next_generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire).saturating_add(1)
    }

    /// Adds a registration guard that will detach handlers when dropped.
    pub fn add_join(&self, join: EpollJoinGuard) {
        self.joins.lock().unwrap().push(join);
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    fn fd_meta(&self) -> EpollFd {
        self.fd_meta.lock().unwrap().clone()
    }

    fn set_pending(&self, bit: u8) -> bool {
        let old_bits = self.pending_bits.fetch_or(bit, Ordering::AcqRel);
        (old_bits & bit) == 0
    }

    fn take_pending_bits(&self) -> u8 {
        self.pending_bits.swap(0, Ordering::AcqRel)
    }

    fn pending_bits(&self) -> u8 {
        self.pending_bits.load(Ordering::Acquire)
    }

    fn mark_enqueued(&self) -> bool {
        self.enqueued
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    fn clear_enqueued(&self) {
        self.enqueued.store(false, Ordering::Release);
    }
}

#[derive(Debug, Clone, Copy)]
struct ReadyItem {
    /// Watched fd key used to resolve current subscription state.
    fd: WasiFd,
    /// Generation snapshot captured when enqueued.
    generation: u64,
}

/// Maps epoll readiness flags into internal pending-bit positions.
pub(crate) fn epoll_type_to_pending_bit(readiness: EpollType) -> Option<u8> {
    if readiness == EpollType::EPOLLIN {
        Some(READABLE_BIT)
    } else if readiness == EpollType::EPOLLOUT {
        Some(WRITABLE_BIT)
    } else if readiness == EpollType::EPOLLHUP {
        Some(HUP_BIT)
    } else if readiness == EpollType::EPOLLERR {
        Some(ERR_BIT)
    } else {
        None
    }
}

/// Maps a source interest callback variant into the internal pending-bit mask.
fn interest_to_pending_bit(interest: InterestType) -> u8 {
    match interest {
        InterestType::Readable => READABLE_BIT,
        InterestType::Writable => WRITABLE_BIT,
        InterestType::Closed => HUP_BIT,
        InterestType::Error => ERR_BIT,
    }
}

/// Converts consumed pending bits into caller-visible epoll events.
fn pending_bits_to_events(bits: u8, mask: EpollType) -> Vec<EpollType> {
    let mut events = Vec::with_capacity(4);
    if let Some(bit) = epoll_type_to_pending_bit(EpollType::EPOLLIN)
        && (bits & bit) != 0
        && mask.contains(EpollType::EPOLLIN)
    {
        events.push(EpollType::EPOLLIN);
    }
    if let Some(bit) = epoll_type_to_pending_bit(EpollType::EPOLLOUT)
        && (bits & bit) != 0
        && mask.contains(EpollType::EPOLLOUT)
    {
        events.push(EpollType::EPOLLOUT);
    }
    if let Some(bit) = epoll_type_to_pending_bit(EpollType::EPOLLHUP)
        && (bits & bit) != 0
    {
        events.push(EpollType::EPOLLHUP);
    }
    if let Some(bit) = epoll_type_to_pending_bit(EpollType::EPOLLERR)
        && (bits & bit) != 0
    {
        events.push(EpollType::EPOLLERR);
    }
    events
}

fn epoll_mask_to_pending_bits(mask: EpollType) -> u8 {
    let mut bits = 0;
    if mask.contains(EpollType::EPOLLIN) {
        bits |= READABLE_BIT;
    }
    if mask.contains(EpollType::EPOLLOUT) {
        bits |= WRITABLE_BIT;
    }
    // EPOLLHUP/EPOLLERR are always reported by epoll when present.
    bits |= HUP_BIT;
    bits |= ERR_BIT;
    bits
}

/// Re-enqueues a subscription if new pending bits arrived during/after consumer drain.
fn repair_ready_queue_after_drain(
    epoll_state: &Arc<EpollState>,
    fd: WasiFd,
    sub_state: &Arc<EpollSubState>,
) {
    if sub_state.pending_bits() != 0 && sub_state.mark_enqueued() {
        epoll_state.enqueue_ready(fd, sub_state.generation());
    }
}

/// Drains ready items into `(EpollFd, readiness)` events up to `maxevents`.
///
/// This is the consumer hot path used by `epoll_wait`. It is designed to be:
/// - O(number of dequeued ready items)
/// - tolerant of stale queue entries
/// - race-safe with producers setting bits concurrently
pub(crate) fn drain_ready_events(
    epoll_state: &Arc<EpollState>,
    maxevents: usize,
) -> Vec<(EpollFd, EpollType)> {
    let mut ret: Vec<(EpollFd, EpollType)> = Vec::new();
    while ret.len() < maxevents {
        let Some(item) = epoll_state.dequeue_ready() else {
            break;
        };

        let Some(sub_state) = epoll_state.subscription(item.fd) else {
            epoll_empty_dequeue_entry();
            continue;
        };

        if sub_state.generation() != item.generation {
            epoll_stale_generation_drop();
            continue;
        }

        let bits = sub_state.take_pending_bits();
        sub_state.clear_enqueued();

        if bits == 0 {
            repair_ready_queue_after_drain(epoll_state, item.fd, &sub_state);
            epoll_empty_dequeue_entry();
            continue;
        }

        let event = sub_state.fd_meta();
        let mut undispatched_bits = bits & epoll_mask_to_pending_bits(event.events());
        for readiness in pending_bits_to_events(bits, event.events()) {
            if ret.len() >= maxevents {
                break;
            }
            ret.push((event.clone(), readiness));
            if let Some(bit) = epoll_type_to_pending_bit(readiness) {
                undispatched_bits &= !bit;
            }
        }

        if undispatched_bits != 0 {
            sub_state
                .pending_bits
                .fetch_or(undispatched_bits, Ordering::AcqRel);
        }
        repair_ready_queue_after_drain(epoll_state, item.fd, &sub_state);

        if ret.len() >= maxevents {
            break;
        }
    }
    ret
}

#[derive(Debug)]
struct EpollHandler {
    /// Watched fd associated with the subscription.
    fd: WasiFd,
    /// Parent epoll state for queueing and wakeups.
    epoll_state: Arc<EpollState>,
    /// Per-subscription state updated by interest callbacks.
    sub_state: Arc<EpollSubState>,
}

impl EpollHandler {
    fn new(fd: WasiFd, epoll_state: Arc<EpollState>, sub_state: Arc<EpollSubState>) -> Box<Self> {
        Box::new(Self {
            fd,
            epoll_state,
            sub_state,
        })
    }
}

impl InterestHandler for EpollHandler {
    /// Producer path:
    /// set pending bits, enqueue once, and wake one waiter.
    fn push_interest(&mut self, interest: InterestType) {
        EPOLL_ENQUEUE_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
        let bit = interest_to_pending_bit(interest);
        if !self.sub_state.set_pending(bit) {
            EPOLL_ENQUEUE_DEDUPE_HITS.fetch_add(1, Ordering::Relaxed);
            return;
        }

        if self.sub_state.mark_enqueued() {
            self.epoll_state
                .enqueue_ready(self.fd, self.sub_state.generation());
        } else {
            EPOLL_ENQUEUE_DEDUPE_HITS.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Clears one readiness bit from this subscription only.
    fn pop_interest(&mut self, interest: InterestType) -> bool {
        let bit = interest_to_pending_bit(interest);
        let old = self
            .sub_state
            .pending_bits
            .fetch_and(!bit, Ordering::AcqRel);
        (old & bit) != 0
    }

    /// Checks whether this subscription currently has a readiness bit set.
    fn has_interest(&self, interest: InterestType) -> bool {
        let bit = interest_to_pending_bit(interest);
        (self.sub_state.pending_bits() & bit) != 0
    }
}

/// Registers an epoll interest handler on the watched fd and returns a guard.
///
/// `None` means the fd kind does not support handler attachment for epoll.
pub(crate) fn register_epoll_handler(
    state: &Arc<WasiState>,
    event: &EpollFd,
    epoll_state: Arc<EpollState>,
    sub_state: Arc<EpollSubState>,
) -> Result<Option<EpollJoinGuard>, Errno> {
    let mut type_ = Eventtype::FdRead;
    let mut peb = PollEventBuilder::new();
    if event.events().contains(EpollType::EPOLLOUT) {
        type_ = Eventtype::FdWrite;
        peb = peb.add(PollEvent::PollOut);
    }
    if event.events().contains(EpollType::EPOLLIN) {
        type_ = Eventtype::FdRead;
        peb = peb.add(PollEvent::PollIn);
    }
    // EPOLLERR/EPOLLHUP are always delivered by epoll regardless of requested mask.
    peb = peb.add(PollEvent::PollError);
    peb = peb.add(PollEvent::PollHangUp);

    let s = Subscription {
        userdata: event.data2(),
        type_,
        data: SubscriptionUnion {
            fd_readwrite: SubscriptionFsReadwrite {
                file_descriptor: event.fd(),
            },
        },
    };

    let fd_guard = poll_fd_guard(state, peb.build(), event.fd(), s)?;
    let handler = EpollHandler::new(event.fd(), epoll_state, sub_state);

    match &fd_guard.mode {
        InodeValFilePollGuardMode::File(_) => {
            // Intentionally ignored, epoll doesn't work with files
            return Ok(None);
        }
        InodeValFilePollGuardMode::Socket { inner, .. } => {
            let mut inner = inner.protected.write().unwrap();
            inner.set_handler(handler).map_err(net_error_into_io_err)?;
            drop(inner);
        }
        InodeValFilePollGuardMode::EventNotifications(inner) => inner.set_interest_handler(handler),
        InodeValFilePollGuardMode::DuplexPipe { pipe } => {
            let inner = pipe.write().unwrap();
            inner.set_interest_handler(handler);
        }
        InodeValFilePollGuardMode::PipeRx { rx } => {
            let inner = rx.write().unwrap();
            inner.set_interest_handler(handler);
        }
        InodeValFilePollGuardMode::PipeTx { .. } => {
            // The sending end of a pipe can't have an interest handler, since we
            // only support "readable" interest on pipes; they're considered to
            // always be writable.
            return Ok(None);
        }
    }

    Ok(Some(EpollJoinGuard::new(fd_guard)))
}

/// Increments stale-generation dequeue metric.
pub(crate) fn epoll_stale_generation_drop() {
    EPOLL_STALE_GENERATION_DROPS.fetch_add(1, Ordering::Relaxed);
}

/// Increments empty dequeue metric.
pub(crate) fn epoll_empty_dequeue_entry() {
    EPOLL_EMPTY_DEQUEUE_ENTRIES.fetch_add(1, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_epoll_event_ctl(fd: WasiFd) -> EpollEventCtl {
        EpollEventCtl {
            events: EpollType::EPOLLIN,
            ptr: 77,
            fd,
            data1: 88,
            data2: 99,
        }
    }

    fn test_epoll_handler(fd: WasiFd) -> (Arc<EpollState>, Arc<EpollSubState>, Box<EpollHandler>) {
        let epoll_state = Arc::new(EpollState::new());
        let sub_state = Arc::new(EpollSubState::new(
            EpollFd::new(
                EpollType::EPOLLIN
                    | EpollType::EPOLLOUT
                    | EpollType::EPOLLERR
                    | EpollType::EPOLLHUP,
                0,
                fd,
                0,
                0,
            ),
            1,
        ));
        let handler = EpollHandler::new(fd, epoll_state.clone(), sub_state.clone());
        (epoll_state, sub_state, handler)
    }

    fn test_sub_state(fd: WasiFd, generation: u64) -> Arc<EpollSubState> {
        Arc::new(EpollSubState::new(
            EpollFd::new(
                EpollType::EPOLLIN
                    | EpollType::EPOLLOUT
                    | EpollType::EPOLLERR
                    | EpollType::EPOLLHUP,
                0,
                fd,
                0,
                0,
            ),
            generation,
        ))
    }

    #[test]
    fn epoll_fd_from_event_ctl_uses_explicit_fd() {
        let event = test_epoll_event_ctl(1234);
        let epoll_fd = EpollFd::from_event_ctl(5678, &event);
        assert_eq!(epoll_fd.fd(), 5678);
        assert_eq!(epoll_fd.ptr(), 77);
        assert_eq!(epoll_fd.data1(), 88);
        assert_eq!(epoll_fd.data2(), 99);
        assert_eq!(epoll_fd.events(), EpollType::EPOLLIN);
    }

    #[test]
    fn epoll_handler_pop_interest_is_scoped_to_fd() {
        let epoll_state = Arc::new(EpollState::new());
        let sub_state1 = Arc::new(EpollSubState::new(
            EpollFd::new(EpollType::EPOLLIN, 0, 10, 0, 0),
            1,
        ));
        let sub_state2 = Arc::new(EpollSubState::new(
            EpollFd::new(EpollType::EPOLLIN, 0, 11, 0, 0),
            1,
        ));
        let mut handler1 = EpollHandler::new(10, epoll_state.clone(), sub_state1.clone());
        let mut handler2 = EpollHandler::new(11, epoll_state.clone(), sub_state2.clone());

        handler1.push_interest(InterestType::Readable);
        handler2.push_interest(InterestType::Readable);

        assert!(handler1.has_interest(InterestType::Readable));
        assert!(handler2.has_interest(InterestType::Readable));

        assert!(handler1.pop_interest(InterestType::Readable));
        assert!(!handler1.has_interest(InterestType::Readable));
        assert!(
            handler2.has_interest(InterestType::Readable),
            "popping one fd interest must not clear another fd with the same readiness"
        );

        assert!(sub_state1.pending_bits() == 0);
        assert!(sub_state2.pending_bits() != 0);
        assert_eq!(epoll_state.ready.lock().unwrap().len(), 2);
    }

    #[test]
    fn epoll_handler_dedupes_queue_until_consumer_drains() {
        let (epoll_state, sub_state, mut handler) = test_epoll_handler(7);

        handler.push_interest(InterestType::Readable);
        handler.push_interest(InterestType::Readable);
        handler.push_interest(InterestType::Writable);

        assert_eq!(
            epoll_state.ready.lock().unwrap().len(),
            1,
            "multiple pushes while enqueued must keep a single queue entry"
        );
        assert!(handler.has_interest(InterestType::Readable));
        assert!(handler.has_interest(InterestType::Writable));

        epoll_state.ready.lock().unwrap().pop_front().unwrap();
        sub_state.take_pending_bits();
        sub_state.clear_enqueued();

        handler.push_interest(InterestType::Readable);
        assert_eq!(
            epoll_state.ready.lock().unwrap().len(),
            1,
            "after drain, a new event should enqueue again"
        );
    }

    #[test]
    fn epoll_type_to_pending_bit_has_stable_mapping() {
        assert_eq!(
            epoll_type_to_pending_bit(EpollType::EPOLLIN),
            Some(READABLE_BIT)
        );
        assert_eq!(
            epoll_type_to_pending_bit(EpollType::EPOLLOUT),
            Some(WRITABLE_BIT)
        );
        assert_eq!(
            epoll_type_to_pending_bit(EpollType::EPOLLHUP),
            Some(HUP_BIT)
        );
        assert_eq!(
            epoll_type_to_pending_bit(EpollType::EPOLLERR),
            Some(ERR_BIT)
        );
    }

    #[test]
    fn interest_to_pending_bit_has_stable_mapping() {
        assert_eq!(
            interest_to_pending_bit(InterestType::Readable),
            READABLE_BIT
        );
        assert_eq!(
            interest_to_pending_bit(InterestType::Writable),
            WRITABLE_BIT
        );
        assert_eq!(interest_to_pending_bit(InterestType::Closed), HUP_BIT);
        assert_eq!(interest_to_pending_bit(InterestType::Error), ERR_BIT);
    }

    #[test]
    fn pending_bits_to_events_always_includes_hup_and_err() {
        let events = pending_bits_to_events(HUP_BIT | ERR_BIT, EpollType::EPOLLIN);
        assert_eq!(events, vec![EpollType::EPOLLHUP, EpollType::EPOLLERR]);
    }

    #[test]
    fn epoll_mask_to_pending_bits_always_tracks_hup_and_err() {
        let bits = epoll_mask_to_pending_bits(EpollType::empty());
        assert_eq!(bits & HUP_BIT, HUP_BIT);
        assert_eq!(bits & ERR_BIT, ERR_BIT);
    }

    #[test]
    fn drain_ready_events_keeps_multi_fd_same_readiness_isolated() {
        let epoll_state = Arc::new(EpollState::new());

        let sub_a = test_sub_state(10, 1);
        let sub_b = test_sub_state(11, 1);
        let readable_bit = epoll_type_to_pending_bit(EpollType::EPOLLIN).unwrap();

        sub_a.pending_bits.store(readable_bit, Ordering::Release);
        sub_a.enqueued.store(true, Ordering::Release);
        sub_b.pending_bits.store(readable_bit, Ordering::Release);
        sub_b.enqueued.store(true, Ordering::Release);

        epoll_state.insert_subscription(10, sub_a);
        epoll_state.insert_subscription(11, sub_b);
        epoll_state.enqueue_ready(10, 1);
        epoll_state.enqueue_ready(11, 1);

        let events = drain_ready_events(&epoll_state, 8);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].0.fd(), 10);
        assert_eq!(events[1].0.fd(), 11);
        assert_eq!(events[0].1, EpollType::EPOLLIN);
        assert_eq!(events[1].1, EpollType::EPOLLIN);
    }

    #[test]
    fn drain_ready_events_requeues_undispatched_bits_when_budget_exhausted() {
        let epoll_state = Arc::new(EpollState::new());
        let sub = Arc::new(EpollSubState::new(
            EpollFd::new(EpollType::EPOLLIN | EpollType::EPOLLOUT, 0, 90, 0, 0),
            1,
        ));

        sub.pending_bits
            .store(READABLE_BIT | WRITABLE_BIT, Ordering::Release);
        sub.enqueued.store(true, Ordering::Release);
        epoll_state.insert_subscription(90, sub.clone());
        epoll_state.enqueue_ready(90, 1);

        let first = drain_ready_events(&epoll_state, 1);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].1, EpollType::EPOLLIN);
        assert_eq!(sub.pending_bits(), WRITABLE_BIT);
        assert!(sub.enqueued.load(Ordering::Acquire));
        assert_eq!(epoll_state.ready.lock().unwrap().len(), 1);

        let second = drain_ready_events(&epoll_state, 1);
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].1, EpollType::EPOLLOUT);
        assert_eq!(sub.pending_bits(), 0);
        assert!(!sub.enqueued.load(Ordering::Acquire));
        assert_eq!(epoll_state.ready.lock().unwrap().len(), 0);
    }

    #[test]
    fn drain_ready_events_drops_stale_generation_items() {
        let epoll_state = Arc::new(EpollState::new());

        let sub = test_sub_state(22, 2);
        let readable_bit = epoll_type_to_pending_bit(EpollType::EPOLLIN).unwrap();
        sub.pending_bits.store(readable_bit, Ordering::Release);
        sub.enqueued.store(true, Ordering::Release);

        epoll_state.insert_subscription(22, sub.clone());
        epoll_state.enqueue_ready(22, 1);

        let events = drain_ready_events(&epoll_state, 8);
        assert!(
            events.is_empty(),
            "stale generation items must not emit events"
        );
        assert_eq!(
            sub.pending_bits.load(Ordering::Acquire),
            readable_bit,
            "stale dequeue must not clear pending bits for current generation"
        );
    }

    #[test]
    fn repair_ready_queue_after_drain_requeues_when_new_bits_arrive() {
        let epoll_state = Arc::new(EpollState::new());
        let sub = test_sub_state(44, 3);
        let writable_bit = epoll_type_to_pending_bit(EpollType::EPOLLOUT).unwrap();
        sub.pending_bits.store(writable_bit, Ordering::Release);
        sub.enqueued.store(false, Ordering::Release);

        repair_ready_queue_after_drain(&epoll_state, 44, &sub);

        assert!(sub.enqueued.load(Ordering::Acquire));
        let queued = epoll_state.ready.lock().unwrap().pop_front().unwrap();
        assert_eq!(queued.fd, 44);
    }
}
