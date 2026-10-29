//! io_uring (`io_uring/*.c`): `io_uring_setup`, `io_uring_enter` and `io_uring_register`.
//!
//! **Rings.** A ring is a kernel object behind an anonymous file. Its submission and completion
//! rings (`struct io_rings` with the CQE array, then the SQ index array) are one region of
//! shared pages of the host's, and the SQEs another; `mmap` of the file at `IORING_OFF_SQ_RING`
//! or `IORING_OFF_CQ_RING` maps the first (`IORING_FEAT_SINGLE_MMAP`), at `IORING_OFF_SQES` the
//! second, through the address space as every mapped object is ([`FileObject::mmap_pages`]).
//! The kernel reads and writes the rings through the pages from any thread, so a completion can
//! be posted from a wakeup or a timer as well as from a call.
//!
//! **Requests.** `io_uring_enter` submits SQEs and issues each request at once in the caller,
//! as Linux's inline issue does, but never lets it block (`IO_URING_F_NONBLOCK`): a read or
//! write asks for `RWF_NOWAIT`, a socket call for `MSG_DONTWAIT`, `accept` and `connect` go
//! without waiting. A request on a file that would block is armed on the file's wait queues
//! instead (`io_arm_poll_handler`), and a wakeup turns it into task work: it is issued again
//! in a call on the same memory, as its buffers are in that memory. Regular files never block,
//! so their requests complete inline. Polls, timeouts and link timeouts need no memory and
//! complete from the wakeup or the timer that ends them. There is no thread pool
//! (`io-wq`): `IOSQE_ASYNC` arms a pollable file's request at once instead of trying it first.
//!
//! **Task work.** Linux runs a ring's task work when the submitter returns to userspace, or,
//! with `IORING_SETUP_DEFER_TASKRUN`, only in `io_uring_enter`. Calls here cannot run work on
//! the way back to the guest, so every ring runs it as a deferred one does: in `io_uring_enter`,
//! on its way in and while it waits for completions. While work waits, the ring polls readable
//! (`io_has_work`) and the SQ ring's flags say `IORING_SQ_TASKRUN`, which tells liburing to
//! enter the kernel even to peek.
//!
//! **Completions** go to the CQ ring, or, while it is full, to an overflow list that is flushed
//! as the application makes room (`IORING_FEAT_NODROP`, with `IORING_SQ_CQ_OVERFLOW` set in
//! the meantime). Each one signals a registered eventfd unless the CQ ring's flags say
//! `IORING_CQ_EVENTFD_DISABLED`, and wakes `io_uring_enter` waiters and pollers of the ring.
//!
//! Not supported: `IORING_SETUP_SQPOLL` and `IORING_SETUP_IOPOLL` (`EINVAL`), rings in user
//! memory (`IORING_SETUP_NO_MMAP`), provided buffer rings, and the operations the probe
//! leaves out.

mod kbuf;
mod ops;
mod register;
mod rsrc;
mod tctx;

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

use wasixv2_abi::{
    ConstSigsetPtr, GuestLayout, GuestPtr, IoCqringOffsets, IoSqringOffsets, IoUringCqe,
    IoUringGeteventsArg, IoUringParams, IoUringParamsPtr, IoUringSqe, IoringCqFlags,
    IoringEnterFlags, IoringFeatFlags, IoringSetupFlags, IoringSqFlags, KernelTimespec, OpenFlags,
    VoidPtr,
};
use wasixv2_host::Host;
use wasixv2_pages::SharedPages;
use wasixv2_syscalls::{
    Caller, Errno, Fd, LocalFuture, SysErr, SysResult, TaskId, read_value, write_value,
};
use wasixv2_wait::{
    PollMask, PollTable, Pollable, WaitQueue, WaitRegistration, poll_wait, wait_event,
    wait_event_timeout,
};

use crate::{
    Kernel,
    cred::Cred,
    eventfd::EventFd,
    file::{Backing, FileObject, Io, MappedPages, OpenFile},
    mm::PAGE_SIZE,
    sync::MutexExt,
    timer::{Timer, Timers},
};

pub(crate) use ops::Req;
pub(crate) use tctx::{Cancel, Tctx};

/// `IORING_MAX_ENTRIES` and `IORING_MAX_CQ_ENTRIES`.
const MAX_ENTRIES: u32 = 32768;
const MAX_CQ_ENTRIES: u32 = 2 * MAX_ENTRIES;
/// The setup flags a ring here takes (`IORING_SETUP_FLAGS` minus polling, rings in user
/// memory and what builds on them).
const SETUP_FLAGS: u32 = IoringSetupFlags::IORING_SETUP_CQSIZE.0
    | IoringSetupFlags::IORING_SETUP_CLAMP.0
    | IoringSetupFlags::IORING_SETUP_ATTACH_WQ.0
    | IoringSetupFlags::IORING_SETUP_R_DISABLED.0
    | IoringSetupFlags::IORING_SETUP_SUBMIT_ALL.0
    | IoringSetupFlags::IORING_SETUP_COOP_TASKRUN.0
    | IoringSetupFlags::IORING_SETUP_TASKRUN_FLAG.0
    | IoringSetupFlags::IORING_SETUP_SQE128.0
    | IoringSetupFlags::IORING_SETUP_CQE32.0
    | IoringSetupFlags::IORING_SETUP_SINGLE_ISSUER.0
    | IoringSetupFlags::IORING_SETUP_DEFER_TASKRUN.0
    | IoringSetupFlags::IORING_SETUP_NO_SQARRAY.0;
/// `IORING_ENTER_FLAGS`.
const ENTER_FLAGS: u32 = 0xff;
/// What a ring here does of `IORING_FEAT_*`.
const FEATURES: u32 = IoringFeatFlags::IORING_FEAT_SINGLE_MMAP.0
    | IoringFeatFlags::IORING_FEAT_NODROP.0
    | IoringFeatFlags::IORING_FEAT_SUBMIT_STABLE.0
    | IoringFeatFlags::IORING_FEAT_RW_CUR_POS.0
    | IoringFeatFlags::IORING_FEAT_CUR_PERSONALITY.0
    | IoringFeatFlags::IORING_FEAT_FAST_POLL.0
    | IoringFeatFlags::IORING_FEAT_POLL_32BITS.0
    | IoringFeatFlags::IORING_FEAT_EXT_ARG.0
    | IoringFeatFlags::IORING_FEAT_NATIVE_WORKERS.0
    | IoringFeatFlags::IORING_FEAT_CQE_SKIP.0
    | IoringFeatFlags::IORING_FEAT_LINKED_FILE.0
    | IoringFeatFlags::IORING_FEAT_NO_IOWAIT.0;

/// Offsets in the rings region (`struct io_rings`).
const SQ_HEAD: u64 = 0;
const SQ_TAIL: u64 = 4;
const CQ_HEAD: u64 = 8;
const CQ_TAIL: u64 = 12;
const SQ_RING_MASK: u64 = 16;
const CQ_RING_MASK: u64 = 20;
const SQ_RING_ENTRIES: u64 = 24;
const CQ_RING_ENTRIES: u64 = 28;
const SQ_DROPPED: u64 = 32;
const SQ_FLAGS: u64 = 36;
const CQ_FLAGS: u64 = 40;
const CQ_OVERFLOW: u64 = 44;
/// The CQEs start a cache line in (`____cacheline_aligned_in_smp`).
const CQES: u64 = 64;

/// The fixed shape of a ring.
struct Shape {
    setup: u32,
    sq_entries: u32,
    cq_entries: u32,
    sqe_size: u64,
    cqe_size: u64,
    /// Where the SQ index array is, unless `IORING_SETUP_NO_SQARRAY`.
    sq_array: Option<u64>,
    rings_size: u64,
    sqes_size: u64,
}

impl Shape {
    /// `io_uring_fill_params`'s sizes and `rings_size`.
    fn new(setup: u32, sq_entries: u32, cq_entries: u32) -> Self {
        let has = |flag: IoringSetupFlags| setup & flag.0 != 0;
        let cqe_size = if has(IoringSetupFlags::IORING_SETUP_CQE32) {
            32
        } else {
            16
        };
        let sqe_size = if has(IoringSetupFlags::IORING_SETUP_SQE128) {
            128
        } else {
            64
        };
        let cqes_end = CQES + u64::from(cq_entries) * cqe_size;
        let (sq_array, rings_end) = match has(IoringSetupFlags::IORING_SETUP_NO_SQARRAY) {
            true => (None, cqes_end),
            false => {
                let array = cqes_end.next_multiple_of(64);
                (Some(array), array + u64::from(sq_entries) * 4)
            }
        };
        Self {
            setup,
            sq_entries,
            cq_entries,
            sqe_size,
            cqe_size,
            sq_array,
            rings_size: rings_end.next_multiple_of(PAGE_SIZE),
            sqes_size: (u64::from(sq_entries) * sqe_size).next_multiple_of(PAGE_SIZE),
        }
    }

    fn has(&self, flag: IoringSetupFlags) -> bool {
        self.setup & flag.0 != 0
    }
}

/// A completion waiting for room on the CQ ring.
#[derive(Clone, Copy, Debug)]
struct Cqe {
    user_data: u64,
    res: i32,
    flags: u32,
}

/// A ring (`struct io_ring_ctx`).
pub(crate) struct Ctx {
    me: Weak<Ctx>,
    shape: Shape,
    rings: Arc<dyn SharedPages>,
    sqes: Arc<dyn SharedPages>,
    state: Mutex<State>,
    /// Held while SQEs are taken and requests issued (`uring_lock`).
    submit: futures_util::lock::Mutex<()>,
    /// Woken by every completion and by new task work (`cq_wait`).
    cq_wait: WaitQueue,
    /// The ring file's readiness (`poll_wq`).
    poll_wq: WaitQueue,
    timers: Timers,
    host: Arc<dyn Host>,
    /// Fires at once when armed, to handle the wakeups of armed requests and polls with
    /// nothing locked (Linux runs them as task work).
    deferred: Timer,
}

#[derive(Default)]
struct State {
    /// `IORING_SETUP_R_DISABLED` until `IORING_REGISTER_ENABLE_RINGS`.
    disabled: bool,
    /// The SQ head as the kernel has it (`cached_sq_head`).
    sq_head: u32,
    sq_dropped: u32,
    /// The CQ tail as the kernel has it (`cached_cq_tail`).
    cq_tail: u32,
    /// Completions posted that were not timeouts' own, which `IORING_OP_TIMEOUT`'s count
    /// counts (`cached_cq_tail - cq_timeouts`).
    seq: u64,
    overflow: VecDeque<Cqe>,
    last_id: u64,
    /// Requests that wait: armed on a file, polls, timeouts.
    waiting: HashMap<u64, ops::Waiting>,
    /// Requests to issue in a call on their memory (task work).
    ready: VecDeque<Req>,
    /// Waiting requests whose file woke them, for the deferred timer.
    woken: Vec<u64>,
    /// Requests submitted and not completed, for `IOSQE_IO_DRAIN`.
    inflight: u32,
    /// Requests held back by a drain, in submission order.
    drain: VecDeque<Req>,
    /// `IORING_REGISTER_FILES`.
    files: Option<Vec<Option<rsrc::FileNode>>>,
    file_alloc_start: u32,
    file_alloc_end: u32,
    file_alloc_hint: u32,
    /// `IORING_REGISTER_BUFFERS`: address and length of each.
    buffers: Option<Vec<Option<rsrc::BufferNode>>>,
    /// `IORING_OP_PROVIDE_BUFFERS`' groups: address, length and id of each buffer.
    pbufs: HashMap<u16, kbuf::Group>,
    /// `IORING_REGISTER_EVENTFD`, and whether only async completions signal it.
    eventfd: Option<(Arc<OpenFile>, bool)>,
    /// `IORING_REGISTER_PERSONALITY`'s credentials, by id from 1 (`ctx->personalities`),
    /// and the id to try next.
    personalities: std::collections::BTreeMap<u16, Cred>,
    pers_next: u16,
    /// `IORING_SETUP_SINGLE_ISSUER`'s one submitter (`submitter_task`), set by setup or by
    /// enabling a disabled ring.
    submitter: Option<TaskId>,
}

pub(crate) use kbuf::Pbuf;

impl Ctx {
    fn read_word(&self, offset: u64) -> u32 {
        let mut word = [0; 4];
        let _ = self.rings.read_at(&mut word, offset);
        u32::from_le_bytes(word)
    }

    fn write_word(&self, offset: u64, value: u32) {
        let _ = self.rings.write_at(&value.to_le_bytes(), offset);
    }

    /// Events on the CQ ring the application has not consumed.
    fn cq_ready(&self) -> u32 {
        let tail = self.state.lock_or_recover().cq_tail;
        tail.wrapping_sub(self.read_word(CQ_HEAD))
    }

    /// Whether the SQ ring is full (`__io_sqring_full`).
    fn sq_full(&self) -> bool {
        let head = self.state.lock_or_recover().sq_head;
        self.read_word(SQ_TAIL).wrapping_sub(head) == self.shape.sq_entries
    }

    /// Whether task work waits for a call on memory `mm`.
    fn has_work(&self, mm: Option<u64>) -> bool {
        let state = self.state.lock_or_recover();
        let overflow = !state.overflow.is_empty();
        overflow
            || state
                .ready
                .iter()
                .any(|req| mm.is_none_or(|mm| req.mm == mm))
    }

    /// Keeps the SQ ring's flags word in step: `IORING_SQ_CQ_OVERFLOW` while completions wait
    /// for room, `IORING_SQ_TASKRUN` while task work waits.
    fn update_sq_flags(&self, state: &State) {
        let mut flags = 0;
        if !state.overflow.is_empty() {
            flags |= IoringSqFlags::IORING_SQ_CQ_OVERFLOW.0;
        }
        if !state.ready.is_empty() {
            flags |= IoringSqFlags::IORING_SQ_TASKRUN.0;
        }
        self.write_word(SQ_FLAGS, flags);
    }

    /// Writes `cqe` at the tail, if the ring has room.
    fn fill_cqe(&self, state: &mut State, cqe: Cqe) -> bool {
        let head = self.read_word(CQ_HEAD);
        if state.cq_tail.wrapping_sub(head) >= self.shape.cq_entries {
            return false;
        }
        let at = state.cq_tail & (self.shape.cq_entries - 1);
        let mut bytes = vec![0; self.shape.cqe_size as usize];
        let entry = IoUringCqe {
            user_data: cqe.user_data,
            res: cqe.res,
            flags: cqe.flags,
        };
        entry.write_to(&mut bytes);
        let _ = self
            .rings
            .write_at(&bytes, CQES + u64::from(at) * self.shape.cqe_size);
        state.cq_tail = state.cq_tail.wrapping_add(1);
        self.write_word(CQ_TAIL, state.cq_tail);
        true
    }

    /// `__io_cqring_overflow_flush`: moves what the overflow list holds to the ring as far as
    /// it has room. Returns whether anything moved.
    fn flush_overflow(&self) -> bool {
        let moved = {
            let mut state = self.state.lock_or_recover();
            let mut moved = false;
            while let Some(&cqe) = state.overflow.front() {
                if !self.fill_cqe(&mut state, cqe) {
                    break;
                }
                state.overflow.pop_front();
                moved = true;
            }
            if moved {
                self.update_sq_flags(&state);
            }
            moved
        };
        if moved {
            self.notify(true);
        }
        moved
    }

    /// Posts a completion (`io_fill_cqe_req`), behind any that overflowed. A timeout's own
    /// completion does not count towards other timeouts; any other may end some.
    pub(crate) fn post(&self, user_data: u64, res: i32, flags: u32, timeout: bool) {
        let fired = {
            let mut state = self.state.lock_or_recover();
            let cqe = Cqe {
                user_data,
                res,
                flags,
            };
            if !state.overflow.is_empty() || !self.fill_cqe(&mut state, cqe) {
                state.overflow.push_back(cqe);
                self.update_sq_flags(&state);
            }
            if !timeout {
                state.seq += 1;
                ops::expired_timeouts(&mut state)
            } else {
                Vec::new()
            }
        };
        self.notify(true);
        for waiting in fired {
            self.finish_waiting(waiting, 0);
        }
    }

    /// Auxiliary CQEs (MSG_RING) may not use the overflow list.
    fn post_aux(&self, data: u64, res: i32, flags: u32) -> bool {
        let expired = {
            let mut state = self.state.lock_or_recover();
            if !state.overflow.is_empty()
                || !self.fill_cqe(
                    &mut state,
                    Cqe {
                        user_data: data,
                        res,
                        flags,
                    },
                )
            {
                return false;
            }
            state.seq += 1;
            ops::expired_timeouts(&mut state)
        };
        self.notify(true);
        for waiting in expired {
            self.finish_waiting(waiting, 0);
        }
        true
    }

    /// Signals the eventfd, wakes `io_uring_enter` waiters and the ring's pollers.
    fn notify(&self, completed: bool) {
        let eventfd = self.state.lock_or_recover().eventfd.clone();
        let disabled = self.read_word(CQ_FLAGS) & IoringCqFlags::IORING_CQ_EVENTFD_DISABLED.0 != 0;
        if completed
            && !disabled
            && let Some((file, _)) = eventfd
            && let Some(eventfd) = file.downcast::<EventFd>()
        {
            eventfd.signal();
        }
        self.cq_wait.wake(PollMask::READABLE);
        self.poll_wq.wake(PollMask::READABLE);
    }

    /// Queues `req` as task work, to be issued by a call on its memory.
    pub(crate) fn queue_ready(&self, req: Req) {
        {
            let mut state = self.state.lock_or_recover();
            state.ready.push_back(req);
            self.update_sq_flags(&state);
        }
        self.notify(false);
    }

    /// A waiting request's file woke it: the deferred timer looks at it.
    pub(crate) fn woken(&self, id: u64) {
        let first = {
            let mut state = self.state.lock_or_recover();
            state.woken.push(id);
            state.woken.len() == 1
        };
        if first {
            self.deferred.start(Duration::ZERO);
        }
    }

    /// From the timer driver: handles the wakeups of armed requests and polls.
    fn run_woken(&self) {
        let woken = std::mem::take(&mut self.state.lock_or_recover().woken);
        for id in woken {
            ops::handle_wakeup(self, id);
        }
    }

    fn timers(&self) -> &Timers {
        &self.timers
    }

    /// `io_allowed_run_tw`: a deferred ring's task work is its submitter's alone.
    fn allowed_run_tw(&self, task: TaskId) -> bool {
        !self.shape.has(IoringSetupFlags::IORING_SETUP_DEFER_TASKRUN)
            || self.state.lock_or_recover().submitter == Some(task)
    }

    /// `io_uring_try_cancel_requests` for the task `task`: its requests that `how` takes
    /// complete with `ECANCELED`.
    fn cancel_task(&self, task: TaskId, how: Cancel) {
        let cancelled = ops::cancel(self, |req| req.task == task && how.matches(req), true);
        for req in cancelled {
            self.finish(req, -Errno::ECANCELED.0, 0, true);
        }
    }

    fn host(&self) -> &Arc<dyn Host> {
        &self.host
    }
}

/// The ring's open file (`io_uring_fops`).
pub(crate) struct IoRing(Arc<Ctx>);

impl Drop for IoRing {
    /// `io_ring_ctx_wait_and_kill`: the last close cancels what still waits.
    fn drop(&mut self) {
        let (waiting, drained, ready) = {
            let mut state = self.0.state.lock_or_recover();
            let waiting: Vec<_> = state.waiting.drain().map(|(_, w)| w).collect();
            let drained: Vec<_> = state.drain.drain(..).collect();
            let ready: Vec<_> = state.ready.drain(..).collect();
            (waiting, drained, ready)
        };
        drop((waiting, drained, ready));
    }
}

impl Pollable for IoRing {
    /// `io_uring_poll`: writable while the SQ ring has room, readable while completions or task
    /// work wait.
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, &self.0.poll_wq);
        let mut mask = PollMask::empty();
        if !self.0.sq_full() {
            mask |= PollMask::WRITABLE;
        }
        if self.0.cq_ready() > 0 || self.0.has_work(None) {
            mask |= PollMask::READABLE;
        }
        mask
    }
}

impl FileObject for IoRing {
    fn read<'a>(&'a self, _: Io<'a>, _: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async { Err(Errno::EINVAL.into()) })
    }

    fn write<'a>(&'a self, _: Io<'a>, _: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async { Err(Errno::EINVAL.into()) })
    }

    /// `io_uring_mmap` (`io_uring_validate_mmap_request`): the rings at `IORING_OFF_SQ_RING`
    /// and `IORING_OFF_CQ_RING`, the SQEs at `IORING_OFF_SQES`, no further than each reaches.
    fn mmap_pages(&self, offset: u64, len: u64) -> Option<Result<MappedPages, Errno>> {
        const SQ_RING: u64 = 0;
        const CQ_RING: u64 = 0x800_0000;
        const SQES: u64 = 0x1000_0000;
        let ctx = &self.0;
        let (pages, size) = match offset {
            SQ_RING | CQ_RING => (ctx.rings.clone(), ctx.shape.rings_size),
            SQES => (ctx.sqes.clone(), ctx.shape.sqes_size),
            off if off & 0xf800_0000 == 0x8000_0000 => {
                let pages = match kbuf::mmap(ctx, off) {
                    Ok(pages) => pages,
                    Err(err) => return Some(Err(err)),
                };
                let size = pages.size();
                (pages, size)
            }
            _ => return Some(Err(Errno::EINVAL)),
        };
        if len > size {
            return Some(Err(Errno::EINVAL));
        }
        Some(Ok((pages, 0)))
    }

    /// `io_uring_show_fdinfo`.
    fn show_fdinfo(&self) -> String {
        let ctx = &self.0;
        let state = ctx.state.lock_or_recover();
        let sq_tail = ctx.read_word(SQ_TAIL);
        let cq_head = ctx.read_word(CQ_HEAD);
        let mut out = format!(
            "SqMask:\t0x{:x}\nSqHead:\t{}\nSqTail:\t{}\nCachedSqHead:\t{}\n\
             CqMask:\t0x{:x}\nCqHead:\t{}\nCqTail:\t{}\nCachedCqTail:\t{}\n\
             SQEs:\t{}\nCQEs:\t{}\n",
            ctx.shape.sq_entries - 1,
            ctx.read_word(SQ_HEAD),
            sq_tail,
            state.sq_head,
            ctx.shape.cq_entries - 1,
            cq_head,
            ctx.read_word(CQ_TAIL),
            state.cq_tail,
            sq_tail.wrapping_sub(state.sq_head),
            state.cq_tail.wrapping_sub(cq_head),
        );
        let files = state.files.as_ref();
        out += &format!("UserFiles:\t{}\n", files.map_or(0, Vec::len));
        for (i, file) in files.into_iter().flatten().enumerate() {
            if let Some(file) = file {
                let name = String::from_utf8_lossy(&file.item.path().dentry.name()).into_owned();
                out += &format!("{i:5}: {name}\n");
            }
        }
        let buffers = state.buffers.as_ref();
        out += &format!("UserBufs:\t{}\n", buffers.map_or(0, Vec::len));
        for (i, buffer) in buffers.into_iter().flatten().enumerate() {
            if let Some(buffer) = buffer {
                let (addr, len) = (buffer.item.addr, buffer.item.len);
                out += &format!("{i:5}: 0x{addr:x}/{len}\n");
            }
        }
        out += "PollList:\n";
        for waiting in state.waiting.values() {
            if let Some((op, events)) = waiting.poll_info() {
                out += &format!("  op={op}, task_works=0\n  mask=0x{:x}\n", events.0);
            }
        }
        out += "CqOverflowList:\n";
        for cqe in &state.overflow {
            out += &format!(
                "  user_data=0x{:x}, res={}, flags=0x{:x}\n",
                cqe.user_data, cqe.res, cqe.flags
            );
        }
        out
    }
}

/// The ring behind `file`; `EOPNOTSUPP` for any other file (`io_is_uring_fops`).
pub(crate) fn ring_of(file: &OpenFile) -> Result<Arc<Ctx>, Errno> {
    file.downcast::<IoRing>()
        .map(|ring| ring.0.clone())
        .ok_or(Errno::EOPNOTSUPP)
}

/// Whether `file` is a ring, which a ring may not hold (registered files, `IORING_OP_CLOSE`).
pub(crate) fn is_ring(file: &OpenFile) -> bool {
    file.downcast::<IoRing>().is_some()
}

impl Kernel {
    /// `io_uring_setup` and `io_uring_create`.
    pub(crate) async fn sys_io_uring_setup(
        &self,
        caller: &Caller<'_>,
        entries: u32,
        params: IoUringParamsPtr,
    ) -> SysResult<i32> {
        let mut p: IoUringParams =
            read_value(caller.memory(), GuestPtr::new(params.addr())).await?;
        if p.resv.iter().any(|&r| r != 0) {
            return Err(Errno::EINVAL.into());
        }
        if p.flags & !SETUP_FLAGS != 0 {
            return Err(Errno::EINVAL.into());
        }
        let shape = io_uring_sizes(&p, entries)?;
        let setup = p.flags;
        // `io_uring_sanitise_params`.
        let has = |flag: IoringSetupFlags| setup & flag.0 != 0;
        if has(IoringSetupFlags::IORING_SETUP_DEFER_TASKRUN)
            && !has(IoringSetupFlags::IORING_SETUP_SINGLE_ISSUER)
        {
            return Err(Errno::EINVAL.into());
        }
        if has(IoringSetupFlags::IORING_SETUP_TASKRUN_FLAG)
            && !has(IoringSetupFlags::IORING_SETUP_COOP_TASKRUN)
            && !has(IoringSetupFlags::IORING_SETUP_DEFER_TASKRUN)
        {
            return Err(Errno::EINVAL.into());
        }
        // `IORING_SETUP_ATTACH_WQ` names another ring, whose workers there are none of.
        if setup & IoringSetupFlags::IORING_SETUP_ATTACH_WQ.0 != 0 {
            let wq = i32::try_from(p.wq_fd).map_err(|_| Errno::EBADF)?;
            let file = self.file(caller.task(), Fd(wq))?;
            if !is_ring(&file) {
                return Err(Errno::EINVAL.into());
            }
        }
        let rings = self
            .host
            .shared_pages(shape.rings_size)
            .map_err(|_| Errno::ENOMEM)?;
        let sqes = self
            .host
            .shared_pages(shape.sqes_size)
            .map_err(|_| Errno::ENOMEM)?;
        p.sq_entries = shape.sq_entries;
        p.cq_entries = shape.cq_entries;
        p.features = FEATURES;
        p.sq_off = IoSqringOffsets {
            head: SQ_HEAD as u32,
            tail: SQ_TAIL as u32,
            ring_mask: SQ_RING_MASK as u32,
            ring_entries: SQ_RING_ENTRIES as u32,
            flags: SQ_FLAGS as u32,
            dropped: SQ_DROPPED as u32,
            array: shape.sq_array.unwrap_or(0) as u32,
            resv1: 0,
            user_addr: 0,
        };
        p.cq_off = IoCqringOffsets {
            head: CQ_HEAD as u32,
            tail: CQ_TAIL as u32,
            ring_mask: CQ_RING_MASK as u32,
            ring_entries: CQ_RING_ENTRIES as u32,
            overflow: CQ_OVERFLOW as u32,
            cqes: CQES as u32,
            flags: CQ_FLAGS as u32,
            resv1: 0,
            user_addr: 0,
        };
        let ctx = Ctx::new(shape, (rings, sqes), self.timers.clone(), self.host.clone());
        if ctx.shape.has(IoringSetupFlags::IORING_SETUP_SINGLE_ISSUER)
            && !ctx.shape.has(IoringSetupFlags::IORING_SETUP_R_DISABLED)
        {
            ctx.state.lock_or_recover().submitter = Some(caller.task());
        }
        write_value(caller.memory(), params, &p).await?;
        let file = OpenFile::new(
            Backing::Stream(Arc::new(IoRing(ctx)), self.pseudo.anon("[io_uring]")),
            OpenFlags::O_RDWR,
        );
        let [fd] = self.install_files(caller, [file], true)?;
        Ok(fd.0)
    }

    /// `io_uring_enter`: submits up to `to_submit` SQEs, then with `IORING_ENTER_GETEVENTS`
    /// waits for `min_complete` completions. Returns how many were submitted, or the wait's
    /// error when nothing was.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn sys_io_uring_enter(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        to_submit: u32,
        min_complete: u32,
        flags: u32,
        argp: wasixv2_abi::ConstVoidPtr,
        argsz: u32,
    ) -> SysResult<i32> {
        if flags & !ENTER_FLAGS != 0 {
            return Err(Errno::EINVAL.into());
        }
        let registered = flags & IoringEnterFlags::IORING_ENTER_REGISTERED_RING.0 != 0;
        let file = self.ring_ctx_file(caller, fd, registered)?;
        let ctx = ring_of(&file)?;
        let (disabled, submitter) = {
            let state = ctx.state.lock_or_recover();
            (state.disabled, state.submitter)
        };
        if disabled {
            return Err(Errno::EBADFD.into());
        }
        let mm = self.mm(caller.task()).id();
        let mut ret = 0;
        if to_submit > 0 {
            // `__io_uring_add_tctx_node_from_submit`.
            if ctx.shape.has(IoringSetupFlags::IORING_SETUP_SINGLE_ISSUER)
                && submitter != Some(caller.task())
            {
                return Err(Errno::EEXIST.into());
            }
            self.tctx(caller.task()).add(&ctx);
            let _submit = ctx.submit.lock().await;
            self.run_task_work(caller, &ctx, mm).await;
            ret = self.submit_sqes(caller, &ctx, mm, to_submit).await?;
            if ret != to_submit as i32 {
                return Ok(ret);
            }
        }
        if flags & IoringEnterFlags::IORING_ENTER_GETEVENTS.0 != 0 {
            let waited = self
                .cqring_wait(caller, &ctx, mm, (min_complete, flags), (argp, argsz))
                .await;
            if ret == 0 {
                return waited;
            }
        } else if to_submit == 0 && ctx.allowed_run_tw(caller.task()) {
            let _submit = ctx.submit.lock().await;
            self.run_task_work(caller, &ctx, mm).await;
        }
        Ok(ret)
    }

    /// `io_run_task_work` for a ring: issues the requests a wakeup made ready, those of the
    /// caller's memory, and flushes overflowed completions. The caller holds `submit`.
    async fn run_task_work(&self, caller: &Caller<'_>, ctx: &Arc<Ctx>, mm: u64) {
        ctx.flush_overflow();
        loop {
            let req = {
                let mut state = ctx.state.lock_or_recover();
                let at = state.ready.iter().position(|req| req.mm == mm);
                let req = at.and_then(|at| state.ready.remove(at));
                ctx.update_sq_flags(&state);
                req
            };
            let Some(req) = req else { break };
            self.issue(caller, ctx, req).await;
        }
    }

    /// `io_submit_sqes`: takes up to `nr` SQEs off the ring and submits them. A request that
    /// fails to start completes with its error and ends the batch, unless
    /// `IORING_SETUP_SUBMIT_ALL`; it counts as submitted.
    async fn submit_sqes(
        &self,
        caller: &Caller<'_>,
        ctx: &Arc<Ctx>,
        mm: u64,
        nr: u32,
    ) -> SysResult<i32> {
        let tail = ctx.read_word(SQ_TAIL);
        let head = ctx.state.lock_or_recover().sq_head;
        let entries = tail.wrapping_sub(head).min(ctx.shape.sq_entries).min(nr);
        let mut link: Option<Req> = None;
        let mut submitted = 0;
        for _ in 0..entries {
            let Some(sqe) = self.next_sqe(ctx) else {
                continue;
            };
            submitted += 1;
            let ok = self.submit_sqe(caller, ctx, mm, sqe, &mut link).await;
            if !ok && !ctx.shape.has(IoringSetupFlags::IORING_SETUP_SUBMIT_ALL) {
                break;
            }
        }
        // A chain the batch left open still runs (`io_submit_state_end`).
        if let Some(head) = link.take() {
            self.queue_sqe(caller, ctx, head).await;
        }
        let head = ctx.state.lock_or_recover().sq_head;
        ctx.write_word(SQ_HEAD, head);
        Ok(submitted)
    }

    /// `io_get_sqe`: the next SQE, or `None` for an index past the ring, which is dropped.
    fn next_sqe(&self, ctx: &Ctx) -> Option<IoUringSqe> {
        let mut state = ctx.state.lock_or_recover();
        let head = state.sq_head;
        state.sq_head = head.wrapping_add(1);
        let mask = ctx.shape.sq_entries - 1;
        let index = match ctx.shape.sq_array {
            Some(array) => ctx.read_word(array + u64::from(head & mask) * 4),
            None => head & mask,
        };
        if index >= ctx.shape.sq_entries {
            state.sq_dropped += 1;
            ctx.write_word(SQ_DROPPED, state.sq_dropped);
            return None;
        }
        drop(state);
        let mut bytes = [0; IoUringSqe::SIZE];
        let at = u64::from(index) * ctx.shape.sqe_size;
        ctx.sqes.read_at(&mut bytes, at).ok()?;
        IoUringSqe::read_from(&bytes)
    }

    /// `io_cqring_wait`: waits until `min_complete` completions are on the ring, running task
    /// work as it comes, for at most the timeout of the extended argument, under its signal
    /// mask. A signal ends the wait with `EINTR`, the timeout with `ETIME`, unless completions
    /// are there by then.
    async fn cqring_wait(
        &self,
        caller: &Caller<'_>,
        ctx: &Arc<Ctx>,
        mm: u64,
        (min_complete, flags): (u32, u32),
        (argp, argsz): (wasixv2_abi::ConstVoidPtr, u32),
    ) -> SysResult<i32> {
        let (sigmask, sigsz, ts) = self.ext_arg(caller, flags, argp, argsz).await?;
        // Only the submitter runs a deferred ring's task work.
        if !ctx.allowed_run_tw(caller.task()) {
            return Err(Errno::EEXIST.into());
        }
        let deadline = match ts {
            None => None,
            Some(ts) => {
                let ts = crate::time::from_timespec(ts)?;
                Some(
                    match flags & IoringEnterFlags::IORING_ENTER_ABS_TIMER.0 != 0 {
                        true => ts,
                        false => self.host.monotonic().saturating_add(ts),
                    },
                )
            }
        };
        let saved = self.set_user_sigmask(caller, sigmask, sigsz).await?;
        let min = min_complete.min(ctx.shape.cq_entries);
        let interrupt = self.interrupt(caller);
        let ret = loop {
            {
                let _submit = ctx.submit.lock().await;
                self.run_task_work(caller, ctx, mm).await;
            }
            if min == 0 || ctx.cq_ready() >= min {
                break Ok(0);
            }
            let ready = || (ctx.cq_ready() >= min || ctx.has_work(Some(mm))).then_some(());
            let waited = match deadline {
                None => wait_event(&ctx.cq_wait, ready, &*interrupt).await.map(Some),
                Some(at) => {
                    let timer = self.host.sleep_until(at);
                    wait_event_timeout(&ctx.cq_wait, ready, &*interrupt, timer).await
                }
            };
            match waited {
                Ok(Some(())) => {}
                Ok(None) => break Err(Errno::ETIME),
                Err(_) => break Err(Errno::EINTR),
            }
        };
        let interrupted = ret == Err(Errno::EINTR);
        self.restore_sigmask(caller, saved, interrupted);
        match ret {
            Err(_) if ctx.cq_ready() > 0 => Ok(0),
            ret => Ok(ret?),
        }
    }

    /// `io_get_ext_arg`: the signal mask and timeout of the wait. Without
    /// `IORING_ENTER_EXT_ARG`, `argp` is a signal mask of `argsz` bytes.
    async fn ext_arg(
        &self,
        caller: &Caller<'_>,
        flags: u32,
        argp: wasixv2_abi::ConstVoidPtr,
        argsz: u32,
    ) -> SysResult<(ConstSigsetPtr, u32, Option<KernelTimespec>)> {
        if flags & IoringEnterFlags::IORING_ENTER_EXT_ARG.0 == 0 {
            return Ok((GuestPtr::new(argp.addr()), argsz, None));
        }
        // No registered wait regions (`IORING_ENTER_EXT_ARG_REG`).
        if flags & IoringEnterFlags::IORING_ENTER_EXT_ARG_REG.0 != 0 {
            return Err(Errno::EINVAL.into());
        }
        if argsz as usize != IoUringGeteventsArg::SIZE {
            return Err(Errno::EINVAL.into());
        }
        let arg: IoUringGeteventsArg =
            read_value(caller.memory(), GuestPtr::new(argp.addr())).await?;
        let ts = match arg.ts {
            0 => None,
            ts => {
                let ts = u32::try_from(ts).map_err(|_| Errno::EFAULT)?;
                Some(read_value(caller.memory(), GuestPtr::new(ts)).await?)
            }
        };
        let sigmask = u32::try_from(arg.sigmask).map_err(|_| Errno::EFAULT)?;
        Ok((GuestPtr::new(sigmask), arg.sigmask_sz, ts))
    }

    /// `io_uring_register`.
    pub(crate) async fn sys_io_uring_register(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        opcode: u32,
        arg: VoidPtr,
        nr_args: u32,
    ) -> SysResult<i32> {
        let use_registered = wasixv2_abi::IoUringRegisterOp::IORING_REGISTER_USE_REGISTERED_RING.0;
        let registered = opcode & use_registered != 0;
        let opcode = opcode & !use_registered;
        if opcode >= wasixv2_abi::IoUringRegisterOp::IORING_REGISTER_LAST.0 {
            return Err(Errno::EINVAL.into());
        }
        let file = self.ring_ctx_file(caller, fd, registered)?;
        let ctx = ring_of(&file)?;
        let _submit = ctx.submit.lock().await;
        let submitter = ctx.state.lock_or_recover().submitter;
        if submitter.is_some_and(|task| task != caller.task()) {
            return Err(Errno::EEXIST.into());
        }
        self.register(caller, &ctx, opcode, arg.addr(), nr_args)
            .await
    }
}

/// `io_uring_fill_params`' checks and the ring sizes.
fn io_uring_sizes(p: &IoUringParams, entries: u32) -> Result<Shape, Errno> {
    let clamp = p.flags & IoringSetupFlags::IORING_SETUP_CLAMP.0 != 0;
    if entries == 0 {
        return Err(Errno::EINVAL);
    }
    let entries = match entries > MAX_ENTRIES {
        true if clamp => MAX_ENTRIES,
        true => return Err(Errno::EINVAL),
        false => entries,
    };
    let sq_entries = entries.next_power_of_two();
    let cq_entries = match p.flags & IoringSetupFlags::IORING_SETUP_CQSIZE.0 != 0 {
        true => {
            if p.cq_entries == 0 {
                return Err(Errno::EINVAL);
            }
            let cq = match p.cq_entries > MAX_CQ_ENTRIES {
                true if clamp => MAX_CQ_ENTRIES,
                true => return Err(Errno::EINVAL),
                false => p.cq_entries,
            };
            let cq = cq.next_power_of_two();
            if cq < sq_entries {
                return Err(Errno::EINVAL);
            }
            cq
        }
        false => 2 * sq_entries,
    };
    Ok(Shape::new(p.flags, sq_entries, cq_entries))
}

impl Ctx {
    fn new(
        shape: Shape,
        (rings, sqes): (Arc<dyn SharedPages>, Arc<dyn SharedPages>),
        timers: Timers,
        host: Arc<dyn Host>,
    ) -> Arc<Self> {
        let disabled = shape.has(IoringSetupFlags::IORING_SETUP_R_DISABLED);
        let ctx = Arc::new_cyclic(|me: &Weak<Ctx>| {
            let weak = me.clone();
            let deferred = timers.timer(Box::new(move || {
                if let Some(ctx) = weak.upgrade() {
                    ctx.run_woken();
                }
                None
            }));
            Self {
                me: me.clone(),
                shape,
                rings,
                sqes,
                state: Mutex::new(State {
                    disabled,
                    ..State::default()
                }),
                submit: Default::default(),
                cq_wait: WaitQueue::new(),
                poll_wq: WaitQueue::new(),
                timers,
                host,
                deferred,
            }
        });
        let shape = &ctx.shape;
        ctx.write_word(SQ_RING_MASK, shape.sq_entries - 1);
        ctx.write_word(CQ_RING_MASK, shape.cq_entries - 1);
        ctx.write_word(SQ_RING_ENTRIES, shape.sq_entries);
        ctx.write_word(CQ_RING_ENTRIES, shape.cq_entries);
        ctx
    }
}

/// A wake entry's registration on a file's queues, which keeps the entry there until dropped.
pub(crate) type Registrations = Vec<WaitRegistration>;

/// `SysErr` as a completion's result: an interrupted call is `EINTR`.
pub(crate) fn cqe_res(result: SysResult<i32>) -> i32 {
    match result {
        Ok(res) => res,
        Err(SysErr::Errno(errno)) => -errno.0,
        Err(_) => -Errno::EINTR.0,
    }
}
