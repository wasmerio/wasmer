//! Anonymous pipes, as Linux's `fs/pipe.c`, with a byte buffer instead of page slots.

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex, MutexGuard, Weak,
        atomic::{AtomicU32, Ordering},
    },
};

use wasixv2_abi::{FcntlCmd, FdPair, GuestPtrMut, IoctlRequest, OpenFlags, SiCode};
use wasixv2_syscalls::{Caller, Errno, Fd, LocalFuture, SysErr, SysResult, write_value};
use wasixv2_vfs::Bound;
use wasixv2_wait::{Interrupt, PollMask, PollTable, Pollable, WaitQueue, poll_wait, wait_event};

use crate::{
    Kernel,
    fasync::{Fasync, FasyncList},
    file::{Backing, FileObject, Io, OpenFile, ReadSink, taken},
    sync::MutexExt,
};

/// Writes up to this size are atomic: all or nothing.
pub(crate) const PIPE_BUF: usize = 4096;
/// The page size pipe sizes are rounded to. Linux's, not the guest's 64 KiB, so the sizes
/// programs expect (`F_GETPIPE_SZ` = 65536) stay the same.
const PIPE_PAGE: usize = 4096;
/// Linux's default: 16 pages (`PIPE_DEF_BUFFERS`).
pub(crate) const DEFAULT_SIZE: usize = 16 * PIPE_PAGE;

/// `fs.pipe-max-size` (`pipe_max_size`, `fs/pipe.c`): no caller without `CAP_SYS_RESOURCE`
/// makes a pipe larger, and a new pipe it makes holds at most this much. One for the machine.
pub(crate) struct PipeMaxSize {
    max: AtomicU32,
    soft: AtomicU32,
    hard: AtomicU32,
    users: Mutex<HashMap<u32, usize>>,
}

/// Reserved pipe pages. The charge follows the pipe, not individual descriptors.
pub(crate) struct Budget {
    size: usize,
    charge: Charge,
}

struct Charge {
    limits: Arc<PipeMaxSize>,
    uid: u32,
    pages: usize,
}

impl Drop for Charge {
    fn drop(&mut self) {
        let mut users = self.limits.users.lock_or_recover();
        if let Some(total) = users.get_mut(&self.uid) {
            *total -= self.pages;
            if *total == 0 {
                users.remove(&self.uid);
            }
        }
    }
}

impl Charge {
    /// fs/pipe.c pipe_set_size checks quotas only on growth, against the
    /// prospective total, and rolls the accounting back on failure.
    fn resize(&mut self, pages: usize, unprivileged: bool) -> Result<(), Errno> {
        let mut users = self.limits.users.lock_or_recover();
        let total = users.entry(self.uid).or_default();
        let new = *total - self.pages + pages;
        if pages > self.pages && unprivileged && self.limits.over_limit(new) {
            return Err(Errno::EPERM);
        }
        *total = new;
        self.pages = pages;
        Ok(())
    }
}

impl PipeMaxSize {
    /// Linux's default, 1 MiB.
    const DEFAULT: u32 = 1 << 20;

    pub(crate) fn get(&self) -> u32 {
        self.max.load(Ordering::Relaxed)
    }

    /// `proc_dopipe_max_size`'s write: the value rounded as a pipe's size is
    /// (`round_pipe_size`), `ERANGE` for one past 2 GiB, which rounds to nothing.
    pub(crate) fn set(&self, value: u32) -> Result<(), Errno> {
        let size = round_pipe_size(value).ok_or(Errno::ERANGE)?;
        self.max.store(size as u32, Ordering::Relaxed);
        Ok(())
    }

    pub(crate) fn user_limit(&self, hard: bool) -> u32 {
        let limit = if hard { &self.hard } else { &self.soft };
        limit.load(Ordering::Relaxed)
    }

    pub(crate) fn set_user_limit(&self, hard: bool, pages: u32) -> Result<(), Errno> {
        // fs/pipe.c uses proc_doulongvec_minmax: unsigned long is u32 in the guest.
        let limit = if hard { &self.hard } else { &self.soft };
        limit.store(pages, Ordering::Relaxed);
        Ok(())
    }

    fn over_limit(&self, pages: usize) -> bool {
        [
            self.soft.load(Ordering::Relaxed),
            self.hard.load(Ordering::Relaxed),
        ]
        .iter()
        .any(|&limit| limit != 0 && pages > limit as usize)
    }

    /// fs/pipe.c alloc_pipe_info accounts the default first; over the soft
    /// limit an unprivileged user gets PIPE_MIN_DEF_BUFFERS (two pages).
    pub(crate) fn allocate(
        self: &Arc<Self>,
        uid: u32,
        mut size: usize,
        unprivileged: bool,
    ) -> Result<Budget, Errno> {
        let mut users = self.users.lock_or_recover();
        let total = users.get(&uid).copied().unwrap_or(0);
        let soft = self.soft.load(Ordering::Relaxed) as usize;
        if unprivileged && soft != 0 && total + size / PIPE_PAGE > soft {
            size = 2 * PIPE_PAGE;
        }
        let pages = size / PIPE_PAGE;
        let hard = self.hard.load(Ordering::Relaxed) as usize;
        if unprivileged && hard != 0 && total + pages > hard {
            return Err(Errno::ENFILE);
        }
        users.insert(uid, total + pages);
        Ok(Budget {
            size,
            charge: Charge {
                limits: self.clone(),
                uid,
                pages,
            },
        })
    }
}

impl Default for PipeMaxSize {
    fn default() -> Self {
        Self {
            max: AtomicU32::new(Self::DEFAULT),
            soft: AtomicU32::new(16384),
            hard: AtomicU32::new(0),
            users: Mutex::default(),
        }
    }
}

/// `round_pipe_size`: at least a page, rounded up to a power of two; `None` past 2 GiB.
fn round_pipe_size(size: u32) -> Option<usize> {
    match size {
        _ if size > 1 << 31 => None,
        _ => Some((size as usize).max(PIPE_PAGE).next_power_of_two()),
    }
}

pub(crate) struct Pipe {
    notification: bool,
    state: Mutex<State>,
    /// Held by a reader from looking at the data until it consumed it (`pipe->mutex` around
    /// `copy_page_to_iter`), so what it leaves behind on a fault is still first. Never held while waiting: readers wait on `rd_wait` first.
    io_lock: futures_util::lock::Mutex<()>,
    /// Readers wait here for data or the last writer to leave.
    rd_wait: WaitQueue,
    /// Writers wait here for room or the last reader to leave.
    wr_wait: WaitQueue,
    /// The files open for reading, and for writing, that get I/O signals (`fasync_readers`,
    /// `fasync_writers`).
    fasync_readers: FasyncList,
    fasync_writers: FasyncList,
}

struct State {
    watch: Option<crate::watch_queue::Queue>,
    buf: VecDeque<u8>,
    /// fs/pipe.c PIPE_BUF_FLAG_PACKET; stream runs may merge, packets never do.
    segments: VecDeque<(usize, bool)>,
    capacity: usize,
    charge: Charge,
    /// Room promised to a splice into the pipe that is still reading its input
    /// ([`Reservation`]), which Linux keeps by holding the pipe's lock across the read.
    reserved: usize,
    readers: usize,
    writers: usize,
    /// Read and write ends ever attached (`r_counter`, `w_counter`), so a FIFO opener can wait
    /// for a partner to arrive, even one that left again at once.
    r_counter: u64,
    w_counter: u64,
}

impl State {
    fn used(&self) -> usize {
        self.segments
            .iter()
            .map(|&(len, packet)| {
                if packet {
                    len.next_multiple_of(PIPE_PAGE)
                } else {
                    len
                }
            })
            .sum()
    }

    fn append(&mut self, data: &[u8], packet: bool) {
        if data.is_empty() {
            return;
        }
        self.buf.extend(data);
        if !packet && let Some((len, false)) = self.segments.back_mut() {
            *len += data.len();
            return;
        }
        self.segments.push_back((data.len(), packet));
    }

    /// pipe_read stops after a packet even if space remains in the user's buffer.
    fn read_len(&self, max: usize) -> usize {
        let mut n = 0;
        for &(len, packet) in &self.segments {
            n += len;
            if packet {
                break;
            }
        }
        n.min(max)
    }

    fn consume(&mut self, mut n: usize, packet_read: bool) {
        while n > 0 {
            let (len, packet) = self.segments.front_mut().expect("buffer segment");
            let take = n.min(*len);
            self.buf.drain(..take);
            *len -= take;
            n -= take;
            if packet_read && *packet {
                self.buf.drain(..*len);
                self.segments.pop_front();
                break;
            }
            if *len == 0 {
                self.segments.pop_front();
            }
        }
    }

    fn readable(&self) -> bool {
        self.watch
            .as_ref()
            .map_or(!self.buf.is_empty(), |q| q.readable())
    }

    fn input_len(&self) -> usize {
        self.watch.as_ref().map_or(self.buf.len(), |q| q.len())
    }

    fn free(&self) -> usize {
        if let Some(queue) = &self.watch {
            // fs/pipe.c pipe_poll tests max_usage slots, even though kernel producers may
            // fill the larger watch notification ring allocated by watch_queue_set_size.
            return (self.capacity / PIPE_PAGE).saturating_sub(queue.slots()) * PIPE_PAGE;
        }
        self.capacity.saturating_sub(self.used() + self.reserved)
    }
}

/// One end of a pipe; each open file has its own, so dropping the open file closes the end.
pub(crate) struct PipeEnd {
    pipe: Arc<Pipe>,
    writer: bool,
    /// A read end reports `HUP` without writers only once `w_counter` differs from this
    /// (`f_pipe`): a FIFO opened for reading without blocking, before any writer came, does
    /// not hang up.
    hup_unless: u64,
}

impl Drop for PipeEnd {
    /// `pipe_release`: wakes everybody, so they see EOF or EPIPE, and the last end of one
    /// side signals the other side's.
    fn drop(&mut self) {
        let alone = {
            let mut state = self.pipe.state.lock_or_recover();
            match self.writer {
                true => state.writers -= 1,
                false => state.readers -= 1,
            }
            (state.readers == 0) != (state.writers == 0)
        };
        self.pipe.rd_wait.wake(PollMask::empty());
        self.pipe.wr_wait.wake(PollMask::empty());
        if alone {
            self.pipe.signal_readers();
            self.pipe.signal_writers();
        }
    }
}

/// A pipe with no ends yet that holds `capacity` bytes (`alloc_pipe_info`).
fn new_pipe(budget: Budget, notification: bool) -> Arc<Pipe> {
    let Budget {
        size: capacity,
        charge,
    } = budget;
    Arc::new(Pipe {
        notification,
        state: Mutex::new(State {
            watch: notification.then(crate::watch_queue::Queue::default),
            buf: VecDeque::new(),
            segments: VecDeque::new(),
            capacity,
            charge,
            reserved: 0,
            readers: 0,
            writers: 0,
            r_counter: 1,
            w_counter: 1,
        }),
        io_lock: futures_util::lock::Mutex::new(()),
        rd_wait: WaitQueue::new(),
        wr_wait: WaitQueue::new(),
        fasync_readers: FasyncList::default(),
        fasync_writers: FasyncList::default(),
    })
}

/// A new pipe's read and write ends, holding `capacity` bytes.
fn pipe(budget: Budget, notification: bool) -> (PipeEnd, PipeEnd) {
    let pipe = new_pipe(budget, notification);
    let (read, _) = PipeEnd::attach(&pipe, false);
    let (write, _) = PipeEnd::attach(&pipe, true);
    (read, write)
}

// ---- fs: FIFOs (`fs/fifo.rs`) ----

/// A pipe with no ends yet holding `capacity` bytes, for a FIFO inode's first open.
pub(crate) fn fifo_pipe(budget: Budget) -> Arc<Pipe> {
    new_pipe(budget, false)
}

/// The other side of a pipe as an end was attached: how many ends it had open, and how many it
/// ever had (its counter).
#[derive(Clone, Copy)]
pub(crate) struct Partner {
    pub(crate) open: usize,
    pub(crate) counter: u64,
}

impl Pipe {
    pub(crate) fn is_notification(&self) -> bool {
        self.notification
    }

    pub(crate) fn add_notification_watch(&self, watch: Weak<crate::watch_queue::Watch>) {
        if let Some(queue) = &mut self.state.lock_or_recover().watch {
            queue.watches.retain(|w| w.strong_count() != 0);
            queue.watches.push(watch);
        }
    }

    pub(crate) fn post_notification(&self, bytes: Vec<u8>, filtered: bool) {
        let posted = self
            .state
            .lock_or_recover()
            .watch
            .as_mut()
            .is_some_and(|q| q.post(bytes, filtered));
        if posted {
            self.rd_wait.wake(PollMask::READABLE);
            self.signal_readers();
        }
    }

    /// `pipe_fasync` for a file with a read end (`read`), a write end, or both.
    pub(crate) fn fasync(&self, fasync: &Fasync<'_>, read: bool, write: bool) {
        if read {
            self.fasync_readers.helper(fasync);
        }
        if write {
            self.fasync_writers.helper(fasync);
        }
    }

    /// `kill_fasync(&pipe->fasync_readers, SIGIO, POLL_IN)`.
    fn signal_readers(&self) {
        self.fasync_readers.kill(SiCode::POLL_IN);
    }

    /// `kill_fasync(&pipe->fasync_writers, SIGIO, POLL_OUT)`.
    fn signal_writers(&self) {
        self.fasync_writers.kill(SiCode::POLL_OUT);
    }

    /// The number of open read ends.
    pub(crate) fn readers(&self) -> usize {
        self.state.lock_or_recover().readers
    }

    /// `wait_for_partner`: waits until an end of the other side has been attached since
    /// `partner` was seen, whether or not it is still open.
    pub(crate) async fn wait_for_partner(
        &self,
        writers: bool,
        partner: Partner,
        interrupt: &dyn wasixv2_wait::Interrupt,
    ) -> SysResult<()> {
        let check = || {
            let state = self.state.lock_or_recover();
            let counter = if writers {
                state.w_counter
            } else {
                state.r_counter
            };
            (counter != partner.counter).then_some(())
        };
        wait_event(&self.rd_wait, check, interrupt)
            .await
            .map_err(|_| SysErr::Restart)
    }
}

impl PipeEnd {
    /// Opens another end of `pipe` and wakes whoever waits for one. Returns the other side as
    /// it was then.
    pub(crate) fn attach(pipe: &Arc<Pipe>, writer: bool) -> (Self, Partner) {
        let partner = {
            let mut state = pipe.state.lock_or_recover();
            match writer {
                true => {
                    state.writers += 1;
                    state.w_counter += 1;
                }
                false => {
                    state.readers += 1;
                    state.r_counter += 1;
                }
            }
            match writer {
                true => Partner {
                    open: state.readers,
                    counter: state.r_counter,
                },
                false => Partner {
                    open: state.writers,
                    counter: state.w_counter,
                },
            }
        };
        pipe.rd_wait.wake(PollMask::empty());
        pipe.wr_wait.wake(PollMask::empty());
        let end = Self {
            pipe: pipe.clone(),
            writer,
            hup_unless: 0,
        };
        (end, partner)
    }

    /// Suppresses `HUP` on this read end until a writer has come (and gone): what a FIFO
    /// opened `O_RDONLY | O_NONBLOCK` with no writer gets.
    pub(crate) fn hup_after_writer(&mut self, partner: Partner) {
        self.hup_unless = partner.counter;
    }
}

/// What takes data out of a pipe: the reader's buffers, or a splice's output. Given the data
/// at the front of the pipe, it returns how much of it to consume; an error consumes nothing.
pub(crate) trait Consumer {
    fn take<'a>(&'a mut self, data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>>;
}

/// A read into the kernel's own buffer: the data is there already.
struct InPlace;

impl Consumer for InPlace {
    fn take<'a>(&'a mut self, data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move { Ok(data.len()) })
    }
}

/// A read into the user's buffers, consumed only once all of it arrived ([`taken`]).
pub(crate) struct ToUser<'a>(pub(crate) &'a mut dyn ReadSink);

impl Consumer for ToUser<'_> {
    fn take<'a>(&'a mut self, data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move { taken(data.len(), self.0.copy_out(data).await) })
    }
}

/// What [`Pipe::move_to`] and [`Pipe::link_to`] found.
pub(crate) enum Moved {
    /// This many bytes went over.
    Bytes(usize),
    /// The input pipe is empty; whether it still has writers.
    InEmpty { writers: bool },
    /// The output pipe has no room.
    OutFull,
    /// The output pipe has no reader: `EPIPE` and `SIGPIPE`.
    NoReaders,
}

impl Pipe {
    /// Waits until the pipe has data or no writer (`ipipe_prep`, `splice_from_pipe_next`),
    /// or fails with `EAGAIN` when `nonblock`. The pipe may be empty again by the time the
    /// caller looks.
    pub(crate) async fn wait_readable(
        &self,
        nonblock: bool,
        interrupt: &dyn Interrupt,
    ) -> SysResult<()> {
        let ready = || {
            let state = self.state.lock_or_recover();
            if state.readable() || state.writers == 0 {
                return Some(Ok(()));
            }
            nonblock.then_some(Err(Errno::EAGAIN))
        };
        wait_event(&self.rd_wait, ready, interrupt)
            .await
            .map_err(|_| SysErr::Restart)??;
        Ok(())
    }

    /// Waits until the pipe has room (`wait_for_space`, `opipe_prep`), or fails with `EAGAIN`
    /// when `nonblock`; `EPIPE` without readers, for which the caller raises `SIGPIPE`.
    pub(crate) async fn wait_room(
        &self,
        nonblock: bool,
        interrupt: &dyn Interrupt,
    ) -> SysResult<()> {
        let ready = || {
            let state = self.state.lock_or_recover();
            if state.readers == 0 {
                return Some(Err(Errno::EPIPE));
            }
            if state.free() > 0 {
                return Some(Ok(()));
            }
            nonblock.then_some(Err(Errno::EAGAIN))
        };
        wait_event(&self.wr_wait, ready, interrupt)
            .await
            .map_err(|_| SysErr::Restart)??;
        Ok(())
    }

    /// Promises the room for at most `max` more bytes to the caller, which fills it once its
    /// input is read; possibly none, if others took the room meanwhile.
    pub(crate) fn reserve(self: &Arc<Self>, max: usize) -> Reservation {
        let mut state = self.state.lock_or_recover();
        let len = max.min(state.free());
        state.reserved += len;
        Reservation {
            pipe: self.clone(),
            len,
        }
    }

    /// Consumes data from the front for `out`, at most `buf.len()` bytes, with `buf` as the
    /// kernel's buffer: waits for data without the I/O lock, then under it looks at the data,
    /// hands it to `out` and consumes what `out` took (`pipe_read`, `__splice_from_pipe`).
    /// 0 at the end of the data, when the pipe is empty and has no writer.
    pub(crate) async fn consume(
        &self,
        nonblock: bool,
        interrupt: &dyn Interrupt,
        buf: &mut [u8],
        out: &mut dyn Consumer,
        packet_read: bool,
    ) -> SysResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.notification {
            return self
                .consume_notifications(nonblock, interrupt, buf, out)
                .await;
        }
        loop {
            self.wait_readable(nonblock, interrupt).await?;
            let _io = self.io_lock.lock().await;
            let n = {
                let state = self.state.lock_or_recover();
                if state.buf.is_empty() {
                    match state.writers {
                        0 => return Ok(0),
                        _ => continue,
                    }
                }
                let n = if packet_read {
                    state.read_len(buf.len())
                } else {
                    buf.len().min(state.buf.len())
                };
                for (dst, src) in buf.iter_mut().zip(state.buf.range(..n)) {
                    *dst = *src;
                }
                n
            };
            let n = out.take(&buf[..n]).await?;
            if n > 0 {
                let was_full = {
                    let mut state = self.state.lock_or_recover();
                    let was_full = state.free() == 0;
                    state.consume(n, packet_read);
                    was_full
                };
                // fs/pipe.c pipe_read wakes writers after freeing a slot from
                // a full pipe. Reading an already writable pipe is no new edge.
                if was_full {
                    self.wr_wait.wake(PollMask::WRITABLE);
                }
                self.signal_writers();
            }
            return Ok(n);
        }
    }

    /// fs/pipe.c pipe_read: PIPE_BUF_FLAG_WHOLE records are never split or consumed
    /// on a short copy. A loss notification follows the buffer that recorded the overflow.
    async fn consume_notifications(
        &self,
        nonblock: bool,
        interrupt: &dyn Interrupt,
        buf: &mut [u8],
        out: &mut dyn Consumer,
    ) -> SysResult<usize> {
        loop {
            self.wait_readable(nonblock, interrupt).await?;
            let _io = self.io_lock.lock().await;
            let mut done = 0;
            loop {
                let note = self
                    .state
                    .lock_or_recover()
                    .watch
                    .as_ref()
                    .and_then(|q| q.front());
                let Some(note) = note else {
                    break;
                };
                if note.len() > buf.len() - done {
                    return if done > 0 {
                        Ok(done)
                    } else {
                        Err(Errno::ENOBUFS.into())
                    };
                }
                let end = done + note.len();
                buf[done..end].copy_from_slice(&note);
                let copied = out.take(&buf[done..end]).await;
                match copied {
                    Ok(n) if n == note.len() => {}
                    _ if done > 0 => return Ok(done),
                    _ => return Err(Errno::EFAULT.into()),
                }
                self.state.lock_or_recover().watch.as_mut().unwrap().pop();
                done = end;
                if done == buf.len() {
                    break;
                }
            }
            if done > 0 {
                return Ok(done);
            }
            if self.state.lock_or_recover().writers == 0 {
                return Ok(0);
            }
        }
    }

    /// Moves at most `len` bytes from the front of this pipe to the end of `out`
    /// (`splice_pipe_to_pipe`), under both pipes' locks, taken in address order as
    /// `pipe_double_lock` does, and this pipe's I/O lock first, as any consumer.
    pub(crate) async fn move_to(&self, out: &Pipe, len: usize) -> Moved {
        let _io = self.io_lock.lock().await;
        self.transfer(out, len, true)
    }

    /// Copies at most `len` bytes from the front of this pipe to the end of `out` and leaves
    /// them here too (`link_pipe`).
    pub(crate) fn link_to(&self, out: &Pipe, len: usize) -> Moved {
        self.transfer(out, len, false)
    }

    fn transfer(&self, out: &Pipe, len: usize, consume: bool) -> Moved {
        let (mut input, mut output) = lock_both(self, out);
        if output.readers == 0 {
            return Moved::NoReaders;
        }
        if input.buf.is_empty() {
            return Moved::InEmpty {
                writers: input.writers > 0,
            };
        }
        // splice_pipe_to_pipe/link_pipe preserve packet flags, including partial packets.
        let mut n = 0;
        for &(segment, packet) in &input.segments {
            let free = output.free();
            if (packet && free < PIPE_PAGE) || free == 0 || n == len {
                break;
            }
            let take = segment.min(len - n).min(free);
            let bytes: Vec<_> = input.buf.range(n..n + take).copied().collect();
            output.append(&bytes, packet);
            n += take;
            if take < segment {
                break;
            }
        }
        if n == 0 {
            return Moved::OutFull;
        }
        if consume {
            input.consume(n, false);
        }
        drop((input, output));
        // `wakeup_pipe_readers` and `wakeup_pipe_writers`.
        out.rd_wait.wake(PollMask::READABLE);
        out.signal_readers();
        if consume {
            self.wr_wait.wake(PollMask::WRITABLE);
            self.signal_writers();
        }
        Moved::Bytes(n)
    }

    /// The pipe's size (`F_GETPIPE_SZ`).
    pub(crate) fn capacity(&self) -> usize {
        self.state.lock_or_recover().capacity
    }
}

/// The states of two distinct pipes, locked in address order.
fn lock_both<'a>(a: &'a Pipe, b: &'a Pipe) -> (MutexGuard<'a, State>, MutexGuard<'a, State>) {
    debug_assert!(!std::ptr::eq(a, b), "a pipe is never spliced to itself");
    match (a as *const Pipe) < (b as *const Pipe) {
        true => {
            let a = a.state.lock_or_recover();
            (a, b.state.lock_or_recover())
        }
        false => {
            let b = b.state.lock_or_recover();
            (a.state.lock_or_recover(), b)
        }
    }
}

/// Room in a pipe promised to one splice ([`Pipe::reserve`]); what is not filled is given
/// back when it is dropped.
pub(crate) struct Reservation {
    pipe: Arc<Pipe>,
    len: usize,
}

impl Reservation {
    /// The room promised.
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// Appends `data`, at most the room promised, and wakes the readers.
    pub(crate) fn fill(mut self, data: &[u8]) {
        debug_assert!(data.len() <= self.len);
        {
            let mut state = self.pipe.state.lock_or_recover();
            state.reserved -= self.len;
            state.append(data, false);
        }
        self.len = 0;
        if !data.is_empty() {
            // `wakeup_pipe_readers`.
            self.pipe.rd_wait.wake(PollMask::READABLE);
            self.pipe.signal_readers();
        }
        self.pipe.wr_wait.wake(PollMask::WRITABLE);
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if self.len == 0 {
            return;
        }
        self.pipe.state.lock_or_recover().reserved -= self.len;
        self.pipe.wr_wait.wake(PollMask::WRITABLE);
    }
}

/// The pipe behind a pipe end or a FIFO (`get_pipe_info`), which `splice`, `tee` and
/// `vmsplice` work on.
pub(crate) fn pipe_info(file: &OpenFile) -> Option<Arc<Pipe>> {
    if let Some(end) = file.downcast::<PipeEnd>() {
        return Some(end.pipe.clone());
    }
    file.downcast::<crate::fs::FifoFile>()
        .map(|fifo| fifo.pipe().clone())
}

impl PipeEnd {
    /// `pipe_read`: at most `buf.len()` bytes, into `to` or into `buf` itself without one.
    /// Writers' owners hear of it however it ends but interrupted while waiting: with room
    /// made, or with nothing to read.
    async fn read_bytes(
        &self,
        io: Io<'_>,
        buf: &mut [u8],
        to: Option<&mut dyn ReadSink>,
    ) -> SysResult<usize> {
        let read = self.consume_into(io, buf, to).await;
        // A read that consumed data signalled as it did; one interrupted while waiting returns
        // without a signal.
        if !buf.is_empty() && !matches!(read, Ok(1..) | Err(SysErr::Restart)) {
            self.pipe.signal_writers();
        }
        read
    }

    async fn consume_into(
        &self,
        io: Io<'_>,
        buf: &mut [u8],
        to: Option<&mut dyn ReadSink>,
    ) -> SysResult<usize> {
        let nonblock = io.nonblocking();
        match to {
            Some(to) => {
                let mut out = ToUser(to);
                self.pipe
                    .consume(nonblock, io.interrupt, buf, &mut out, true)
                    .await
            }
            None => {
                self.pipe
                    .consume(nonblock, io.interrupt, buf, &mut InPlace, true)
                    .await
            }
        }
    }

    /// `pipe_write`. Writes of at most `PIPE_BUF` bytes go in whole or wait; larger ones go in
    /// as room appears and return early only when non-blocking or interrupted. Finding no
    /// reader is `EPIPE`, or the count so far, and is reported to `io` either way: Linux sends
    /// `SIGPIPE` whenever `!pipe->readers`. Readers' owners hear of it however it ends, and
    /// before it waits for room again.
    async fn write_bytes(&self, io: Io<'_>, data: &[u8]) -> SysResult<usize> {
        // fs/pipe.c anon_pipe_write checks watch queues even for a zero-length write.
        if self.pipe.notification {
            return Err(Errno::EXDEV.into());
        }
        if data.is_empty() {
            return Ok(0);
        }
        let written = self.write_steps(io, data).await;
        self.pipe.signal_readers();
        written
    }

    async fn write_steps(&self, io: Io<'_>, data: &[u8]) -> SysResult<usize> {
        let pipe = &self.pipe;
        let packet = io.flags.contains(OpenFlags::O_DIRECT);
        let atomic = data.len() <= PIPE_BUF;
        let mut written = 0;
        loop {
            let step = || {
                let mut state = pipe.state.lock_or_recover();
                if state.readers == 0 {
                    io.set_broken();
                    return Some(Err(Errno::EPIPE));
                }
                let free = state.free();
                let fits = if packet {
                    free >= PIPE_PAGE
                } else if atomic {
                    free >= data.len()
                } else {
                    free > 0
                };
                if fits {
                    let room = if packet {
                        free / PIPE_PAGE * PIPE_PAGE
                    } else {
                        free
                    };
                    let n = room.min(data.len() - written);
                    if packet {
                        for chunk in data[written..written + n].chunks(PIPE_BUF) {
                            state.append(chunk, true);
                        }
                    } else {
                        state.append(&data[written..written + n], false);
                    }
                    return Some(Ok(n));
                }
                io.nonblocking().then_some(Err(Errno::EAGAIN))
            };
            let result = match wait_event(&pipe.wr_wait, step, io.interrupt).await {
                Ok(result) => result,
                Err(_) if written > 0 => return Ok(written),
                Err(_) => return Err(SysErr::Restart),
            };
            match result {
                Ok(n) => {
                    written += n;
                    pipe.rd_wait.wake(PollMask::READABLE);
                    if written == data.len() {
                        return Ok(written);
                    }
                    pipe.signal_readers();
                }
                Err(_) if written > 0 => return Ok(written),
                Err(errno) => return Err(errno.into()),
            }
        }
    }

    pub(crate) fn set_watch_size(&self, notes: u32, unprivileged: bool) -> Result<i32, Errno> {
        let mut state = self.pipe.state.lock_or_recover();
        let capacity = state
            .watch
            .as_ref()
            .ok_or(Errno::ENODEV)?
            .capacity_for(notes)?;
        state.charge.resize(capacity / PIPE_PAGE, unprivileged)?;
        state.watch.as_mut().expect("checked").set_size(notes)?;
        state.capacity = capacity;
        Ok(0)
    }

    fn input_len(&self) -> usize {
        self.pipe.state.lock_or_recover().input_len()
    }

    /// `pipe_set_size` with `round_pipe_size`: growing past `max`, `fs.pipe-max-size`, takes
    /// `CAP_SYS_RESOURCE` (`privileged`); shrinking is always allowed.
    fn set_size(
        &self,
        size: u32,
        max: u32,
        privileged: bool,
        unprivileged: bool,
    ) -> Result<usize, Errno> {
        if self.pipe.notification {
            return Err(Errno::EBUSY);
        }
        let size = round_pipe_size(size).ok_or(Errno::EINVAL)?;
        let mut state = self.pipe.state.lock_or_recover();
        if size > state.capacity && size > max as usize && !privileged {
            return Err(Errno::EPERM);
        }
        if state.used() > size {
            return Err(Errno::EBUSY);
        }
        state.charge.resize(size / PIPE_PAGE, unprivileged)?;
        state.capacity = size;
        drop(state);
        self.pipe.wr_wait.wake(PollMask::WRITABLE);
        Ok(size)
    }
}

impl Pollable for PipeEnd {
    /// `pipe_poll`.
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, &self.pipe.rd_wait);
        poll_wait(&mut table, &self.pipe.wr_wait);
        let state = self.pipe.state.lock_or_recover();
        let mut mask = PollMask::empty();
        if !self.writer {
            if state.input_len() != 0 {
                mask |= PollMask::READABLE;
            }
            if state.writers == 0 && state.w_counter != self.hup_unless {
                mask |= PollMask::HUP;
            }
        } else {
            // Writable while an atomic write would fit, which is when Linux has a free slot.
            if state.free() >= PIPE_BUF.min(state.capacity) {
                mask |= PollMask::WRITABLE;
            }
            if state.readers == 0 {
                mask |= PollMask::ERR;
            }
        }
        mask
    }
}

impl FileObject for PipeEnd {
    fn read<'a>(&'a self, io: Io<'a>, buf: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(self.read_bytes(io, buf, None))
    }

    fn read_to<'a>(
        &'a self,
        io: Io<'a>,
        buf: &'a mut [u8],
        to: &'a mut dyn ReadSink,
    ) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(self.read_bytes(io, buf, Some(to)))
    }

    fn write<'a>(&'a self, io: Io<'a>, data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(self.write_bytes(io, data))
    }

    fn buffered(&self) -> usize {
        self.input_len()
    }

    fn fasync(&self, fasync: &Fasync<'_>) -> Option<Result<(), Errno>> {
        self.pipe.fasync(fasync, !self.writer, self.writer);
        Some(Ok(()))
    }

    fn ioctl<'a>(
        &'a self,
        caller: &'a Caller<'a>,
        request: IoctlRequest,
        arg: u32,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(async move {
            if request == IoctlRequest::IOC_WATCH_QUEUE_SET_SIZE {
                return Ok(self.set_watch_size(arg, true)?);
            }
            if request == IoctlRequest::IOC_WATCH_QUEUE_SET_FILTER {
                if !self.pipe.notification {
                    return Err(Errno::ENODEV.into());
                }
                let filter = crate::watch_queue::read_filter(caller, arg).await?;
                self.pipe
                    .state
                    .lock_or_recover()
                    .watch
                    .as_mut()
                    .unwrap()
                    .set_filter(filter);
                return Ok(0);
            }
            if request != IoctlRequest::FIONREAD {
                return Err(Errno::ENOTTY.into());
            }
            let len = self.input_len() as i32;
            write_value(caller.memory(), GuestPtrMut::new(arg), &len).await?;
            Ok(0)
        })
    }
}

/// `pipe_fcntl`: `F_GETPIPE_SZ` and `F_SETPIPE_SZ`; `EBADF` for anything but a pipe.
/// `privileged` is whether the caller has `CAP_SYS_RESOURCE`, which lifts `max`, the
/// machine's `fs.pipe-max-size`.
pub(crate) fn fcntl(
    file: &OpenFile,
    cmd: FcntlCmd,
    arg: u32,
    max: u32,
    privileged: bool,
    unprivileged: bool,
) -> SysResult<i32> {
    let end = file.downcast::<PipeEnd>().ok_or(Errno::EBADF)?;
    let size = match cmd {
        FcntlCmd::F_GETPIPE_SZ => end.pipe.state.lock_or_recover().capacity,
        _ => end.set_size(arg, max, privileged, unprivileged)?,
    };
    Ok(size as i32)
}

impl Kernel {
    /// `do_pipe2`.
    pub(crate) async fn sys_pipe2(
        &self,
        caller: &Caller<'_>,
        fildes: FdPair,
        flags: OpenFlags,
    ) -> SysResult<i32> {
        let allowed = OpenFlags::O_CLOEXEC
            | OpenFlags::O_NONBLOCK
            | OpenFlags::O_DIRECT
            | OpenFlags::O_NOTIFICATION_PIPE;
        if !(flags & !allowed).is_empty() {
            return Err(Errno::EINVAL.into());
        }
        let owner = self.cred(caller).fs_owner();
        let status =
            flags & (OpenFlags::O_NONBLOCK | OpenFlags::O_DIRECT | OpenFlags::O_NOTIFICATION_PIPE);
        let [read, write] = self.pipe_files(self.new_pipe_budget(caller)?, owner, status);
        let cloexec = flags.contains(OpenFlags::O_CLOEXEC);
        self.install_pair(caller, fildes, [read, write], cloexec)
            .await?;
        Ok(0)
    }

    /// A new pipe of `size` bytes as its two open files, with the status flags `status`, on a
    /// pipefs inode owned by `owner`.
    pub(crate) fn pipe_files(
        &self,
        budget: Budget,
        owner: (u32, u32),
        status: OpenFlags,
    ) -> [OpenFile; 2] {
        let (read, write) = pipe(budget, status.contains(OpenFlags::O_NOTIFICATION_PIPE));
        let status = status & !OpenFlags::O_NOTIFICATION_PIPE;
        // The pipe hangs off its inode, as a FIFO's does, so `/proc/<pid>/fd/<n>` reopens it.
        let path = self.pseudo.pipe(owner);
        let pipe: Bound = read.pipe.clone();
        path.dentry.inode().bind(&pipe);
        let read = OpenFile::new(
            Backing::Stream(Arc::new(read), path.clone()),
            // fs/pipe.c:create_pipe_files applies O_DIRECT to the writer only.
            OpenFlags::O_RDONLY | (status & !OpenFlags::O_DIRECT),
        );
        let write = OpenFile::new(
            Backing::Stream(Arc::new(write), path),
            OpenFlags::O_WRONLY | status,
        );
        [read, write]
    }

    /// How much a pipe the caller makes holds (`alloc_pipe_info`): `fs.pipe-max-size` bounds
    /// it unless the caller has `CAP_SYS_RESOURCE`.
    pub(crate) fn new_pipe_budget(&self, caller: &Caller<'_>) -> Result<Budget, Errno> {
        let cred = self.cred(caller);
        let resource = cred.capable_init(wasixv2_vfs::Cap::SysResource);
        let size = if resource {
            DEFAULT_SIZE
        } else {
            DEFAULT_SIZE.min(self.pipe_max_size.get() as usize)
        };
        self.pipe_max_size
            .allocate(cred.user.real.0, size, self.pipe_user_unprivileged(caller))
    }

    pub(crate) fn pipe_user_unprivileged(&self, caller: &Caller<'_>) -> bool {
        let cred = self.cred(caller);
        !cred.capable_init(wasixv2_vfs::Cap::SysResource)
            && !cred.capable_init(wasixv2_vfs::Cap::SysAdmin)
    }

    /// Installs `files` at the lowest free descriptors: all of them, or none with `EMFILE`.
    pub(crate) fn install_files<const N: usize>(
        &self,
        caller: &Caller<'_>,
        files: [impl Into<Arc<OpenFile>>; N],
        cloexec: bool,
    ) -> Result<[Fd; N], Errno> {
        let fds = self.reserve_fds::<N>(caller, cloexec)?;
        let mut files = files.into_iter();
        Ok(fds.map(|fd| fd.install(files.next().expect("one file per fd").into())))
    }

    /// Installs a new pair of files and writes their descriptors to the guest, as `do_pipe2`
    /// does: the numbers are reserved first and the files installed only once the copy
    /// succeeded, so a fault never closes descriptors another thread may already use.
    pub(crate) async fn install_pair(
        &self,
        caller: &Caller<'_>,
        out: FdPair,
        files: [impl Into<Arc<OpenFile>>; 2],
        cloexec: bool,
    ) -> SysResult<()> {
        let fds = self.reserve_fds::<2>(caller, cloexec)?;
        let pair = [fds[0].fd().0, fds[1].fd().0];
        write_value(caller.memory(), GuestPtrMut::new(out.addr()), &pair)
            .await
            .map_err(|_| Errno::EFAULT)?;
        let [read, write] = files;
        let [rfd, wfd] = fds;
        rfd.install(read.into());
        wfd.install(write.into());
        Ok(())
    }
}
