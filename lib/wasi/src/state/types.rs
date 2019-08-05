/// types for use in the WASI filesystem
use crate::syscalls::types::*;
use std::{
    fs,
    io::{self, Read, Seek, Write},
    path::PathBuf,
    time::SystemTime,
};

/// Error type for external users
#[derive(Debug, PartialEq, Eq)]
#[allow(dead_code)]
// dead code beacuse this is for external use
pub enum WasiFsError {
    /// The fd given as a base was not a directory so the operation was not possible
    BaseNotDirectory,
    /// Expected a file but found not a file
    NotAFile,
    /// The fd given was not usable
    InvalidFd,
    /// File exists
    AlreadyExists,
    /// Something failed when doing IO. These errors can generally not be handled.
    /// It may work if tried again.
    IOError,
    /// A WASI error without an external name.  If you encounter this it means
    /// that there's probably a bug on our side (maybe as simple as forgetting to wrap
    /// this error, but perhaps something broke)
    UnknownError(__wasi_errno_t),
}

impl WasiFsError {
    pub fn from_wasi_err(err: __wasi_errno_t) -> WasiFsError {
        match err {
            __WASI_EBADF => WasiFsError::InvalidFd,
            __WASI_EEXIST => WasiFsError::AlreadyExists,
            __WASI_EIO => WasiFsError::IOError,
            _ => WasiFsError::UnknownError(err),
        }
    }
}

/// This trait relies on your file closing when it goes out of scope via `Drop`
pub trait WasiFile: std::fmt::Debug + Write + Read + Seek {
    /// the last time the file was accessed in nanoseconds as a UNIX timestamp
    fn last_accessed(&self) -> __wasi_timestamp_t;
    /// the last time the file was modified in nanoseconds as a UNIX timestamp
    fn last_modified(&self) -> __wasi_timestamp_t;
    /// the time at which the file was created in nanoseconds as a UNIX timestamp
    fn created_time(&self) -> __wasi_timestamp_t;
    /// set the last time the file was accessed in nanoseconds as a UNIX timestamp
    // TODO: stablize this in 0.7.0 by removing default impl
    fn set_last_accessed(&self, _last_accessed: __wasi_timestamp_t) {
        panic!("Default implementation for compatibilty in the 0.6.X releases; this will be removed in 0.7.0.  Please implement WasiFile::set_last_accessed for your type before then");
    }
    /// set the last time the file was modified in nanoseconds as a UNIX timestamp
    // TODO: stablize this in 0.7.0 by removing default impl
    fn set_last_modified(&self, _last_modified: __wasi_timestamp_t) {
        panic!("Default implementation for compatibilty in the 0.6.X releases; this will be removed in 0.7.0.  Please implement WasiFile::set_last_modified for your type before then");
    }
    /// set the time at which the file was created in nanoseconds as a UNIX timestamp
    // TODO: stablize this in 0.7.0 by removing default impl
    fn set_created_time(&self, _created_time: __wasi_timestamp_t) {
        panic!("Default implementation for compatibilty in the 0.6.X releases; this will be removed in 0.7.0.  Please implement WasiFile::set_created_time for your type before then");
    }
    /// the size of the file in bytes
    fn size(&self) -> u64;
    /// Change the size of the file, if the `new_size` is greater than the current size
    /// the extra bytes will be allocated and zeroed
    // TODO: stablize this in 0.7.0 by removing default impl
    fn set_len(&mut self, _new_size: __wasi_filesize_t) -> Option<()> {
        panic!("Default implementation for compatibilty in the 0.6.X releases; this will be removed in 0.7.0.  Please implement WasiFile::allocate for your type before then");
    }

    /// Request deletion of the file
    // TODO: stablize this in 0.7.0 by removing default impl
    fn unlink(&mut self) -> Option<()> {
        panic!("Default implementation for compatibilty in the 0.6.X releases; this will be removed in 0.7.0.  Please implement WasiFile::unlink for your type before then");
    }
}

/// A thin wrapper around `std::fs::File`
#[derive(Debug)]
pub struct HostFile {
    pub inner: fs::File,
    pub host_path: PathBuf,
}

impl HostFile {
    /// creates a new host file from a `std::fs::File` and a path
    pub fn new(file: fs::File, host_path: PathBuf) -> Self {
        Self {
            inner: file,
            host_path,
        }
    }

    pub fn metadata(&self) -> fs::Metadata {
        self.inner.metadata().unwrap()
    }
}

impl Read for HostFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        self.inner.read_to_end(buf)
    }
    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        self.inner.read_to_string(buf)
    }
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        self.inner.read_exact(buf)
    }
}
impl Seek for HostFile {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}
impl Write for HostFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.inner.write_all(buf)
    }
    fn write_fmt(&mut self, fmt: ::std::fmt::Arguments) -> io::Result<()> {
        self.inner.write_fmt(fmt)
    }
}

impl WasiFile for HostFile {
    fn last_accessed(&self) -> u64 {
        self.metadata()
            .accessed()
            .ok()
            .and_then(|ct| ct.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|ct| ct.as_nanos() as u64)
            .unwrap_or(0)
    }

    fn set_last_accessed(&self, _last_accessed: __wasi_timestamp_t) {
        // TODO: figure out how to do this
    }

    fn last_modified(&self) -> u64 {
        self.metadata()
            .modified()
            .ok()
            .and_then(|ct| ct.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|ct| ct.as_nanos() as u64)
            .unwrap_or(0)
    }

    fn set_last_modified(&self, _last_modified: __wasi_timestamp_t) {
        // TODO: figure out how to do this
    }

    fn created_time(&self) -> u64 {
        self.metadata()
            .created()
            .ok()
            .and_then(|ct| ct.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|ct| ct.as_nanos() as u64)
            .unwrap_or(0)
    }

    fn set_created_time(&self, _created_time: __wasi_timestamp_t) {
        // TODO: figure out how to do this
    }

    fn size(&self) -> u64 {
        self.metadata().len()
    }

    fn set_len(&mut self, new_size: __wasi_filesize_t) -> Option<()> {
        fs::File::set_len(&self.inner, new_size).ok()
    }

    fn unlink(&mut self) -> Option<()> {
        std::fs::remove_file(&self.host_path).ok()
    }
}

#[derive(Debug)]
pub struct Stdout(pub std::io::Stdout);
impl Read for Stdout {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "can not read from stdout",
        ))
    }
    fn read_to_end(&mut self, _buf: &mut Vec<u8>) -> io::Result<usize> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "can not read from stdout",
        ))
    }
    fn read_to_string(&mut self, _buf: &mut String) -> io::Result<usize> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "can not read from stdout",
        ))
    }
    fn read_exact(&mut self, _buf: &mut [u8]) -> io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "can not read from stdout",
        ))
    }
}
impl Seek for Stdout {
    fn seek(&mut self, _pos: io::SeekFrom) -> io::Result<u64> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "can not seek stdout",
        ))
    }
}
impl Write for Stdout {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.0.write_all(buf)
    }
    fn write_fmt(&mut self, fmt: ::std::fmt::Arguments) -> io::Result<()> {
        self.0.write_fmt(fmt)
    }
}

impl WasiFile for Stdout {
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
}

#[derive(Debug)]
pub struct Stderr(pub std::io::Stderr);
impl Read for Stderr {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "can not read from stderr",
        ))
    }
    fn read_to_end(&mut self, _buf: &mut Vec<u8>) -> io::Result<usize> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "can not read from stderr",
        ))
    }
    fn read_to_string(&mut self, _buf: &mut String) -> io::Result<usize> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "can not read from stderr",
        ))
    }
    fn read_exact(&mut self, _buf: &mut [u8]) -> io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "can not read from stderr",
        ))
    }
}
impl Seek for Stderr {
    fn seek(&mut self, _pos: io::SeekFrom) -> io::Result<u64> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "can not seek stderr",
        ))
    }
}
impl Write for Stderr {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.0.write_all(buf)
    }
    fn write_fmt(&mut self, fmt: ::std::fmt::Arguments) -> io::Result<()> {
        self.0.write_fmt(fmt)
    }
}

impl WasiFile for Stderr {
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
}

#[derive(Debug)]
pub struct Stdin(pub std::io::Stdin);
impl Read for Stdin {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        self.0.read_to_end(buf)
    }
    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        self.0.read_to_string(buf)
    }
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        self.0.read_exact(buf)
    }
}
impl Seek for Stdin {
    fn seek(&mut self, _pos: io::SeekFrom) -> io::Result<u64> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "can not seek stdin",
        ))
    }
}
impl Write for Stdin {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "can not write to stdin",
        ))
    }
    fn flush(&mut self) -> io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "can not write to stdin",
        ))
    }
    fn write_all(&mut self, _buf: &[u8]) -> io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "can not write to stdin",
        ))
    }
    fn write_fmt(&mut self, _fmt: ::std::fmt::Arguments) -> io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "can not write to stdin",
        ))
    }
}

impl WasiFile for Stdin {
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
}

/*
TODO: Think about using this
trait WasiFdBacking: std::fmt::Debug {
    fn get_stat(&self) -> &__wasi_filestat_t;
    fn get_stat_mut(&mut self) -> &mut __wasi_filestat_t;
    fn is_preopened(&self) -> bool;
    fn get_name(&self) -> &str;
}
*/
