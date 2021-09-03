/// types for use in the WASI filesystem
use crate::syscalls::types::*;
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::convert::TryInto;
use std::{
    collections::VecDeque,
    io::{self, Read, Seek, Write},
};

#[cfg(feature = "host-fs")]
pub use wasmer_vfs::host_fs::{Stderr, Stdin, Stdout};
#[cfg(feature = "mem-fs")]
pub use wasmer_vfs::mem_fs::{Stderr, Stdin, Stdout};

use wasmer_vfs::{FsError, VirtualFile};

pub fn fs_error_from_wasi_err(err: __wasi_errno_t) -> FsError {
    match err {
        __WASI_EBADF => FsError::InvalidFd,
        __WASI_EEXIST => FsError::AlreadyExists,
        __WASI_EIO => FsError::IOError,
        __WASI_EADDRINUSE => FsError::AddressInUse,
        __WASI_EADDRNOTAVAIL => FsError::AddressNotAvailable,
        __WASI_EPIPE => FsError::BrokenPipe,
        __WASI_ECONNABORTED => FsError::ConnectionAborted,
        __WASI_ECONNREFUSED => FsError::ConnectionRefused,
        __WASI_ECONNRESET => FsError::ConnectionReset,
        __WASI_EINTR => FsError::Interrupted,
        __WASI_EINVAL => FsError::InvalidInput,
        __WASI_ENOTCONN => FsError::NotConnected,
        __WASI_ENODEV => FsError::NoDevice,
        __WASI_ENOENT => FsError::EntityNotFound,
        __WASI_EPERM => FsError::PermissionDenied,
        __WASI_ETIMEDOUT => FsError::TimedOut,
        __WASI_EPROTO => FsError::UnexpectedEof,
        __WASI_EAGAIN => FsError::WouldBlock,
        __WASI_ENOSPC => FsError::WriteZero,
        __WASI_ENOTEMPTY => FsError::DirectoryNotEmpty,
        _ => FsError::UnknownError,
    }
}

pub fn fs_error_into_wasi_err(fs_error: FsError) -> __wasi_errno_t {
    match fs_error {
        FsError::AlreadyExists => __WASI_EEXIST,
        FsError::AddressInUse => __WASI_EADDRINUSE,
        FsError::AddressNotAvailable => __WASI_EADDRNOTAVAIL,
        FsError::BaseNotDirectory => __WASI_ENOTDIR,
        FsError::BrokenPipe => __WASI_EPIPE,
        FsError::ConnectionAborted => __WASI_ECONNABORTED,
        FsError::ConnectionRefused => __WASI_ECONNREFUSED,
        FsError::ConnectionReset => __WASI_ECONNRESET,
        FsError::Interrupted => __WASI_EINTR,
        FsError::InvalidData => __WASI_EIO,
        FsError::InvalidFd => __WASI_EBADF,
        FsError::InvalidInput => __WASI_EINVAL,
        FsError::IOError => __WASI_EIO,
        FsError::NoDevice => __WASI_ENODEV,
        FsError::NotAFile => __WASI_EINVAL,
        FsError::NotConnected => __WASI_ENOTCONN,
        FsError::EntityNotFound => __WASI_ENOENT,
        FsError::PermissionDenied => __WASI_EPERM,
        FsError::TimedOut => __WASI_ETIMEDOUT,
        FsError::UnexpectedEof => __WASI_EPROTO,
        FsError::WouldBlock => __WASI_EAGAIN,
        FsError::WriteZero => __WASI_ENOSPC,
        FsError::DirectoryNotEmpty => __WASI_ENOTEMPTY,
        FsError::Lock | FsError::UnknownError => __WASI_EIO,
    }
}

#[derive(Debug, Clone)]
pub enum PollEvent {
    /// Data available to read
    PollIn = 1,
    /// Data available to write (will still block if data is greater than space available unless
    /// the fd is configured to not block)
    PollOut = 2,
    /// Something didn't work. ignored as input
    PollError = 4,
    /// Connection closed. ignored as input
    PollHangUp = 8,
    /// Invalid request. ignored as input
    PollInvalid = 16,
}

impl PollEvent {
    fn from_i16(raw_num: i16) -> Option<PollEvent> {
        Some(match raw_num {
            1 => PollEvent::PollIn,
            2 => PollEvent::PollOut,
            4 => PollEvent::PollError,
            8 => PollEvent::PollHangUp,
            16 => PollEvent::PollInvalid,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct PollEventBuilder {
    inner: PollEventSet,
}

pub type PollEventSet = i16;

#[derive(Debug)]
pub struct PollEventIter {
    pes: PollEventSet,
    i: usize,
}

impl Iterator for PollEventIter {
    type Item = PollEvent;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pes == 0 || self.i > 15 {
            None
        } else {
            while self.i < 16 {
                let result = PollEvent::from_i16(self.pes & (1 << self.i));
                self.pes &= !(1 << self.i);
                self.i += 1;
                if let Some(r) = result {
                    return Some(r);
                }
            }
            unreachable!("Internal logic error in PollEventIter");
        }
    }
}

pub fn iterate_poll_events(pes: PollEventSet) -> PollEventIter {
    PollEventIter { pes, i: 0 }
}

#[cfg(unix)]
fn poll_event_set_to_platform_poll_events(mut pes: PollEventSet) -> i16 {
    let mut out = 0;
    for i in 0..16 {
        out |= match PollEvent::from_i16(pes & (1 << i)) {
            Some(PollEvent::PollIn) => libc::POLLIN,
            Some(PollEvent::PollOut) => libc::POLLOUT,
            Some(PollEvent::PollError) => libc::POLLERR,
            Some(PollEvent::PollHangUp) => libc::POLLHUP,
            Some(PollEvent::PollInvalid) => libc::POLLNVAL,
            _ => 0,
        };
        pes &= !(1 << i);
    }
    out
}

#[cfg(unix)]
fn platform_poll_events_to_pollevent_set(mut num: i16) -> PollEventSet {
    let mut peb = PollEventBuilder::new();
    for i in 0..16 {
        peb = match num & (1 << i) {
            libc::POLLIN => peb.add(PollEvent::PollIn),
            libc::POLLOUT => peb.add(PollEvent::PollOut),
            libc::POLLERR => peb.add(PollEvent::PollError),
            libc::POLLHUP => peb.add(PollEvent::PollHangUp),
            libc::POLLNVAL => peb.add(PollEvent::PollInvalid),
            _ => peb,
        };
        num &= !(1 << i);
    }
    peb.build()
}

impl PollEventBuilder {
    pub fn new() -> PollEventBuilder {
        PollEventBuilder { inner: 0 }
    }

    pub fn add(mut self, event: PollEvent) -> PollEventBuilder {
        self.inner |= event as PollEventSet;
        self
    }

    pub fn build(self) -> PollEventSet {
        self.inner
    }
}

#[cfg(unix)]
pub(crate) fn poll(
    selfs: &[&dyn VirtualFile],
    events: &[PollEventSet],
    seen_events: &mut [PollEventSet],
) -> Result<u32, FsError> {
    if !(selfs.len() == events.len() && events.len() == seen_events.len()) {
        return Err(FsError::InvalidInput);
    }
    let mut fds = selfs
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.get_fd().map(|rfd| (i, rfd)))
        .map(|(i, host_fd)| libc::pollfd {
            fd: host_fd.try_into().unwrap(),
            events: poll_event_set_to_platform_poll_events(events[i]),
            revents: 0,
        })
        .collect::<Vec<_>>();
    let result = unsafe { libc::poll(fds.as_mut_ptr(), selfs.len() as _, 1) };

    if result < 0 {
        // TODO: check errno and return value
        return Err(FsError::IOError);
    }
    // convert result and write back values
    for (i, fd) in fds.into_iter().enumerate() {
        seen_events[i] = platform_poll_events_to_pollevent_set(fd.revents);
    }
    // unwrap is safe because we check for negative values above
    Ok(result.try_into().unwrap())
}

#[cfg(not(unix))]
pub(crate) fn poll(
    _selfs: &[&dyn VirtualFile],
    _events: &[PollEventSet],
    _seen_events: &mut [PollEventSet],
) -> Result<(), FsError> {
    unimplemented!("VirtualFile::poll is not implemented for non-Unix-like targets yet");
}

pub trait WasiPath {}

/// For piping stdio. Stores all output / input in a byte-vector.
#[derive(Debug, Default)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct Pipe {
    buffer: VecDeque<u8>,
}

impl Pipe {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Read for Pipe {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let amt = std::cmp::min(buf.len(), self.buffer.len());
        for (i, byte) in self.buffer.drain(..amt).enumerate() {
            buf[i] = byte;
        }
        Ok(amt)
    }
}

impl Write for Pipe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.extend(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Seek for Pipe {
    fn seek(&mut self, _pos: io::SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not seek in a pipe",
        ))
    }
}

#[cfg_attr(feature = "enable-serde", typetag::serde)]
impl VirtualFile for Pipe {
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
        self.buffer.len() as u64
    }
    fn set_len(&mut self, len: u64) -> Result<(), FsError> {
        self.buffer.resize(len as usize, 0);
        Ok(())
    }
    fn unlink(&mut self) -> Result<(), FsError> {
        Ok(())
    }
    fn bytes_available(&self) -> Result<usize, FsError> {
        Ok(self.buffer.len())
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
