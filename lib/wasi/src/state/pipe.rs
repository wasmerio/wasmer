use crate::syscalls::types::*;
use crate::syscalls::{read_bytes, write_bytes};
use bytes::{Buf, Bytes};
use std::convert::TryInto;
use std::io::{self, Read};
use std::ops::DerefMut;
use std::sync::mpsc;
use std::sync::Mutex;
use wasmer::WasmSlice;
use wasmer::{MemorySize, MemoryView};
use wasmer_wasi_types::wasi::Errno;

#[derive(Debug)]
pub struct WasiPipe {
    /// Sends bytes down the pipe
    tx: Mutex<mpsc::Sender<Vec<u8>>>,
    /// Receives bytes from the pipe
    rx: Mutex<mpsc::Receiver<Vec<u8>>>,
    /// Buffers the last read message from the pipe while its being consumed
    read_buffer: Option<Bytes>,
}

impl WasiPipe {
    pub fn new() -> (WasiPipe, WasiPipe) {
        let (tx1, rx1) = mpsc::channel();
        let (tx2, rx2) = mpsc::channel();

        let pipe1 = WasiPipe {
            tx: Mutex::new(tx1),
            rx: Mutex::new(rx2),
            read_buffer: None,
        };

        let pipe2 = WasiPipe {
            tx: Mutex::new(tx2),
            rx: Mutex::new(rx1),
            read_buffer: None,
        };

        (pipe1, pipe2)
    }

    pub fn recv<M: MemorySize>(
        &mut self,
        memory: &MemoryView,
        iov: WasmSlice<__wasi_iovec_t<M>>,
    ) -> Result<usize, Errno> {
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
            let data = rx.recv().map_err(|_| Errno::Io)?;
            self.read_buffer.replace(Bytes::from(data));
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
        let tx = self.tx.lock().unwrap();
        tx.send(buf).map_err(|_| Errno::Io)?;
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

impl Read for WasiPipe {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            if let Some(inner_buf) = self.read_buffer.as_mut() {
                let buf_len = inner_buf.len();
                if buf_len > 0 {
                    let mut reader = inner_buf.as_ref();
                    let read = reader.read(buf).map(|_| buf_len as usize)?;
                    inner_buf.advance(read);
                    return Ok(read);
                }
            }
            let rx = self.rx.lock().unwrap();
            let data = rx.recv().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "the wasi pipe is not connected".to_string(),
                )
            })?;
            self.read_buffer.replace(Bytes::from(data));
        }
    }
}
