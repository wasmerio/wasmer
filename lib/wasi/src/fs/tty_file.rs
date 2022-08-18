use std::{
    io::{
        self,
        *
    },
    sync::Arc
};
use wasmer_vbus::FileDescriptor;
use wasmer_vfs::VirtualFile;

#[derive(Debug)]
pub struct TtyFile {
    runtime: Arc<dyn crate::WasiRuntimeImplementation + Send + Sync + 'static>,
    stdin: Box<dyn VirtualFile + Send + Sync + 'static>
}

impl TtyFile
{
    pub fn new(runtime: Arc<dyn crate::WasiRuntimeImplementation + Send + Sync + 'static>, stdin: Box<dyn VirtualFile + Send + Sync + 'static>) -> Self {
        Self {
            runtime,
            stdin
        }
    }
}

impl Seek for TtyFile {
    fn seek(&mut self, _pos: SeekFrom) -> io::Result<u64> {
        Ok(0)
    }
}
impl Write for TtyFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.runtime.stdout(buf)?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Read for TtyFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.stdin.read(buf)
    }
}

impl VirtualFile for TtyFile {
    fn last_accessed(&self) -> u64 {
        self.stdin.last_accessed()
    }
    fn last_modified(&self) -> u64 {
        self.stdin.last_modified()
    }
    fn created_time(&self) -> u64 {
        self.stdin.created_time()
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
        self.stdin.bytes_available()
    }
    fn bytes_available_read(&self) -> wasmer_vfs::Result<Option<usize>> {
        self.stdin.bytes_available_read()
    }
    fn bytes_available_write(&self) -> wasmer_vfs::Result<Option<usize>> {
        self.stdin.bytes_available_write()
    }
    fn get_fd(&self) -> Option<FileDescriptor> {
        None
    }
    fn is_open(&self) -> bool {
        true
    }
    fn get_special_fd(&self) -> Option<u32> {
        None
    }
}
