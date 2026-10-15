//! Anonymous pipes, as Linux's `fs/pipe.c`, with a byte buffer instead of page slots.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use wasixv2_abi::{FcntlCmd, FdPair, GuestPtrMut, IoctlRequest, OpenFlags};
use wasixv2_syscalls::{Caller, Errno, Fd, LocalFuture, SysErr, SysResult, write_value};
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, poll_wait, wait_event};

use crate::{
    Kernel,
    file::{Backing, FileObject, Io, OpenFile},
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
}

impl Drop for PipeEnd {
    /// `pipe_release`: wakes everybody, so they see EOF or EPIPE.
    fn drop(&mut self) {
        {
            let mut state = self.pipe.state.lock().unwrap();
            match self.writer {
                true => state.writers -= 1,
                false => state.readers -= 1,
            }
        }
        self.pipe.rd_wait.wake(PollMask::empty());
        self.pipe.wr_wait.wake(PollMask::empty());
    }
}

/// A new pipe's read and write ends.
pub(crate) fn pipe() -> (PipeEnd, PipeEnd) {
    let pipe = Arc::new(Pipe {
        state: Mutex::new(State {
            buf: VecDeque::new(),
            capacity: DEFAULT_SIZE,
            readers: 1,
            writers: 1,
        }),
        rd_wait: WaitQueue::new(),
        wr_wait: WaitQueue::new(),
    });
    let read = PipeEnd {
        pipe: pipe.clone(),
        writer: false,
    };
    (read, PipeEnd { pipe, writer: true })
}

// ---- fs: FIFOs (`fs/fifo.rs`) ----

/// A pipe with no ends yet, for a FIFO inode's first open.
pub(crate) fn fifo_pipe() -> Arc<Pipe> {
    Arc::new(Pipe {
        state: Mutex::new(State {
            buf: VecDeque::new(),
            capacity: DEFAULT_SIZE,
            readers: 0,
            writers: 0,
        }),
        rd_wait: WaitQueue::new(),
        wr_wait: WaitQueue::new(),
    })
}

impl Pipe {
    /// The number of open read and write ends.
    pub(crate) fn ends(&self) -> (usize, usize) {
        let state = self.state.lock().unwrap();
        (state.readers, state.writers)
    }

    /// Waits until `ready` holds for the end counts (`wait_for_partner`).
    pub(crate) async fn wait_ends(
        &self,
        ready: impl Fn(usize, usize) -> bool,
        interrupt: &dyn wasixv2_wait::Interrupt,
    ) -> SysResult<()> {
        let check = || {
            let (readers, writers) = self.ends();
            ready(readers, writers).then_some(())
        };
        wait_event(&self.rd_wait, check, interrupt)
            .await
            .map_err(|_| SysErr::Restart)
    }
}

impl PipeEnd {
    /// Opens another end of `pipe` and wakes whoever waits for one.
    pub(crate) fn attach(pipe: &Arc<Pipe>, writer: bool) -> Self {
        {
            let mut state = pipe.state.lock().unwrap();
            match writer {
                true => state.writers += 1,
                false => state.readers += 1,
            }
        }
        pipe.rd_wait.wake(PollMask::empty());
        pipe.wr_wait.wake(PollMask::empty());
        Self {
            pipe: pipe.clone(),
            writer,
        }
    }
}

impl PipeEnd {
    /// `pipe_read`.
    async fn read_bytes(&self, io: Io<'_>, buf: &mut [u8]) -> SysResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let pipe = &self.pipe;
        let try_read = || {
            let mut state = pipe.state.lock().unwrap();
            if !state.buf.is_empty() {
                let n = buf.len().min(state.buf.len());
                for (dst, src) in buf.iter_mut().zip(state.buf.drain(..n)) {
                    *dst = src;
                }
                return Some(Ok(n));
            }
            if state.writers == 0 {
                return Some(Ok(0));
            }
            io.nonblocking().then_some(Err(Errno::EAGAIN))
        };
        let result = wait_event(&pipe.rd_wait, try_read, io.interrupt)
            .await
            .map_err(|_| SysErr::Restart)?;
        if matches!(result, Ok(n) if n > 0) {
            pipe.wr_wait.wake(PollMask::WRITABLE);
        }
        Ok(result?)
    }

    /// `pipe_write`. Writes of at most `PIPE_BUF` bytes go in whole or wait; larger ones go in
    /// as room appears and return early only when non-blocking or interrupted.
    async fn write_bytes(&self, io: Io<'_>, data: &[u8]) -> SysResult<usize> {
        if data.is_empty() {
            return Ok(0);
        }
        let pipe = &self.pipe;
        let atomic = data.len() <= PIPE_BUF;
        let mut written = 0;
        loop {
            let step = || {
                let mut state = pipe.state.lock().unwrap();
                if state.readers == 0 {
                    // TODO: raise SIGPIPE once signals are delivered.
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
        self.pipe.state.lock().unwrap().buf.len()
    }

    /// `pipe_set_size` with `round_pipe_size`.
    fn set_size(&self, size: u32) -> Result<usize, Errno> {
        let size = size as usize;
        let size = match size {
            0 => return Err(Errno::EINVAL),
            _ if size > 1 << 31 => return Err(Errno::EINVAL),
            _ => size.max(PIPE_PAGE).next_power_of_two(),
        };
        // The caller has no CAP_SYS_RESOURCE.
        if size > MAX_SIZE {
            return Err(Errno::EPERM);
        }
        let mut state = self.pipe.state.lock().unwrap();
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
        let state = self.pipe.state.lock().unwrap();
        let mut mask = PollMask::empty();
        if !self.writer {
            if !state.buf.is_empty() {
                mask |= PollMask::READABLE;
            }
            if state.writers == 0 {
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
        Box::pin(self.read_bytes(io, buf))
    }

    fn write<'a>(&'a self, io: Io<'a>, data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(self.write_bytes(io, data))
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
pub(crate) fn fcntl(file: &OpenFile, cmd: FcntlCmd, arg: u32) -> SysResult<i32> {
    let end = file.downcast::<PipeEnd>().ok_or(Errno::EBADF)?;
    let size = match cmd {
        FcntlCmd::F_GETPIPE_SZ => end.pipe.state.lock().unwrap().capacity,
        _ => end.set_size(arg)?,
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
        let status = flags & OpenFlags::O_NONBLOCK;
        let read = OpenFile::new(
            Backing::Stream(Arc::new(read)),
            OpenFlags::O_RDONLY | status,
        );
        let write = OpenFile::new(
            Backing::Stream(Arc::new(write)),
            OpenFlags::O_WRONLY | status,
        );
        let cloexec = flags.contains(OpenFlags::O_CLOEXEC);
        let fds = self.install_files(caller, [read, write], cloexec)?;
        self.copy_out_fd_pair(caller, fildes, fds).await?;
        Ok(0)
    }

    /// Installs `files` at the lowest free descriptors: all of them, or none with `EMFILE`.
    pub(crate) fn install_files<const N: usize>(
        &self,
        caller: &Caller<'_>,
        files: [OpenFile; N],
        cloexec: bool,
    ) -> Result<[Fd; N], Errno> {
        let table = self.files(caller.task());
        let mut table = table.lock().unwrap();
        if !table.has_room(N) {
            return Err(Errno::EMFILE);
        }
        Ok(files.map(|file| {
            table
                .install(Arc::new(file), cloexec)
                .expect("room checked above")
        }))
    }

    /// Writes a new descriptor pair to the guest. On a fault the descriptors are closed again,
    /// as Linux only installs them after the copy succeeded.
    pub(crate) async fn copy_out_fd_pair(
        &self,
        caller: &Caller<'_>,
        out: FdPair,
        fds: [Fd; 2],
    ) -> SysResult<()> {
        let pair = [fds[0].0, fds[1].0];
        let result = write_value(caller.memory(), GuestPtrMut::new(out.addr()), &pair).await;
        if result.is_err() {
            let table = self.files(caller.task());
            let mut table = table.lock().unwrap();
            let removed = fds.map(|fd| table.remove(fd));
            drop(table);
            drop(removed);
            return Err(Errno::EFAULT.into());
        }
        Ok(())
    }
}
