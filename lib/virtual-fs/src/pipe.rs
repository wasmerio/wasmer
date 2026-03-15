use std::io::{IoSlice, SeekFrom};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Poll};
use tokio::io::ReadBuf;
use virtual_mio::{InterestHandler, InterestType};

use crate::{ArcFile, FsError, VirtualFile};


#[derive(Debug)]
struct PipeBuffer {
    buf: std::collections::VecDeque<u8>,
    capacity: usize,
    write_closed: bool,
    read_closed: bool,
    ///
    read_waker: Option<std::task::Waker>,
    write_waker: Option<std::task::Waker>,
    /// Conditional Variable for OS threads synchronization
    /// Used to wake up threads blocked on read when data is written.
    not_empty: Arc<std::sync::Condvar>,
    /// Conditional Variable for OS threads synchronization
    /// Used to wake up threads blocked on write when space is freed up.
    not_full: Arc<std::sync::Condvar>,
    ///
    interest_handler: Option<Box<dyn InterestHandler>>,
}

impl PipeBuffer{
    fn new(capacity: usize) -> PipeBuffer{
        PipeBuffer {
            buf: std::collections::VecDeque::with_capacity(capacity),
            capacity,
            write_closed: false,
            read_closed: false,
            read_waker: None,
            write_waker: None,
            interest_handler: None,
            not_empty: Arc::new(std::sync::Condvar::new()),
            not_full: Arc::new(std::sync::Condvar::new()),
        }
    }

    fn close_write(&mut self){
        self.write_closed = true;
        if let Some(w) = self.read_waker.take() { w.wake(); }
        self.not_empty.notify_all();
    }
    fn close_read(&mut self) {
        self.read_closed = true;
        if let Some(w) = self.write_waker.take() { w.wake(); }
        self.not_full.notify_all();
    }
    fn is_read_closed(&self) -> bool{
        self.read_closed
    }
    fn is_write_closed(&self) -> bool{
        self.write_closed
    }
    fn is_empty(&self) -> bool{
        self.buf.is_empty()
    }
    fn len(&self) -> usize{
        self.buf.len()
    }
    fn available_capacity(&self) -> usize{
        self.capacity - self.len()
    }
    fn write_bytes(&mut self, data: &[u8]) -> usize {
        let space = self.available_capacity();
        let to_write = std::cmp::min(space, data.len());
        self.buf.extend(&data[..to_write]);

        // If a reader is waiting for bytes, wake them
        if let Some(w) = self.read_waker.take() { w.wake(); }
        // If a thread is waiting for bytes, notify them
        self.not_empty.notify_all();
        // If an interest handler is registered, fire it
    self.fire_interest_readable();

        to_write
    }

    /// Drains bytes from the buffer into `buf`, up to the minimum of `buf.len()` and the amount of
    /// data available.
    /// # Returns
    /// * `usize` - the number of bytes read into `buf`
    fn read_bytes(&mut self, buf: &mut [u8]) -> usize{
        let to_read = std::cmp::min(self.buf.len(), buf.len());
        for i in 0..to_read {
            if let Some(byte) = self.buf.pop_front() {
                buf[i] = byte;
            } else {
                break;
            }
        }

        // If a writer is waiting for space, wake them
        if let Some(w) = self.write_waker.take() { w.wake(); }
        // If a thread is waiting for space, notify them
        self.not_full.notify_all();
        // If an interest handler is registered, fire it
        self.fire_interest_writable();

        to_read
    }

    /// Reads bytes from the buffer into the provided `ReadBuf`, advancing its internal cursor.
    /// # Returns
    /// * `usize` - the number of bytes read into `buf`
    fn read_into_tokio_buf(&mut self, buf: &mut ReadBuf<'_>){
        let to_read = std::cmp::min(self.buf.len(), buf.remaining());
        for _ in 0..to_read {
            if let Some(byte) = self.buf.pop_front() {
                buf.put_slice(&[byte]);
            } else {
                break;
            }
        }
        // If a writer is waiting for space, wake them
        if let Some(w) = self.write_waker.take() { w.wake(); }
        // If a thread is waiting for space, notify them
        self.not_full.notify_all();
        // If an interest handler is registered, fire it
        self.fire_interest_writable();

    }
    fn store_read_waker(&mut self, waker: std::task::Waker){
        self.read_waker = Some(waker);
    }
    fn store_write_waker(&mut self, waker: std::task::Waker){
        self.write_waker = Some(waker);
    }

    fn set_interest_handler(&mut self, handler: Box<dyn InterestHandler>){
        self.interest_handler = Some(handler);
    }
    fn take_interest_handler(&mut self) -> Option<Box<dyn InterestHandler>>{
        self.interest_handler.take()
    }
    fn fire_interest_readable(&mut self){
        if let Some(handler) = self.interest_handler.as_mut() {
            handler.push_interest(InterestType::Readable);
        }
    }
    fn fire_interest_writable(&mut self){
        if let Some(handler) = self.interest_handler.as_mut() {
            handler.push_interest(InterestType::Writable);
        }
    }
}

type SharedBuf = Arc<std::sync::Mutex<PipeBuffer>>;

// --------------------------------------------------------------------
// Transmitter / Receiver
// --------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PipeTx {
    buf: SharedBuf,
}
#[derive(Debug, Clone)]
pub struct PipeRx {
    buf: SharedBuf,
}

impl PipeTx {
    pub fn close(&mut self) {
        // PipeBuffer::close_write(&mut self)
        //   sets write_closed = true
        //   wakes read_waker so poll_read sees EOF
        //   notifies condvar so blocking readers unblock and see EOF
        let mut buf = self.buf.lock().expect("pipe buffer mutex was poisoned");
        buf.close_write();
    }

    /// Returns how many bytes can be written right now without blocking.
    pub fn poll_write_ready(self: Pin<&mut Self>) -> Poll<std::io::Result<usize>> {
        let buf = self.buf.lock().expect("pipe buffer mutex was poisoned");
        if buf.is_read_closed() {
            // No readers — any write would BrokenPipe
            return Poll::Ready(Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe)));
        }
        // PipeBuffer::available_capacity(&self) -> usize
        //   capacity - data.len()
        let space = buf.available_capacity();
        if space == 0 {
            Poll::Ready(Ok(0))
        } else {
            Poll::Ready(Ok(space))
        }
    }
}

impl std::io::Write for PipeTx {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut inner = self.buf.lock().expect("pipe buffer mutex was poisoned");

        if inner.is_read_closed() {
            return Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe));
        }

        // POSIX: atomic writes <= PIPE_BUF must be all-or-nothing
        // If there isn't enough space right now, return WouldBlock
        if inner.available_capacity() < buf.len().min(PIPE_BUF) {
            return Err(std::io::Error::from(std::io::ErrorKind::WouldBlock));
        }

        Ok(inner.write_bytes(buf))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        // Pipes have no internal flush concept
        Ok(())
    }
}

impl tokio::io::AsyncWrite for PipeTx {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let mut inner = self.buf.lock().expect("pipe buffer mutex was poisoned");

        if inner.is_read_closed() {
            return Poll::Ready(Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe)));
        }

        if inner.available_capacity() < buf.len().min(PIPE_BUF) {
            // Full — register waker and suspend
            // PipeBuffer::store_write_waker(&mut self, waker: Waker)
            inner.store_write_waker(cx.waker().clone());
            return Poll::Pending;
        }

        let written = inner.write_bytes(buf);

        Poll::Ready(Ok(written))
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        self.close();
        Poll::Ready(Ok(()))
    }
}

impl std::io::Seek for PipeTx {
    fn seek(&mut self, _: SeekFrom) -> std::io::Result<u64> { Ok(0) }
}

impl tokio::io::AsyncSeek for PipeTx {
    fn start_seek(self: Pin<&mut Self>, _: SeekFrom) -> std::io::Result<()> { Ok(()) }
    fn poll_complete(self: Pin<&mut Self>, _: &mut std::task::Context<'_>) -> Poll<std::io::Result<u64>> {
        Poll::Ready(Ok(0))
    }
}


impl PipeRx {
    pub fn close(&mut self) {
        // PipeBuffer::close_read(&mut self)
        //   sets read_closed = true
        //   wakes write_waker so poll_write sees BrokenPipe
        //   notifies condvar so blocking writers unblock
        let mut buf = self.buf.lock().expect("pipe buffer mutex was poisoned");
        buf.close_read();
    }

    /// Tries to read data into `buf` without blocking.
    /// # Returns
    /// * `Option<usize>`
    ///     - `Some(n)` if n bytes were read into `buf`
    ///     - `Some(0)` if the write end is closed and there is no more data (EOF)
    ///     - `None` if there is no data available right now but the write end is still open
    ///             (would block)
    pub fn try_read(&mut self, buf: &mut [u8]) -> Option<usize> {
        let mut inner = self.buf.lock().expect("pipe buffer mutex was poisoned");
        if inner.is_empty() {
            return if inner.is_write_closed() { Some(0) } else { None };
        }
        Some(inner.read_bytes(buf))
    }

    /// Called by VirtualFile::poll_read_ready — tells the event loop
    /// whether data is available, and if not, arms the interest handler
    /// so it fires when data arrives.
    pub fn poll_read_ready(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<usize>> {
        let mut inner = self.buf.lock().expect("pipe buffer mutex was poisoned");

        if !inner.is_empty() {
            return Poll::Ready(Ok(inner.len()));
        }

        if inner.is_write_closed() {
            return Poll::Ready(Ok(0)); // EOF
        }

        // We register the waker
        inner.store_read_waker(cx.waker().clone());
        Poll::Pending
    }

    pub fn set_interest_handler(&self, handler: Box<dyn InterestHandler>) {
        self.buf.lock().expect("pipe buffer mutex was poisoned").set_interest_handler(handler);
    }

    pub fn remove_interest_handler(&self) -> Option<Box<dyn InterestHandler>> {
        self.buf.lock().expect("pipe buffer mutex was poisoned").take_interest_handler()
    }
}

impl std::io::Read for PipeRx {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut inner = self.buf.lock().expect("pipe buffer mutex was poisoned");
        loop {
            if !inner.is_empty() {
                return Ok(inner.read_bytes(buf)); // ← break out and read
            }
            if inner.is_write_closed() {
                return Ok(0);
            }
            let cv = inner.not_empty.clone();
            inner = cv.wait(inner).expect("pipe buffer mutex was poisoned while waiting for data");
        }
    }
}

impl tokio::io::AsyncRead for PipeRx {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let mut pipe_buf = self.buf.lock().expect("pipe buffer mutex was poisoned");
        if !pipe_buf.is_empty() {
            // PipeBuffer::read_into_tokio_buf(&mut self, buf: &mut ReadBuf<'_>) -> usize
            //   drains bytes into the ReadBuf, advances its cursor
            pipe_buf.read_into_tokio_buf(buf);
            return Poll::Ready(Ok(()));
        }
        if pipe_buf.is_write_closed() {
            return Poll::Ready(Ok(())); // EOF
        }

        // We register the waker
        pipe_buf.store_read_waker(cx.waker().clone());

        Poll::Pending
    }
}

impl std::io::Seek for PipeRx {
    fn seek(&mut self, _: SeekFrom) -> std::io::Result<u64> { Ok(0) }
}

impl tokio::io::AsyncSeek for PipeRx {
    fn start_seek(self: Pin<&mut Self>, _: SeekFrom) -> std::io::Result<()> { Ok(()) }
    fn poll_complete(self: Pin<&mut Self>, _: &mut std::task::Context<'_>) -> Poll<std::io::Result<u64>> {
        Poll::Ready(Ok(0))
    }
}

// --------------------------------------------------------------------
// Pipe
// --------------------------------------------------------------------



/// POSIX atomicity guarantee: writes of this many bytes or fewer
/// are guaranteed to be atomic (not interleaved with other writes).
/// Matches Linux <limits.h>.
pub const PIPE_BUF: usize = 4096;

/// Total pipe buffer capacity in bytes. Matches the Linux default.
/// This is how much data the pipe can hold before writers block.
pub const PIPE_CAPACITY: usize = 65536;

// Each pipe end is separately cloneable. The overall pipe
// remains open as long as at least one tx end and one rx
// end are still alive.
// As such, closing a pipe isn't a well-defined operation,
// since more references to the ends may still be alive.
#[derive(Debug, Clone)]
pub struct Pipe {
    /// Transmit side of the pipe
    send: PipeTx,
    /// Receive side of the pipe
    recv: PipeRx,
}
impl Pipe {
    pub fn new() -> Self {
        let buf = Arc::new(std::sync::Mutex::new(PipeBuffer::new(PIPE_CAPACITY)));
        Self {
            send: PipeTx {
                buf: buf.clone(),
            },
            recv: PipeRx {
                buf,
            },
        }
    }

    pub fn channel() -> (Pipe, Pipe) {
        let (tx1, rx1) = Pipe::new().split();
        let (tx2, rx2) = Pipe::new().split();

        let end1 = Pipe::combine(tx1, rx2);
        let end2 = Pipe::combine(tx2, rx1);
        (end1, end2)
    }

    pub fn split(self) -> (PipeTx, PipeRx) {
        (self.send, self.recv)
    }

    pub fn combine(tx: PipeTx, rx: PipeRx) -> Self {
        Self { send: tx, recv: rx }
    }

    pub fn try_read(&mut self, buf: &mut [u8]) -> Option<usize> {
        self.recv.try_read(buf)
    }

    pub fn set_interest_handler(&self, handler: Box<dyn InterestHandler>) {
        self.recv.set_interest_handler(handler);
    }

    pub fn remove_interest_handler(&self) -> Option<Box<dyn InterestHandler>> {
        self.recv.remove_interest_handler()
    }

    pub fn close(&mut self) {
        self.send.close();
        self.recv.close();
    }
}

impl Default for Pipe {
    fn default() -> Self {
        Self::new()
    }
}
impl std::io::Write for Pipe {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.send.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.send.flush()
    }
}

impl std::io::Read for Pipe {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.recv.read(buf)
    }
}

impl tokio::io::AsyncRead for Pipe {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = Pin::new(&mut self.recv);
        this.poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for Pipe {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = Pin::new(&mut self.send);
        this.poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Result<(), std::io::Error>> {
        let this = Pin::new(&mut self.send);
        this.poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        let this = Pin::new(&mut self.send);
        this.poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<Result<usize, std::io::Error>> {
        let this = Pin::new(&mut self.send);
        this.poll_write_vectored(cx, bufs)
    }
}

impl tokio::io::AsyncSeek for Pipe {
    fn start_seek(mut self: Pin<&mut Self>, position: SeekFrom) -> std::io::Result<()> {
        let this = Pin::new(&mut self.recv);
        this.start_seek(position)
    }

    fn poll_complete(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<std::io::Result<u64>> {
        let this = Pin::new(&mut self.recv);
        this.poll_complete(cx)
    }
}

impl VirtualFile for Pipe {
    /// the last time the file was accessed in nanoseconds as a UNIX timestamp
    fn last_accessed(&self) -> u64 {
        0
    }

    /// the last time the file was modified in nanoseconds as a UNIX timestamp
    fn last_modified(&self) -> u64 {
        0
    }

    /// the time at which the file was created in nanoseconds as a UNIX timestamp
    fn created_time(&self) -> u64 {
        0
    }

    /// the size of the file in bytes
    fn size(&self) -> u64 {
        0
    }

    /// Change the size of the file, if the `new_size` is greater than the current size
    /// the extra bytes will be allocated and zeroed
    fn set_len(&mut self, _new_size: u64) -> crate::Result<()> {
        Ok(())
    }

    /// Request deletion of the file
    fn unlink(&mut self) -> Result<(), FsError> {
        Ok(())
    }

    /// Indicates if the file is opened or closed. This function must not block
    /// Defaults to a status of being constantly open
    fn is_open(&self) -> bool {
        let buf = self.send.buf.lock().expect("pipe buffer mutex was poisoned");
        !buf.is_write_closed() && !buf.is_read_closed()
    }

    /// Polls the file for when there is data to be read
    fn poll_read_ready(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.recv).poll_read_ready(cx)
    }

    /// Polls the file for when it is available for writing
    fn poll_write_ready(
        mut self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.send).poll_write_ready()
    }
}

impl std::io::Seek for Pipe {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        self.recv.seek(from)
    }
}



// --------------------------------------------------------------------
// Duplex Pipe
// --------------------------------------------------------------------


/// A pair of pipes that are connected together.
#[derive(Clone, Debug)]
pub struct DuplexPipe {
    front: Pipe,
    back: Pipe,
}

impl DuplexPipe {
    /// Get the sender pipe.
    pub fn front(&self) -> &Pipe {
        &self.front
    }

    /// Get the receiver pipe.
    pub fn back(&self) -> &Pipe {
        &self.back
    }

    /// Get the mutable sender pipe.
    pub fn front_mut(&mut self) -> &mut Pipe {
        &mut self.front
    }

    /// Get the receiver pipe.
    pub fn back_mut(&mut self) -> &mut Pipe {
        &mut self.back
    }

    /// Split into two pipes that are connected to each other
    pub fn split(self) -> (Pipe, Pipe) {
        (self.front, self.back)
    }

    /// Combines two ends of a duplex pipe back together again
    pub fn combine(front: Pipe, back: Pipe) -> Self {
        Self { front, back }
    }

    pub fn reverse(self) -> Self {
        let (front, back) = self.split();
        Self::combine(back, front)
    }
}

impl Default for DuplexPipe {
    fn default() -> Self {
        Self::new()
    }
}

impl DuplexPipe {
    pub fn new() -> DuplexPipe {
        let (end1, end2) = Pipe::channel();
        Self {
            front: end1,
            back: end2,
        }
    }
}

/// Shared version of BidiPipe for situations where you need
/// to emulate the old behaviour of `Pipe` (both send and recv on one channel).
pub type WasiBidirectionalSharedPipePair = ArcFile<DuplexPipe>;



#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    // ── Basic Pipe ────────────────────────────────────────────────

    #[tokio::test]
    async fn pipe_basic_read_write() {
        let mut pipe = Pipe::new();
        pipe.write_all(b"hello").unwrap();

        let mut buf = [0u8; 5];
        pipe.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"hello");
    }

    #[tokio::test]
    async fn pipe_read_from_closed_write_end_returns_eof() {
        let (mut tx, mut rx) = Pipe::new().split();
        tx.write_all(b"data").unwrap();
        tx.close();

        let mut buf = [0u8; 4];
        rx.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"data");

        let n = rx.read(&mut buf).unwrap();
        assert_eq!(n, 0, "expected EOF after write end closed");
    }

    #[tokio::test]
    async fn pipe_read_returns_eof_immediately_when_write_end_closed_and_empty() {
        let (mut tx, mut rx) = Pipe::new().split();
        tx.close();

        let mut buf = [0u8; 4];
        let n = rx.read(&mut buf).unwrap();
        assert_eq!(n, 0, "expected immediate EOF on empty closed pipe");
    }

    #[tokio::test]
    async fn pipe_write_to_closed_read_end_returns_broken_pipe() {
        let (mut tx, mut rx) = Pipe::new().split();
        rx.close();

        let result = tx.write(b"hello");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::BrokenPipe);
    }

    #[tokio::test]
    async fn pipe_write_returns_would_block_when_full() {
        let mut pipe = Pipe::new();
        let mut bytes_written = 0usize;
        #[allow(unused_assignments)]
        let mut got_would_block = false;

        loop {
            match pipe.write(b"x") {
                Ok(n) => {
                    bytes_written += n;
                    if bytes_written > 2 * 1024 * 1024 {
                        panic!(
                            "BUG: wrote {} bytes without WouldBlock — pipe buffer is unbounded",
                            bytes_written
                        );
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    got_would_block = true;
                    break;
                }
                Err(e) => panic!("unexpected error: {}", e),
            }
        }

        assert!(got_would_block, "expected WouldBlock when pipe buffer is full");
        assert!(
            bytes_written <= PIPE_CAPACITY,
            "pipe buffer should be bounded at {}B, but accepted {} bytes",
            PIPE_CAPACITY,
            bytes_written
        );
    }

    #[tokio::test]
    async fn pipe_write_resumes_after_drain() {
        let mut pipe = Pipe::new();

        loop {
            match pipe.write(b"x") {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => panic!("unexpected error filling pipe: {}", e),
            }
        }

        let mut drain_buf = [0u8; 1024];
        let drained = pipe.read(&mut drain_buf).unwrap();
        assert!(drained > 0, "expected to drain some bytes");

        let result = pipe.write(b"y");
        assert!(
            result.is_ok(),
            "expected write to succeed after draining, got: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn pipe_multiple_writes_and_reads_preserve_order() {
        let mut pipe = Pipe::new();
        pipe.write_all(b"first").unwrap();
        pipe.write_all(b"second").unwrap();

        let mut buf = [0u8; 11];
        let mut total = 0;
        while total < 11 {
            total += pipe.read(&mut buf[total..]).unwrap();
        }
        assert_eq!(&buf, b"firstsecond");
    }

    #[tokio::test]
    async fn pipe_is_open_reflects_write_end_state() {
        let mut pipe = Pipe::new();
        assert!(pipe.is_open());
        pipe.send.close();
        assert!(!pipe.is_open(), "pipe should be closed after send end closes");
    }

    #[tokio::test]
    async fn pipe_is_open_reflects_read_end_state() {
        let mut pipe = Pipe::new();
        assert!(pipe.is_open());
        pipe.recv.close();
        assert!(!pipe.is_open(), "pipe should be closed after recv end closes");
    }

    #[tokio::test]
    async fn pipe_partial_read_leaves_remainder() {
        let mut pipe = Pipe::new();
        pipe.write_all(b"hello").unwrap();

        let mut buf = [0u8; 3];
        let n = pipe.read(&mut buf).unwrap();
        assert!(n > 0);
        assert_eq!(&buf[..n], &b"hello"[..n]);

        // Remainder is still readable
        let mut rest = [0u8; 5];
        let m = pipe.read(&mut rest).unwrap();
        assert_eq!(&buf[..n], &b"hello"[..n]);
        assert_eq!(n + m, 5);
    }

    #[tokio::test]
    async fn pipe_write_exactly_pipe_buf_succeeds_when_space_available() {
        let mut pipe = Pipe::new();
        let data = vec![0u8; PIPE_BUF];
        let result = pipe.write(&data);
        assert!(result.is_ok(), "write of exactly PIPE_BUF bytes should succeed");
        assert_eq!(result.unwrap(), PIPE_BUF);
    }

    #[tokio::test]
    async fn pipe_write_larger_than_pipe_buf_may_partial_write() {
        let mut pipe = Pipe::new();

        // Fill until only PIPE_BUF - 1 bytes remain
        let target_remaining = PIPE_BUF - 1;
        let mut filled = 0usize;
        while filled < PIPE_CAPACITY - target_remaining {
            match pipe.write(b"x") {
                Ok(n) => filled += n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => panic!("unexpected: {}", e),
            }
        }

        // Available space is now less than PIPE_BUF
        // A write of exactly PIPE_BUF must fail atomically
        let data = vec![0u8; PIPE_BUF];
        let result = pipe.write(&data);
        assert!(
            result.is_err(),
            "write of PIPE_BUF bytes must fail when less than PIPE_BUF space available"
        );
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::WouldBlock);

    }

    #[tokio::test]
    async fn pipe_try_read_returns_none_when_empty() {
        let (_, mut rx) = Pipe::new().split();
        let mut buf = [0u8; 4];
        assert_eq!(rx.try_read(&mut buf), None, "expected None on empty pipe");
    }

    #[tokio::test]
    async fn pipe_try_read_returns_zero_on_closed_empty_pipe() {
        let (mut tx, mut rx) = Pipe::new().split();
        tx.close();
        let mut buf = [0u8; 4];
        assert_eq!(rx.try_read(&mut buf), Some(0), "expected EOF (Some(0))");
    }

    #[tokio::test]
    async fn pipe_try_read_returns_data_when_available() {
        let (mut tx, mut rx) = Pipe::new().split();
        tx.write_all(b"hi").unwrap();
        let mut buf = [0u8; 2];
        assert_eq!(rx.try_read(&mut buf), Some(2));
        assert_eq!(&buf, b"hi");
    }

    #[tokio::test]
    async fn pipe_close_both_ends() {
        let mut pipe = Pipe::new();
        pipe.close();
        assert!(!pipe.is_open());

        let result = pipe.write(b"x");
        assert!(result.is_err());
    }

    // ── POSIX atomicity ───────────────────────────────────────────

    #[tokio::test]
    async fn pipe_atomic_write_all_or_nothing_at_pipe_buf_boundary() {
        let mut pipe = Pipe::new();

        // We need available_capacity < PIPE_BUF
        // So we must fill at least PIPE_CAPACITY - PIPE_BUF + 1 bytes
        let fill_target = PIPE_CAPACITY - PIPE_BUF + 1;
        let mut filled = 0usize;
        while filled < fill_target {
            match pipe.write(b"x") {
                Ok(n) => filled += n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => panic!("unexpected fill error: {}", e),
            }
        }

        // Sanity check — if we hit WouldBlock before fill_target
        // the pipe is full anyway, which also satisfies available < PIPE_BUF
        let buf_inner = pipe.send.buf.lock().expect("pipe buffer mutex was poisoned");
        let available = buf_inner.available_capacity();
        assert!(
            available < PIPE_BUF,
            "test setup failed: {} bytes available, need < {}",
            available, PIPE_BUF
        );
        drop(buf_inner);

        let atomic_data = vec![0xCDu8; PIPE_BUF];
        let result = pipe.write(&atomic_data);
        assert!(
            result.is_err(),
            "atomic write must fail when only {} bytes available, got Ok",
            available
        );
        assert_eq!(
        result.unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock,
    );
    }

    #[tokio::test]
    async fn pipe_atomic_write_fails_entirely_when_insufficient_space() {
        let mut pipe = Pipe::new();

        let fill_target = PIPE_CAPACITY - PIPE_BUF + 1;
        let mut filled = 0usize;
        while filled < fill_target {
            match pipe.write(b"x") {
                Ok(n) => filled += n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => panic!("unexpected fill error: {}", e),
            }
        }

        let available = pipe.send.buf.lock().unwrap().available_capacity();

        assert!(
            available < PIPE_BUF,
            "test setup failed: {} bytes available, need < {}",
            available,
            PIPE_BUF
        );

        let atomic_data = vec![0xCDu8; PIPE_BUF];
        let result = pipe.write(&atomic_data);
        assert!(
            result.is_err(),
            "atomic write must fail when only {} bytes available, got Ok",
            available
        );
    }

    // ── Capacity ──────────────────────────────────────────────────

    #[tokio::test]
    async fn pipe_capacity_is_exactly_pipe_capacity() {
        let mut pipe = Pipe::new();
        let mut total = 0usize;

        // Write 1 byte at a time to count exact capacity
        loop {
            match pipe.write(b"x") {
                Ok(n) => total += n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => panic!("unexpected: {}", e),
            }
        }

        assert_eq!(
            total, PIPE_CAPACITY,
            "pipe capacity should be exactly PIPE_CAPACITY bytes"
        );
    }

    // ── DuplexPipe ────────────────────────────────────────────────

    #[tokio::test]
    async fn duplex_pipe_front_to_back() {
        let mut dp = DuplexPipe::new();
        dp.front_mut().write_all(b"ping").unwrap();

        let mut buf = [0u8; 4];
        dp.back_mut().read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"ping");
    }

    #[tokio::test]
    async fn duplex_pipe_back_to_front() {
        let mut dp = DuplexPipe::new();
        dp.back_mut().write_all(b"pong").unwrap();

        let mut buf = [0u8; 4];
        dp.front_mut().read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"pong");
    }

    #[tokio::test]
    async fn duplex_pipe_bidirectional() {
        let mut dp = DuplexPipe::new();

        dp.front_mut().write_all(b"hello").unwrap();
        dp.back_mut().write_all(b"world").unwrap();

        let mut buf = [0u8; 5];

        dp.back_mut().read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"hello");

        dp.front_mut().read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"world");
    }

    #[tokio::test]
    async fn duplex_pipe_reverse_swaps_directions() {
        let mut dp = DuplexPipe::new();
        dp.front_mut().write_all(b"data").unwrap();

        let mut dp = dp.reverse();
        let mut buf = [0u8; 4];
        dp.front_mut().read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"data");
    }

    #[tokio::test]
    async fn duplex_pipe_split_and_combine_roundtrip() {
        let dp = DuplexPipe::new();
        let (mut front, mut back) = dp.split();

        front.write_all(b"split").unwrap();
        let mut buf = [0u8; 5];
        back.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"split");

        let mut dp = DuplexPipe::combine(front, back);
        dp.back_mut().write_all(b"combined").unwrap();
        let mut buf = [0u8; 8];
        dp.front_mut().read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"combined");
    }

    #[tokio::test]
    async fn duplex_pipe_independent_capacities() {
        // Each direction has its own independent PIPE_CAPACITY
        let mut dp = DuplexPipe::new();
        let mut front_written = 0usize;
        let mut back_written = 0usize;

        loop {
            match dp.front_mut().write(b"x") {
                Ok(n) => front_written += n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => panic!("unexpected: {}", e),
            }
        }

        loop {
            match dp.back_mut().write(b"y") {
                Ok(n) => back_written += n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => panic!("unexpected: {}", e),
            }
        }

        assert_eq!(front_written, PIPE_CAPACITY, "front direction capacity wrong");
        assert_eq!(back_written, PIPE_CAPACITY, "back direction capacity wrong");
    }

    #[tokio::test]
    async fn duplex_pipe_closing_one_direction_does_not_affect_other() {
        let mut dp = DuplexPipe::new();

        // Close front's write end (back's read end sees EOF)
        dp.front_mut().send.close();

        // back → front direction should still work
        dp.back_mut().write_all(b"still works").unwrap();
        let mut buf = [0u8; 11];
        dp.front_mut().read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"still works");
    }

    // ── Threading ────────────────────────────────────────────────

    #[tokio::test]
    async fn pipe_threaded_producer_consumer() {
        use std::thread;

        let (mut tx, mut rx) = Pipe::new().split();

        let producer = thread::spawn(move || {
            for i in 0u8..=255 {
                // Retry on WouldBlock — simulate blocking write
                loop {
                    match tx.write(&[i]) {
                        Ok(_) => break,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::yield_now();
                        }
                        Err(e) => panic!("producer error: {}", e),
                    }
                }
            }
        });

        let consumer = thread::spawn(move || {
            let mut received = Vec::new();
            let mut buf = [0u8; 1];
            while received.len() < 256 {
                match rx.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => received.extend_from_slice(&buf[..n]),
                    Err(e) => panic!("consumer error: {}", e),
                }
            }
            received
        });

        producer.join().unwrap();
        let received = consumer.join().unwrap();

        assert_eq!(received.len(), 256);
        for (i, &byte) in received.iter().enumerate() {
            assert_eq!(byte, i as u8, "byte at position {} was wrong", i);
        }
    }
}