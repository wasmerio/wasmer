use crate::syscalls::types::*;
use crate::syscalls::{read_bytes, write_bytes};
use bytes::{Buf, Bytes};
use wasmer_vfs::VirtualFile;
use std::convert::TryInto;
use std::io::{Read, Write, Seek};
use std::ops::DerefMut;
use std::sync::mpsc::{self, TryRecvError};
use std::sync::Mutex;
use std::time::Duration;
use wasmer::WasmSlice;
use wasmer::{MemorySize, MemoryView};

#[derive(Debug)]
pub struct WasiPipe {
    /// Sends bytes down the pipe
    tx: Mutex<mpsc::Sender<Vec<u8>>>,
    /// Receives bytes from the pipe
    rx: Mutex<mpsc::Receiver<Vec<u8>>>,
    /// Buffers the last read message from the pipe while its being consumed
    read_buffer: Mutex<Option<Bytes>>,
}

impl WasiPipe {
    pub fn new() -> (WasiPipe, WasiPipe) {
        let (tx1, rx1) = mpsc::channel();
        let (tx2, rx2) = mpsc::channel();

        let pipe1 = WasiPipe {
            tx: Mutex::new(tx1),
            rx: Mutex::new(rx2),
            read_buffer: Mutex::new(None),
        };

        let pipe2 = WasiPipe {
            tx: Mutex::new(tx2),
            rx: Mutex::new(rx1),
            read_buffer: Mutex::new(None),
        };

        (pipe1, pipe2)
    }

    pub fn recv<M: MemorySize>(
        &mut self,
        memory: &MemoryView,
        iov: WasmSlice<__wasi_iovec_t<M>>,
        timeout: Duration,
    ) -> Result<usize, __wasi_errno_t> {
        let mut elapsed = Duration::ZERO;
        let mut tick_wait = 0u64;
        loop {
            {
                let mut read_buffer = self.read_buffer.lock().unwrap();
                if let Some(buf) = read_buffer.as_mut() {
                    let buf_len = buf.len();
                    if buf_len > 0 {
                        let reader = buf.as_ref();
                        let read = read_bytes(reader, memory, iov).map(|a| a as usize)?;
                        buf.advance(read);
                        return Ok(read);
                    }
                }
            }
            let rx = self.rx.lock().unwrap();
            let data = match rx.try_recv() {
                Ok(a) => a,
                Err(TryRecvError::Empty) => {
                    if elapsed > timeout {
                        return Err(__WASI_ETIMEDOUT);
                    }
                    // Linearly increasing wait time
                    tick_wait += 1;
                    let wait_time = u64::min(tick_wait / 10, 20);
                    let wait_time = std::time::Duration::from_millis(wait_time);
                    std::thread::park_timeout(wait_time);
                    elapsed += wait_time;
                    continue;
                }
                Err(TryRecvError::Disconnected) => {
                    return Ok(0);
                }
            };
            drop(rx);

            let mut read_buffer = self.read_buffer.lock().unwrap();
            read_buffer.replace(Bytes::from(data));
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
        {
            let mut read_buffer = self.read_buffer.lock().unwrap();
            read_buffer.take();
        }
    }    
}

impl Read for WasiPipe {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            {
                let mut read_buffer = self.read_buffer.lock().unwrap();
                if let Some(inner_buf) = read_buffer.as_mut() {
                    let buf_len = inner_buf.len();
                    if buf_len > 0 {
                        let mut reader = inner_buf.as_ref();
                        let read = reader.read(buf).map(|_| buf_len as usize)?;
                        inner_buf.advance(read);
                        return Ok(read);
                    }
                }
            }
            let rx = self.rx.lock().unwrap();
            if let Ok(data) = rx.recv() {
                drop(rx);

                let mut read_buffer = self.read_buffer.lock().unwrap();
                read_buffer.replace(Bytes::from(data));
            } else {
                return Ok(0);
            }
        }
    }
}

impl Write for WasiPipe {
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

impl Seek for WasiPipe {
    fn seek(&mut self, _pos: std::io::SeekFrom) -> std::io::Result<u64> {
        Ok(0)
    }
}

impl VirtualFile
for WasiPipe
{
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
    fn set_len(&mut self, _new_size: u64) -> wasmer_vfs::Result<()> {
        Ok(())
    }

    /// Request deletion of the file
    fn unlink(&mut self) -> wasmer_vfs::Result<()> {
        Ok(())
    }

    /// Store file contents and metadata to disk
    /// Default implementation returns `Ok(())`.  You should implement this method if you care
    /// about flushing your cache to permanent storage
    fn sync_to_disk(&self) -> wasmer_vfs::Result<()> {
        Ok(())
    }

    /// Returns the number of bytes available.  This function must not block
    fn bytes_available(&self) -> wasmer_vfs::Result<usize> {
        Ok(self.bytes_available_read()?.unwrap_or(0usize)
            + self.bytes_available_write()?.unwrap_or(0usize))
    }

    /// Returns the number of bytes available.  This function must not block
    /// Defaults to `None` which means the number of bytes is unknown
    fn bytes_available_read(&self) -> wasmer_vfs::Result<Option<usize>> {
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
            let rx = self.rx.lock().unwrap();
            if let Ok(data) = rx.try_recv() {
                drop(rx);

                let mut read_buffer = self.read_buffer.lock().unwrap();
                read_buffer.replace(Bytes::from(data));
            } else {
                return Ok(Some(0));
            }
        }
    }

    /// Returns the number of bytes available.  This function must not block
    /// Defaults to `None` which means the number of bytes is unknown
    fn bytes_available_write(&self) -> wasmer_vfs::Result<Option<usize>> {
        Ok(None)
    }

    /// Indicates if the file is opened or closed. This function must not block
    /// Defaults to a status of being constantly open
    fn is_open(&self) -> bool {
        true
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
}
