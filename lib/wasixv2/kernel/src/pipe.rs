//! Anonymous pipes, as Linux's `fs/pipe.c`, with a byte buffer instead of page slots.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use wasixv2_abi::{FcntlCmd, FdPair, GuestPtrMut, IoctlRequest, OpenFlags};
use wasixv2_syscalls::{Caller, Errno, Fd, LocalFuture, SysErr, SysResult, write_value};
use wasixv2_vfs::Bound;
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, poll_wait, wait_event};

use crate::{
    Kernel,
    file::{Backing, FileObject, Io, OpenFile, ReadSink, taken},
    sync::MutexExt,
};

/// Writes up to this size are atomic: all or nothing.
pub(crate) const PIPE_BUF: usize = 4096;
/// The page size pipe sizes are rounded to. Linux's, not the guest's 64 KiB, so the sizes
/// programs expect (`F_GETPIPE_SZ` = 65536) stay the same.
const PIPE_PAGE: usize = 4096;
/// Linux's default: 16 pages.
const DEFAULT_SIZE: usize = 16 * PIPE_PAGE;
/// `/proc/sys/fs/pipe-max-size`; unprivileged callers cannot go above it.
const MAX_SIZE: usize = 1024 * 1024;

pub(crate) struct Pipe {
    state: Mutex<State>,
    /// Held by a reader from looking at the data until it consumed it (`pipe->mutex` around
    /// `copy_page_to_iter`), so what it leaves behind on a fault is still first. Never held while waiting: readers wait on `rd_wait` first.
    io_lock: futures_util::lock::Mutex<()>,
    /// Readers wait here for data or the last writer to leave.
    rd_wait: WaitQueue,
    /// Writers wait here for room or the last reader to leave.
    wr_wait: WaitQueue,
}

struct State {
    buf: VecDeque<u8>,
    capacity: usize,
    readers: usize,
    writers: usize,
    /// Read and write ends ever attached (`r_counter`, `w_counter`), so a FIFO opener can wait
    /// for a partner to arrive, even one that left again at once.
    r_counter: u64,
    w_counter: u64,
}

impl State {
    fn free(&self) -> usize {
        self.capacity.saturating_sub(self.buf.len())
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
    /// `pipe_release`: wakes everybody, so they see EOF or EPIPE.
    fn drop(&mut self) {
        {
            let mut state = self.pipe.state.lock_or_recover();
            match self.writer {
                true => state.writers -= 1,
                false => state.readers -= 1,
            }
        }
        self.pipe.rd_wait.wake(PollMask::empty());
        self.pipe.wr_wait.wake(PollMask::empty());
    }
}

/// A pipe with no ends yet (`alloc_pipe_info`).
fn new_pipe() -> Arc<Pipe> {
    Arc::new(Pipe {
        state: Mutex::new(State {
            buf: VecDeque::new(),
            capacity: DEFAULT_SIZE,
            readers: 0,
            writers: 0,
            r_counter: 1,
            w_counter: 1,
        }),
        io_lock: futures_util::lock::Mutex::new(()),
        rd_wait: WaitQueue::new(),
        wr_wait: WaitQueue::new(),
    })
}

/// A new pipe's read and write ends.
pub(crate) fn pipe() -> (PipeEnd, PipeEnd) {
    let pipe = new_pipe();
    let (read, _) = PipeEnd::attach(&pipe, false);
    let (write, _) = PipeEnd::attach(&pipe, true);
    (read, write)
}

// ---- fs: FIFOs (`fs/fifo.rs`) ----

/// A pipe with no ends yet, for a FIFO inode's first open.
pub(crate) fn fifo_pipe() -> Arc<Pipe> {
    new_pipe()
}

/// The other side of a pipe as an end was attached: how many ends it had open, and how many it
/// ever had (its counter).
#[derive(Clone, Copy)]
pub(crate) struct Partner {
    pub(crate) open: usize,
    pub(crate) counter: u64,
}

impl Pipe {
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

impl PipeEnd {
    /// `pipe_read`: copies at most `buf.len()` bytes to `to`, or into `buf` itself without
    /// one, and consumes them only once all arrived ([`taken`]). Waits for data without the I/O lock, then takes
    /// the lock and looks again, since another reader may have emptied the pipe meanwhile.
    async fn read_bytes(
        &self,
        io: Io<'_>,
        buf: &mut [u8],
        mut to: Option<&mut dyn ReadSink>,
    ) -> SysResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let pipe = &self.pipe;
        loop {
            let ready = || {
                let state = pipe.state.lock_or_recover();
                if !state.buf.is_empty() || state.writers == 0 {
                    return Some(Ok(()));
                }
                io.nonblocking().then_some(Err(Errno::EAGAIN))
            };
            wait_event(&pipe.rd_wait, ready, io.interrupt)
                .await
                .map_err(|_| SysErr::Restart)??;
            let _io = pipe.io_lock.lock().await;
            let n = {
                let state = pipe.state.lock_or_recover();
                if state.buf.is_empty() {
                    match state.writers {
                        0 => return Ok(0),
                        _ => continue,
                    }
                }
                let n = buf.len().min(state.buf.len());
                for (dst, src) in buf.iter_mut().zip(state.buf.range(..n)) {
                    *dst = *src;
                }
                n
            };
            let copied = match to.as_mut() {
                Some(to) => to.copy_out(&buf[..n]).await,
                None => n,
            };
            let n = taken(n, copied)?;
            pipe.state.lock_or_recover().buf.drain(..n);
            pipe.wr_wait.wake(PollMask::WRITABLE);
            return Ok(n);
        }
    }

    /// `pipe_write`. Writes of at most `PIPE_BUF` bytes go in whole or wait; larger ones go in
    /// as room appears and return early only when non-blocking or interrupted. Finding no
    /// reader is `EPIPE`, or the count so far, and is reported to `io` either way: Linux sends
    /// `SIGPIPE` whenever `!pipe->readers`.
    async fn write_bytes(&self, io: Io<'_>, data: &[u8]) -> SysResult<usize> {
        if data.is_empty() {
            return Ok(0);
        }
        let pipe = &self.pipe;
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
                let fits = if atomic { free >= data.len() } else { free > 0 };
                if fits {
                    let n = free.min(data.len() - written);
                    state.buf.extend(&data[written..written + n]);
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
                }
                Err(_) if written > 0 => return Ok(written),
                Err(errno) => return Err(errno.into()),
            }
        }
    }

    fn input_len(&self) -> usize {
        self.pipe.state.lock_or_recover().buf.len()
    }

    /// `pipe_set_size` with `round_pipe_size`.
    fn set_size(&self, size: u32, privileged: bool) -> Result<usize, Errno> {
        let size = size as usize;
        let size = match size {
            0 => return Err(Errno::EINVAL),
            _ if size > 1 << 31 => return Err(Errno::EINVAL),
            _ => size.max(PIPE_PAGE).next_power_of_two(),
        };
        // CAP_SYS_RESOURCE lifts the limit.
        if size > MAX_SIZE && !privileged {
            return Err(Errno::EPERM);
        }
        let mut state = self.pipe.state.lock_or_recover();
        if state.buf.len() > size {
            return Err(Errno::EBUSY);
        }
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
            if !state.buf.is_empty() {
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

    fn ioctl<'a>(
        &'a self,
        caller: &'a Caller<'a>,
        request: IoctlRequest,
        arg: u32,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(async move {
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
/// `privileged` is whether the caller has `CAP_SYS_RESOURCE`, which lifts the size limit.
pub(crate) fn fcntl(file: &OpenFile, cmd: FcntlCmd, arg: u32, privileged: bool) -> SysResult<i32> {
    let end = file.downcast::<PipeEnd>().ok_or(Errno::EBADF)?;
    let size = match cmd {
        FcntlCmd::F_GETPIPE_SZ => end.pipe.state.lock_or_recover().capacity,
        _ => end.set_size(arg, privileged)?,
    };
    Ok(size as i32)
}

impl Kernel {
    /// `do_pipe2`.
    // TODO: O_DIRECT (packet mode) is accepted but the pipe stays a byte stream.
    pub(crate) async fn sys_pipe2(
        &self,
        caller: &Caller<'_>,
        fildes: FdPair,
        flags: OpenFlags,
    ) -> SysResult<i32> {
        let allowed = OpenFlags::O_CLOEXEC | OpenFlags::O_NONBLOCK | OpenFlags::O_DIRECT;
        if !(flags & !allowed).is_empty() {
            return Err(Errno::EINVAL.into());
        }
        let (read, write) = pipe();
        // The pipe hangs off its inode, as a FIFO's does, so `/proc/<pid>/fd/<n>` reopens it.
        let path = self.pseudo.pipe(self.cred(caller).fs_owner());
        let pipe: Bound = read.pipe.clone();
        path.dentry.inode().bind(&pipe);
        let status = flags & OpenFlags::O_NONBLOCK;
        let read = OpenFile::new(
            Backing::Stream(Arc::new(read), path.clone()),
            OpenFlags::O_RDONLY | status,
        );
        let write = OpenFile::new(
            Backing::Stream(Arc::new(write), path),
            OpenFlags::O_WRONLY | status,
        );
        let cloexec = flags.contains(OpenFlags::O_CLOEXEC);
        self.install_pair(caller, fildes, [read, write], cloexec)
            .await?;
        Ok(0)
    }

    /// Installs `files` at the lowest free descriptors: all of them, or none with `EMFILE`.
    pub(crate) fn install_files<const N: usize>(
        &self,
        caller: &Caller<'_>,
        files: [OpenFile; N],
        cloexec: bool,
    ) -> Result<[Fd; N], Errno> {
        let fds = self.reserve_fds::<N>(caller, cloexec)?;
        let mut files = files.into_iter();
        Ok(fds.map(|fd| fd.install(Arc::new(files.next().expect("one file per fd")))))
    }

    /// Installs a new pair of files and writes their descriptors to the guest, as `do_pipe2`
    /// does: the numbers are reserved first and the files installed only once the copy
    /// succeeded, so a fault never closes descriptors another thread may already use.
    pub(crate) async fn install_pair(
        &self,
        caller: &Caller<'_>,
        out: FdPair,
        files: [OpenFile; 2],
        cloexec: bool,
    ) -> SysResult<()> {
        let fds = self.reserve_fds::<2>(caller, cloexec)?;
        let pair = [fds[0].fd().0, fds[1].fd().0];
        write_value(caller.memory(), GuestPtrMut::new(out.addr()), &pair)
            .await
            .map_err(|_| Errno::EFAULT)?;
        let [read, write] = files;
        let [rfd, wfd] = fds;
        rfd.install(Arc::new(read));
        wfd.install(Arc::new(write));
        Ok(())
    }
}
