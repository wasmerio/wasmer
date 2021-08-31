//! Default implementations for capturing the stdout/stderr output of a WASI program.

use std::collections::VecDeque;
use std::io::{self, Read, Seek, Write};
use wasmer_wasi::{WasiFile, WasiFsError};

/// For capturing stdout/stderr. Stores all output in a string.
#[derive(Debug)]
pub struct OutputCapturer {
    pub(crate) buffer: VecDeque<u8>,
}

impl OutputCapturer {
    pub fn new() -> Self {
        Self {
            buffer: VecDeque::new(),
        }
    }
}

impl WasiFile for OutputCapturer {
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
        0
    }
    fn set_len(&mut self, _len: u64) -> Result<(), WasiFsError> {
        Ok(())
    }
    fn unlink(&mut self) -> Result<(), WasiFsError> {
        Ok(())
    }
    fn bytes_available(&self) -> Result<usize, WasiFsError> {
        // return an arbitrary amount
        Ok(1024)
    }
}

// fail when reading or Seeking
impl Read for OutputCapturer {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from capturing stdout",
        ))
    }
    fn read_to_end(&mut self, _buf: &mut Vec<u8>) -> std::io::Result<usize> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "can not read from capturing stdout",
        ))
    }
    fn read_to_string(&mut self, _buf: &mut String) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from capturing stdout",
        ))
    }
    fn read_exact(&mut self, _buf: &mut [u8]) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from capturing stdout",
        ))
    }
}
impl Seek for OutputCapturer {
    fn seek(&mut self, _pos: io::SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not seek capturing stdout",
        ))
    }
}
impl Write for OutputCapturer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.extend(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.buffer.extend(buf);
        Ok(())
    }
}
