//! Requests: submission (`io_init_req`, links, drains), issue of each operation, arming on a
//! file's wait queues, polls, timeouts, link timeouts and cancellation (`io_uring/io_uring.c`,
//! `rw.c`, `net.c`, `poll.c`, `timeout.c`, `cancel.c`, `openclose.c`, `fs.c`, `sync.c`,
//! `kbuf.c`, `nop.c`).

use std::{
    collections::VecDeque,
    sync::{Arc, Weak},
    time::Duration,
};

use wasixv2_abi::{
    AtFlags, FallocFlags, GuestPtr, GuestPtrMut, IoUringOp, IoUringSqe, IoringCqeFlags,
    IoringOpFlags, IosqeFlags, Iovec, KernelTimespec, MsgFlags, OpenFlags, RwfFlags, ShutdownHow,
    SocketFlags, StatxMask,
};
use wasixv2_syscalls::{Caller, Errno, Fd, SysErr, SysResult, TaskId, read_value};
use wasixv2_wait::{PollMask, PollTable, WaitEntry, WaitQueue};

use super::{Ctx, Pbuf, Registrations, State, cqe_res, is_ring};
use crate::{
    Kernel,
    cred::Cred,
    file::{Backing, OpenFile},
    fs::io::{At, fallocate_mode, read_iovecs, rw_flags},
    sync::MutexExt,
    timer::Timer,
};

/// The operations this kernel issues; the probe reports them.
pub(crate) const SUPPORTED: &[IoUringOp] = &[
    IoUringOp::IORING_OP_NOP,
    IoUringOp::IORING_OP_READV,
    IoUringOp::IORING_OP_WRITEV,
    IoUringOp::IORING_OP_FSYNC,
    IoUringOp::IORING_OP_READ_FIXED,
    IoUringOp::IORING_OP_WRITE_FIXED,
    IoUringOp::IORING_OP_POLL_ADD,
    IoUringOp::IORING_OP_POLL_REMOVE,
    IoUringOp::IORING_OP_SENDMSG,
    IoUringOp::IORING_OP_RECVMSG,
    IoUringOp::IORING_OP_TIMEOUT,
    IoUringOp::IORING_OP_TIMEOUT_REMOVE,
    IoUringOp::IORING_OP_ACCEPT,
    IoUringOp::IORING_OP_ASYNC_CANCEL,
    IoUringOp::IORING_OP_LINK_TIMEOUT,
    IoUringOp::IORING_OP_CONNECT,
    IoUringOp::IORING_OP_FALLOCATE,
    IoUringOp::IORING_OP_OPENAT,
    IoUringOp::IORING_OP_CLOSE,
    IoUringOp::IORING_OP_STATX,
    IoUringOp::IORING_OP_READ,
    IoUringOp::IORING_OP_WRITE,
    IoUringOp::IORING_OP_SEND,
    IoUringOp::IORING_OP_RECV,
    IoUringOp::IORING_OP_OPENAT2,
    IoUringOp::IORING_OP_PROVIDE_BUFFERS,
    IoUringOp::IORING_OP_REMOVE_BUFFERS,
    IoUringOp::IORING_OP_SHUTDOWN,
];

/// `SQE_VALID_FLAGS`.
const SQE_VALID_FLAGS: u8 = IosqeFlags::IOSQE_FIXED_FILE.0
    | IosqeFlags::IOSQE_IO_DRAIN.0
    | IosqeFlags::IOSQE_IO_LINK.0
    | IosqeFlags::IOSQE_IO_HARDLINK.0
    | IosqeFlags::IOSQE_ASYNC.0
    | IosqeFlags::IOSQE_BUFFER_SELECT.0
    | IosqeFlags::IOSQE_CQE_SKIP_SUCCESS.0;
const LINK_FLAGS: u8 = IosqeFlags::IOSQE_IO_LINK.0 | IosqeFlags::IOSQE_IO_HARDLINK.0;
/// `MAX_BIDS_PER_BGID`.
const MAX_BIDS_PER_BGID: u32 = 1 << 16;

const fn op(op: IoUringOp) -> u8 {
    op.0
}

/// One request (`struct io_kiocb`), with the SQE it was submitted with
/// (`IORING_FEAT_SUBMIT_STABLE`: the SQE is copied at submission).
pub(crate) struct Req {
    pub(crate) id: u64,
    pub(crate) sqe: IoUringSqe,
    /// The memory it was submitted on, whose calls issue it again.
    pub(crate) mm: u64,
    /// The task that submitted it (`req->tctx`).
    pub(crate) task: TaskId,
    /// Its personality's credentials, which it is issued with (`REQ_F_CREDS`).
    creds: Option<Cred>,
    /// Its file, found at its first issue (`IORING_FEAT_LINKED_FILE`).
    file: Option<Arc<OpenFile>>,
    /// The requests linked after it.
    link: VecDeque<Req>,
    /// The `IORING_OP_LINK_TIMEOUT` that guards it, and its timer once armed.
    link_timeout: Option<LinkTimeout>,
    /// Its submission failed: it completes with this.
    failed: Option<Errno>,
    /// A wakeup made it ready again: an attempt that would block arms it again rather than
    /// failing.
    retried: bool,
    /// A non-blocking `connect` is under way (`io_connect`'s `in_progress`).
    connecting: bool,
    /// The provided buffer it took (`REQ_F_BUFFER_SELECTED`).
    pbuf: Option<Pbuf>,
}

/// A link timeout and its armed timer.
struct LinkTimeout {
    req: Box<Req>,
    timer: Option<Timer>,
}

/// What a waiting request waits for.
pub(crate) enum Waiting {
    /// Armed on its file's queues; a wakeup makes it task work.
    Armed {
        req: Req,
        registrations: Registrations,
    },
    /// `IORING_OP_POLL_ADD`.
    Poll {
        req: Req,
        events: PollMask,
        multishot: bool,
        _registrations: Registrations,
    },
    /// `IORING_OP_TIMEOUT`: its timer, and the completion count that ends it.
    Timeout {
        req: Req,
        _timer: Timer,
        target: Option<u64>,
        /// Firings left for a multishot timeout; 0 for an endless one.
        repeats: Option<u32>,
    },
}

impl Waiting {
    fn req(&self) -> &Req {
        match self {
            Waiting::Armed { req, .. }
            | Waiting::Poll { req, .. }
            | Waiting::Timeout { req, .. } => req,
        }
    }

    fn into_req(self) -> Req {
        match self {
            Waiting::Armed { req, .. }
            | Waiting::Poll { req, .. }
            | Waiting::Timeout { req, .. } => req,
        }
    }

    /// For fdinfo: the opcode and events of a poll or an armed request.
    pub(crate) fn poll_info(&self) -> Option<(u8, PollMask)> {
        match self {
            Waiting::Poll { req, events, .. } => Some((req.sqe.opcode, *events)),
            Waiting::Armed { req, .. } => Some((req.sqe.opcode, PollMask::empty())),
            Waiting::Timeout { .. } => None,
        }
    }
}

/// How an attempt at a request ended.
enum Outcome {
    /// Complete with this result and these CQE flags; `fail` breaks its link.
    Done { res: i32, flags: u32, fail: bool },
    /// A multishot request's result; it goes on.
    More { res: i32, flags: u32 },
    /// It would block until the file reports one of these events.
    Again(PollMask),
    /// It waits for its own wakeup or timer.
    Wait(Box<Waiting>),
}

impl Outcome {
    fn of(result: SysResult<i32>) -> Self {
        let res = cqe_res(result);
        Outcome::Done {
            res,
            flags: 0,
            fail: res < 0,
        }
    }
}

impl Req {
    fn new(id: u64, sqe: IoUringSqe, (mm, task): (u64, TaskId)) -> Self {
        Self {
            id,
            sqe,
            mm,
            task,
            creds: None,
            file: None,
            link: VecDeque::new(),
            link_timeout: None,
            failed: None,
            retried: false,
            connecting: false,
            pbuf: None,
        }
    }

    fn has(&self, flag: IosqeFlags) -> bool {
        self.sqe.flags & flag.0 != 0
    }

    fn opcode(&self) -> u8 {
        self.sqe.opcode
    }

    pub(crate) fn is_timeout(&self) -> bool {
        self.opcode() == op(IoUringOp::IORING_OP_TIMEOUT)
            || self.opcode() == op(IoUringOp::IORING_OP_LINK_TIMEOUT)
    }

    /// Whether it or a request linked to it holds a ring's file (`io_match_linked` of
    /// `REQ_F_INFLIGHT`).
    pub(crate) fn holds_ring(&self) -> bool {
        let ring = |req: &Req| req.file.as_deref().is_some_and(is_ring);
        ring(self) || self.link.iter().any(ring)
    }

    /// An empty request in place of one that moved on.
    fn taken(&self) -> Self {
        Req::new(0, self.sqe, (0, TaskId(0)))
    }
}

/// A request's entry on its file's queues (`io_poll_wake`).
struct Entry {
    ctx: Weak<Ctx>,
    id: u64,
    events: PollMask,
}

impl WaitEntry for Entry {
    fn wake(&self, events: PollMask) -> bool {
        if !events.is_empty() && (events & self.events).is_empty() {
            return false;
        }
        if let Some(ctx) = self.ctx.upgrade() {
            ctx.woken(self.id);
        }
        true
    }
}

/// Registers an entry for request `id` on the queues of `file`, for `events`; returns the
/// file's readiness and the registrations.
fn register(ctx: &Ctx, file: &OpenFile, id: u64, events: PollMask) -> (PollMask, Registrations) {
    let mut registrations = Vec::new();
    let mut queue_proc = |queue: &WaitQueue, _: PollMask| {
        let entry = Entry {
            ctx: ctx.me.clone(),
            id,
            events,
        };
        registrations.push(queue.add(Arc::new(entry)));
    };
    let mask =
        wasixv2_wait::Pollable::poll(file, Some(&mut PollTable::new(events, &mut queue_proc)));
    (mask & events, registrations)
}

impl Ctx {
    fn next_id(&self) -> u64 {
        let mut state = self.state.lock_or_recover();
        state.last_id += 1;
        state.last_id
    }

    /// Completes `req` (`io_req_complete_post`): its link timeout loses, its CQE is posted
    /// unless `IOSQE_CQE_SKIP_SUCCESS` skips a success, and its link goes on, or, if it
    /// failed and was not hard-linked, is cancelled.
    pub(crate) fn finish(&self, mut req: Req, res: i32, flags: u32, fail: bool) {
        if let Some(lt) = req.link_timeout.take() {
            drop(lt.timer);
            self.finish(*lt.req, -Errno::ECANCELED.0, 0, true);
        }
        if let Some(pbuf) = req.pbuf.take() {
            self.recycle(&req, pbuf);
        }
        let skip = req.has(IosqeFlags::IOSQE_CQE_SKIP_SUCCESS) && res >= 0;
        if !skip {
            self.post(req.sqe.user_data, res, flags, req.is_timeout());
        }
        let link = std::mem::take(&mut req.link);
        let broken = fail && !req.has(IosqeFlags::IOSQE_IO_HARDLINK);
        self.request_done();
        self.continue_link(link, broken);
    }

    /// Starts the next request of a link, or fails all of them with `ECANCELED`.
    fn continue_link(&self, mut link: VecDeque<Req>, broken: bool) {
        let Some(mut next) = link.pop_front() else {
            return;
        };
        next.link = link;
        match broken {
            true => self.finish(next, -Errno::ECANCELED.0, 0, true),
            false => self.queue_ready(next),
        }
    }

    /// One request fewer in flight; a drain whose predecessors all completed goes on.
    fn request_done(&self) {
        let released = {
            let mut state = self.state.lock_or_recover();
            state.inflight = state.inflight.saturating_sub(1);
            release_drained(&mut state)
        };
        for req in released {
            self.queue_ready(req);
        }
    }

    /// Completes a request that waited, ending its wait.
    pub(crate) fn finish_waiting(&self, waiting: Waiting, res: i32) {
        let req = waiting.into_req();
        self.finish(req, res, 0, res < 0);
    }

    /// Gives a provided buffer back to its group's head (`io_kbuf_recycle_legacy`).
    fn recycle(&self, req: &Req, pbuf: Pbuf) {
        let mut state = self.state.lock_or_recover();
        let group = state.pbufs.entry(req.sqe.buf_index).or_default();
        group.push_front(pbuf);
    }

    /// Removes the waiting request `id`.
    fn take_waiting(&self, id: u64) -> Option<Waiting> {
        self.state.lock_or_recover().waiting.remove(&id)
    }
}

/// The drained requests whose predecessors all completed (`io_queue_deferred`): those queued
/// behind a drain wait until only they are in flight.
fn release_drained(state: &mut State) -> Vec<Req> {
    let mut released = Vec::new();
    while let Some(front) = state.drain.front() {
        let drains = front.has(IosqeFlags::IOSQE_IO_DRAIN);
        let held: u32 = state.drain.iter().map(|req| 1 + chain_len(req)).sum();
        if drains && (state.inflight > held || !released.is_empty()) {
            break;
        }
        released.push(state.drain.pop_front().expect("a front"));
    }
    released
}

/// The count-based timeouts the completions so far ended (`io_flush_timeouts`).
pub(crate) fn expired_timeouts(state: &mut State) -> Vec<Waiting> {
    let seq = state.seq;
    let ids: Vec<u64> = state
        .waiting
        .iter()
        .filter(|(_, w)| matches!(w, Waiting::Timeout { target: Some(t), .. } if *t <= seq))
        .map(|(&id, _)| id)
        .collect();
    ids.into_iter()
        .filter_map(|id| state.waiting.remove(&id))
        .collect()
}

/// From the timer driver: a waiting request's file woke it. An armed request becomes task
/// work; a poll that finds its events completes, or, multishot, posts and waits on.
pub(crate) fn handle_wakeup(ctx: &Ctx, id: u64) {
    let poll = {
        let state = ctx.state.lock_or_recover();
        match state.waiting.get(&id) {
            Some(Waiting::Armed { .. }) => None,
            Some(Waiting::Poll {
                req,
                events,
                multishot,
                ..
            }) => Some((req.file.clone(), *events, *multishot, req.sqe.user_data)),
            _ => return,
        }
    };
    let Some((file, events, multishot, user_data)) = poll else {
        if let Some(Waiting::Armed { mut req, .. }) = ctx.take_waiting(id) {
            req.retried = true;
            ctx.queue_ready(req);
        }
        return;
    };
    let mask = match file {
        Some(file) => wasixv2_wait::Pollable::poll(&*file, None) & events,
        None => return,
    };
    if mask.is_empty() {
        return;
    }
    match multishot {
        true => ctx.post(
            user_data,
            mask.0 as i32,
            IoringCqeFlags::IORING_CQE_F_MORE.0,
            false,
        ),
        false => {
            if let Some(waiting) = ctx.take_waiting(id) {
                ctx.finish(waiting.into_req(), mask.0 as i32, 0, false);
            }
        }
    }
}

/// `io_timeout_fn`: a timeout's timer fired.
fn timeout_fired(ctx: &Ctx, id: u64) -> Option<Duration> {
    let mut state = ctx.state.lock_or_recover();
    let Some(Waiting::Timeout { req, repeats, .. }) = state.waiting.get_mut(&id) else {
        return None;
    };
    let user_data = req.sqe.user_data;
    let interval = timeout_interval(req);
    // A multishot timeout goes on until its count of firings is spent.
    if let (Some(left), Some(interval)) = (repeats.as_mut(), interval) {
        let last = *left == 1;
        if *left > 1 {
            *left -= 1;
        }
        if !last {
            drop(state);
            let more = IoringCqeFlags::IORING_CQE_F_MORE.0;
            ctx.post(user_data, -Errno::ETIME.0, more, true);
            return Some(ctx.host().monotonic().saturating_add(interval));
        }
    }
    let waiting = state.waiting.remove(&id);
    drop(state);
    if let Some(waiting) = waiting {
        let success =
            waiting.req().sqe.op_flags & IoringOpFlags::IORING_TIMEOUT_ETIME_SUCCESS.0 != 0;
        let req = waiting.into_req();
        ctx.finish(req, -Errno::ETIME.0, 0, !success);
    }
    None
}

/// A multishot timeout's period: its relative timeout.
fn timeout_interval(req: &Req) -> Option<Duration> {
    let multishot = req.sqe.op_flags & IoringOpFlags::IORING_TIMEOUT_MULTISHOT.0 != 0;
    multishot.then(|| Duration::from_nanos(req.sqe.addr3))
}

/// `io_link_timeout_fn`: a link timeout fired before the request it guards completed, which is
/// cancelled, if it still waits.
fn link_timeout_fired(ctx: &Ctx, guarded: u64) {
    let (waiting, ready) = {
        let mut state = ctx.state.lock_or_recover();
        match state.waiting.remove(&guarded) {
            Some(waiting) => (Some(waiting), None),
            None => {
                let at = state.ready.iter().position(|req| req.id == guarded);
                (None, at.and_then(|at| state.ready.remove(at)))
            }
        }
    };
    let req = waiting.map(Waiting::into_req).or(ready);
    let Some(mut req) = req else { return };
    if let Some(lt) = req.link_timeout.take() {
        ctx.finish(*lt.req, -Errno::ETIME.0, 0, true);
    }
    ctx.finish(req, -Errno::ECANCELED.0, 0, true);
}

impl Kernel {
    /// `io_submit_sqe`: checks the SQE (`io_init_req`) and adds it to the link being built,
    /// or queues it. A request that fails its checks completes with the error and fails the
    /// link it is in; returns false then.
    pub(super) async fn submit_sqe(
        &self,
        caller: &Caller<'_>,
        ctx: &Arc<Ctx>,
        mm: u64,
        sqe: IoUringSqe,
        link: &mut Option<Req>,
    ) -> bool {
        let mut req = Req::new(ctx.next_id(), sqe, (mm, caller.task()));
        let mut init = init_req(ctx, &mut req);
        if init.is_ok() && req.opcode() == op(IoUringOp::IORING_OP_LINK_TIMEOUT) {
            // `io_link_timeout_prep`: the timeout is read now; its timer starts once the
            // request it guards waits.
            match self.timeout_prep(caller, &sqe, true).await {
                Ok(at) => req.sqe.addr3 = at.interval.as_nanos() as u64,
                Err(errno) => init = Err(errno),
            }
        }
        if let Err(errno) = init {
            req.failed = Some(errno);
        }
        ctx.state.lock_or_recover().inflight += 1;
        let linked = req.sqe.flags & LINK_FLAGS != 0;
        let link_timeout = req.opcode() == op(IoUringOp::IORING_OP_LINK_TIMEOUT);
        match link {
            Some(head) => {
                let failed = req.failed.is_some();
                if failed && head.failed.is_none() {
                    head.failed = Some(Errno::ECANCELED);
                }
                let last = match head.link.is_empty() {
                    true => &mut *head,
                    false => head.link.back_mut().expect("a link"),
                };
                if link_timeout && last.link_timeout.is_none() && !failed {
                    last.link_timeout = Some(LinkTimeout {
                        req: Box::new(req),
                        timer: None,
                    });
                } else {
                    if link_timeout && !failed {
                        req.failed = Some(Errno::EINVAL);
                    }
                    head.link.push_back(req);
                }
                if !linked {
                    let head = link.take().expect("a chain");
                    self.queue_sqe(caller, ctx, head).await;
                }
            }
            None if link_timeout => {
                req.failed.get_or_insert(Errno::EINVAL);
                self.queue_sqe(caller, ctx, req).await;
            }
            None if linked => *link = Some(req),
            None => self.queue_sqe(caller, ctx, req).await,
        }
        init.is_ok()
    }

    /// `io_queue_sqe`: a failed chain completes at once; a drained request, and any behind a
    /// drain, waits for its predecessors; the rest are issued now.
    pub(super) async fn queue_sqe(&self, caller: &Caller<'_>, ctx: &Arc<Ctx>, req: Req) {
        if let Some(errno) = req.failed {
            fail_chain(ctx, req, errno);
            return;
        }
        let deferred = {
            let mut state = ctx.state.lock_or_recover();
            let drain = req.has(IosqeFlags::IOSQE_IO_DRAIN);
            let others = state.inflight - 1 - chain_len(&req);
            if (drain && others > 0) || !state.drain.is_empty() {
                state.drain.push_back(req);
                None
            } else {
                Some(req)
            }
        };
        if let Some(req) = deferred {
            self.issue(caller, ctx, req).await;
        }
    }

    /// `io_issue_sqe`: one attempt at `req` that does not block, and what follows from it.
    pub(super) async fn issue(&self, caller: &Caller<'_>, ctx: &Arc<Ctx>, mut req: Req) {
        loop {
            let lent = req
                .creds
                .clone()
                .map(|cred| self.lend_cred(caller.task(), cred));
            let issued = self.issue_op(caller, ctx, &mut req).await;
            drop(lent);
            let outcome = match issued {
                Ok(outcome) => outcome,
                Err(errno) => Outcome::of(Err(errno.into())),
            };
            match outcome {
                Outcome::Done { res, flags, fail } => {
                    ctx.finish(req, res, flags, fail);
                    return;
                }
                Outcome::More { res, flags } => {
                    let more = flags | IoringCqeFlags::IORING_CQE_F_MORE.0;
                    ctx.post(req.sqe.user_data, res, more, false);
                }
                Outcome::Again(events) => {
                    arm(ctx, req, events);
                    return;
                }
                Outcome::Wait(waiting) => {
                    wait(ctx, *waiting);
                    return;
                }
            }
        }
    }

    /// The request's file: a registered one for `IOSQE_FIXED_FILE` (`io_file_get_fixed`), else
    /// the caller's descriptor (`io_file_get_normal`).
    fn req_file(
        &self,
        caller: &Caller<'_>,
        ctx: &Ctx,
        req: &mut Req,
    ) -> Result<Arc<OpenFile>, Errno> {
        if let Some(file) = &req.file {
            return Ok(file.clone());
        }
        let file = match req.has(IosqeFlags::IOSQE_FIXED_FILE) {
            true => {
                let state = ctx.state.lock_or_recover();
                let files = state.files.as_ref().ok_or(Errno::EBADF)?;
                let slot = files.get(req.sqe.fd as u32 as usize).ok_or(Errno::EBADF)?;
                slot.clone().ok_or(Errno::EBADF)?
            }
            false => self.file(caller.task(), Fd(req.sqe.fd))?,
        };
        req.file = Some(file.clone());
        Ok(file)
    }

    /// One attempt at the request's operation.
    async fn issue_op(
        &self,
        caller: &Caller<'_>,
        ctx: &Arc<Ctx>,
        req: &mut Req,
    ) -> Result<Outcome, Errno> {
        let sqe = req.sqe;
        match IoUringOp(sqe.opcode) {
            IoUringOp::IORING_OP_NOP => self.op_nop(caller, ctx, req),
            IoUringOp::IORING_OP_READ
            | IoUringOp::IORING_OP_READV
            | IoUringOp::IORING_OP_READ_FIXED
            | IoUringOp::IORING_OP_WRITE
            | IoUringOp::IORING_OP_WRITEV
            | IoUringOp::IORING_OP_WRITE_FIXED => self.op_rw(caller, ctx, req).await,
            IoUringOp::IORING_OP_FSYNC => {
                if sqe.addr != 0 || sqe.buf_index != 0 || sqe.splice_fd_in != 0 {
                    return Err(Errno::EINVAL);
                }
                let datasync = IoringOpFlags::IORING_FSYNC_DATASYNC.0;
                if sqe.op_flags & !datasync != 0 {
                    return Err(Errno::EINVAL);
                }
                let file = self.req_file(caller, ctx, req)?;
                let Backing::Inode(inner) = &file.backing else {
                    return Err(Errno::EINVAL);
                };
                let synced = inner.sync(sqe.op_flags & datasync != 0).await;
                Ok(Outcome::of(synced.map(|()| 0).map_err(SysErr::from)))
            }
            IoUringOp::IORING_OP_POLL_ADD => self.op_poll_add(caller, ctx, req),
            IoUringOp::IORING_OP_POLL_REMOVE => op_poll_remove(ctx, &sqe),
            IoUringOp::IORING_OP_TIMEOUT => self.op_timeout(caller, ctx, req).await,
            IoUringOp::IORING_OP_TIMEOUT_REMOVE => self.op_timeout_remove(caller, ctx, &sqe).await,
            IoUringOp::IORING_OP_LINK_TIMEOUT => Err(Errno::EINVAL),
            IoUringOp::IORING_OP_ASYNC_CANCEL => self.op_async_cancel(caller, ctx, &sqe),
            IoUringOp::IORING_OP_ACCEPT => self.op_accept(caller, ctx, req).await,
            IoUringOp::IORING_OP_CONNECT => self.op_connect(caller, ctx, req).await,
            IoUringOp::IORING_OP_SEND
            | IoUringOp::IORING_OP_RECV
            | IoUringOp::IORING_OP_SENDMSG
            | IoUringOp::IORING_OP_RECVMSG => self.op_net(caller, ctx, req).await,
            IoUringOp::IORING_OP_SHUTDOWN => {
                if sqe.off != 0 || sqe.addr != 0 || sqe.op_flags != 0 || sqe.buf_index != 0 {
                    return Err(Errno::EINVAL);
                }
                if sqe.splice_fd_in != 0 {
                    return Err(Errno::EINVAL);
                }
                let file = self.req_file(caller, ctx, req)?;
                let how = ShutdownHow(sqe.len as i32);
                Ok(Outcome::of(self.uring_shutdown(file, how)))
            }
            IoUringOp::IORING_OP_FALLOCATE => {
                if sqe.buf_index != 0 || sqe.op_flags != 0 || sqe.splice_fd_in != 0 {
                    return Err(Errno::EINVAL);
                }
                let (offset, len) = (sqe.off as i64, sqe.addr as i64);
                let alloc = fallocate_mode(FallocFlags(sqe.len), offset, len)?;
                let file = self.req_file(caller, ctx, req)?;
                if file.flags().contains(OpenFlags::O_PATH) {
                    return Err(Errno::EBADF);
                }
                let done = self.fallocate_file(caller, &file, alloc, offset, len).await;
                Ok(Outcome::of(done))
            }
            IoUringOp::IORING_OP_OPENAT | IoUringOp::IORING_OP_OPENAT2 => {
                self.op_openat(caller, req).await
            }
            IoUringOp::IORING_OP_CLOSE => self.op_close(caller, ctx, req),
            IoUringOp::IORING_OP_STATX => {
                if sqe.buf_index != 0 || sqe.splice_fd_in != 0 {
                    return Err(Errno::EINVAL);
                }
                if req.has(IosqeFlags::IOSQE_FIXED_FILE) {
                    return Err(Errno::EBADF);
                }
                let path = GuestPtr::new(sqe.addr as u32);
                let buffer = GuestPtrMut::new(sqe.off as u32);
                let flags = AtFlags(sqe.op_flags);
                let statx =
                    self.sys_statx(caller, Fd(sqe.fd), path, flags, StatxMask(sqe.len), buffer);
                Ok(Outcome::of(statx.await))
            }
            IoUringOp::IORING_OP_PROVIDE_BUFFERS => op_provide_buffers(ctx, &sqe),
            IoUringOp::IORING_OP_REMOVE_BUFFERS => op_remove_buffers(ctx, &sqe),
            _ => Err(Errno::EOPNOTSUPP),
        }
    }

    /// `io_nop`: completes with 0, or `IORING_NOP_INJECT_RESULT`'s `len`; with
    /// `IORING_NOP_FILE` its descriptor must be good.
    fn op_nop(&self, caller: &Caller<'_>, ctx: &Ctx, req: &mut Req) -> Result<Outcome, Errno> {
        let flags = req.sqe.op_flags;
        let known = IoringOpFlags::IORING_NOP_INJECT_RESULT.0
            | IoringOpFlags::IORING_NOP_FILE.0
            | IoringOpFlags::IORING_NOP_FIXED_FILE.0
            | IoringOpFlags::IORING_NOP_TW.0;
        if flags & !known != 0 {
            return Err(Errno::EINVAL);
        }
        if flags & IoringOpFlags::IORING_NOP_FILE.0 != 0 {
            if flags & IoringOpFlags::IORING_NOP_FIXED_FILE.0 != 0 {
                req.sqe.flags |= IosqeFlags::IOSQE_FIXED_FILE.0;
            }
            self.req_file(caller, ctx, req)?;
        }
        let res = match flags & IoringOpFlags::IORING_NOP_INJECT_RESULT.0 != 0 {
            true => req.sqe.len as i32,
            false => 0,
        };
        Ok(Outcome::Done {
            res,
            flags: 0,
            fail: res < 0,
        })
    }

    /// `io_read` and `io_write` (`rw.c`): a non-blocking attempt; a pollable file that would
    /// block is armed for `EPOLLIN` or `EPOLLOUT`, unless `RWF_NOWAIT` asked for `EAGAIN`.
    async fn op_rw(&self, caller: &Caller<'_>, ctx: &Ctx, req: &mut Req) -> Result<Outcome, Errno> {
        let sqe = req.sqe;
        let opcode = IoUringOp(sqe.opcode);
        let write = matches!(
            opcode,
            IoUringOp::IORING_OP_WRITE
                | IoUringOp::IORING_OP_WRITEV
                | IoUringOp::IORING_OP_WRITE_FIXED
        );
        let fixed = matches!(
            opcode,
            IoUringOp::IORING_OP_READ_FIXED | IoUringOp::IORING_OP_WRITE_FIXED
        );
        let vectored = matches!(
            opcode,
            IoUringOp::IORING_OP_READV | IoUringOp::IORING_OP_WRITEV
        );
        // `io_prep_rw`.
        if sqe.ioprio != 0 {
            crate::aio::ioprio_check_cap(self, caller, sqe.ioprio as i16)?;
        }
        let flags = RwfFlags(sqe.op_flags);
        rw_flags(flags)?;
        let select = req.has(IosqeFlags::IOSQE_BUFFER_SELECT);
        if select && (write || fixed || (vectored && sqe.len != 1)) {
            return Err(Errno::EINVAL);
        }
        let file = self.req_file(caller, ctx, req)?;
        match write {
            true => file.check_writable()?,
            false => file.check_readable()?,
        }
        let iovecs = match (vectored, fixed, select) {
            (_, _, true) => {
                let pbuf = match req.pbuf {
                    Some(pbuf) => pbuf,
                    None => {
                        let len = match vectored {
                            true => {
                                read_iovecs(caller.memory(), GuestPtr::new(sqe.addr as u32), 1)
                                    .await?[0]
                                    .iov_len
                            }
                            false => sqe.len,
                        };
                        let pbuf = select_buffer(ctx, sqe.buf_index, len)?;
                        req.pbuf = Some(pbuf);
                        pbuf
                    }
                };
                vec![Iovec {
                    iov_base: pbuf.addr,
                    iov_len: pbuf.len,
                }]
            }
            (true, _, _) => {
                read_iovecs(caller.memory(), GuestPtr::new(sqe.addr as u32), sqe.len).await?
            }
            (_, true, _) => vec![fixed_buffer(ctx, &sqe)?],
            _ => {
                let addr = u32::try_from(sqe.addr).map_err(|_| Errno::EFAULT)?;
                vec![Iovec {
                    iov_base: addr,
                    iov_len: sqe.len,
                }]
            }
        };
        let wanted: u64 = iovecs.iter().map(|iov| u64::from(iov.iov_len)).sum();
        // `-1` is the file position (`IORING_FEAT_RW_CUR_POS`).
        let at = match sqe.off as i64 {
            -1 => At::Position,
            off if matches!(file.backing, Backing::Stream(..)) => At::Offset(off.max(0) as u64),
            off => At::Offset(u64::try_from(off).map_err(|_| Errno::EINVAL)?),
        };
        let nowait = flags | RwfFlags::RWF_NOWAIT;
        let done = match write {
            true => {
                self.write_from_iovecs(caller, &file, &iovecs, at, nowait)
                    .await
            }
            false => {
                self.read_into_iovecs_flags(caller, &file, &iovecs, at, nowait)
                    .await
            }
        };
        let events = match write {
            true => PollMask::WRITABLE,
            false => PollMask::READABLE,
        };
        match done {
            Err(SysErr::Errno(Errno::EAGAIN)) if !flags.contains(RwfFlags::RWF_NOWAIT) => {
                if let Some(pbuf) = req.pbuf.take() {
                    ctx.recycle(req, pbuf);
                }
                if file.can_poll() {
                    return Ok(Outcome::Again(events));
                }
                Err(Errno::EAGAIN)
            }
            Ok(n) => Ok(Outcome::Done {
                res: n as i32,
                flags: req.pbuf.take().map_or(0, buffer_flags),
                fail: u64::from(n) < wanted,
            }),
            Err(err) => Ok(Outcome::of(Err(err))),
        }
    }

    /// `io_send`, `io_recv`, `io_sendmsg` and `io_recvmsg` (`net.c`): with `MSG_DONTWAIT`
    /// added; sends never raise `SIGPIPE` (`MSG_NOSIGNAL`).
    async fn op_net(
        &self,
        caller: &Caller<'_>,
        ctx: &Ctx,
        req: &mut Req,
    ) -> Result<Outcome, Errno> {
        let sqe = req.sqe;
        let opcode = IoUringOp(sqe.opcode);
        let send = matches!(
            opcode,
            IoUringOp::IORING_OP_SEND | IoUringOp::IORING_OP_SENDMSG
        );
        let msg = matches!(
            opcode,
            IoUringOp::IORING_OP_SENDMSG | IoUringOp::IORING_OP_RECVMSG
        );
        let iou = u32::from(sqe.ioprio);
        let poll_first = IoringOpFlags::IORING_RECVSEND_POLL_FIRST.0;
        let multishot = IoringOpFlags::IORING_RECV_MULTISHOT.0;
        let known = match send {
            true => poll_first,
            false => poll_first | multishot,
        };
        if iou & !known != 0 {
            return Err(Errno::EINVAL);
        }
        let select = req.has(IosqeFlags::IOSQE_BUFFER_SELECT);
        if select && (send || msg) {
            return Err(Errno::EINVAL);
        }
        let multishot = iou & multishot != 0;
        if multishot && (!select || req.has(IosqeFlags::IOSQE_CQE_SKIP_SUCCESS)) {
            return Err(Errno::EINVAL);
        }
        let file = self.req_file(caller, ctx, req)?;
        let user_flags = MsgFlags(sqe.op_flags);
        // `REQ_F_NOWAIT`: the application asked not to wait.
        let nowait = user_flags.contains(MsgFlags::MSG_DONTWAIT);
        if iou & poll_first != 0 && !req.retried && !nowait {
            req.retried = true;
            let events = if send {
                PollMask::WRITABLE
            } else {
                PollMask::READABLE
            };
            return Ok(Outcome::Again(events));
        }
        let mut flags = user_flags | MsgFlags::MSG_DONTWAIT;
        if send {
            flags |= MsgFlags::MSG_NOSIGNAL;
        }
        let (addr, len) = match select {
            true => {
                let pbuf = match req.pbuf {
                    Some(pbuf) => pbuf,
                    None => {
                        let pbuf = select_buffer(ctx, sqe.buf_index, sqe.len)?;
                        req.pbuf = Some(pbuf);
                        pbuf
                    }
                };
                (pbuf.addr, pbuf.len)
            }
            false => (sqe.addr as u32, sqe.len),
        };
        let done = match opcode {
            IoUringOp::IORING_OP_SEND => {
                self.uring_send(caller, file.clone(), (addr, len), flags)
                    .await
            }
            IoUringOp::IORING_OP_RECV => {
                self.uring_recv(caller, file.clone(), (addr, len), flags)
                    .await
            }
            IoUringOp::IORING_OP_SENDMSG => {
                self.uring_sendmsg(caller, file.clone(), addr, flags).await
            }
            _ => self.uring_recvmsg(caller, file.clone(), addr, flags).await,
        };
        let events = if send {
            PollMask::WRITABLE
        } else {
            PollMask::READABLE
        };
        match done {
            Err(SysErr::Errno(Errno::EAGAIN)) if !nowait => {
                if let Some(pbuf) = req.pbuf.take() {
                    ctx.recycle(req, pbuf);
                }
                Ok(Outcome::Again(events))
            }
            Ok(n) => {
                let flags = req.pbuf.take().map_or(0, buffer_flags);
                // A stream's end ends a multishot receive.
                if multishot && n > 0 {
                    return Ok(Outcome::More {
                        res: n as i32,
                        flags,
                    });
                }
                let waitall = user_flags.contains(MsgFlags::MSG_WAITALL);
                Ok(Outcome::Done {
                    res: n as i32,
                    flags,
                    fail: waitall && !msg && n < len as usize,
                })
            }
            Err(err) => Ok(Outcome::of(Err(err))),
        }
    }

    /// `io_accept`: `accept4` without waiting; `IORING_ACCEPT_MULTISHOT` goes on accepting.
    async fn op_accept(
        &self,
        caller: &Caller<'_>,
        ctx: &Ctx,
        req: &mut Req,
    ) -> Result<Outcome, Errno> {
        let sqe = req.sqe;
        if sqe.len != 0 || sqe.buf_index != 0 {
            return Err(Errno::EINVAL);
        }
        let iou = u32::from(sqe.ioprio);
        let multishot = IoringOpFlags::IORING_ACCEPT_MULTISHOT.0;
        let dontwait = IoringOpFlags::IORING_ACCEPT_DONTWAIT.0;
        let poll_first = IoringOpFlags::IORING_ACCEPT_POLL_FIRST.0;
        if iou & !(multishot | dontwait | poll_first) != 0 {
            return Err(Errno::EINVAL);
        }
        // No direct descriptors.
        if sqe.splice_fd_in != 0 {
            return Err(Errno::EINVAL);
        }
        let file = self.req_file(caller, ctx, req)?;
        if iou & poll_first != 0 && !req.retried {
            req.retried = true;
            return Ok(Outcome::Again(PollMask::READABLE));
        }
        let at = (sqe.addr as u32, sqe.off as u32);
        let accepted = self
            .uring_accept(caller, file, at, SocketFlags(sqe.op_flags), true)
            .await;
        match accepted {
            Err(SysErr::Errno(Errno::EAGAIN)) if iou & dontwait == 0 => {
                Ok(Outcome::Again(PollMask::READABLE))
            }
            Ok(fd) if iou & multishot != 0 => Ok(Outcome::More {
                res: fd.0,
                flags: 0,
            }),
            accepted => Ok(Outcome::of(accepted.map(|fd| fd.0))),
        }
    }

    /// `io_connect`: a connection under way is waited for by polling for `EPOLLOUT` and
    /// asking again, which then finds it made (`EISCONN`).
    async fn op_connect(
        &self,
        caller: &Caller<'_>,
        ctx: &Ctx,
        req: &mut Req,
    ) -> Result<Outcome, Errno> {
        let sqe = req.sqe;
        if sqe.len != 0 || sqe.buf_index != 0 || sqe.op_flags != 0 || sqe.splice_fd_in != 0 {
            return Err(Errno::EINVAL);
        }
        let file = self.req_file(caller, ctx, req)?;
        let addr = (sqe.addr as u32, sqe.off as u32);
        let connected = self.uring_connect(caller, file, addr, true).await;
        match connected {
            Err(SysErr::Errno(Errno::EINPROGRESS | Errno::EAGAIN)) => {
                req.connecting = true;
                Ok(Outcome::Again(PollMask::WRITABLE))
            }
            Err(SysErr::Errno(Errno::EALREADY)) if req.connecting => {
                Ok(Outcome::Again(PollMask::WRITABLE))
            }
            Err(SysErr::Errno(Errno::EISCONN)) if req.connecting => Ok(Outcome::of(Ok(0))),
            connected => Ok(Outcome::of(connected)),
        }
    }

    /// `io_openat` and `io_openat2`: `openat` in the caller's descriptor table. No direct
    /// descriptors.
    async fn op_openat(&self, caller: &Caller<'_>, req: &Req) -> Result<Outcome, Errno> {
        let sqe = req.sqe;
        if sqe.buf_index != 0 || sqe.splice_fd_in != 0 {
            return Err(Errno::EINVAL);
        }
        if req.has(IosqeFlags::IOSQE_FIXED_FILE) {
            return Err(Errno::EBADF);
        }
        let path = GuestPtr::new(sqe.addr as u32);
        let opened = match IoUringOp(sqe.opcode) {
            IoUringOp::IORING_OP_OPENAT => {
                let flags = OpenFlags(sqe.op_flags) | OpenFlags::O_LARGEFILE;
                self.sys_openat(caller, Fd(sqe.fd), path, flags, sqe.len)
                    .await
            }
            _ => {
                let how = GuestPtr::new(sqe.off as u32);
                self.sys_openat2(caller, Fd(sqe.fd), path, how, sqe.len)
                    .await
            }
        };
        Ok(Outcome::of(opened.map(|fd| fd.0)))
    }

    /// `io_close`: closes a descriptor of the caller, or empties a registered file's slot
    /// (`file_index`). A ring cannot be closed this way (`EBADF`).
    fn op_close(&self, caller: &Caller<'_>, ctx: &Ctx, req: &Req) -> Result<Outcome, Errno> {
        let sqe = req.sqe;
        if sqe.off != 0 || sqe.addr != 0 || sqe.len != 0 || sqe.op_flags != 0 || sqe.buf_index != 0
        {
            return Err(Errno::EINVAL);
        }
        if req.has(IosqeFlags::IOSQE_FIXED_FILE) {
            return Err(Errno::EBADF);
        }
        let slot = sqe.splice_fd_in as u32;
        if slot != 0 {
            if sqe.fd != 0 {
                return Err(Errno::EINVAL);
            }
            let taken = {
                let mut state = ctx.state.lock_or_recover();
                let files = state.files.as_mut().ok_or(Errno::ENXIO)?;
                let entry = files.get_mut(slot as usize - 1).ok_or(Errno::EINVAL)?;
                entry.take().ok_or(Errno::EBADF)?
            };
            drop(taken);
            return Ok(Outcome::of(Ok(0)));
        }
        let file = self.file(caller.task(), Fd(sqe.fd))?;
        if is_ring(&file) {
            return Err(Errno::EBADF);
        }
        drop(file);
        Ok(Outcome::of(self.sys_close(caller, Fd(sqe.fd))))
    }

    /// `io_poll_add`: completes at once if the file is ready; else waits, once or, with
    /// `IORING_POLL_ADD_MULTI`, posting each time it becomes ready.
    fn op_poll_add(
        &self,
        caller: &Caller<'_>,
        ctx: &Arc<Ctx>,
        req: &mut Req,
    ) -> Result<Outcome, Errno> {
        let sqe = req.sqe;
        if sqe.buf_index != 0 || sqe.off != 0 || sqe.addr != 0 {
            return Err(Errno::EINVAL);
        }
        let multi = IoringOpFlags::IORING_POLL_ADD_MULTI.0;
        let level = IoringOpFlags::IORING_POLL_ADD_LEVEL.0;
        if sqe.len & !(multi | level) != 0 {
            return Err(Errno::EINVAL);
        }
        let multishot = sqe.len & multi != 0;
        if multishot && req.has(IosqeFlags::IOSQE_CQE_SKIP_SUCCESS) {
            return Err(Errno::EINVAL);
        }
        let file = self.req_file(caller, ctx, req)?;
        // `io_poll_parse_events`: the POLL bits are the EPOLL ones here.
        let events = PollMask(sqe.op_flags & 0xffff) | PollMask::ERR | PollMask::HUP;
        let (mask, registrations) = register(ctx, &file, req.id, events);
        if !mask.is_empty() && !multishot {
            return Ok(Outcome::Done {
                res: mask.0 as i32,
                flags: 0,
                fail: false,
            });
        }
        if !mask.is_empty() {
            let more = IoringCqeFlags::IORING_CQE_F_MORE.0;
            ctx.post(sqe.user_data, mask.0 as i32, more, false);
        }
        let req = std::mem::replace(req, req.taken());
        Ok(Outcome::Wait(Box::new(Waiting::Poll {
            req,
            events,
            multishot,
            _registrations: registrations,
        })))
    }

    /// `io_timeout`: a timer, and with a count, a number of completions to wait for.
    async fn op_timeout(
        &self,
        caller: &Caller<'_>,
        ctx: &Arc<Ctx>,
        req: &mut Req,
    ) -> Result<Outcome, Errno> {
        let sqe = req.sqe;
        let at = self.timeout_prep(caller, &sqe, false).await?;
        let multishot = sqe.op_flags & IoringOpFlags::IORING_TIMEOUT_MULTISHOT.0 != 0;
        let target = match (sqe.off, multishot) {
            (0, _) | (_, true) => None,
            (count, false) => Some(ctx.state.lock_or_recover().seq + count),
        };
        let repeats = multishot.then_some(sqe.off as u32);
        // A multishot timeout's period, kept where the SQE has room.
        req.sqe.addr3 = at.interval.as_nanos() as u64;
        let id = req.id;
        let weak = ctx.me.clone();
        let timer = ctx.timers().timer(Box::new(move || {
            weak.upgrade().and_then(|ctx| timeout_fired(&ctx, id))
        }));
        timer.start(at.deadline);
        let req = std::mem::replace(req, req.taken());
        Ok(Outcome::Wait(Box::new(Waiting::Timeout {
            req,
            _timer: timer,
            target,
            repeats,
        })))
    }

    /// `io_timeout_prep`: the timeout's monotonic deadline and its relative length.
    async fn timeout_prep(
        &self,
        caller: &Caller<'_>,
        sqe: &IoUringSqe,
        link: bool,
    ) -> Result<Deadline, Errno> {
        if sqe.buf_index != 0 || sqe.len != 1 || sqe.splice_fd_in != 0 {
            return Err(Errno::EINVAL);
        }
        let flags = sqe.op_flags;
        let known = IoringOpFlags::IORING_TIMEOUT_ABS.0
            | IoringOpFlags::IORING_TIMEOUT_CLOCK_MASK.0
            | IoringOpFlags::IORING_TIMEOUT_ETIME_SUCCESS.0
            | IoringOpFlags::IORING_TIMEOUT_MULTISHOT.0
            | IoringOpFlags::IORING_TIMEOUT_IMMEDIATE_ARG.0;
        if flags & !known != 0 {
            return Err(Errno::EINVAL);
        }
        let clock = flags & IoringOpFlags::IORING_TIMEOUT_CLOCK_MASK.0;
        if clock.count_ones() > 1 {
            return Err(Errno::EINVAL);
        }
        let abs = flags & IoringOpFlags::IORING_TIMEOUT_ABS.0 != 0;
        let multishot = flags & IoringOpFlags::IORING_TIMEOUT_MULTISHOT.0 != 0;
        if (multishot && (abs || link)) || (link && sqe.off != 0) {
            return Err(Errno::EINVAL);
        }
        let ts = match flags & IoringOpFlags::IORING_TIMEOUT_IMMEDIATE_ARG.0 != 0 {
            true => Duration::from_nanos(sqe.addr),
            false => {
                let addr = u32::try_from(sqe.addr).map_err(|_| Errno::EFAULT)?;
                let ts: KernelTimespec = read_value(caller.memory(), GuestPtr::new(addr))
                    .await
                    .map_err(|_| Errno::EFAULT)?;
                crate::time::from_timespec(ts)?
            }
        };
        Ok(self.timeout_deadline(caller, ts, abs, clock))
    }

    /// The monotonic deadline of a timeout of `ts` on the clock `clock` names (of
    /// `IORING_TIMEOUT_CLOCK_MASK`), absolute on the realtime clock or, for `CLOCK_BOOTTIME`,
    /// the monotonic one, which never sleeps here. An absolute monotonic or boot time is in
    /// the caller's time namespace (`io_parse_user_time`'s `timens_ktime_to_host`).
    fn timeout_deadline(
        &self,
        caller: &Caller<'_>,
        ts: Duration,
        abs: bool,
        clock: u32,
    ) -> Deadline {
        let realtime = clock == IoringOpFlags::IORING_TIMEOUT_REALTIME.0;
        let ts = match (abs, realtime) {
            (true, false) => {
                let on = match clock == IoringOpFlags::IORING_TIMEOUT_BOOTTIME.0 {
                    true => crate::time::Clock::Boottime,
                    false => crate::time::Clock::Monotonic,
                };
                self.timens_to_host(caller, on, ts)
            }
            _ => ts,
        };
        let now = self.host.monotonic();
        let deadline = match (abs, realtime) {
            (false, _) => now.saturating_add(ts),
            (true, false) => ts,
            (true, true) => now.saturating_add(ts.saturating_sub(self.host.realtime())),
        };
        Deadline {
            deadline,
            interval: ts,
        }
    }

    /// `io_timeout_remove`: cancels a timeout (its CQE says `ECANCELED`), or with
    /// `IORING_TIMEOUT_UPDATE` re-arms it; `ENOENT` if none matches.
    async fn op_timeout_remove(
        &self,
        caller: &Caller<'_>,
        ctx: &Arc<Ctx>,
        sqe: &IoUringSqe,
    ) -> Result<Outcome, Errno> {
        if sqe.buf_index != 0 || sqe.len != 0 || sqe.splice_fd_in != 0 {
            return Err(Errno::EINVAL);
        }
        let flags = sqe.op_flags;
        let update_mask = IoringOpFlags::IORING_TIMEOUT_UPDATE_MASK.0;
        let target = sqe.addr;
        if flags & update_mask == 0 {
            if flags != 0 {
                return Err(Errno::EINVAL);
            }
            let found = {
                let mut state = ctx.state.lock_or_recover();
                let id = state.waiting.iter().find_map(|(&id, w)| {
                    let timeout = matches!(w, Waiting::Timeout { .. });
                    (timeout && w.req().sqe.user_data == target).then_some(id)
                });
                id.and_then(|id| state.waiting.remove(&id))
            };
            let Some(waiting) = found else {
                return Ok(Outcome::of(Err(Errno::ENOENT.into())));
            };
            ctx.finish_waiting(waiting, -Errno::ECANCELED.0);
            return Ok(Outcome::of(Ok(0)));
        }
        if (flags & update_mask).count_ones() > 1 {
            return Err(Errno::EINVAL);
        }
        if flags & IoringOpFlags::IORING_LINK_TIMEOUT_UPDATE.0 != 0 {
            return Ok(Outcome::of(Err(Errno::ENOENT.into())));
        }
        let addr = u32::try_from(sqe.off).map_err(|_| Errno::EFAULT)?;
        let ts: KernelTimespec = read_value(caller.memory(), GuestPtr::new(addr))
            .await
            .map_err(|_| Errno::EFAULT)?;
        let ts = crate::time::from_timespec(ts)?;
        let abs = flags & IoringOpFlags::IORING_TIMEOUT_ABS.0 != 0;
        let clock = flags & IoringOpFlags::IORING_TIMEOUT_CLOCK_MASK.0;
        let at = self.timeout_deadline(caller, ts, abs, clock);
        let state = ctx.state.lock_or_recover();
        let timer = state.waiting.values().find_map(|w| match w {
            Waiting::Timeout { req, _timer, .. } if req.sqe.user_data == target => Some(_timer),
            _ => None,
        });
        let Some(timer) = timer else {
            return Ok(Outcome::of(Err(Errno::ENOENT.into())));
        };
        timer.start(at.deadline);
        Ok(Outcome::of(Ok(0)))
    }

    /// `io_async_cancel`: cancels the waiting requests that match, by `user_data`, descriptor,
    /// opcode or any; 0 or, with `IORING_ASYNC_CANCEL_ALL`, how many, else `ENOENT`.
    fn op_async_cancel(
        &self,
        caller: &Caller<'_>,
        ctx: &Arc<Ctx>,
        sqe: &IoUringSqe,
    ) -> Result<Outcome, Errno> {
        if sqe.off != 0 || sqe.splice_fd_in != 0 {
            return Err(Errno::EINVAL);
        }
        let flags = sqe.op_flags;
        let all = IoringOpFlags::IORING_ASYNC_CANCEL_ALL.0;
        let by_fd = IoringOpFlags::IORING_ASYNC_CANCEL_FD.0;
        let any = IoringOpFlags::IORING_ASYNC_CANCEL_ANY.0;
        let fd_fixed = IoringOpFlags::IORING_ASYNC_CANCEL_FD_FIXED.0;
        let userdata = IoringOpFlags::IORING_ASYNC_CANCEL_USERDATA.0;
        let by_op = IoringOpFlags::IORING_ASYNC_CANCEL_OP.0;
        if flags & !(all | by_fd | any | fd_fixed | userdata | by_op) != 0 {
            return Err(Errno::EINVAL);
        }
        if flags & any != 0 && flags & (by_fd | by_op) != 0 {
            return Err(Errno::EINVAL);
        }
        let file = match flags & by_fd != 0 && flags & fd_fixed == 0 {
            true => Some(self.file(caller.task(), Fd(sqe.fd))?),
            false => None,
        };
        let matches = |req: &Req| -> bool {
            if flags & any != 0 {
                return true;
            }
            let mut matched = true;
            if flags & by_fd != 0 {
                matched &= match &file {
                    Some(file) => req.file.as_ref().is_some_and(|f| Arc::ptr_eq(f, file)),
                    None => req.has(IosqeFlags::IOSQE_FIXED_FILE) && req.sqe.fd == sqe.fd,
                };
            }
            if flags & by_op != 0 {
                matched &= u32::from(req.sqe.opcode) == sqe.len;
            }
            if flags & (by_fd | by_op) == 0 || flags & userdata != 0 {
                matched &= req.sqe.user_data == sqe.addr;
            }
            matched
        };
        let cancelled = cancel(ctx, matches, flags & (all | any) != 0);
        let n = cancelled.len() as i32;
        for req in cancelled {
            ctx.finish(req, -Errno::ECANCELED.0, 0, true);
        }
        Ok(Outcome::of(match n {
            0 => Err(Errno::ENOENT.into()),
            n if flags & (all | any) != 0 => Ok(n),
            _ => Ok(0),
        }))
    }
}

/// When a timeout ends, and its relative length.
struct Deadline {
    deadline: Duration,
    interval: Duration,
}

/// `io_init_req`: the opcode, the SQE flags, the personality and buffer selection.
fn init_req(ctx: &Ctx, req: &mut Req) -> Result<(), Errno> {
    let sqe = req.sqe;
    if sqe.opcode >= op(IoUringOp::IORING_OP_LAST) {
        return Err(Errno::EINVAL);
    }
    if sqe.flags & !SQE_VALID_FLAGS != 0 {
        return Err(Errno::EINVAL);
    }
    if sqe.personality != 0 {
        let state = ctx.state.lock_or_recover();
        let creds = state.personalities.get(&sqe.personality);
        req.creds = Some(creds.ok_or(Errno::EINVAL)?.clone());
    }
    let selects = [
        IoUringOp::IORING_OP_READ,
        IoUringOp::IORING_OP_READV,
        IoUringOp::IORING_OP_RECV,
        IoUringOp::IORING_OP_RECVMSG,
    ];
    if req.has(IosqeFlags::IOSQE_BUFFER_SELECT) && !selects.iter().any(|&o| o.0 == sqe.opcode) {
        return Err(Errno::EOPNOTSUPP);
    }
    if !SUPPORTED.iter().any(|o| o.0 == sqe.opcode) {
        return Err(Errno::EOPNOTSUPP);
    }
    Ok(())
}

/// How many requests are linked after `req`, its link timeouts included.
fn chain_len(req: &Req) -> u32 {
    let lt = |r: &Req| u32::from(r.link_timeout.is_some());
    lt(req) + req.link.iter().map(|r| 1 + lt(r)).sum::<u32>()
}

/// Fails every request of a chain whose submission failed (`io_req_defer_failed`): each with
/// its own error, or `ECANCELED`.
fn fail_chain(ctx: &Ctx, mut req: Req, errno: Errno) {
    let mut link = std::mem::take(&mut req.link);
    if let Some(lt) = req.link_timeout.take() {
        ctx.finish(*lt.req, -Errno::ECANCELED.0, 0, true);
    }
    ctx.finish(req, -errno.0, 0, true);
    while let Some(mut next) = link.pop_front() {
        let errno = next.failed.unwrap_or(Errno::ECANCELED);
        if let Some(lt) = next.link_timeout.take() {
            ctx.finish(*lt.req, -Errno::ECANCELED.0, 0, true);
        }
        ctx.finish(next, -errno.0, 0, true);
    }
}

/// Arms `req` on its file's queues for `events` (`io_arm_poll_handler`), and its link timeout.
/// A file that became ready meanwhile makes it task work at once; one that cannot wait fails
/// it with `EAGAIN`.
fn arm(ctx: &Arc<Ctx>, mut req: Req, events: PollMask) {
    let Some(file) = req.file.clone() else {
        ctx.finish(req, -Errno::EAGAIN.0, 0, true);
        return;
    };
    arm_link_timeout(ctx, &mut req);
    let id = req.id;
    // In the table before its entries are on the queues, so no wakeup misses it.
    ctx.state.lock_or_recover().waiting.insert(
        id,
        Waiting::Armed {
            req,
            registrations: Vec::new(),
        },
    );
    let events = events | PollMask::ERR | PollMask::HUP;
    let (mask, registrations) = register(ctx, &file, id, events);
    let ready = !mask.is_empty();
    let unwaitable = registrations.is_empty();
    let taken = {
        let mut state = ctx.state.lock_or_recover();
        match state.waiting.get_mut(&id) {
            Some(Waiting::Armed {
                registrations: kept,
                ..
            }) if !ready && !unwaitable => {
                *kept = registrations;
                return;
            }
            Some(_) => state.waiting.remove(&id),
            // A wakeup made it ready already, or it was cancelled.
            None => None,
        }
    };
    let Some(Waiting::Armed { mut req, .. }) = taken else {
        return;
    };
    match ready {
        true => {
            req.retried = true;
            ctx.queue_ready(req);
        }
        false => ctx.finish(req, -Errno::EAGAIN.0, 0, true),
    }
}

/// Keeps a request that waits for its wakeup or timer, with its link timeout armed.
fn wait(ctx: &Arc<Ctx>, mut waiting: Waiting) {
    let req = match &mut waiting {
        Waiting::Armed { req, .. } | Waiting::Poll { req, .. } | Waiting::Timeout { req, .. } => {
            req
        }
    };
    let id = req.id;
    arm_link_timeout(ctx, req);
    let poll = matches!(waiting, Waiting::Poll { .. });
    let fired = {
        let mut state = ctx.state.lock_or_recover();
        state.waiting.insert(id, waiting);
        // A count already reached.
        expired_timeouts(&mut state)
    };
    for waiting in fired {
        ctx.finish_waiting(waiting, 0);
    }
    // A wakeup before the poll was in the table found nothing: look again.
    if poll {
        ctx.woken(id);
    }
}

/// `io_queue_linked_timeout`: starts the link timeout of a request that goes on waiting.
fn arm_link_timeout(ctx: &Arc<Ctx>, req: &mut Req) {
    let Some(lt) = &mut req.link_timeout else {
        return;
    };
    if lt.timer.is_some() {
        return;
    }
    let sqe = lt.req.sqe;
    let ts = Duration::from_nanos(sqe.addr3);
    let abs = sqe.op_flags & IoringOpFlags::IORING_TIMEOUT_ABS.0 != 0;
    let deadline = match abs {
        true => ts,
        false => ctx.host().monotonic().saturating_add(ts),
    };
    let guarded = req.id;
    let weak = ctx.me.clone();
    let timer = ctx.timers().timer(Box::new(move || {
        if let Some(ctx) = weak.upgrade() {
            link_timeout_fired(&ctx, guarded);
        }
        None
    }));
    timer.start(deadline);
    lt.timer = Some(timer);
}

/// Takes the waiting, ready and drained requests that match, every one or the first.
pub(super) fn cancel(ctx: &Ctx, matches: impl Fn(&Req) -> bool, all: bool) -> Vec<Req> {
    let mut waiting = Vec::new();
    let mut cancelled = Vec::new();
    let mut state = ctx.state.lock_or_recover();
    let ids: Vec<u64> = state
        .waiting
        .iter()
        .filter(|(_, w)| matches(w.req()))
        .map(|(&id, _)| id)
        .collect();
    for id in ids {
        if !all && !waiting.is_empty() {
            break;
        }
        waiting.extend(state.waiting.remove(&id));
    }
    let queues: &mut State = &mut state;
    for queue in [&mut queues.ready, &mut queues.drain] {
        let mut at = 0;
        while at < queue.len() && (all || (cancelled.is_empty() && waiting.is_empty())) {
            if matches(&queue[at]) {
                cancelled.extend(queue.remove(at));
            } else {
                at += 1;
            }
        }
    }
    drop(state);
    // Entries leave their queues with the state unlocked: a wake running them takes it.
    waiting
        .into_iter()
        .map(Waiting::into_req)
        .chain(cancelled)
        .collect()
}

/// `io_poll_remove`: cancels a poll (its CQE says `ECANCELED`) or updates its events and
/// `user_data`; `ENOENT` if none matches.
fn op_poll_remove(ctx: &Arc<Ctx>, sqe: &IoUringSqe) -> Result<Outcome, Errno> {
    if sqe.buf_index != 0 || sqe.splice_fd_in != 0 {
        return Err(Errno::EINVAL);
    }
    let update_events = IoringOpFlags::IORING_POLL_UPDATE_EVENTS.0;
    let update_user_data = IoringOpFlags::IORING_POLL_UPDATE_USER_DATA.0;
    let multi = IoringOpFlags::IORING_POLL_ADD_MULTI.0;
    let flags = sqe.len;
    if flags & !(update_events | update_user_data | multi) != 0 {
        return Err(Errno::EINVAL);
    }
    let target = sqe.addr;
    let poll = |w: &Waiting| matches!(w, Waiting::Poll { .. }) && w.req().sqe.user_data == target;
    if flags & (update_events | update_user_data) == 0 {
        let found = cancel(
            ctx,
            |req| req.sqe.user_data == target && req.opcode() == op(IoUringOp::IORING_OP_POLL_ADD),
            false,
        );
        let Some(req) = found.into_iter().next() else {
            return Ok(Outcome::of(Err(Errno::ENOENT.into())));
        };
        ctx.finish(req, -Errno::ECANCELED.0, 0, true);
        return Ok(Outcome::of(Ok(0)));
    }
    let mut state = ctx.state.lock_or_recover();
    let Some(waiting) = state.waiting.values_mut().find(|w| poll(w)) else {
        return Ok(Outcome::of(Err(Errno::ENOENT.into())));
    };
    if let Waiting::Poll {
        req,
        events,
        multishot,
        ..
    } = waiting
    {
        if flags & update_events != 0 {
            *events = PollMask(sqe.op_flags & 0xffff) | PollMask::ERR | PollMask::HUP;
            *multishot = flags & multi != 0;
        }
        if flags & update_user_data != 0 {
            req.sqe.user_data = sqe.off;
        }
    }
    Ok(Outcome::of(Ok(0)))
}

/// `io_provide_buffers`: `nbufs` buffers of `len` bytes from `addr`, with ids from `off`, to
/// group `buf_group`.
fn op_provide_buffers(ctx: &Ctx, sqe: &IoUringSqe) -> Result<Outcome, Errno> {
    if sqe.op_flags != 0 || sqe.splice_fd_in != 0 {
        return Err(Errno::EINVAL);
    }
    let nbufs = sqe.fd as u32;
    if nbufs == 0 || nbufs > MAX_BIDS_PER_BGID {
        return Err(Errno::E2BIG);
    }
    if sqe.len == 0 {
        return Err(Errno::EINVAL);
    }
    let size = u64::from(sqe.len) * u64::from(nbufs);
    let end = sqe.addr.checked_add(size).ok_or(Errno::EOVERFLOW)?;
    if end > 1 << 32 {
        return Err(Errno::EFAULT);
    }
    if sqe.off > u64::from(u16::MAX) {
        return Err(Errno::E2BIG);
    }
    if sqe.off + u64::from(nbufs) > u64::from(MAX_BIDS_PER_BGID) {
        return Err(Errno::EINVAL);
    }
    let mut state = ctx.state.lock_or_recover();
    let group = state.pbufs.entry(sqe.buf_index).or_default();
    let mut added = 0;
    for i in 0..nbufs {
        if group.len() as u32 == MAX_BIDS_PER_BGID {
            break;
        }
        group.push_back(Pbuf {
            addr: (sqe.addr + u64::from(i) * u64::from(sqe.len)) as u32,
            len: sqe.len.min(crate::MAX_RW_COUNT as u32),
            bid: (sqe.off + u64::from(i)) as u16,
        });
        added += 1;
    }
    Ok(Outcome::of(match added {
        0 => Err(Errno::EOVERFLOW.into()),
        _ => Ok(0),
    }))
}

/// `io_remove_buffers`: up to `nbufs` buffers of group `buf_group`; how many went.
fn op_remove_buffers(ctx: &Ctx, sqe: &IoUringSqe) -> Result<Outcome, Errno> {
    if sqe.op_flags != 0 || sqe.addr != 0 || sqe.len != 0 || sqe.off != 0 || sqe.splice_fd_in != 0 {
        return Err(Errno::EINVAL);
    }
    let nbufs = sqe.fd as u32;
    if nbufs == 0 || nbufs > MAX_BIDS_PER_BGID {
        return Err(Errno::EINVAL);
    }
    let mut state = ctx.state.lock_or_recover();
    let Some(group) = state.pbufs.get_mut(&sqe.buf_index) else {
        return Ok(Outcome::of(Err(Errno::ENOENT.into())));
    };
    let n = (nbufs as usize).min(group.len());
    group.drain(..n);
    Ok(Outcome::of(Ok(n as i32)))
}

/// `io_buffer_select`: the first buffer of group `bgid`, cut to `len` if that is less;
/// `ENOBUFS` if the group has none.
fn select_buffer(ctx: &Ctx, bgid: u16, len: u32) -> Result<Pbuf, Errno> {
    let mut state = ctx.state.lock_or_recover();
    let group = state.pbufs.get_mut(&bgid).ok_or(Errno::ENOBUFS)?;
    let mut pbuf = group.pop_front().ok_or(Errno::ENOBUFS)?;
    if len != 0 && len < pbuf.len {
        pbuf.len = len;
    }
    Ok(pbuf)
}

/// The CQE flags of a completion that used a provided buffer.
fn buffer_flags(pbuf: Pbuf) -> u32 {
    IoringCqeFlags::IORING_CQE_F_BUFFER.0 | (u32::from(pbuf.bid) << 16)
}

/// `io_import_fixed`: the range `[addr, addr + len)` must lie in registered buffer
/// `buf_index`.
fn fixed_buffer(ctx: &Ctx, sqe: &IoUringSqe) -> Result<Iovec, Errno> {
    let state = ctx.state.lock_or_recover();
    let buffers = state.buffers.as_ref().ok_or(Errno::EFAULT)?;
    let &(base, size) = buffers
        .get(usize::from(sqe.buf_index))
        .ok_or(Errno::EFAULT)?;
    let end = sqe
        .addr
        .checked_add(u64::from(sqe.len))
        .ok_or(Errno::EFAULT)?;
    if sqe.addr < u64::from(base) || end > u64::from(base) + u64::from(size) {
        return Err(Errno::EFAULT);
    }
    Ok(Iovec {
        iov_base: sqe.addr as u32,
        iov_len: sqe.len,
    })
}
