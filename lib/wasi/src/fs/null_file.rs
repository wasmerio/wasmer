use std::io::{self, *};

use wasmer_vbus::FileDescriptor;
use wasmer_vfs::{VirtualFile, ClonableVirtualFile};

#[derive(Debug, Clone, Default)]
pub struct NullFile {}

impl Seek for NullFile {
    fn seek(&mut self, _pos: SeekFrom) -> io::Result<u64> {
        Ok(0)
    }
}
impl Write for NullFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Read for NullFile {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Ok(0)
    }
}

impl VirtualFile for NullFile {
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
    fn get_fd(&self) -> Option<FileDescriptor> {
        None
    }
}

impl ClonableVirtualFile for NullFile {}