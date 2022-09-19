use crate::syscalls::types::*;
use crate::syscalls::{read_bytes, write_bytes};
use bytes::{Buf, Bytes};
use std::convert::TryInto;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::ops::DerefMut;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use wasmer::WasmSlice;
use wasmer::{MemorySize, MemoryView};
use wasmer_vfs::{FsError, VirtualFile};

#[derive(Debug)]
pub struct WasiPipe {
    /// Sends bytes down the pipe
    tx: Mutex<mpsc::Sender<Vec<u8>>>,
    /// Receives bytes from the pipe
    rx: Mutex<mpsc::Receiver<Vec<u8>>>,
    /// Buffers the last read message from the pipe while its being consumed
    read_buffer: Option<Bytes>,
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
}

impl Default for WasiBidirectionalPipePair {
    fn default() -> Self {
        Self::new()
    }
}

impl WasiBidirectionalPipePair {
    pub fn new() -> WasiBidirectionalPipePair {
        let (tx1, rx1) = mpsc::channel();
        let (tx2, rx2) = mpsc::channel();

        let pipe1 = WasiPipe {
            tx: Mutex::new(tx1),
            rx: Mutex::new(rx2),
            read_buffer: None,
            block: true,
        };

        let pipe2 = WasiPipe {
            tx: Mutex::new(tx2),
            rx: Mutex::new(rx1),
            read_buffer: None,
            block: true,
        };

        WasiBidirectionalPipePair {
            send: pipe1,
            recv: pipe2,
        }
    }

    pub fn with_blocking(mut self, block: bool) -> Self {
        self.set_blocking(block);
        self
    }

    /// Whether to block on reads (ususally for waiting for stdin keyboard input). Default: `true`
    pub fn set_blocking(&mut self, block: bool) {
        self.send.set_blocking(block);
        self.recv.set_blocking(block);
    }
}

/// Shared version of WasiBidirectionalPipePair for situations where you need
/// to emulate the old behaviour of `Pipe` (both send and recv on one channel).
#[derive(Debug, Clone)]
pub struct WasiBidirectionalSharedPipePair {
    inner: Arc<Mutex<WasiBidirectionalPipePair>>,
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

    pub fn with_blocking(mut self, block: bool) -> Self {
        self.set_blocking(block);
        self
    }

    /// Whether to block on reads (ususally for waiting for stdin keyboard input). Default: `true`
    pub fn set_blocking(&mut self, block: bool) {
        self.inner.lock().unwrap().set_blocking(block);
    }
}

impl Write for WasiBidirectionalSharedPipePair {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.inner.lock().as_mut().map(|l| l.write(buf)) {
            Ok(r) => r,
            Err(_) => Ok(0),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self.inner.lock().as_mut().map(|l| l.flush()) {
            Ok(r) => r,
            Err(_) => Ok(()),
        }
    }
}

impl Seek for WasiBidirectionalSharedPipePair {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        Ok(0)
    }
}

impl Read for WasiBidirectionalSharedPipePair {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.inner.lock().as_mut().map(|l| l.read(buf)) {
            Ok(r) => r,
            Err(_) => Ok(0),
        }
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

    pub fn recv<M: MemorySize>(
        &mut self,
        memory: &MemoryView,
        iov: WasmSlice<__wasi_iovec_t<M>>,
    ) -> Result<usize, __wasi_errno_t> {
        loop {
            if let Some(buf) = self.read_buffer.as_mut() {
                let buf_len = buf.len();
                if buf_len > 0 {
                    let reader = buf.as_ref();
                    let read = read_bytes(reader, memory, iov).map(|_| buf_len as usize)?;
                    buf.advance(read);
                    return Ok(read);
                }
            }
            let rx = self.rx.lock().unwrap();
            let data = rx.recv().map_err(|_| __WASI_EIO)?;
            self.read_buffer.replace(Bytes::from(data));
        }
    }

    pub fn send<M: MemorySize>(
        &mut self,
        memory: &MemoryView,
        iov: WasmSlice<__wasi_ciovec_t<M>>,
    ) -> Result<usize, __wasi_errno_t> {
        let buf_len: M::Offset = iov
            .iter()
            .filter_map(|a| a.read().ok())
            .map(|a| a.buf_len)
            .sum();
        let buf_len: usize = buf_len.try_into().map_err(|_| __WASI_EINVAL)?;
        let mut buf = Vec::with_capacity(buf_len);
        write_bytes(&mut buf, memory, iov)?;
        let tx = self.tx.lock().unwrap();
        tx.send(buf).map_err(|_| __WASI_EIO)?;
        Ok(buf_len)
    }

    pub fn close(&mut self) {
        let (mut null_tx, _) = mpsc::channel();
        let (_, mut null_rx) = mpsc::channel();
        {
            let mut guard = self.rx.lock().unwrap();
            std::mem::swap(guard.deref_mut(), &mut null_rx);
        }
        {
            let mut guard = self.tx.lock().unwrap();
            std::mem::swap(guard.deref_mut(), &mut null_tx);
        }
        self.read_buffer.take();
    }
}

impl Write for WasiPipe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let buf_len = buf.len();
        let tx = self.tx.lock().unwrap();
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
        loop {
            if let Some(inner_buf) = self.read_buffer.as_mut() {
                let buf_len = inner_buf.len();
                if buf_len > 0 {
                    if inner_buf.len() > buf.len() {
                        let mut reader = inner_buf.as_ref();
                        let read = reader.read_exact(buf).map(|_| buf.len())?;
                        inner_buf.advance(read);
                        return Ok(read);
                    } else {
                        let mut reader = inner_buf.as_ref();
                        let read = reader.read(buf).map(|_| buf_len as usize)?;
                        inner_buf.advance(read);
                        return Ok(read);
                    }
                } else if !self.block {
                    return Ok(0);
                }
            }
            let rx = self.rx.lock().unwrap();

            // We need to figure out whether we need to block here.
            // The problem is that in cases of multiple buffered reads like:
            //
            // println!("abc");
            // println!("def");
            //
            // get_stdout() // would only return "abc\n" instead of "abc\ndef\n"

            let data = match rx.try_recv() {
                Ok(mut s) => {
                    s.append(&mut rx.try_iter().flat_map(|f| f.into_iter()).collect());
                    s
                }
                Err(_) => {
                    // could not immediately receive bytes, so we need to block
                    match rx.recv() {
                        Ok(o) => o,
                        // Errors can happen if the sender has been dropped already
                        // In this case, just return 0 to indicate that we can't read any
                        // bytes anymore
                        Err(_) => {
                            return Ok(0);
                        }
                    }
                }
            };
            self.read_buffer.replace(Bytes::from(data));
        }
    }
}

impl VirtualFile for WasiPipe {
    fn last_accessed(&self) -> u64 {
        0
    }
    fn last_modified(&self) -> u64 {
        0
    }
    fn created_time(&self) -> u64 {
        0
    }
    fn size(&self) -> u64 {
        self.read_buffer
            .as_ref()
            .map(|s| s.len() as u64)
            .unwrap_or_default()
    }
    fn set_len(&mut self, _: u64) -> Result<(), FsError> {
        Ok(())
    }
    fn unlink(&mut self) -> Result<(), FsError> {
        Ok(())
    }
    fn bytes_available_read(&self) -> Result<Option<usize>, FsError> {
        Ok(Some(
            self.read_buffer
                .as_ref()
                .map(|s| s.len())
                .unwrap_or_default(),
        ))
    }
}
