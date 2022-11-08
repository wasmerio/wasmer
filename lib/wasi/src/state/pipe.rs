use crate::syscalls::types::*;
use crate::syscalls::{read_bytes, write_bytes};
use bytes::{Buf, Bytes};
use futures::Future;
use tokio::sync::mpsc::error::TryRecvError;
use std::convert::TryInto;
use std::io::{self, Read, Seek, SeekFrom, Write, ErrorKind};
use std::io::{Read, Seek, Write};
use std::ops::DerefMut;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;
use tokio::sync::mpsc::{self, TryRecvError};
use std::time::Duration;
use tokio::sync::{mpsc, TryLockError};
use tokio::sync::Mutex;
use wasmer::WasmSlice;
use wasmer::{MemorySize, MemoryView};
use wasmer_vfs::{FsError, VirtualFile};
use wasmer_wasi_types::wasi::Errno;
use wasmer_vfs::VirtualFile;

#[derive(Debug)]
pub struct WasiPipe {
    /// Sends bytes down the pipe
    tx: Mutex<mpsc::UnboundedSender<Vec<u8>>>,
    /// Receives bytes from the pipe
    rx: Mutex<mpsc::UnboundedReceiver<Vec<u8>>>,
    /// Buffers the last read message from the pipe while its being consumed
    read_buffer: std::sync::Mutex<Option<Bytes>>,
    /// Whether the pipe should block or not block to wait for stdin reads
    block: bool,
}

/// Pipe pair of (a, b) WasiPipes that are connected together
#[derive(Debug)]
pub struct WasiBidirectionalPipePair {
    pub send: WasiPipe,
    pub recv: WasiPipe,
}

impl Write for WasiBidirectionalPipePair {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.send.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.send.flush()
    }
}

impl Seek for WasiBidirectionalPipePair {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        Ok(0)
    }
}

impl Read for WasiBidirectionalPipePair {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.recv.read(buf)
    }
}

impl VirtualFile for WasiBidirectionalPipePair {
    fn last_accessed(&self) -> u64 {
        self.recv.last_accessed()
    }
    fn last_modified(&self) -> u64 {
        self.recv.last_modified()
    }
    fn created_time(&self) -> u64 {
        self.recv.created_time()
    }
    fn size(&self) -> u64 {
        self.recv.size()
    }
    fn set_len(&mut self, i: u64) -> Result<(), FsError> {
        self.recv.set_len(i)
    }
    fn unlink(&mut self) -> Result<(), FsError> {
        self.recv.unlink()
    }
    fn bytes_available_read(&self) -> Result<Option<usize>, FsError> {
        self.recv.bytes_available_read()
    }
    fn poll_read_ready(
        &self,
        cx: &mut std::task::Context<'_>,
        register_root_waker: &Arc<dyn Fn(Waker) + Send + Sync + 'static>,
    ) -> std::task::Poll<Result<usize>> {
        self.recv.poll_read_ready(cx, register_root_waker)
    }
    fn poll_write_ready(
        &self,
        cx: &mut std::task::Context<'_>,
        register_root_waker: &Arc<dyn Fn(Waker) + Send + Sync + 'static>,
    ) -> std::task::Poll<Result<usize>> {
        self.send.poll_write_ready(cx, register_root_waker)
    }
    fn read_async<'a>(&'a mut self, max_size: usize, register_root_waker: &'_ Arc<dyn Fn(Waker) + Send + Sync + 'static>) -> Box<dyn Future<Output=io::Result<Vec<u8>>> + 'a>
    where Self: Sized
    {
        self.recv.read_async(max_size, register_root_waker)
    }
    fn write_async<'a>(&'a mut self, buf: &'a [u8], register_root_waker: &'_ Arc<dyn Fn(Waker) + Send + Sync + 'static>) -> Box<dyn Future<Output=io::Result<usize>> + 'a>
    where Self: Sized
    {
        self.send.write_async(buf, register_root_waker)
    }
}

impl Default for WasiBidirectionalPipePair {
    fn default() -> Self {
        Self::new()
    }
}

impl WasiBidirectionalPipePair {
    pub fn new() -> WasiBidirectionalPipePair {
        let (tx1, rx1) = mpsc::unbounded_channel();
        let (tx2, rx2) = mpsc::unbounded_channel();

        let pipe1 = WasiPipe {
            tx: Mutex::new(tx1),
            rx: Mutex::new(rx2),
            read_buffer: Mutex::new(None),
            block: true,
        };

        let pipe2 = WasiPipe {
            tx: Mutex::new(tx2),
            rx: Mutex::new(rx1),
            read_buffer: Mutex::new(None),
            block: true,
        };

        WasiBidirectionalPipePair {
            send: pipe1,
            recv: pipe2,
        }
    }

    #[allow(dead_code)]
    pub fn with_blocking(mut self, block: bool) -> Self {
        self.set_blocking(block);
        self
    }

    /// Whether to block on reads (ususally for waiting for stdin keyboard input). Default: `true`
    #[allow(dead_code)]
    pub fn set_blocking(&mut self, block: bool) {
        self.send.set_blocking(block);
        self.recv.set_blocking(block);
    }
}

/// Shared version of WasiBidirectionalPipePair for situations where you need
/// to emulate the old behaviour of `Pipe` (both send and recv on one channel).
#[derive(Debug, Clone)]
pub struct WasiBidirectionalSharedPipePair {
    inner: Arc<std::sync::Mutex<WasiBidirectionalPipePair>>,
}

impl Default for WasiBidirectionalSharedPipePair {
    fn default() -> Self {
        Self::new()
    }
}

impl WasiBidirectionalSharedPipePair {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(WasiBidirectionalPipePair::new())),
        }
    }

    #[allow(dead_code)]
    pub fn with_blocking(mut self, block: bool) -> Self {
        self.set_blocking(block);
        self
    }

    /// Whether to block on reads (ususally for waiting for stdin keyboard input). Default: `true`
    #[allow(dead_code)]
    pub fn set_blocking(&mut self, block: bool) {
        self.inner.lock().unwrap().set_blocking(block);
    }
}

impl Write for WasiBidirectionalSharedPipePair {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.lock().unwrap().write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.lock().unwrap().flush()
    }
}

impl Seek for WasiBidirectionalSharedPipePair {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        Ok(0)
    }
}

impl Read for WasiBidirectionalSharedPipePair {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.lock().unwrap().read(buf)
    }
}

impl VirtualFile for WasiBidirectionalSharedPipePair {
    fn last_accessed(&self) -> u64 {
        self.inner.lock().map(|l| l.last_accessed()).unwrap_or(0)
    }
    fn last_modified(&self) -> u64 {
        self.inner.lock().map(|l| l.last_modified()).unwrap_or(0)
    }
    fn created_time(&self) -> u64 {
        self.inner.lock().map(|l| l.created_time()).unwrap_or(0)
    }
    fn size(&self) -> u64 {
        self.inner.lock().map(|l| l.size()).unwrap_or(0)
    }
    fn set_len(&mut self, i: u64) -> Result<(), FsError> {
        match self.inner.lock().as_mut().map(|l| l.set_len(i)) {
            Ok(r) => r,
            Err(_) => Err(FsError::Lock),
        }
    }
    fn unlink(&mut self) -> Result<(), FsError> {
        match self.inner.lock().as_mut().map(|l| l.unlink()) {
            Ok(r) => r,
            Err(_) => Err(FsError::Lock),
        }
    }
    fn bytes_available_read(&self) -> Result<Option<usize>, FsError> {
        self.inner
            .lock()
            .map(|l| l.bytes_available_read())
            .unwrap_or(Ok(None))
    }
    fn poll_read_ready(
        &self,
        cx: &mut std::task::Context<'_>,
        register_root_waker: &Arc<dyn Fn(Waker) + Send + Sync + 'static>,
    ) -> std::task::Poll<Result<usize>> {
        self.inner
            .lock()
            .unwrap()
            .poll_read_ready(cx, register_root_waker)
    }
    fn poll_write_ready(
        &self,
        cx: &mut std::task::Context<'_>,
        register_root_waker: &Arc<dyn Fn(Waker) + Send + Sync + 'static>,
    ) -> std::task::Poll<Result<usize>> {
        self.inner
            .lock()
            .unwrap()
            .poll_write_ready(cx, register_root_waker)
    }
    fn read_async<'a>(&'a mut self, max_size: usize, register_root_waker: &'_ Arc<dyn Fn(Waker) + Send + Sync + 'static>) -> Box<dyn Future<Output=io::Result<Vec<u8>>> + 'a>
    where Self: Sized
    {
        self.inner
            .lock()
            .unwrap()
            .read_async(max_size, register_root_waker)
    }
    fn write_async<'a>(&'a mut self, buf: &'a [u8], register_root_waker: &'_ Arc<dyn Fn(Waker) + Send + Sync + 'static>) -> Box<dyn Future<Output=io::Result<usize>> + 'a>
    where Self: Sized
    {
        self.inner
            .lock()
            .unwrap()
            .write_async(buf, register_root_waker)
    }
}

impl WasiPipe {
    /// Same as `set_blocking`, but as a builder method
    pub fn with_blocking(mut self, block: bool) -> Self {
        self.set_blocking(block);
        self
    }

    /// Whether to block on reads (ususally for waiting for stdin keyboard input). Default: `true`
    pub fn set_blocking(&mut self, block: bool) {
        self.block = block;
    }

    pub async fn recv<M: MemorySize>(
        &mut self,
        max_size: usize,
    ) -> Result<Bytes, Errno> {
        let mut no_more = None;
        loop {
            {
                let mut read_buffer = self.read_buffer.lock().unwrap();
                if let Some(inner_buf) = read_buffer.as_mut() {
                    let buf_len = inner_buf.len();
                    if buf_len > 0 {
                        let read = buf_len.min(max_size);
                        let ret = inner_buf.slice(..read);
                        inner_buf.advance(read);
                        return Ok(ret);
                    }
                }
            }
            if let Some(no_more) = no_more.take() {
                return no_more;
            }
            let data = {
                let mut rx = match self.rx.try_lock() {
                    Ok(a) => a,
                    Err(_) => {
                        match self.block {
                            true => self.rx.lock().await,
                            false => { no_more = Some(Err(Errno::Again)); continue; }
                        }
                    }
                };
                match self.block {
                    true => match rx.recv().await {
                        Some(a) => a,
                        None => { no_more = Some(Ok(0)); continue; },
                    },
                    false => {
                        match rx.try_recv() {
                            Ok(a) => a,
                            Err(TryRecvError::Empty) => { no_more = Some(Err(Errno::Again)); continue; },
                            Err(TryRecvError::Disconnected) => { no_more = Some(Ok(0)); continue; }
                        }
                    }
                }
            };

            let mut read_buffer = self.read_buffer.lock().unwrap();
            read_buffer.replace(Bytes::from(data));
        }
    }

    pub fn send<M: MemorySize>(
        &mut self,
        memory: &MemoryView,
        iov: WasmSlice<__wasi_ciovec_t<M>>,
    ) -> Result<usize, Errno> {
        let buf_len: M::Offset = iov
            .iter()
            .filter_map(|a| a.read().ok())
            .map(|a| a.buf_len)
            .sum();
        let buf_len: usize = buf_len.try_into().map_err(|_| Errno::Inval)?;
        let mut buf = Vec::with_capacity(buf_len);
        write_bytes(&mut buf, memory, iov)?;
        let tx = self.tx.blocking_lock();
        tx.send(buf).map_err(|_| Errno::Io)?;
        Ok(buf_len)
    }

    pub fn close(&mut self) {
        let (mut null_tx, _) = mpsc::unbounded_channel();
        let (_, mut null_rx) = mpsc::unbounded_channel();
        {
            let mut guard = self.rx.blocking_lock();
            std::mem::swap(guard.deref_mut(), &mut null_rx);
        }
        {
            let mut guard = self.tx.blocking_lock();
            std::mem::swap(guard.deref_mut(), &mut null_tx);
        }
        {
            let mut read_buffer = self.read_buffer.lock().unwrap();
            read_buffer.take();
        }
    }
}

impl Write for WasiPipe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let buf_len = buf.len();
        let tx = self.tx.blocking_lock();
        tx.send(buf.to_vec())
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("{e}")))?;
        Ok(buf_len)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Seek for WasiPipe {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        Ok(0)
    }
}

impl Read for WasiPipe {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut no_more = None;
        loop {
            {
                let mut read_buffer = self.read_buffer.lock().unwrap();
                if let Some(inner_buf) = read_buffer.as_mut() {
                    let buf_len = inner_buf.len();
                    if buf_len > 0 {
                        let read = buf_len.min(max_size);
                        let ret = inner_buf.slice(..read);
                        inner_buf.advance(read);
                        return Ok(ret);
                    }
                }
            }
            if let Some(no_more) = no_more.take() {
                return no_more;
            }
            let data = {
                let mut rx = match self.rx.try_lock() {
                    Ok(a) => a,
                    Err(_) => {
                        match self.block {
                            true => self.rx.blocking_lock(),
                            false => { no_more = Some(Err(Errno::Again)); continue; }
                        }
                    }
                };
                match self.block {
                    true => match rx.blocking_recv(){
                        Some(a) => a,
                        None => { no_more = Some(Ok(0)); continue; },
                    },
                    false => {
                        match rx.try_recv() {
                            Ok(a) => a,
                            Err(TryRecvError::Empty) => { no_more = Some(Err(Errno::Again)); continue; },
                            Err(TryRecvError::Disconnected) => { no_more = Some(Ok(0)); continue; }
                        }
                    }
                }
            };

            let mut read_buffer = self.read_buffer.lock().unwrap();
            read_buffer.replace(Bytes::from(data));
        }
    }
}

impl std::io::Write for WasiPipe {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let tx = match self.tx.try_lock() {
            Ok(a) => a,
            Err(_) => {
                match self.block {
                    true => self.tx.blocking_lock(),
                    false => return Err(Into::<std::io::Error>::into(std::io::ErrorKind::WouldBlock)),
                }
            }
        };
        let tx = self.tx.lock().unwrap();
        tx.send(buf.to_vec())
            .map_err(|_| Into::<std::io::Error>::into(std::io::ErrorKind::BrokenPipe))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl VirtualFile for WasiPipe {
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
        self.bytes_available_read()
            .unwrap_or_default()
            .map(|a| a as u64)
            .unwrap_or_default()
    }

    /// Change the size of the file, if the `new_size` is greater than the current size
    /// the extra bytes will be allocated and zeroed
    fn set_len(&mut self, _new_size: u64) -> Result<(), FsError> {
        Ok(())
    }

    /// Request deletion of the file
    fn unlink(&mut self) -> Result<(), FsError> {
        Ok(())
    }

    /// Store file contents and metadata to disk
    /// Default implementation returns `Ok(())`.  You should implement this method if you care
    /// about flushing your cache to permanent storage
    fn sync_to_disk(&self) -> Result<(), FsError> {
        Ok(())
    }

    /// Returns the number of bytes available.  This function must not block
    fn bytes_available(&self) -> Result<usize, FsError> {
        Ok(self.bytes_available_read()?.unwrap_or(0usize)
            + self.bytes_available_write()?.unwrap_or(0usize))
    }

    /// Returns the number of bytes available.  This function must not block
    /// Defaults to `None` which means the number of bytes is unknown
    fn bytes_available_read(&self) -> Result<Option<usize>, FsError> {
        let mut no_more = None;
        loop {
            {
                let read_buffer = self.read_buffer.lock().unwrap();
                if let Some(inner_buf) = read_buffer.as_ref() {
                    let buf_len = inner_buf.len();
                    if buf_len > 0 {
                        return Ok(Some(buf_len));
                    }
                }
            }
            if let Some(no_more) = no_more.take() {
                return no_more;
            }
            let data = {
                let mut rx = match self.rx.try_lock() {
                    Ok(a) => a,
                    Err(_) => { no_more = Some(Ok(None)); continue; }
                };
                match rx.try_recv() {
                    Ok(a) => a,
                    Err(TryRecvError::Empty) => { no_more = Some(Ok(None)); continue; },
                    Err(TryRecvError::Disconnected) => { no_more = Some(Ok(Some(0))); continue; }
                }
            };

            let mut read_buffer = self.read_buffer.lock().unwrap();
            read_buffer.replace(Bytes::from(data));
        }
    }

    /// Returns the number of bytes available.  This function must not block
    /// Defaults to `None` which means the number of bytes is unknown
    fn bytes_available_write(&self) -> Result<Option<usize>, FsError> {
        self.tx.try_lock()
            .map(|_| Ok(8192))
            .unwrap_or_else(|| Ok(Some(0)))
    }

    /// Indicates if the file is opened or closed. This function must not block
    /// Defaults to a status of being constantly open
    fn is_open(&self) -> bool {
        self.tx.try_lock()
            .map(|a| a.is_closed() == false)
            .unwrap_or_else(|| true)
    }

    /// Returns a special file descriptor when opening this file rather than
    /// generating a new one
    fn get_special_fd(&self) -> Option<u32> {
        None
    }

    /// Used for polling.  Default returns `None` because this method cannot be implemented for most types
    /// Returns the underlying host fd
    fn get_fd(&self) -> Option<wasmer_vfs::FileDescriptor> {
        None
    }

    fn poll_read_ready(
        &self,
        cx: &mut std::task::Context<'_>,
        register_root_waker: &Arc<dyn Fn(Waker) + Send + Sync + 'static>,
    ) -> std::task::Poll<Result<usize>> {
        let mut no_more = None;
        loop {
            {
                let read_buffer = self.read_buffer.lock().unwrap();
                if let Some(inner_buf) = read_buffer.as_ref() {
                    let buf_len = inner_buf.len();
                    if buf_len > 0 {
                        return Poll::Ready(Ok(buf_len));
                    }
                }
            }
            if let Some(no_more) = no_more.take() {
                return no_more;
            }
            let data = {
                let mut rx = self.rx.lock();
                let rx = Pin::new(&mut rx);
                match rx.poll(cx) {
                    Poll::Pending => { no_more = Some(Poll::Pending); continue; }
                    Poll::Ready(mut rx) => {
                        let rx = Pin::new(&mut rx);
                        match rx.poll_recv(cx) {
                            Poll::Pending => { no_more = Some(Poll::Pending); continue; }
                            Poll::Ready(Some(a)) => a,
                            Poll::Ready(None) => { no_more = Some(Poll::Ready(Ok(0))); continue; }
                        }
                    }
                }
            };

            let mut read_buffer = self.read_buffer.lock().unwrap();
            read_buffer.replace(Bytes::from(data));
        }
    }

    fn poll_write_ready(
        &self,
        cx: &mut std::task::Context<'_>,
        register_root_waker: &Arc<dyn Fn(Waker) + Send + Sync + 'static>,
    ) -> std::task::Poll<Result<usize>> {
        let mut tx = self.tx.lock();
        let tx = Pin::new(&mut tx);
        tx.poll(cx)
            .map(|_| Ok(8192))
    }

    fn read_async<'a>(&'a mut self, max_size: usize, register_root_waker: &'_ Arc<dyn Fn(Waker) + Send + Sync + 'static>) -> Box<dyn Future<Output=io::Result<Vec<u8>>> + 'a>
    where Self: Sized
    {
        Box::new(
            async move {
                self.recv(max_size)
                    .await
                    .map_err(|err| Into::<std::io::Error>::into(err))
            }
        )
    }

    fn write_async<'a>(&'a mut self, buf: &'a [u8], register_root_waker: &'_ Arc<dyn Fn(Waker) + Send + Sync + 'static>) -> Box<dyn Future<Output=io::Result<usize>> + 'a>
    where Self: Sized
    {
        Box::new(
            async move {
                let tx = match self.tx.try_lock() {
                    Ok(a) => a,
                    Err(_) => {
                        match self.block {
                            true => self.tx.lock().await,
                            false => return Err(Into::<std::io::Error>::into(std::io::ErrorKind::WouldBlock)),
                        }
                    }
                };
                tx.send(buf.to_vec())
                    .map_err(|_| Into::<std::io::Error>::into(std::io::ErrorKind::BrokenPipe))?;
                Ok(buf.len())
            }
        )
    }
}
