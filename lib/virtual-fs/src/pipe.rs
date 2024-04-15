use bytes::{Buf, Bytes};
#[cfg(feature = "futures")]
use futures::Future;
use std::io::IoSlice;
use std::io::{self, Read, Seek, SeekFrom};
use std::ops::DerefMut;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::Context;
use std::task::Poll;
use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite};
use tokio::sync::{mpsc, mpsc::error::TryRecvError};

use crate::{ArcFile, FsError, VirtualFile};

#[derive(Debug, Clone)]
pub struct Pipe {
    /// Transmit side of the pipe
    send: PipeTx,
    /// Receive side of the pipe
    recv: PipeRx,
}

#[derive(Debug, Clone)]
pub struct PipeTx {
    /// Sends bytes down the pipe
    tx: Arc<Mutex<mpsc::UnboundedSender<Vec<u8>>>>,
}

#[derive(Debug, Clone)]
pub struct PipeRx {
    /// Receives bytes from the pipe
    /// Also, buffers the last read message from the pipe while its being consumed
    rx: Arc<Mutex<PipeReceiver>>,
}

impl PipeRx {
    fn try_read(&mut self, buf: &mut [u8]) -> Option<usize> {
        let max_size = buf.len();

        let mut rx = self.rx.lock().unwrap();
        loop {
            {
                if let Some(read_buffer) = rx.buffer.as_mut() {
                    let buf_len = read_buffer.len();
                    if buf_len > 0 {
                        let mut read = buf_len.min(max_size);
                        let mut inner_buf = &read_buffer[..read];
                        read = match Read::read(&mut inner_buf, buf) {
                            Ok(a) => a,
                            Err(_) => return None,
                        };
                        read_buffer.advance(read);
                        return Some(read);
                    }
                }
            }
            let data = {
                match rx.chan.try_recv() {
                    Ok(a) => a,
                    Err(TryRecvError::Empty) => {
                        return None;
                    }
                    Err(TryRecvError::Disconnected) => {
                        return Some(0);
                    }
                }
            };
            rx.buffer.replace(Bytes::from(data));
        }
    }
}

#[derive(Debug)]
struct PipeReceiver {
    chan: mpsc::UnboundedReceiver<Vec<u8>>,
    buffer: Option<Bytes>,
}

impl Pipe {
    fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        Pipe {
            send: PipeTx {
                tx: Arc::new(Mutex::new(tx)),
            },
            recv: PipeRx {
                rx: Arc::new(Mutex::new(PipeReceiver {
                    chan: rx,
                    buffer: None,
                })),
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
}

impl From<Pipe> for PipeTx {
    fn from(val: Pipe) -> Self {
        val.send
    }
}

impl From<Pipe> for PipeRx {
    fn from(val: Pipe) -> Self {
        val.recv
    }
}

impl Pipe {
    pub fn close(&self) {
        self.send.close();
    }
}

impl PipeTx {
    pub fn close(&self) {
        // TODO: proper close() implementation - Propably want to store the writer in an Option<>
        let (mut null_tx, _) = mpsc::unbounded_channel();
        {
            let mut guard = self.tx.lock().unwrap();
            std::mem::swap(guard.deref_mut(), &mut null_tx);
        }
    }
}

impl Seek for Pipe {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.recv.seek(from)
    }
}

impl Seek for PipeRx {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        Ok(0)
    }
}

impl Seek for PipeTx {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        Ok(0)
    }
}

impl Read for Pipe {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.recv.read(buf)
    }
}

impl Read for PipeRx {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let max_size = buf.len();

        let mut rx = self.rx.lock().unwrap();
        loop {
            {
                if let Some(read_buffer) = rx.buffer.as_mut() {
                    let buf_len = read_buffer.len();
                    if buf_len > 0 {
                        let mut read = buf_len.min(max_size);
                        let mut inner_buf = &read_buffer[..read];
                        read = Read::read(&mut inner_buf, buf)?;
                        read_buffer.advance(read);
                        return Ok(read);
                    }
                }
            }
            let data = {
                match rx.chan.blocking_recv() {
                    Some(a) => a,
                    None => {
                        return Ok(0);
                    }
                }
            };
            rx.buffer.replace(Bytes::from(data));
        }
    }
}

impl std::io::Write for Pipe {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.send.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.send.flush()
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.send.write_all(buf)
    }

    fn write_fmt(&mut self, fmt: std::fmt::Arguments<'_>) -> io::Result<()> {
        self.send.write_fmt(fmt)
    }

    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        self.send.write_vectored(bufs)
    }
}

impl std::io::Write for PipeTx {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let tx = self.tx.lock().unwrap();
        tx.send(buf.to_vec())
            .map_err(|_| Into::<std::io::Error>::into(std::io::ErrorKind::BrokenPipe))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl AsyncSeek for Pipe {
    fn start_seek(mut self: Pin<&mut Self>, position: SeekFrom) -> io::Result<()> {
        let this = Pin::new(&mut self.recv);
        this.start_seek(position)
    }

    fn poll_complete(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        let this = Pin::new(&mut self.recv);
        this.poll_complete(cx)
    }
}

impl AsyncSeek for PipeRx {
    fn start_seek(self: Pin<&mut Self>, _position: SeekFrom) -> io::Result<()> {
        Ok(())
    }
    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Ok(0))
    }
}

impl AsyncSeek for PipeTx {
    fn start_seek(self: Pin<&mut Self>, _position: SeekFrom) -> io::Result<()> {
        Ok(())
    }
    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Ok(0))
    }
}

impl AsyncWrite for Pipe {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = Pin::new(&mut self.send);
        this.poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let this = Pin::new(&mut self.send);
        this.poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        let this = Pin::new(&mut self.send);
        this.poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<Result<usize, io::Error>> {
        let this = Pin::new(&mut self.send);
        this.poll_write_vectored(cx, bufs)
    }
}

impl AsyncWrite for PipeTx {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let guard = self.tx.lock().unwrap();
        match guard.send(buf.to_vec()) {
            Ok(()) => Poll::Ready(Ok(buf.len())),
            Err(_) => Poll::Ready(Err(Into::<std::io::Error>::into(
                std::io::ErrorKind::BrokenPipe,
            ))),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.close();
        Poll::Ready(Ok(()))
    }
}

impl AsyncRead for Pipe {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = Pin::new(&mut self.recv);
        this.poll_read(cx, buf)
    }
}

impl AsyncRead for PipeRx {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let mut rx = self.rx.lock().unwrap();
        loop {
            {
                if let Some(inner_buf) = rx.buffer.as_mut() {
                    let buf_len = inner_buf.len();
                    if buf_len > 0 {
                        let read = buf_len.min(buf.remaining());
                        buf.put_slice(&inner_buf[..read]);
                        inner_buf.advance(read);
                        return Poll::Ready(Ok(()));
                    }
                }
            }
            let mut rx = Pin::new(rx.deref_mut());
            let data = match rx.chan.poll_recv(cx) {
                Poll::Ready(Some(a)) => a,
                Poll::Ready(None) => return Poll::Ready(Ok(())),
                Poll::Pending => return Poll::Pending,
            };

            rx.buffer.replace(Bytes::from(data));
        }
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
        self.send
            .tx
            .try_lock()
            .map(|a| !a.is_closed())
            .unwrap_or_else(|_| true)
    }

    /// Polls the file for when there is data to be read
    fn poll_read_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        let mut rx = self.recv.rx.lock().unwrap();
        loop {
            {
                if let Some(inner_buf) = rx.buffer.as_mut() {
                    let buf_len = inner_buf.len();
                    if buf_len > 0 {
                        return Poll::Ready(Ok(buf_len));
                    }
                }
            }

            let mut pinned_rx = Pin::new(&mut rx.chan);
            let data = match pinned_rx.poll_recv(cx) {
                Poll::Ready(Some(a)) => a,
                Poll::Ready(None) => return Poll::Ready(Ok(0)),
                Poll::Pending => return Poll::Pending,
            };

            rx.buffer.replace(Bytes::from(data));
        }
    }

    /// Polls the file for when it is available for writing
    fn poll_write_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        let tx = self.send.tx.lock().unwrap();
        if tx.is_closed() {
            Poll::Ready(Ok(0))
        } else {
            Poll::Ready(Ok(8192))
        }
    }
}

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
