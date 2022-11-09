use std::io::{self, *};

use crate::FileDescriptor;
use crate::VirtualFile;
use wasmer_wasi_types::wasi::Fd;

/// A "special" file is a file that is locked
/// to one file descriptor (i.e. stdout => 0, stdin => 1), etc.
#[derive(Debug)]
pub struct SpecialFile {
    fd: Fd,
}

impl SpecialFile {
    pub fn new(fd: Fd) -> Self {
        Self { fd }
    }
}

impl Seek for SpecialFile {
    fn seek(&mut self, _pos: SeekFrom) -> io::Result<u64> {
        Ok(0)
    }
}
impl Write for SpecialFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Read for SpecialFile {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Ok(0)
    }
}

impl VirtualFile for SpecialFile {
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
    fn set_len(&mut self, _new_size: u64) -> crate::Result<()> {
        Ok(())
    }
    fn unlink(&mut self) -> crate::Result<()> {
        Ok(())
    }
    fn bytes_available(&self) -> crate::Result<usize> {
        Ok(0)
    }
    fn get_special_fd(&self) -> Option<u32> {
        Some(self.fd)
    }
    fn get_fd(&self) -> Option<FileDescriptor> {
        None
    }
}
