use std::io::{self, *};

use wasmer_vbus::FileDescriptor;
use wasmer_vfs::VirtualFile;
use wasmer_wasi_types::__wasi_fd_t;

#[derive(Debug)]
pub struct SpecialFile {
    fd: __wasi_fd_t
}

impl SpecialFile {
    pub fn new(fd: __wasi_fd_t) -> Self {
        Self {
            fd
        }
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
    fn set_len(&mut self, _new_size: u64) -> wasmer_vfs::Result<()> {
        Ok(())
    }
    fn unlink(&mut self) -> wasmer_vfs::Result<()> {
        Ok(())
    }
    fn bytes_available(&self) -> wasmer_vfs::Result<usize> {
        Ok(0)
    }
    fn get_special_fd(&self) -> Option<u32> {
        Some(self.fd)
    }    
    fn get_fd(&self) -> Option<FileDescriptor> {
        None
    }
}