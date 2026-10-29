//! Linux native asynchronous I/O (`fs/aio.c`): `io_setup`, `io_destroy`, `io_submit`,
//! `io_cancel` and `io_pgetevents`.
//!
//! A context (`kioctx`) belongs to an address space (`mm->ioctx_table`) and owns a completion
//! ring: an `aio_ring` header followed by `io_event`s, in shared pages of the host's
//! ([`Host::shared_pages`]) that `io_setup` maps into the caller's memory, as `aio_setup_ring`
//! maps them with `do_mmap`; the context's id is the ring's address. The kernel writes events
//! and the tail through the pages from any thread, and the guest sees them at once, so libaio's
//! look at the ring finds them; `io_getevents` moves the head, which userspace may move too.
//!
//! As on Linux for files without `O_DIRECT`, reads, writes and syncs run to completion inside
//! `io_submit` and post their event there. `IOCB_CMD_POLL` waits: its entry sits on the file's
//! wait queues and a wakeup has the kernel's timer driver poll the file again and complete it
//! (`aio_poll_wake` and `aio_poll_complete_work`, which runs from a workqueue), with nothing
//! locked, so the completion may signal an eventfd whose queue woke it. A poll is the only
//! request `io_cancel` cancels.
//!
//! A fork child's memory still shows its parent's rings, which Linux leaves out of the child
//! (`VM_DONTCOPY`); the child has no contexts of its own, so every call on one of them fails.
//!
//! [`Host::shared_pages`]: wasixv2_host::Host::shared_pages

use std::{
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use wasixv2_abi::{
    AioContext, AioContextPtr, AioRing, AioRingMisc, AioSigset, ConstAioSigsetPtr,
    ConstIocbPtrArray, GuestLayout, GuestPtr, GuestPtrMut, IoEvent, IoEventArray, IoEventPtr, Iocb,
    IocbCmd, IocbFlags, IocbPtr, Iovec, KernelTimespec, KernelTimespecPtr, RwfFlags, Slong,
};
use wasixv2_pages::SharedPages;
use wasixv2_syscalls::{Caller, Errno, Fd, SysErr, SysResult, read_value, write_value};
use wasixv2_vfs::Cap;
use wasixv2_wait::{
    NoInterrupt, PollMask, PollTable, Pollable, WaitEntry, WaitQueue, WaitRegistration, wait_event,
    wait_event_timeout,
};

use crate::{
    Kernel,
    eventfd::EventFd,
    file::{Backing, OpenFile},
    fs::io::{At, read_iovecs},
    mm::PAGE_SIZE,
    sync::MutexExt,
    timer::{Timer, Timers},
};

/// `KIOCB_KEY`, what `io_submit` writes to `aio_key` and `io_cancel` expects there.
const KIOCB_KEY: u32 = 0;
/// `sizeof(struct aio_ring)`: the events start right after it.
const RING_HEADER: u64 = AioRing::SIZE as u64;
const EVENT_SIZE: u64 = IoEvent::SIZE as u64;
/// Offsets of the header's words the kernel reads and writes (`struct aio_ring`).
const RING_ID: u64 = 0;
const RING_HEAD: u64 = 8;
const RING_TAIL: u64 = 12;

/// `fs.aio-nr` and `fs.aio-max-nr`: the events every context of the machine asked for, and
/// the most they may ask for together.
pub(crate) struct AioSysctl {
    nr: Mutex<u64>,
    max: AtomicU64,
}

impl Default for AioSysctl {
    /// `aio_max_nr`'s default, `0x10000`.
    fn default() -> Self {
        Self {
            nr: Mutex::new(0),
            max: AtomicU64::new(0x10000),
        }
    }
}

impl AioSysctl {
    pub(crate) fn nr(&self) -> u64 {
        *self.nr.lock_or_recover()
    }

    pub(crate) fn max(&self) -> u64 {
        self.max.load(Ordering::Relaxed)
    }

    /// `proc_doulongvec_minmax`'s write, of a 32-bit `unsigned long`.
    pub(crate) fn set_max(&self, max: u32) {
        self.max.store(max.into(), Ordering::Relaxed);
    }

    /// Counts `reqs` more requests, unless that passes the limit (`EAGAIN`).
    fn take(&self, reqs: u64) -> Result<(), Errno> {
        let mut nr = self.nr.lock_or_recover();
        match nr.checked_add(reqs) {
            Some(total) if total <= self.max() => {
                *nr = total;
                Ok(())
            }
            _ => Err(Errno::EAGAIN),
        }
    }

    /// `aio_nr_sub`.
    fn give_back(&self, reqs: u64) {
        let mut nr = self.nr.lock_or_recover();
        *nr = nr.saturating_sub(reqs);
    }
}

/// An address space's contexts (`mm->ioctx_table`), by the index their ring's `id` holds.
#[derive(Default)]
pub(crate) struct IoctxTable(Mutex<Vec<Option<Arc<Kioctx>>>>);

impl IoctxTable {
    /// `ioctx_add_table`: the first free slot, whose index goes in the ring's `id`.
    fn add(&self, ctx: Arc<Kioctx>) -> Result<(), Errno> {
        let mut table = self.0.lock_or_recover();
        let index = match table.iter().position(Option::is_none) {
            Some(index) => index,
            None => {
                table.push(None);
                table.len() - 1
            }
        };
        ctx.write_word(RING_ID, index as u32)?;
        table[index] = Some(ctx);
        Ok(())
    }

    /// `lookup_ioctx`: the context whose ring is at `ctx_id`, found by the index its ring's
    /// `id` holds, as Linux reads it from userspace.
    fn lookup(&self, ctx_id: AioContext) -> Option<Arc<Kioctx>> {
        let table = self.0.lock_or_recover();
        table
            .iter()
            .flatten()
            .find(|ctx| ctx.user_id == ctx_id)
            .filter(|ctx| {
                let id = ctx.read_word(RING_ID).ok();
                id.and_then(|id| table.get(id as usize))
                    .and_then(Option::as_ref)
                    .is_some_and(|found| Arc::ptr_eq(found, ctx))
            })
            .cloned()
    }

    /// Takes `ctx` out of the table; false if another `io_destroy` took it first.
    fn remove(&self, ctx: &Arc<Kioctx>) -> bool {
        let mut table = self.0.lock_or_recover();
        let slot = table
            .iter_mut()
            .find(|slot| slot.as_ref().is_some_and(|c| Arc::ptr_eq(c, ctx)));
        slot.is_some_and(|slot| slot.take().is_some())
    }

    fn take_all(&self) -> Vec<Arc<Kioctx>> {
        let mut table = self.0.lock_or_recover();
        table.drain(..).flatten().collect()
    }
}

/// A context (`struct kioctx`).
pub(crate) struct Kioctx {
    /// The ring's address in the memory, which names the context (`user_id`).
    user_id: AioContext,
    /// Bytes of the ring's mapping.
    mmap_size: u64,
    pages: Arc<dyn SharedPages>,
    /// Events the ring holds (`nr_events`).
    nr_events: u32,
    /// What `io_setup` asked for, as `aio-nr` counts it.
    max_reqs: u32,
    sysctl: Arc<AioSysctl>,
    state: Mutex<CtxState>,
    /// Held while events are taken off the ring (`ring_lock`).
    ring_lock: futures_util::lock::Mutex<()>,
    /// Woken by every completion and by the context's end (`ctx->wait`).
    wait: WaitQueue,
    /// Fires at once when armed, to finish the polls a wakeup scheduled.
    timer: Timer,
}

#[derive(Default)]
struct CtxState {
    /// Where the next event goes (`ctx->tail`).
    tail: u32,
    /// Requests submitted and not yet completed.
    inflight: u32,
    /// The polls that wait (`active_reqs`), which `io_cancel` finds.
    active: Vec<Arc<PollReq>>,
    /// The polls a wakeup scheduled, for the timer to look at.
    scheduled: Vec<Arc<PollReq>>,
    /// `io_destroy` or the end of the address space killed the context.
    dead: bool,
}

impl Kioctx {
    fn read_word(&self, offset: u64) -> Result<u32, Errno> {
        let mut word = [0; 4];
        self.pages
            .read_at(&mut word, offset)
            .map_err(|_| Errno::EFAULT)?;
        Ok(u32::from_le_bytes(word))
    }

    fn write_word(&self, offset: u64, value: u32) -> Result<(), Errno> {
        self.pages
            .write_at(&value.to_le_bytes(), offset)
            .map_err(|_| Errno::EFAULT)
    }

    fn is_dead(&self) -> bool {
        self.state.lock_or_recover().dead
    }

    /// Events on the ring that nobody took yet, by the head userspace may have moved.
    fn unreaped(&self, tail: u32) -> u32 {
        let head = self.read_word(RING_HEAD).unwrap_or(tail) % self.nr_events;
        (tail + self.nr_events - head) % self.nr_events
    }

    /// `aio_get_req`'s `get_reqs_available`: a slot for a request, as long as the ring has
    /// room for every event in flight (`nr_events - 1` of them); `EAGAIN` otherwise.
    fn get_req(&self) -> Result<(), Errno> {
        let mut state = self.state.lock_or_recover();
        if state.inflight + self.unreaped(state.tail) >= self.nr_events - 1 {
            return Err(Errno::EAGAIN);
        }
        state.inflight += 1;
        Ok(())
    }

    /// `put_reqs_available`: a request that failed to submit posts no event.
    fn put_req(&self) {
        let mut state = self.state.lock_or_recover();
        state.inflight -= 1;
    }

    /// `aio_complete`: puts `event` at the tail of the ring, then signals the request's
    /// eventfd and wakes `io_getevents`.
    fn complete(&self, event: IoEvent, eventfd: Option<&OpenFile>) {
        {
            let mut state = self.state.lock_or_recover();
            let tail = state.tail;
            let mut bytes = [0; IoEvent::SIZE];
            event.write_to(&mut bytes);
            let at = RING_HEADER + u64::from(tail) * EVENT_SIZE;
            let _ = self.pages.write_at(&bytes, at);
            state.tail = (tail + 1) % self.nr_events;
            let _ = self.write_word(RING_TAIL, state.tail);
            state.inflight -= 1;
        }
        if let Some(eventfd) = eventfd.and_then(|file| file.downcast::<EventFd>()) {
            eventfd.signal();
        }
        self.wait.wake(PollMask::READABLE);
    }

    /// Whether events wait on the ring.
    fn has_events(&self) -> bool {
        let tail = self.read_word(RING_TAIL).unwrap_or(0);
        let head = self.read_word(RING_HEAD).unwrap_or(tail);
        head % self.nr_events != tail % self.nr_events
    }

    /// `aio_read_events_ring`: copies up to `nr` events from the head of the ring to `events`
    /// and moves the head past them.
    async fn read_events_ring(
        &self,
        caller: &Caller<'_>,
        events: u32,
        nr: u32,
    ) -> Result<u32, Errno> {
        let _ring = self.ring_lock.lock().await;
        let head = self.read_word(RING_HEAD)?;
        let tail = self.read_word(RING_TAIL)?;
        if head == tail {
            return Ok(0);
        }
        let (mut head, tail) = (head % self.nr_events, tail % self.nr_events);
        let mut got = 0;
        while got < nr && head != tail {
            let end = if head <= tail { tail } else { self.nr_events };
            let avail = (end - head).min(nr - got);
            let mut bytes = vec![0; avail as usize * IoEvent::SIZE];
            let at = RING_HEADER + u64::from(head) * EVENT_SIZE;
            self.pages
                .read_at(&mut bytes, at)
                .map_err(|_| Errno::EFAULT)?;
            let to = events + got * IoEvent::SIZE as u32;
            caller
                .memory()
                .write(to, &bytes)
                .await
                .map_err(|_| Errno::EFAULT)?;
            got += avail;
            head = (head + avail) % self.nr_events;
        }
        self.write_word(RING_HEAD, head)?;
        Ok(got)
    }

    /// Schedules the timer to look at `req` again (`schedule_work` of the poll's work).
    fn schedule(&self, req: Arc<PollReq>) {
        let first = {
            let mut state = self.state.lock_or_recover();
            state.scheduled.push(req);
            state.scheduled.len() == 1
        };
        if first {
            self.timer.start(Duration::ZERO);
        }
    }

    /// `aio_poll_complete_work` for every scheduled poll, from the timer driver: one whose
    /// file is ready, or that was cancelled, completes; the others wait on.
    fn run_scheduled(&self) {
        let scheduled = std::mem::take(&mut self.state.lock_or_recover().scheduled);
        for req in scheduled {
            let cancelled = {
                let mut poll = req.state.lock_or_recover();
                if poll.done {
                    continue;
                }
                // A wakeup from here on schedules the poll again.
                poll.scheduled = false;
                poll.cancelled
            };
            let mask = match cancelled {
                true => PollMask::empty(),
                false => req.file.poll(None) & req.events,
            };
            if mask.is_empty() && !cancelled {
                continue;
            }
            {
                let mut poll = req.state.lock_or_recover();
                if std::mem::replace(&mut poll.done, true) {
                    continue;
                }
            }
            req.registrations.lock_or_recover().clear();
            self.state
                .lock_or_recover()
                .active
                .retain(|active| !Arc::ptr_eq(active, &req));
            let event = IoEvent {
                res: i64::from(mask.0),
                ..req.event
            };
            self.complete(event, req.eventfd.as_deref());
        }
    }

    /// `aio_poll_cancel` of the poll for the iocb at `obj`; `EINVAL` if no poll waits for it.
    fn cancel(&self, obj: u64) -> Result<(), Errno> {
        let req = {
            let mut state = self.state.lock_or_recover();
            let at = state
                .active
                .iter()
                .position(|req| req.event.obj == obj)
                .ok_or(Errno::EINVAL)?;
            state.active.remove(at)
        };
        self.cancel_req(req);
        Ok(())
    }

    fn cancel_req(&self, req: Arc<PollReq>) {
        let schedule = {
            let mut poll = req.state.lock_or_recover();
            poll.cancelled = true;
            !std::mem::replace(&mut poll.scheduled, true)
        };
        if schedule {
            self.schedule(req);
        }
    }

    /// `kill_ioctx`: the context takes no more requests, and every poll is cancelled
    /// (`free_ioctx_users`).
    fn kill(&self) {
        let active = {
            let mut state = self.state.lock_or_recover();
            state.dead = true;
            std::mem::take(&mut state.active)
        };
        for req in active {
            self.cancel_req(req);
        }
        self.sysctl.give_back(self.max_reqs.into());
        self.wait.wake(PollMask::empty());
    }
}

/// An `IOCB_CMD_POLL` request (`struct poll_iocb`).
struct PollReq {
    ctx: Weak<Kioctx>,
    file: Arc<OpenFile>,
    /// What it waits for, `EPOLLERR` and `EPOLLHUP` included.
    events: PollMask,
    /// The event it completes with, but for `res`.
    event: IoEvent,
    eventfd: Option<Arc<OpenFile>>,
    registrations: Mutex<Vec<WaitRegistration>>,
    state: Mutex<PollState>,
}

#[derive(Default)]
struct PollState {
    /// The timer will look at the poll (`work_scheduled`).
    scheduled: bool,
    cancelled: bool,
    /// Completed; it posts no other event.
    done: bool,
}

/// The poll's entry on the file's queues (`aio_poll_wake`).
struct PollEntry(Weak<PollReq>);

impl WaitEntry for PollEntry {
    fn wake(&self, events: PollMask) -> bool {
        let Some(req) = self.0.upgrade() else {
            return false;
        };
        if !events.is_empty() && (events & req.events).is_empty() {
            return false;
        }
        let schedule = {
            let mut poll = req.state.lock_or_recover();
            !poll.done && !std::mem::replace(&mut poll.scheduled, true)
        };
        if schedule && let Some(ctx) = req.ctx.upgrade() {
            ctx.schedule(req);
        }
        true
    }
}

/// `ioprio_check_cap` (`block/ioprio.c`): an I/O priority's class and level.
pub(crate) fn ioprio_check_cap(
    kernel: &Kernel,
    caller: &Caller<'_>,
    ioprio: i16,
) -> Result<(), Errno> {
    const CLASS_SHIFT: u16 = 13;
    let ioprio = ioprio as u16;
    let (class, level) = ((ioprio >> CLASS_SHIFT) & 7, ioprio & 7);
    match class {
        // IOPRIO_CLASS_RT.
        1 => {
            let cred = kernel.cred(caller);
            if !cred.capable(Cap::SysAdmin) && !cred.capable(Cap::SysNice) {
                return Err(Errno::EPERM);
            }
        }
        // IOPRIO_CLASS_BE, IOPRIO_CLASS_IDLE.
        2 | 3 => {}
        // IOPRIO_CLASS_NONE.
        0 if level == 0 => {}
        _ => return Err(Errno::EINVAL),
    }
    Ok(())
}

/// What an iocb asks for, read and checked by `io_submit_one`.
struct Submission {
    iocb: Iocb,
    /// Its address, the event's `obj`.
    user_iocb: u32,
    file: Arc<OpenFile>,
    eventfd: Option<Arc<OpenFile>>,
}

impl Submission {
    fn event(&self) -> IoEvent {
        IoEvent {
            data: self.iocb.aio_data,
            obj: u64::from(self.user_iocb),
            res: 0,
            res2: 0,
        }
    }
}

impl Kernel {
    fn lookup_ioctx(&self, caller: &Caller<'_>, ctx_id: AioContext) -> Result<Arc<Kioctx>, Errno> {
        self.mm(caller.task())
            .aio
            .lookup(ctx_id)
            .ok_or(Errno::EINVAL)
    }

    /// `io_setup`: a context for `nr_events` requests, whose id goes to `*ctxp`, which must
    /// hold 0.
    pub(crate) async fn sys_io_setup(
        &self,
        caller: &Caller<'_>,
        nr_events: u32,
        ctxp: AioContextPtr,
    ) -> SysResult<i32> {
        let ctx: AioContext = read_value(caller.memory(), GuestPtr::new(ctxp.addr())).await?;
        if ctx != 0 || nr_events == 0 {
            return Err(Errno::EINVAL.into());
        }
        let ctx = self.ioctx_alloc(caller, nr_events).await?;
        if write_value(caller.memory(), ctxp, &ctx.user_id)
            .await
            .is_err()
        {
            self.kill_ioctx(caller, &ctx).await;
            return Err(Errno::EFAULT.into());
        }
        Ok(0)
    }

    /// `ioctx_alloc` and `aio_setup_ring`.
    async fn ioctx_alloc(&self, caller: &Caller<'_>, max_reqs: u32) -> Result<Arc<Kioctx>, Errno> {
        // Up to half the slots may sit on other CPUs' counters on Linux, so it doubles them.
        let doubled = max_reqs
            .max(crate::sched::cpus().saturating_mul(4))
            .saturating_mul(2);
        if u64::from(doubled) > 0x1000_0000 / EVENT_SIZE {
            return Err(Errno::EINVAL);
        }
        if u64::from(max_reqs) > self.aio.max() {
            return Err(Errno::EAGAIN);
        }
        // The ring holds two more events: one for the head and tail to overlap.
        let size = RING_HEADER + u64::from(doubled + 2) * EVENT_SIZE;
        let mmap_size = size.div_ceil(PAGE_SIZE) * PAGE_SIZE;
        let nr_events = ((mmap_size - RING_HEADER) / EVENT_SIZE) as u32;
        let pages = self
            .host
            .shared_pages(mmap_size)
            .map_err(|_| Errno::ENOMEM)?;
        let header = AioRing {
            id: !0,
            nr: nr_events,
            head: 0,
            tail: 0,
            magic: AioRingMisc::AIO_RING_MAGIC.0,
            compat_features: AioRingMisc::AIO_RING_COMPAT_FEATURES.0,
            incompat_features: AioRingMisc::AIO_RING_INCOMPAT_FEATURES.0,
            header_length: AioRing::SIZE as u32,
        };
        let mut bytes = [0; AioRing::SIZE];
        header.write_to(&mut bytes);
        pages.write_at(&bytes, 0).map_err(|_| Errno::ENOMEM)?;
        let user_id = self
            .map_kernel_pages(caller, pages.clone(), mmap_size)
            .await?;
        let ctx = Kioctx::new(
            &self.timers,
            (user_id, mmap_size, pages),
            (nr_events, max_reqs),
            self.aio.clone(),
        );
        if let Err(errno) = self.aio.take(max_reqs.into()) {
            let _ = self.sys_munmap(caller, user_id, mmap_size as u32).await;
            return Err(errno);
        }
        if let Err(errno) = self.mm(caller.task()).aio.add(ctx.clone()) {
            self.aio.give_back(max_reqs.into());
            let _ = self.sys_munmap(caller, user_id, mmap_size as u32).await;
            return Err(errno);
        }
        Ok(ctx)
    }

    /// `kill_ioctx` from a call: out of the table, its polls cancelled, its ring unmapped
    /// (`vm_munmap`); then, as `io_destroy` does, waits for its requests to end.
    async fn kill_ioctx(&self, caller: &Caller<'_>, ctx: &Arc<Kioctx>) -> bool {
        if !self.mm(caller.task()).aio.remove(ctx) {
            return false;
        }
        ctx.kill();
        let _ = self
            .sys_munmap(caller, ctx.user_id, ctx.mmap_size as u32)
            .await;
        let drained = || (ctx.state.lock_or_recover().inflight == 0).then_some(());
        let _ = wait_event(&ctx.wait, drained, &NoInterrupt).await;
        true
    }

    /// `exit_aio`: the address space ends, and its contexts with it. Their rings go with the
    /// mappings.
    pub(crate) fn exit_aio(&self, mm: &crate::mm::Mm) {
        for ctx in mm.aio.take_all() {
            ctx.kill();
        }
    }

    /// `io_destroy`.
    pub(crate) async fn sys_io_destroy(
        &self,
        caller: &Caller<'_>,
        ctx_id: AioContext,
    ) -> SysResult<i32> {
        let ctx = self.lookup_ioctx(caller, ctx_id)?;
        match self.kill_ioctx(caller, &ctx).await {
            true => Ok(0),
            // `kill_ioctx` lost a race with another `io_destroy`.
            false => Err(Errno::EINVAL.into()),
        }
    }

    /// `io_submit`: submits up to `nr` iocbs, stopping at the first that fails. Returns how
    /// many went in, or the first one's error.
    pub(crate) async fn sys_io_submit(
        &self,
        caller: &Caller<'_>,
        ctx_id: AioContext,
        nr: Slong,
        iocbpp: ConstIocbPtrArray,
    ) -> SysResult<i32> {
        if nr < 0 {
            return Err(Errno::EINVAL.into());
        }
        let ctx = self.lookup_ioctx(caller, ctx_id)?;
        let nr = (nr as u32).min(ctx.nr_events);
        let mut submitted = 0;
        let mut ret = Ok(());
        while submitted < nr {
            let at = iocbpp.addr().wrapping_add(submitted * 4);
            let user_iocb = match read_value(caller.memory(), GuestPtr::<u32>::new(at)).await {
                Ok(user_iocb) => user_iocb,
                Err(_) => {
                    ret = Err(Errno::EFAULT);
                    break;
                }
            };
            if let Err(errno) = self.io_submit_one(caller, &ctx, user_iocb).await {
                ret = Err(errno);
                break;
            }
            submitted += 1;
        }
        match (submitted, ret) {
            (0, Err(errno)) => Err(errno.into()),
            (submitted, _) => Ok(submitted as i32),
        }
    }

    /// `io_submit_one`.
    async fn io_submit_one(
        &self,
        caller: &Caller<'_>,
        ctx: &Arc<Kioctx>,
        user_iocb: u32,
    ) -> Result<(), Errno> {
        let iocb: Iocb = read_value(caller.memory(), GuestPtr::new(user_iocb))
            .await
            .map_err(|_| Errno::EFAULT)?;
        if iocb.aio_reserved2 != 0 {
            return Err(Errno::EINVAL);
        }
        // The buffer must be an address and the count a non-negative `ssize_t`.
        if iocb.aio_buf > u64::from(u32::MAX) || iocb.aio_nbytes > i32::MAX as u64 {
            return Err(Errno::EINVAL);
        }
        ctx.get_req()?;
        let submitted = self.submit_iocb(caller, ctx, iocb, user_iocb).await;
        if submitted.is_err() {
            ctx.put_req();
        }
        submitted
    }

    /// `__io_submit_one`.
    async fn submit_iocb(
        &self,
        caller: &Caller<'_>,
        ctx: &Arc<Kioctx>,
        iocb: Iocb,
        user_iocb: u32,
    ) -> Result<(), Errno> {
        let fd = i32::try_from(iocb.aio_fildes).map_err(|_| Errno::EBADF)?;
        let file = self.file(caller.task(), Fd(fd))?;
        let flags = IocbFlags(iocb.aio_flags);
        let eventfd = match flags.contains(IocbFlags::IOCB_FLAG_RESFD) {
            true => {
                let resfd = i32::try_from(iocb.aio_resfd).map_err(|_| Errno::EBADF)?;
                Some(self.eventfd_ctx_fdget(caller, Fd(resfd))?)
            }
            false => None,
        };
        let key = GuestPtrMut::<u32>::new(user_iocb.wrapping_add(8));
        write_value(caller.memory(), key, &KIOCB_KEY)
            .await
            .map_err(|_| Errno::EFAULT)?;
        let req = Submission {
            iocb,
            user_iocb,
            file,
            eventfd,
        };
        match IocbCmd(iocb.aio_lio_opcode) {
            IocbCmd::IOCB_CMD_PREAD => self.aio_rw(caller, ctx, req, false, false).await,
            IocbCmd::IOCB_CMD_PWRITE => self.aio_rw(caller, ctx, req, true, false).await,
            IocbCmd::IOCB_CMD_PREADV => self.aio_rw(caller, ctx, req, false, true).await,
            IocbCmd::IOCB_CMD_PWRITEV => self.aio_rw(caller, ctx, req, true, true).await,
            IocbCmd::IOCB_CMD_FSYNC => self.aio_fsync(ctx, req, false).await,
            IocbCmd::IOCB_CMD_FDSYNC => self.aio_fsync(ctx, req, true).await,
            IocbCmd::IOCB_CMD_POLL => self.aio_poll(ctx, req),
            _ => Err(Errno::EINVAL),
        }
    }

    /// `aio_read` and `aio_write`: checked, then run to the end; the result is the event's.
    async fn aio_rw(
        &self,
        caller: &Caller<'_>,
        ctx: &Arc<Kioctx>,
        req: Submission,
        write: bool,
        vectored: bool,
    ) -> Result<(), Errno> {
        let iocb = &req.iocb;
        // `aio_prep_rw`.
        if IocbFlags(iocb.aio_flags).contains(IocbFlags::IOCB_FLAG_IOPRIO) {
            ioprio_check_cap(self, caller, iocb.aio_reqprio)?;
        }
        let rw_flags = RwfFlags(iocb.aio_rw_flags as u32);
        crate::fs::io::rw_flags(rw_flags)?;
        match write {
            true => req.file.check_writable()?,
            false => req.file.check_readable()?,
        }
        // `aio_setup_rw`.
        let iovecs = match vectored {
            true => {
                let vec = GuestPtr::new(iocb.aio_buf as u32);
                read_iovecs(caller.memory(), vec, iocb.aio_nbytes as u32).await?
            }
            false => vec![Iovec {
                iov_base: iocb.aio_buf as u32,
                iov_len: iocb.aio_nbytes as u32,
            }],
        };
        // `rw_verify_area`: a file's offset may not be negative; a stream has none.
        let offset = match &req.file.backing {
            Backing::Inode(_) => u64::try_from(iocb.aio_offset).map_err(|_| Errno::EINVAL)?,
            Backing::Stream(..) => 0,
        };
        let at = At::Offset(offset);
        let done = match write {
            true => {
                self.write_from_iovecs(caller, &req.file, &iovecs, at, rw_flags)
                    .await
            }
            false => self.read_into_iovecs(caller, &req.file, &iovecs, at).await,
        };
        let event = IoEvent {
            res: event_result(done),
            ..req.event()
        };
        ctx.complete(event, req.eventfd.as_deref());
        Ok(())
    }

    /// `aio_fsync`, run here rather than from a workqueue.
    async fn aio_fsync(
        &self,
        ctx: &Arc<Kioctx>,
        req: Submission,
        datasync: bool,
    ) -> Result<(), Errno> {
        let iocb = &req.iocb;
        if iocb.aio_buf != 0
            || iocb.aio_offset != 0
            || iocb.aio_nbytes != 0
            || iocb.aio_rw_flags != 0
        {
            return Err(Errno::EINVAL);
        }
        // Only files have an `fsync`.
        let Backing::Inode(inner) = &req.file.backing else {
            return Err(Errno::EINVAL);
        };
        let synced = inner.sync(datasync).await.map(|()| 0);
        let event = IoEvent {
            res: event_result(synced.map_err(SysErr::from)),
            ..req.event()
        };
        ctx.complete(event, req.eventfd.as_deref());
        Ok(())
    }

    /// `aio_poll`: completes at once if the file is ready, else waits on its queues.
    fn aio_poll(&self, ctx: &Arc<Kioctx>, req: Submission) -> Result<(), Errno> {
        let iocb = &req.iocb;
        if iocb.aio_buf > u64::from(u16::MAX) {
            return Err(Errno::EINVAL);
        }
        if iocb.aio_offset != 0 || iocb.aio_nbytes != 0 || iocb.aio_rw_flags != 0 {
            return Err(Errno::EINVAL);
        }
        // `demangle_poll`: the generic architecture's POLL bits are the EPOLL ones.
        let events = PollMask(iocb.aio_buf as u32) | PollMask::ERR | PollMask::HUP;
        let poll = Arc::new(PollReq {
            ctx: Arc::downgrade(ctx),
            file: req.file.clone(),
            events,
            event: req.event(),
            eventfd: req.eventfd.clone(),
            registrations: Mutex::default(),
            state: Mutex::default(),
        });
        let mut registrations = Vec::new();
        let mut queue_proc = |queue: &WaitQueue, _: PollMask| {
            let entry = PollEntry(Arc::downgrade(&poll));
            registrations.push(queue.add(Arc::new(entry)));
        };
        let mask = poll
            .file
            .poll(Some(&mut PollTable::new(events, &mut queue_proc)))
            & events;
        let queued = !registrations.is_empty();
        if !mask.is_empty() {
            // Ready: completed here, its entries gone.
            drop(registrations);
            poll.state.lock_or_recover().done = true;
            let event = IoEvent {
                res: i64::from(mask.0),
                ..poll.event
            };
            ctx.complete(event, poll.eventfd.as_deref());
            return Ok(());
        }
        // A file that cannot be polled.
        if !queued {
            return Err(Errno::EINVAL);
        }
        poll.registrations.lock_or_recover().extend(registrations);
        ctx.state.lock_or_recover().active.push(poll.clone());
        // A wakeup between the look and the registration being kept was scheduled; the timer
        // looks again either way.
        Ok(())
    }

    /// `io_cancel`: cancels the poll for `iocb`, whose event then comes on the ring;
    /// `EINPROGRESS` says so. Nothing else can be cancelled (`EINVAL`).
    pub(crate) async fn sys_io_cancel(
        &self,
        caller: &Caller<'_>,
        ctx_id: AioContext,
        iocb: IocbPtr,
        _result: IoEventPtr,
    ) -> SysResult<i32> {
        let key = GuestPtr::<u32>::new(iocb.addr().wrapping_add(8));
        let key = read_value(caller.memory(), key).await?;
        if key != KIOCB_KEY {
            return Err(Errno::EINVAL.into());
        }
        let ctx = self.lookup_ioctx(caller, ctx_id)?;
        ctx.cancel(u64::from(iocb.addr()))?;
        Err(Errno::EINPROGRESS.into())
    }

    /// `io_pgetevents`: `io_getevents` under the signal mask `usig` gives.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn sys_io_pgetevents(
        &self,
        caller: &Caller<'_>,
        ctx_id: AioContext,
        min_nr: Slong,
        nr: Slong,
        events: IoEventArray,
        timeout: KernelTimespecPtr,
        usig: ConstAioSigsetPtr,
    ) -> SysResult<i32> {
        let ts: Option<KernelTimespec> = match timeout.is_null() {
            true => None,
            false => Some(read_value(caller.memory(), GuestPtr::new(timeout.addr())).await?),
        };
        let ksig = match usig.is_null() {
            true => AioSigset::ZERO,
            false => read_value(caller.memory(), usig).await?,
        };
        let saved = self
            .set_user_sigmask(caller, ksig.sigmask, ksig.sigsetsize)
            .await?;
        let ret = self
            .do_io_getevents(caller, ctx_id, (min_nr, nr), events, ts)
            .await;
        let interrupted = wasixv2_wait::Interrupt::pending(&*self.interrupt(caller));
        self.restore_sigmask(caller, saved, interrupted);
        match ret {
            Ok(0) if interrupted => Err(SysErr::RestartNoHand),
            ret => ret,
        }
    }

    /// `do_io_getevents` and `read_events`: at least `min_nr` and at most `nr` events, waiting
    /// at most `ts` (relative) for them; what came once the time is up or a signal arrived.
    async fn do_io_getevents(
        &self,
        caller: &Caller<'_>,
        ctx_id: AioContext,
        (min_nr, nr): (Slong, Slong),
        events: IoEventArray,
        ts: Option<KernelTimespec>,
    ) -> SysResult<i32> {
        let ctx = self.lookup_ioctx(caller, ctx_id)?;
        if min_nr > nr || min_nr < 0 {
            return Err(Errno::EINVAL.into());
        }
        let (min_nr, nr) = (min_nr as u32, nr as u32);
        // `timespec64_to_ktime`: a time that is not positive does not wait.
        let until = ts.map(|ts| match crate::time::from_timespec(ts) {
            Ok(timeout) => timeout,
            Err(_) if ts.tv_sec < 0 => Duration::ZERO,
            Err(_) => Duration::new(ts.tv_sec as u64, 0),
        });
        let deadline = until.map(|until| self.host.monotonic().saturating_add(until));
        let interrupt = self.interrupt(caller);
        let mut got = 0;
        loop {
            match ctx.read_events_ring(caller, events.addr(), nr - got).await {
                Ok(n) => got += n,
                Err(errno) if got == 0 => return Err(errno.into()),
                Err(_) => break,
            }
            if ctx.is_dead() {
                if got == 0 {
                    return Err(Errno::EINVAL.into());
                }
                break;
            }
            if got >= min_nr || until == Some(Duration::ZERO) {
                break;
            }
            let ready = || (ctx.has_events() || ctx.is_dead()).then_some(());
            let waited = match deadline {
                None => wait_event(&ctx.wait, ready, &*interrupt).await.map(Some),
                Some(at) => {
                    let timer = self.host.sleep_until(at);
                    wait_event_timeout(&ctx.wait, ready, &*interrupt, timer).await
                }
            };
            match waited {
                Ok(Some(())) => {}
                // Timed out, or a signal: what came so far.
                Ok(None) | Err(_) => {
                    if let Ok(n) = ctx.read_events_ring(caller, events.addr(), nr - got).await {
                        got += n;
                    }
                    break;
                }
            }
        }
        Ok(got as i32)
    }
}

impl Kioctx {
    fn new(
        timers: &Timers,
        (user_id, mmap_size, pages): (AioContext, u64, Arc<dyn SharedPages>),
        (nr_events, max_reqs): (u32, u32),
        sysctl: Arc<AioSysctl>,
    ) -> Arc<Self> {
        Arc::new_cyclic(|me: &Weak<Kioctx>| {
            let me = me.clone();
            let timer = timers.timer(Box::new(move || {
                if let Some(ctx) = me.upgrade() {
                    ctx.run_scheduled();
                }
                None
            }));
            Self {
                user_id,
                mmap_size,
                pages,
                nr_events,
                max_reqs,
                sysctl,
                state: Mutex::default(),
                ring_lock: Default::default(),
                wait: WaitQueue::new(),
                timer,
            }
        })
    }
}

/// An event's `res`: the count, or the negated errno. An interrupted transfer cannot be
/// restarted, so it is `EINTR` (`aio_rw_done`).
fn event_result<T: Into<i64>>(result: SysResult<T>) -> i64 {
    match result {
        Ok(n) => n.into(),
        Err(SysErr::Errno(errno)) => -i64::from(errno.0),
        Err(_) => -i64::from(Errno::EINTR.0),
    }
}
