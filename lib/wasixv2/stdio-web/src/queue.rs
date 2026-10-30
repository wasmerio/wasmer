use futures_util::task::AtomicWaker;
use std::{
    collections::VecDeque,
    io,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};
use wasixv2_host::{ByteSink, ByteSource, LocalFuture};
use wasixv2_wait::{NoInterrupt, PollMask, WaitQueue, Waiter};

/// B2 merge point for a coordinator whose JS event loop is elsewhere: publish an atomic
/// ready flag/sequence and notify its owning worker. Must be nonblocking, thread safe and
/// must never touch another worker's JS handles. Register before checking queue state;
/// use B2's sequence-before-poll, predicate-recheck-before-park protocol on the worker.
/// Task futures already receive B2 scheduler Wakers through Context and need no adapter.
/// Implement with B2's Parker::notify or the coordinator's message-ready Waker.
pub trait StreamWake: Send + Sync {
    fn notify(&self);
}

impl StreamWake for Waker {
    fn notify(&self) {
        self.wake_by_ref();
    }
}

struct Shared {
    state: Mutex<State>,
    capacity: usize,
    changed: WaitQueue,
    reader: AtomicWaker,
    coordinator: Mutex<Option<Arc<dyn StreamWake>>>,
}
struct State {
    bytes: VecDeque<u8>,
    closed: bool,
    canceled: bool,
}
impl Shared {
    fn wake(&self) {
        // Like B5, release payload locks before invoking arbitrary wake callbacks.
        self.changed.wake(PollMask::empty());
        self.reader.wake();
        let wake = self.coordinator.lock().unwrap().clone();
        if let Some(wake) = wake {
            wake.notify();
        }
    }
    fn cancel(&self) {
        {
            let mut state = self.state.lock().unwrap();
            state.canceled = true;
            state.bytes.clear();
        }
        self.wake();
    }
}

/// One logical poll reader, as in host::BlockingSource. For multiple independently waiting
/// consumers use `read`, which has a cancellation-safe registration per operation.
/// Share endpoints via Arc; dropping the last Source cancels its peer's blocked writes.
pub struct Source(Arc<Shared>);
/// Share via Arc to retain a producer across guest descriptor duplication/fork. Dropping
/// the last Sink closes the stream; buffered bytes remain readable before persistent EOF.
pub struct Sink(Arc<Shared>);

/// Allocate a bounded queue in the shared runtime heap (never in guest memory).
pub fn channel(capacity: usize) -> io::Result<(Sink, Source)> {
    if capacity == 0 {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let mut bytes = VecDeque::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|_| io::Error::from(io::ErrorKind::OutOfMemory))?;
    let shared = Arc::new(Shared {
        state: Mutex::new(State {
            bytes,
            closed: false,
            canceled: false,
        }),
        capacity,
        changed: WaitQueue::new(),
        reader: AtomicWaker::new(),
        coordinator: Mutex::new(None),
    });
    Ok((Sink(shared.clone()), Source(shared)))
}

impl Source {
    /// Nonblocking coordinator pull. WouldBlock consumes nothing. A consumer acknowledges
    /// bytes by taking them; do not pull more than the downstream Web Stream can buffer.
    pub fn try_read(&self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let n = {
            let mut state = self.0.state.lock().unwrap();
            if state.canceled {
                return Err(io::ErrorKind::ConnectionAborted.into());
            }
            if state.bytes.is_empty() {
                return if state.closed {
                    Ok(0)
                } else {
                    Err(io::ErrorKind::WouldBlock.into())
                };
            }
            let n = buf.len().min(state.bytes.len());
            for (dst, src) in buf.iter_mut().zip(state.bytes.drain(..n)) {
                *dst = src;
            }
            n
        };
        self.0.wake();
        Ok(n)
    }

    /// Async read, with a separate registration for every pending operation. The executor's
    /// Waker must support cross-worker routing; do not use a worker-local JS future Waker.
    pub async fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        let waiter = Waiter::new();
        let _registration = waiter.register(&self.0.changed, PollMask::empty());
        loop {
            match self.try_read(buf) {
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    waiter.sleep(&NoInterrupt, None).await;
                }
                result => return result,
            }
        }
    }

    /// Cancel consumer (Web ReadableStream.cancel): discard queued bytes and fail producers.
    pub fn cancel(&self) {
        self.0.cancel();
    }

    /// Install independent coordinator notification before inspecting this endpoint.
    pub fn set_wake(&self, wake: Arc<dyn StreamWake>) {
        *self.0.coordinator.lock().unwrap() = Some(wake);
    }
}
impl ByteSource for Source {
    fn poll_read(&self, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        self.0.reader.register(cx.waker());
        match self.try_read(buf) {
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Poll::Pending,
            result => Poll::Ready(result),
        }
    }
}
impl Drop for Source {
    fn drop(&mut self) {
        {
            let mut state = self.0.state.lock().unwrap();
            // A consumer may retire after EOF before the producer's drain future repolls.
            // Preserve successful completion; unread data or an open producer still aborts.
            if state.closed && state.bytes.is_empty() {
                return;
            }
            state.canceled = true;
            state.bytes.clear();
        }
        self.0.wake();
    }
}

impl Sink {
    /// Nonblocking coordinator push. Short writes are normal; retry only the remaining
    /// suffix after capacity notification. Unlike a pipe, this transport has no PIPE_BUF.
    /// Linux fs/pipe.c:anon_pipe_read/anon_pipe_write are the kernel pipe authority, not this queue.
    pub fn try_write(&self, data: &[u8]) -> io::Result<usize> {
        if data.is_empty() {
            return Ok(0);
        }
        let n = {
            let mut state = self.0.state.lock().unwrap();
            if state.canceled {
                return Err(io::ErrorKind::ConnectionAborted.into());
            }
            if state.closed {
                return Err(io::ErrorKind::BrokenPipe.into());
            }
            let n = data.len().min(self.0.capacity - state.bytes.len());
            if n == 0 {
                return Err(io::ErrorKind::WouldBlock.into());
            }
            state.bytes.extend(&data[..n]);
            n
        };
        self.0.wake();
        Ok(n)
    }

    /// Close the producer (Web WritableStream.close). Accepted bytes precede persistent EOF.
    pub fn close(&self) {
        self.0.state.lock().unwrap().closed = true;
        self.0.wake();
    }
    /// Abort both sides (Web WritableStream.abort or infrastructure failure).
    pub fn abort(&self) {
        self.0.cancel();
    }

    /// Wait for all currently queued bytes to be taken. Close first for an exit barrier;
    /// otherwise more writes may follow. Cancellation reports failure even if it empties the queue.
    pub async fn drained(&self) -> io::Result<()> {
        let waiter = Waiter::new();
        let _registration = waiter.register(&self.0.changed, PollMask::empty());
        loop {
            {
                let state = self.0.state.lock().unwrap();
                if state.canceled {
                    return Err(io::ErrorKind::ConnectionAborted.into());
                }
                if state.bytes.is_empty() {
                    return Ok(());
                }
            }
            waiter.sleep(&NoInterrupt, None).await;
        }
    }
    /// Capacity notification for a coordinator feeding stdin.
    pub fn set_wake(&self, wake: Arc<dyn StreamWake>) {
        *self.0.coordinator.lock().unwrap() = Some(wake);
    }
}
impl ByteSink for Sink {
    fn write<'a>(&'a self, data: &'a [u8]) -> LocalFuture<'a, io::Result<usize>> {
        Box::pin(async move {
            let waiter = Waiter::new();
            let _registration = waiter.register(&self.0.changed, PollMask::empty());
            loop {
                match self.try_write(data) {
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        waiter.sleep(&NoInterrupt, None).await;
                    }
                    result => return result,
                }
            }
        })
    }
}
impl Drop for Sink {
    fn drop(&mut self) {
        self.close();
    }
}
