use std::sync::Arc;
use std::io::{self, Read, Write, Seek};

#[derive(Debug)]
pub struct RuntimeStdout {
    runtime: Arc<dyn crate::WasiRuntimeImplementation + Send + Sync + 'static>,
}

impl RuntimeStdout {
    pub fn new(runtime: Arc<dyn crate::WasiRuntimeImplementation + Send + Sync + 'static>) -> Self {
        Self {
            runtime
        }
    }
}

impl Read for RuntimeStdout {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stdout",
        ))
    }

    fn read_to_end(&mut self, _buf: &mut Vec<u8>) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stdout",
        ))
    }

    fn read_to_string(&mut self, _buf: &mut String) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stdout",
        ))
    }

    fn read_exact(&mut self, _buf: &mut [u8]) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stdout",
        ))
    }
}

impl Seek for RuntimeStdout {
    fn seek(&mut self, _pos: io::SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(io::ErrorKind::Other, "can not seek stdout"))
    }
}

impl Write for RuntimeStdout {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.runtime.stdout(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.runtime.flush()
    }
}

impl wasmer_vfs::VirtualFile for RuntimeStdout {
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
        tracing::debug!("Calling VirtualFile::set_len on stderr; this is probably a bug");
        Err(wasmer_vfs::FsError::PermissionDenied)
    }

    fn unlink(&mut self) -> wasmer_vfs::Result<()> {
        Ok(())
    }
}

#[derive(Debug)]
pub struct RuntimeStderr {
    runtime: Arc<dyn crate::WasiRuntimeImplementation + Send + Sync + 'static>,
}

impl RuntimeStderr {
    pub fn new(runtime: Arc<dyn crate::WasiRuntimeImplementation + Send + Sync + 'static>) -> Self {
        Self {
            runtime
        }
    }
}

impl Read for RuntimeStderr {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stderr",
        ))
    }

    fn read_to_end(&mut self, _buf: &mut Vec<u8>) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stderr",
        ))
    }

    fn read_to_string(&mut self, _buf: &mut String) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stderr",
        ))
    }

    fn read_exact(&mut self, _buf: &mut [u8]) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stderr",
        ))
    }
}

impl Seek for RuntimeStderr {
    fn seek(&mut self, _pos: io::SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(io::ErrorKind::Other, "can not seek stderr"))
    }
}

impl Write for RuntimeStderr {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.runtime.stderr(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.runtime.flush()
    }
}

impl wasmer_vfs::VirtualFile for RuntimeStderr {
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
        tracing::debug!("Calling VirtualFile::set_len on stderr; this is probably a bug");
        Err(wasmer_vfs::FsError::PermissionDenied)
    }

    fn unlink(&mut self) -> wasmer_vfs::Result<()> {
        Ok(())
    }
}
