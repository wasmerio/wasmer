use std::io::{self, *};

use wasmer_vbus::FileDescriptor;
use wasmer_vfs::VirtualFile;

#[derive(Debug, Default)]
pub struct ZeroFile {}

impl Seek for ZeroFile {
    fn seek(&mut self, _pos: SeekFrom) -> io::Result<u64> {
        Ok(0)
    }
}
impl Write for ZeroFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Read for ZeroFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        for b in buf.iter_mut() {
            *b = 0;
        }
        Ok(buf.len())
    }
}

impl VirtualFile for ZeroFile {
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
