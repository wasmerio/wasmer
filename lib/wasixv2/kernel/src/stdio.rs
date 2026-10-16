use std::{
    collections::VecDeque,
    io,
    sync::{Arc, Mutex},
    task::{Context, Poll, Wake, Waker},
};

use wasixv2_abi::OpenFlags;
use wasixv2_host::{ByteSink, ByteSource, Stdio};
use wasixv2_syscalls::{Errno, LocalFuture, SysErr, SysResult};
use wasixv2_vfs::{Bound, File};
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, poll_wait, wait_event};

use crate::{
    file::{Backing, FileObject, Io, OpenFile},
    pseudo::Pseudo,
};

/// The open files behind descriptors 0, 1 and 2. Each host stream looks like a pipe, with an
/// inode of its own that it hangs off, so opening `/proc/self/fd/1` (`/dev/stdout`) reaches
/// the same stream again, as it reaches the same pipe on Linux.
pub(crate) fn open_files(stdio: Stdio, pseudo: &Pseudo, owner: (u32, u32)) -> [OpenFile; 3] {
    fn open<T: FileObject + Send + Sync>(
        object: Arc<T>,
        flags: OpenFlags,
        pseudo: &Pseudo,
        owner: (u32, u32),
    ) -> OpenFile {
        let path = pseudo.pipe(owner);
        let bound: Bound = object.clone();
        path.dentry.inode().bind(&bound);
        OpenFile::new(Backing::Stream(object, path), flags)
    }
    [
        open(
            Arc::new(HostInput::new(stdio.stdin)),
            OpenFlags::O_RDONLY,
            pseudo,
            owner,
        ),
        open(
            Arc::new(HostOutput(stdio.stdout)),
            OpenFlags::O_WRONLY,
            pseudo,
            owner,
        ),
        open(
            Arc::new(HostOutput(stdio.stderr)),
            OpenFlags::O_WRONLY,
            pseudo,
            owner,
        ),
    ]
}

/// The host stream `file`, a pipe inode, belongs to, if it is one: opening it again shares the
/// stream.
pub(crate) fn reopen(file: &File) -> Option<Arc<dyn FileObject>> {
    let bound = file.path.dentry.inode().bound()?;
    let bound = match bound.downcast::<HostInput>() {
        Ok(input) => return Some(input),
        Err(bound) => bound,
    };
    bound
        .downcast::<HostOutput>()
        .ok()
        .map(|output| output as Arc<dyn FileObject>)
}

/// How much one read from the host source asks for.
const INPUT_CHUNK: usize = 4096;

/// A host input stream, buffered like a pipe's read end: what the source delivered waits here
/// until a guest read takes it, so `poll` can report it, `O_NONBLOCK` reads fail with `EAGAIN`
/// instead of waiting, and a blocked read ends on a signal (`n_tty_read`, `pipe_read`).
struct HostInput {
    source: Arc<dyn ByteSource>,
    state: Mutex<InputState>,
    /// Readers and pollers wait here; the source wakes it through `waker`.
    wait: WaitQueue,
    waker: Waker,
}

#[derive(Default)]
struct InputState {
    data: VecDeque<u8>,
    /// What the source said after the data, if not more data: end of stream or an error.
    /// Consumed by the read that reports it.
    end: Option<Result<(), Errno>>,
}

/// Turns the source's readiness into a wake of the input's queue.
struct QueueWaker(WaitQueue);

impl Wake for QueueWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.wake(PollMask::READABLE);
    }
}

impl HostInput {
    fn new(source: Arc<dyn ByteSource>) -> Self {
        let wait = WaitQueue::new();
        Self {
            source,
            state: Mutex::default(),
            waker: Waker::from(Arc::new(QueueWaker(wait.clone()))),
            wait,
        }
    }

    /// Asks the source for more input once the buffer is used up. A `Pending` source wakes the
    /// queue when that changes.
    fn fill(&self, state: &mut InputState) {
        if !state.data.is_empty() || state.end.is_some() {
            return;
        }
        let mut chunk = [0; INPUT_CHUNK];
        let mut cx = Context::from_waker(&self.waker);
        match self.source.poll_read(&mut cx, &mut chunk) {
            Poll::Ready(Ok(0)) => state.end = Some(Ok(())),
            Poll::Ready(Ok(n)) => state.data.extend(&chunk[..n]),
            Poll::Ready(Err(err)) => state.end = Some(Err(io_errno(err))),
            Poll::Pending => {}
        }
    }

    async fn read_input(&self, io: Io<'_>, buf: &mut [u8]) -> SysResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let take = || {
            let mut state = self.state.lock().unwrap();
            self.fill(&mut state);
            if !state.data.is_empty() {
                let n = buf.len().min(state.data.len());
                for (dst, src) in buf.iter_mut().zip(state.data.drain(..n)) {
                    *dst = src;
                }
                return Some(Ok(n));
            }
            if let Some(end) = state.end.take() {
                return Some(end.map(|()| 0));
            }
            io.nonblocking().then_some(Err(Errno::EAGAIN))
        };
        let result = wait_event(&self.wait, take, io.interrupt)
            .await
            .map_err(|_| SysErr::Restart)?;
        Ok(result?)
    }
}

impl Pollable for HostInput {
    /// Readable while input (or its end) waits to be read, as a pipe's read end.
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, &self.wait);
        let mut state = self.state.lock().unwrap();
        self.fill(&mut state);
        match !state.data.is_empty() || state.end.is_some() {
            true => PollMask::READABLE,
            false => PollMask::empty(),
        }
    }
}

impl FileObject for HostInput {
    fn read<'a>(&'a self, io: Io<'a>, buf: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(self.read_input(io, buf))
    }

    fn write<'a>(&'a self, _: Io<'a>, _: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        // Linux reports writes to a descriptor opened read-only as EBADF.
        Box::pin(async { Err(Errno::EBADF.into()) })
    }
}

// Host output streams have no readiness to report, so they always look ready, as a file
// without a poll operation does on Linux, and a write may then still wait for the host.
struct HostOutput(Arc<dyn ByteSink>);

impl Pollable for HostOutput {
    fn poll(&self, _: Option<&mut PollTable<'_>>) -> PollMask {
        PollMask::DEFAULT
    }
}

impl FileObject for HostOutput {
    fn read<'a>(&'a self, _: Io<'a>, _: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async { Err(Errno::EBADF.into()) })
    }

    fn write<'a>(&'a self, _: Io<'a>, data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move { Ok(self.0.write(data).await.map_err(io_errno)?) })
    }
}

fn io_errno(err: io::Error) -> Errno {
    match err.kind() {
        io::ErrorKind::BrokenPipe => Errno::EPIPE,
        io::ErrorKind::WouldBlock => Errno::EAGAIN,
        io::ErrorKind::Interrupted => Errno::EINTR,
        _ => Errno::EIO,
    }
}
