//! epoll, as Linux's `fs/eventpoll.c`.
//!
//! An epoll instance (`Ep`) holds items keyed by (open file, descriptor number). Each item has
//! entries on the wait queues its file's `poll` named; their callback (`ep_poll_callback`)
//! puts the item on the ready list and wakes the epoll's waiters. `epoll_wait` re-polls ready
//! items to build its events, keeping level-triggered ones on the list. An item goes away on
//! `EPOLL_CTL_DEL`, when its open file's last reference is dropped (not when one of several
//! duplicate descriptors closes), or with its epoll.

use std::{
    cell::Cell,
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex, OnceLock, Weak},
    time::Duration,
};

use wasixv2_abi::{
    ConstEpollEventPtr, ConstKernelTimespecPtr, ConstSigsetPtr, EpollCreateFlags, EpollCtlOp,
    EpollData, EpollEvent, EpollEventArray, EpollEvents, GuestLayout, OpenFlags,
};
use wasixv2_syscalls::{Caller, Errno, Fd, LocalFuture, SysErr, SysResult, read_value};
use wasixv2_wait::{
    PollMask, PollTable, Pollable, WaitEntry, WaitQueue, WaitRegistration, Waiter, Wakeup,
    poll_wait,
};

use crate::{
    Kernel,
    file::{Backing, FileObject, Io, OpenFile},
    poll::Deadline,
    sync::MutexExt,
};

/// Most epolls nested inside each other (`EP_MAX_NESTS`).
const MAX_NESTS: usize = 4;
/// `EP_MAX_EVENTS`.
const MAX_EVENTS: i32 = (i32::MAX as usize / EpollEvent::SIZE) as i32;

const PRIVATE_BITS: u32 = EpollEvents::EPOLLWAKEUP.0
    | EpollEvents::EPOLLONESHOT.0
    | EpollEvents::EPOLLET.0
    | EpollEvents::EPOLLEXCLUSIVE.0;
/// Events an `EPOLLEXCLUSIVE` item may ask for (`EPOLLEXCLUSIVE_OK_BITS`).
const EXCLUSIVE_OK_BITS: u32 = EpollEvents::EPOLLIN.0
    | EpollEvents::EPOLLOUT.0
    | EpollEvents::EPOLLERR.0
    | EpollEvents::EPOLLHUP.0
    | EpollEvents::EPOLLWAKEUP.0
    | EpollEvents::EPOLLET.0
    | EpollEvents::EPOLLEXCLUSIVE.0;

/// An item's key: the open file's address and the descriptor it was added with.
type Key = (usize, i32);

/// An event `send_events` harvested, with what undoes its harvest if it cannot be copied out.
struct Sent {
    key: Key,
    event: EpollEvent,
    /// A one-shot item's mask before the harvest disarmed it.
    oneshot: Option<u32>,
}

thread_local! {
    /// How deep this thread is in wakes and polls of epolls inside epolls.
    static NESTING: Cell<usize> = const { Cell::new(0) };
}

/// Runs `f` one epoll level deeper, or returns `fallback` past `MAX_NESTS` levels. The loop
/// check already keeps nesting within the limit; this is the backstop that keeps a bug from
/// recursing until the host stack overflows (`ep_call_nested` in older Linux).
fn nested<R>(fallback: R, f: impl FnOnce() -> R) -> R {
    struct Level;
    impl Drop for Level {
        fn drop(&mut self) {
            NESTING.with(|depth| depth.set(depth.get() - 1));
        }
    }
    if NESTING.with(|depth| depth.replace(depth.get() + 1)) > MAX_NESTS {
        drop(Level);
        return fallback;
    }
    let _level = Level;
    f()
}

/// Kept on an open file for each item watching it, so the item goes when the file does.
pub(crate) struct EpollLink {
    ep: Weak<Ep>,
    fd: i32,
}

/// The open-file object of an epoll descriptor.
pub(crate) struct Epoll {
    ep: Arc<Ep>,
}

pub(crate) struct Ep {
    state: Mutex<State>,
    /// fs/eventpoll.c:ep->mtx serializes harvests and control changes while
    /// callbacks may still queue readiness under the separate state lock.
    scan: Mutex<()>,
    /// `epoll_wait` callers sleep here (`ep->wq`).
    wq: WaitQueue,
    /// Pollers of the epoll descriptor itself sleep here (`ep->poll_wait`).
    poll_wait: WaitQueue,
    /// The epoll's own open file, whose links name the epolls watching this one.
    file: OnceLock<Weak<OpenFile>>,
}

#[derive(Default)]
struct State {
    items: HashMap<Key, Arc<Item>>,
    /// `rdllist`.
    ready: VecDeque<Key>,
    queued: HashSet<Key>,
}

struct Item {
    key: Key,
    file: Weak<OpenFile>,
    ep: Weak<Ep>,
    event: Mutex<(u32, EpollData)>,
    registrations: Mutex<Vec<WaitRegistration>>,
}

impl Item {
    fn events(&self) -> u32 {
        self.event.lock_or_recover().0
    }
}

/// The item's entry on a file's wait queue: `ep_poll_callback`.
struct ItemEntry {
    item: Weak<Item>,
    exclusive: bool,
}

impl WaitEntry for ItemEntry {
    fn wake(&self, flags: PollMask) -> bool {
        let Some(item) = self.item.upgrade() else {
            return false;
        };
        let Some(ep) = item.ep.upgrade() else {
            return false;
        };
        let events = item.events();
        // A disarmed one-shot item, or events the item does not watch.
        let wanted = events & !PRIVATE_BITS;
        if wanted == 0 || (!flags.is_empty() && flags.0 & wanted == 0) {
            return !self.exclusive;
        }
        let sleepers = ep.wq.is_active();
        ep.queue_ready(&[item.key]);
        nested((), || ep.wake());
        if !self.exclusive {
            return true;
        }
        // An exclusive item takes the wakeup only if somebody waits and the event is one of
        // its own, so other exclusive epolls get the rest.
        let inout = flags.0 & (EpollEvents::EPOLLIN.0 | EpollEvents::EPOLLOUT.0);
        sleepers
            && match inout {
                0 => true,
                bit if bit == EpollEvents::EPOLLIN.0 || bit == EpollEvents::EPOLLOUT.0 => {
                    events & bit != 0
                }
                _ => false,
            }
    }

    fn exclusive(&self) -> bool {
        self.exclusive
    }
}

impl Ep {
    async fn prepare_poll(&self) {
        let items: Vec<_> = self
            .state
            .lock_or_recover()
            .items
            .values()
            .cloned()
            .collect();
        for item in items {
            if let Some(file) = item.file.upgrade() {
                file.prepare_poll(item.events() & !PRIVATE_BITS).await;
            }
        }
    }

    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::default(),
            scan: Mutex::new(()),
            wq: WaitQueue::new(),
            poll_wait: WaitQueue::new(),
            file: OnceLock::new(),
        })
    }

    fn find(&self, key: Key) -> Option<Arc<Item>> {
        self.state.lock_or_recover().items.get(&key).cloned()
    }

    /// `ep_events_available`.
    fn has_ready(&self) -> bool {
        !self.state.lock_or_recover().ready.is_empty()
    }

    /// Appends items to the ready list unless already on it.
    fn queue_ready(&self, keys: &[Key]) {
        let mut state = self.state.lock_or_recover();
        for &key in keys {
            if state.items.contains_key(&key) && state.queued.insert(key) {
                state.ready.push_back(key);
            }
        }
    }

    fn wake(&self) {
        self.wq.wake(PollMask::empty());
        self.poll_wait.wake(PollMask::READABLE);
    }

    /// `ep_item_poll`: the item's file's readiness, filtered by what the item watches.
    fn item_poll(item: &Arc<Item>, register: bool) -> PollMask {
        let Some(file) = item.file.upgrade() else {
            return PollMask::empty();
        };
        let events = item.events();
        let mask = if register {
            let mut registrations = Vec::new();
            let exclusive = events & EpollEvents::EPOLLEXCLUSIVE.0 != 0;
            let mut queue_proc = |queue: &WaitQueue, _key: PollMask| {
                let entry = ItemEntry {
                    item: Arc::downgrade(item),
                    exclusive,
                };
                registrations.push(queue.add(Arc::new(entry)));
            };
            let mask = file.poll(Some(&mut PollTable::new(PollMask(events), &mut queue_proc)));
            item.registrations.lock_or_recover().extend(registrations);
            mask
        } else {
            file.poll(None)
        };
        mask & PollMask(events)
    }

    /// `ep_insert`.
    fn insert(self: &Arc<Self>, key: Key, file: &Arc<OpenFile>, event: (u32, EpollData)) {
        let _scan = self.scan.lock_or_recover();
        let item = Arc::new(Item {
            key,
            file: Arc::downgrade(file),
            ep: Arc::downgrade(self),
            event: Mutex::new(event),
            registrations: Mutex::default(),
        });
        self.state.lock_or_recover().items.insert(key, item.clone());
        file.epoll_links.lock_or_recover().push(EpollLink {
            ep: Arc::downgrade(self),
            fd: key.1,
        });
        if !Self::item_poll(&item, true).is_empty() {
            self.queue_ready(&[key]);
            self.wake();
        }
    }

    /// `ep_modify`.
    fn modify(&self, item: &Arc<Item>, event: (u32, EpollData)) {
        let _scan = self.scan.lock_or_recover();
        *item.event.lock_or_recover() = event;
        if !Self::item_poll(item, false).is_empty() {
            self.queue_ready(&[item.key]);
            self.wake();
        }
    }

    /// Drops an item without touching its file, which is gone or being dropped.
    fn forget(&self, key: Key) -> Option<Arc<Item>> {
        let item = {
            let mut state = self.state.lock_or_recover();
            if state.queued.remove(&key) {
                state.ready.retain(|k| *k != key);
            }
            state.items.remove(&key)
        }?;
        item.registrations.lock_or_recover().clear();
        Some(item)
    }

    /// `ep_remove`: also unlinks the item from its file.
    fn remove(self: &Arc<Self>, key: Key) -> bool {
        let _scan = self.scan.lock_or_recover();
        let Some(item) = self.forget(key) else {
            return false;
        };
        if let Some(file) = item.file.upgrade() {
            let me = Arc::as_ptr(self);
            file.epoll_links
                .lock_or_recover()
                .retain(|link| !(link.ep.as_ptr() == me && link.fd == key.1));
        }
        true
    }

    /// `ep_send_events`: up to `max` events from the ready list, re-polling each item. One-shot
    /// items are disarmed and edge-triggered ones leave the list; [`Ep::unsend`] undoes that
    /// when the events cannot be copied out.
    fn send_events(&self, max: usize) -> Vec<Sent> {
        let _scan = self.scan.lock_or_recover();
        // ep_start_scan splices the ready list, without copying all its items.
        // Unvisited keys retain membership, so callbacks cannot duplicate them.
        let mut txlist = std::mem::take(&mut self.state.lock_or_recover().ready);
        let mut events = Vec::new();
        let mut requeue = Vec::new();
        while events.len() < max {
            let Some(key) = txlist.pop_front() else { break };
            let item = {
                let mut state = self.state.lock_or_recover();
                if !state.queued.remove(&key) {
                    continue;
                }
                state.items.get(&key).cloned()
            };
            let Some(item) = item else { continue };
            let revents = Self::item_poll(&item, false);
            if revents.is_empty() {
                continue;
            }
            let mut event = item.event.lock_or_recover();
            let mut sent = Sent {
                key,
                event: EpollEvent {
                    events: revents.into(),
                    data: event.1,
                },
                oneshot: None,
            };
            if event.0 & EpollEvents::EPOLLONESHOT.0 != 0 {
                sent.oneshot = Some(event.0);
                event.0 &= PRIVATE_BITS;
            } else if event.0 & EpollEvents::EPOLLET.0 == 0 {
                requeue.push(key);
            }
            events.push(sent);
        }
        // `ep_done_scan`: unprocessed items go back to the front, level-triggered ones after
        // them, then whatever arrived meanwhile.
        let mut state = self.state.lock_or_recover();
        for key in requeue {
            if state.items.contains_key(&key) && state.queued.insert(key) {
                txlist.push_back(key);
            }
        }
        txlist.append(&mut state.ready);
        state.ready = txlist;
        let more = !state.ready.is_empty();
        drop(state);
        if more {
            self.wq.wake(PollMask::empty());
        }
        events
    }

    /// Puts harvested items back at the front of the ready list, re-arming one-shot ones that
    /// nobody re-armed meanwhile: their events faulted on the way out, and `ep_deliver_event`
    /// keeps an item whose `epoll_put_uevent` failed for the next wait.
    fn unsend(&self, sent: &[Sent]) {
        for sent in sent {
            let (Some(mask), Some(item)) = (sent.oneshot, self.find(sent.key)) else {
                continue;
            };
            let mut event = item.event.lock_or_recover();
            if event.0 == mask & PRIVATE_BITS {
                event.0 = mask;
            }
        }
        let mut state = self.state.lock_or_recover();
        for sent in sent.iter().rev() {
            if !state.items.contains_key(&sent.key) {
                continue;
            }
            if !state.queued.insert(sent.key) {
                state.ready.retain(|key| *key != sent.key);
            }
            state.ready.push_front(sent.key);
        }
        drop(state);
        self.wake();
    }

    /// The epolls whose items watch this one.
    fn parents(&self) -> Vec<Arc<Ep>> {
        let Some(file) = self.file.get().and_then(Weak::upgrade) else {
            return Vec::new();
        };
        let links = file.epoll_links.lock_or_recover();
        links.iter().filter_map(|link| link.ep.upgrade()).collect()
    }

    /// The epolls this one's items watch.
    fn children(&self) -> Vec<Arc<Ep>> {
        let items: Vec<_> = self
            .state
            .lock_or_recover()
            .items
            .values()
            .cloned()
            .collect();
        items
            .iter()
            .filter_map(|item| item.file.upgrade())
            .filter_map(|file| file.downcast::<Epoll>().map(|epoll| epoll.ep.clone()))
            .collect()
    }
}

/// `ep_loop_check_proc`: how deep epolls nest below `ep`, or `None` for a loop back to
/// `inserting_into` or nesting past the limit.
fn depth_below(ep: &Ep, inserting_into: &Arc<Ep>, depth: usize) -> Option<usize> {
    let mut result = 0;
    for child in ep.children() {
        if Arc::ptr_eq(&child, inserting_into) || depth > MAX_NESTS {
            return None;
        }
        result = result.max(depth_below(&child, inserting_into, depth + 1)? + 1);
        if result > MAX_NESTS {
            return None;
        }
    }
    Some(result)
}

/// `ep_get_upwards_depth_proc`.
fn depth_above(ep: &Ep) -> usize {
    ep.parents()
        .iter()
        .map(|parent| depth_above(parent) + 1)
        .max()
        .unwrap_or(0)
}

/// `ep_loop_check`: whether `to` may go inside `ep`.
fn loop_check(ep: &Arc<Ep>, to: &Ep) -> bool {
    match depth_below(to, ep, 0) {
        Some(depth) => depth + 1 + depth_above(ep) <= MAX_NESTS,
        None => false,
    }
}

/// `eventpoll_release`: `file`'s last reference is gone.
pub(crate) fn release(file: &OpenFile, links: Vec<EpollLink>) {
    let address = file as *const OpenFile as usize;
    for link in links {
        if let Some(ep) = link.ep.upgrade() {
            ep.forget((address, link.fd));
        }
    }
}

impl Drop for Epoll {
    /// `ep_clear_and_put`: removes every item.
    fn drop(&mut self) {
        let keys: Vec<Key> = self
            .ep
            .state
            .lock_or_recover()
            .items
            .keys()
            .copied()
            .collect();
        for key in keys {
            self.ep.remove(key);
        }
    }
}

impl Pollable for Epoll {
    /// `ep_eventpoll_poll`: readable while a ready item really is ready. Items found not
    /// ready leave the ready list.
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, &self.ep.poll_wait);
        nested(PollMask::empty(), || self.ready_mask())
    }
}

impl Epoll {
    /// `get_epoll_tfile_raw_ptr` (`ep_find_tfd`): the file of the `toff`-th item added with
    /// descriptor `tfd`, items in the order of their keys.
    pub(crate) fn target_file(&self, tfd: i32, toff: u32) -> Option<Arc<OpenFile>> {
        let state = self.ep.state.lock_or_recover();
        let mut keys: Vec<Key> = state.items.keys().copied().filter(|k| k.1 == tfd).collect();
        keys.sort_unstable();
        let key = keys.get(usize::try_from(toff).ok()?)?;
        state.items[key].file.upgrade()
    }

    /// The readiness part of `ep_eventpoll_poll`.
    fn ready_mask(&self) -> PollMask {
        let ready: Vec<Key> = self
            .ep
            .state
            .lock_or_recover()
            .ready
            .iter()
            .copied()
            .collect();
        let mut stale = Vec::new();
        let mut mask = PollMask::empty();
        for key in ready {
            let Some(item) = self.ep.find(key) else {
                continue;
            };
            if !Ep::item_poll(&item, false).is_empty() {
                mask = PollMask::READABLE;
                break;
            }
            stale.push(key);
        }
        let mut state = self.ep.state.lock_or_recover();
        for key in stale {
            if state.queued.remove(&key) {
                state.ready.retain(|k| *k != key);
            }
        }
        mask
    }
}

impl FileObject for Epoll {
    fn prepare_poll(&self, _: u32) -> LocalFuture<'_, ()> {
        Box::pin(self.ep.prepare_poll())
    }

    // An epoll descriptor has no read or write operation.
    fn read<'a>(&'a self, _: Io<'a>, _: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async { Err(Errno::EINVAL.into()) })
    }

    fn write<'a>(&'a self, _: Io<'a>, _: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async { Err(Errno::EINVAL.into()) })
    }
}

impl Kernel {
    /// `do_epoll_create`.
    pub(crate) async fn sys_epoll_create1(
        &self,
        caller: &Caller<'_>,
        flags: EpollCreateFlags,
    ) -> SysResult<Fd> {
        if !(flags & !EpollCreateFlags::EPOLL_CLOEXEC).is_empty() {
            return Err(Errno::EINVAL.into());
        }
        let ep = Ep::new();
        let object = Arc::new(Epoll { ep: ep.clone() });
        let file = Arc::new(OpenFile::new(
            Backing::Stream(object, self.pseudo.anon("[eventpoll]")),
            OpenFlags::O_RDWR,
        ));
        ep.file.set(Arc::downgrade(&file)).ok();
        let cloexec = flags.contains(EpollCreateFlags::EPOLL_CLOEXEC);
        self.install(caller, file, cloexec)
    }

    /// `epoll_ctl` and `do_epoll_ctl`, with Linux's error order.
    pub(crate) async fn sys_epoll_ctl(
        &self,
        caller: &Caller<'_>,
        epfd: Fd,
        op: EpollCtlOp,
        fd: Fd,
        event: ConstEpollEventPtr,
    ) -> SysResult<i32> {
        let has_event = op != EpollCtlOp::EPOLL_CTL_DEL;
        let event = match has_event {
            true => Some(read_value(caller.memory(), event).await?),
            false => None,
        };
        let epfile = self.io_file(caller, epfd)?;
        self.epoll_ctl_file(caller, epfile, op, fd, event).await
    }

    pub(crate) async fn epoll_ctl_file(
        &self,
        caller: &Caller<'_>,
        epfile: Arc<OpenFile>,
        op: EpollCtlOp,
        fd: Fd,
        mut event: Option<EpollEvent>,
    ) -> SysResult<i32> {
        // fs/eventpoll.c do_epoll_ctl uses fdget for both descriptors.
        let target = self.io_file(caller, fd)?;
        if !target.can_poll() {
            return Err(Errno::EPERM.into());
        }
        // EPOLLWAKEUP needs CAP_BLOCK_SUSPEND over the initial user namespace; without it Linux
        // drops the flag silently (`ep_take_care_of_epollwakeup`).
        if let Some(event) = &mut event
            && !self
                .cred(caller)
                .capable_init(wasixv2_vfs::Cap::BlockSuspend)
        {
            event.events &= !EpollEvents::EPOLLWAKEUP;
        }
        let ep = match epfile.downcast::<Epoll>() {
            Some(epoll) if !Arc::ptr_eq(&epfile, &target) => epoll.ep.clone(),
            _ => return Err(Errno::EINVAL.into()),
        };
        let target_ep = target.downcast::<Epoll>().map(|epoll| epoll.ep.clone());
        if let Some(event) = &event
            && event.events.contains(EpollEvents::EPOLLEXCLUSIVE)
        {
            if op == EpollCtlOp::EPOLL_CTL_MOD {
                return Err(Errno::EINVAL.into());
            }
            if op == EpollCtlOp::EPOLL_CTL_ADD
                && (target_ep.is_some() || event.events.0 & !EXCLUSIVE_OK_BITS != 0)
            {
                return Err(Errno::EINVAL.into());
            }
        }
        // `epnested_mutex`: the loop check and the insert it allows are one step, or two
        // concurrent ADDs could each pass and together build a cycle.
        let _nesting = (op == EpollCtlOp::EPOLL_CTL_ADD && target_ep.is_some())
            .then(|| self.epoll_nesting.lock_or_recover());
        if op == EpollCtlOp::EPOLL_CTL_ADD
            && let Some(to) = &target_ep
            && !loop_check(&ep, to)
        {
            return Err(Errno::ELOOP.into());
        }

        let key = (Arc::as_ptr(&target) as usize, fd.0);
        let always = EpollEvents::EPOLLERR | EpollEvents::EPOLLHUP;
        let found = ep.find(key);
        let result = match (op, found, event) {
            (EpollCtlOp::EPOLL_CTL_ADD, Some(_), _) => Err(Errno::EEXIST.into()),
            (EpollCtlOp::EPOLL_CTL_ADD, None, Some(event)) => {
                ep.insert(key, &target, ((event.events | always).0, event.data));
                Ok(0)
            }
            (EpollCtlOp::EPOLL_CTL_DEL, Some(_), _) => {
                ep.remove(key);
                Ok(0)
            }
            (EpollCtlOp::EPOLL_CTL_MOD, Some(item), Some(event)) => {
                if item.events() & EpollEvents::EPOLLEXCLUSIVE.0 != 0 {
                    return Err(Errno::EINVAL.into());
                }
                ep.modify(&item, ((event.events | always).0, event.data));
                Ok(0)
            }
            (EpollCtlOp::EPOLL_CTL_DEL | EpollCtlOp::EPOLL_CTL_MOD, None, _) => {
                Err(Errno::ENOENT.into())
            }
            _ => Err(Errno::EINVAL.into()),
        };
        drop(_nesting);
        if result.is_ok()
            && op != EpollCtlOp::EPOLL_CTL_DEL
            && let Some(item) = ep.find(key)
        {
            target.prepare_poll(item.events() & !PRIVATE_BITS).await;
            if !Ep::item_poll(&item, false).is_empty() {
                ep.queue_ready(&[key]);
                ep.wake();
            }
        }
        result
    }

    /// `epoll_pwait`: a timeout in milliseconds, negative for none.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn sys_epoll_pwait(
        &self,
        caller: &Caller<'_>,
        epfd: Fd,
        events: EpollEventArray,
        maxevents: i32,
        timeout: i32,
        sigmask: ConstSigsetPtr,
        sigsetsize: u32,
    ) -> SysResult<i32> {
        let deadline = match timeout {
            ..0 => Deadline::Never,
            0 => Deadline::Now,
            ms => Deadline::At(
                self.host
                    .monotonic()
                    .saturating_add(Duration::from_millis(ms as u64)),
            ),
        };
        self.epoll_pwait_until(
            caller, epfd, events, maxevents, deadline, sigmask, sigsetsize,
        )
        .await
    }

    /// `epoll_pwait2`: a timespec timeout, NULL for none.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn sys_epoll_pwait2(
        &self,
        caller: &Caller<'_>,
        epfd: Fd,
        events: EpollEventArray,
        maxevents: i32,
        timeout: ConstKernelTimespecPtr,
        sigmask: ConstSigsetPtr,
        sigsetsize: u32,
    ) -> SysResult<i32> {
        let deadline = match timeout.is_null() {
            true => Deadline::Never,
            false => self.deadline(read_value(caller.memory(), timeout).await?)?,
        };
        self.epoll_pwait_until(
            caller, epfd, events, maxevents, deadline, sigmask, sigsetsize,
        )
        .await
    }

    /// `do_epoll_pwait`.
    #[allow(clippy::too_many_arguments)]
    async fn epoll_pwait_until(
        &self,
        caller: &Caller<'_>,
        epfd: Fd,
        events: EpollEventArray,
        maxevents: i32,
        deadline: Deadline,
        sigmask: ConstSigsetPtr,
        sigsetsize: u32,
    ) -> SysResult<i32> {
        let saved = self.set_user_sigmask(caller, sigmask, sigsetsize).await?;
        let ret = self
            .do_epoll_wait(caller, epfd, events, maxevents, deadline)
            .await;
        self.restore_sigmask(
            caller,
            saved,
            matches!(ret, Err(SysErr::Errno(Errno::EINTR))),
        );
        ret
    }

    /// `do_epoll_wait` and `ep_poll`.
    async fn do_epoll_wait(
        &self,
        caller: &Caller<'_>,
        epfd: Fd,
        out: EpollEventArray,
        maxevents: i32,
        deadline: Deadline,
    ) -> SysResult<i32> {
        let file = self.file(caller.task(), epfd)?;
        if maxevents <= 0 || maxevents > MAX_EVENTS {
            return Err(Errno::EINVAL.into());
        }
        // `access_ok`: the whole array must lie in the address space.
        let end = u64::from(out.addr()) + maxevents as u64 * EpollEvent::SIZE as u64;
        if end > 1 << 32 {
            return Err(Errno::EFAULT.into());
        }
        let ep = match file.downcast::<Epoll>() {
            Some(epoll) => epoll.ep.clone(),
            None => return Err(Errno::EINVAL.into()),
        };
        drop(file);

        let interrupt = self.interrupt(caller);
        let waiter = Waiter::new();
        let mut timer = self.timer(caller, deadline);
        let mut timed_out = deadline == Deadline::Now;
        let events = loop {
            ep.prepare_poll().await;
            let events = ep.send_events(maxevents as usize);
            if !events.is_empty() || timed_out {
                break events;
            }
            // epoll_wait is never restarted.
            if wasixv2_wait::Interrupt::pending(&*interrupt) {
                return Err(Errno::EINTR.into());
            }
            // On `ep->wq` only while asleep, as `ep_autoremove_wake_function` takes a woken
            // waiter off: wakes that come while this thread harvests go to other waiters.
            let _registration = waiter.register_exclusive(&ep.wq, PollMask::empty());
            if ep.has_ready() {
                continue;
            }
            let timer = timer.as_mut().map(|timer| timer.as_mut());
            match waiter.sleep(&*interrupt, timer).await {
                Wakeup::Woken(_) => {}
                Wakeup::TimedOut => timed_out = true,
                Wakeup::Interrupted => return Err(SysErr::Errno(Errno::EINTR)),
            }
        };
        copy_events(caller, &ep, events, out).await
    }

    /// fs/eventpoll.c:epoll_sendevents, used by io_uring/epoll.c:io_epoll_wait.
    pub(crate) async fn epoll_sendevents(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        out: EpollEventArray,
        maxevents: i32,
    ) -> SysResult<i32> {
        if maxevents <= 0 || maxevents > MAX_EVENTS {
            return Err(Errno::EINVAL.into());
        }
        if u64::from(out.addr()) + maxevents as u64 * EpollEvent::SIZE as u64 > 1 << 32 {
            return Err(Errno::EFAULT.into());
        }
        let ep = &file.downcast::<Epoll>().ok_or(Errno::EINVAL)?.ep;
        copy_events(caller, ep, ep.send_events(maxevents as usize), out).await
    }
}

async fn copy_events(
    caller: &Caller<'_>,
    ep: &Ep,
    events: Vec<Sent>,
    out: EpollEventArray,
) -> SysResult<i32> {
    // One copy of all events; if it faults, none is delivered and all go back.
    let mut raw = vec![0; events.len() * EpollEvent::SIZE];
    for (sent, slot) in events.iter().zip(raw.chunks_exact_mut(EpollEvent::SIZE)) {
        sent.event.write_to(slot);
    }
    if caller.memory().write(out.addr(), &raw).await.is_err() {
        ep.unsend(&events);
        return Err(Errno::EFAULT.into());
    }
    Ok(events.len() as i32)
}
