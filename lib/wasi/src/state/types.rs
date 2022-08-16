/// types for use in the WASI filesystem
use crate::syscalls::types::*;
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
#[cfg(all(unix, feature = "sys-poll"))]
use std::convert::TryInto;
use std::{
    collections::VecDeque,
    io::{self, Read, Seek, Write},
    sync::{Arc, Mutex},
    time::Duration,
};
use wasmer_vbus::BusError;
use wasmer_wasi_types_generated::wasi_snapshot0;

#[cfg(feature = "host-fs")]
pub use wasmer_vfs::host_fs::{Stderr, Stdin, Stdout};
#[cfg(feature = "mem-fs")]
pub use wasmer_vfs::mem_fs::{Stderr, Stdin, Stdout};

use wasmer_vfs::{FsError, VirtualFile};
use wasmer_vnet::NetworkError;

pub fn fs_error_from_wasi_err(err: wasi_snapshot0::Errno) -> FsError {
    match err {
        wasi_snapshot0::Errno::Badf => FsError::InvalidFd,
        wasi_snapshot0::Errno::Exist => FsError::AlreadyExists,
        wasi_snapshot0::Errno::Io => FsError::IOError,
        wasi_snapshot0::Errno::Addrinuse => FsError::AddressInUse,
        wasi_snapshot0::Errno::Addrnotavail => FsError::AddressNotAvailable,
        wasi_snapshot0::Errno::Pipe => FsError::BrokenPipe,
        wasi_snapshot0::Errno::Connaborted => FsError::ConnectionAborted,
        wasi_snapshot0::Errno::Connrefused => FsError::ConnectionRefused,
        wasi_snapshot0::Errno::Connreset => FsError::ConnectionReset,
        wasi_snapshot0::Errno::Intr => FsError::Interrupted,
        wasi_snapshot0::Errno::Inval => FsError::InvalidInput,
        wasi_snapshot0::Errno::Notconn => FsError::NotConnected,
        wasi_snapshot0::Errno::Nodev => FsError::NoDevice,
        wasi_snapshot0::Errno::Noent => FsError::EntityNotFound,
        wasi_snapshot0::Errno::Perm => FsError::PermissionDenied,
        wasi_snapshot0::Errno::Timedout => FsError::TimedOut,
        wasi_snapshot0::Errno::Proto => FsError::UnexpectedEof,
        wasi_snapshot0::Errno::Again => FsError::WouldBlock,
        wasi_snapshot0::Errno::Nospc => FsError::WriteZero,
        wasi_snapshot0::Errno::Notempty => FsError::DirectoryNotEmpty,
        _ => FsError::UnknownError,
    }
}

pub fn fs_error_into_wasi_err(fs_error: FsError) -> wasi_snapshot0::Errno {
    match fs_error {
        FsError::AlreadyExists => wasi_snapshot0::Errno::Exist,
        FsError::AddressInUse => wasi_snapshot0::Errno::Addrinuse,
        FsError::AddressNotAvailable => wasi_snapshot0::Errno::Addrnotavail,
        FsError::BaseNotDirectory => wasi_snapshot0::Errno::Notdir,
        FsError::BrokenPipe => wasi_snapshot0::Errno::Pipe,
        FsError::ConnectionAborted => wasi_snapshot0::Errno::Connaborted,
        FsError::ConnectionRefused => wasi_snapshot0::Errno::Connrefused,
        FsError::ConnectionReset => wasi_snapshot0::Errno::Connreset,
        FsError::Interrupted => wasi_snapshot0::Errno::Intr,
        FsError::InvalidData => wasi_snapshot0::Errno::Io,
        FsError::InvalidFd => wasi_snapshot0::Errno::Badf,
        FsError::InvalidInput => wasi_snapshot0::Errno::Inval,
        FsError::IOError => wasi_snapshot0::Errno::Io,
        FsError::NoDevice => wasi_snapshot0::Errno::Nodev,
        FsError::NotAFile => wasi_snapshot0::Errno::Inval,
        FsError::NotConnected => wasi_snapshot0::Errno::Notconn,
        FsError::EntityNotFound => wasi_snapshot0::Errno::Noent,
        FsError::PermissionDenied => wasi_snapshot0::Errno::Perm,
        FsError::TimedOut => wasi_snapshot0::Errno::Timedout,
        FsError::UnexpectedEof => wasi_snapshot0::Errno::Proto,
        FsError::WouldBlock => wasi_snapshot0::Errno::Again,
        FsError::WriteZero => wasi_snapshot0::Errno::Nospc,
        FsError::DirectoryNotEmpty => wasi_snapshot0::Errno::Notempty,
        FsError::Lock | FsError::UnknownError => wasi_snapshot0::Errno::Io,
    }
}

pub fn net_error_into_wasi_err(net_error: NetworkError) -> wasi_snapshot0::Errno {
    match net_error {
        NetworkError::InvalidFd => wasi_snapshot0::Errno::Badf,
        NetworkError::AlreadyExists => wasi_snapshot0::Errno::Exist,
        NetworkError::Lock => wasi_snapshot0::Errno::Io,
        NetworkError::IOError => wasi_snapshot0::Errno::Io,
        NetworkError::AddressInUse => wasi_snapshot0::Errno::Addrinuse,
        NetworkError::AddressNotAvailable => wasi_snapshot0::Errno::Addrnotavail,
        NetworkError::BrokenPipe => wasi_snapshot0::Errno::Pipe,
        NetworkError::ConnectionAborted => wasi_snapshot0::Errno::Connaborted,
        NetworkError::ConnectionRefused => wasi_snapshot0::Errno::Connrefused,
        NetworkError::ConnectionReset => wasi_snapshot0::Errno::Connreset,
        NetworkError::Interrupted => wasi_snapshot0::Errno::Intr,
        NetworkError::InvalidData => wasi_snapshot0::Errno::Io,
        NetworkError::InvalidInput => wasi_snapshot0::Errno::Inval,
        NetworkError::NotConnected => wasi_snapshot0::Errno::Notconn,
        NetworkError::NoDevice => wasi_snapshot0::Errno::Nodev,
        NetworkError::PermissionDenied => wasi_snapshot0::Errno::Perm,
        NetworkError::TimedOut => wasi_snapshot0::Errno::Timedout,
        NetworkError::UnexpectedEof => wasi_snapshot0::Errno::Proto,
        NetworkError::WouldBlock => wasi_snapshot0::Errno::Again,
        NetworkError::WriteZero => wasi_snapshot0::Errno::Nospc,
        NetworkError::Unsupported => wasi_snapshot0::Errno::Notsup,
        NetworkError::UnknownError => wasi_snapshot0::Errno::Io,
    }
}

pub fn bus_error_into_wasi_err(bus_error: BusError) -> __bus_errno_t {
    use BusError::*;
    match bus_error {
        Serialization => __BUS_ESER,
        Deserialization => __BUS_EDES,
        InvalidWapm => __BUS_EWAPM,
        FetchFailed => __BUS_EFETCH,
        CompileError => __BUS_ECOMPILE,
        InvalidABI => __BUS_EABI,
        Aborted => __BUS_EABORTED,
        BadHandle => __BUS_EBADHANDLE,
        InvalidTopic => __BUS_ETOPIC,
        BadCallback => __BUS_EBADCB,
        Unsupported => __BUS_EUNSUPPORTED,
        BadRequest => __BUS_EBADREQUEST,
        AccessDenied => __BUS_EDENIED,
        InternalError => __BUS_EINTERNAL,
        MemoryAllocationFailed => __BUS_EALLOC,
        InvokeFailed => __BUS_EINVOKE,
        AlreadyConsumed => __BUS_ECONSUMED,
        MemoryAccessViolation => __BUS_EMEMVIOLATION,
        UnknownError => __BUS_EUNKNOWN,
    }
}

pub fn wasi_error_into_bus_err(bus_error: __bus_errno_t) -> BusError {
    use BusError::*;
    match bus_error {
        __BUS_ESER => Serialization,
        __BUS_EDES => Deserialization,
        __BUS_EWAPM => InvalidWapm,
        __BUS_EFETCH => FetchFailed,
        __BUS_ECOMPILE => CompileError,
        __BUS_EABI => InvalidABI,
        __BUS_EABORTED => Aborted,
        __BUS_EBADHANDLE => BadHandle,
        __BUS_ETOPIC => InvalidTopic,
        __BUS_EBADCB => BadCallback,
        __BUS_EUNSUPPORTED => Unsupported,
        __BUS_EBADREQUEST => BadRequest,
        __BUS_EDENIED => AccessDenied,
        __BUS_EINTERNAL => InternalError,
        __BUS_EALLOC => MemoryAllocationFailed,
        __BUS_EINVOKE => InvokeFailed,
        __BUS_ECONSUMED => AlreadyConsumed,
        __BUS_EMEMVIOLATION => MemoryAccessViolation,
        /*__BUS_EUNKNOWN |*/ _ => UnknownError,
    }
}

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
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

#[cfg(all(unix, feature = "sys-poll"))]
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

#[cfg(all(unix, feature = "sys-poll"))]
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

#[allow(dead_code)]
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

#[cfg(all(unix, feature = "sys-poll"))]
pub(crate) fn poll(
    selfs: &[&(dyn VirtualFile + Send + Sync + 'static)],
    events: &[PollEventSet],
    seen_events: &mut [PollEventSet],
    timeout: Duration,
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
    let result = unsafe {
        libc::poll(
            fds.as_mut_ptr(),
            selfs.len() as _,
            timeout.as_millis() as i32,
        )
    };

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

#[cfg(any(not(unix), not(feature = "sys-poll")))]
pub(crate) fn poll(
    files: &[&(dyn VirtualFile + Send + Sync + 'static)],
    events: &[PollEventSet],
    seen_events: &mut [PollEventSet],
    timeout: Duration,
) -> Result<u32, FsError> {
    if !(files.len() == events.len() && events.len() == seen_events.len()) {
        tracing::debug!("the slice length of 'files', 'events' and 'seen_events' must be the same (files={}, events={}, seen_events={})", files.len(), events.len(), seen_events.len());
        return Err(FsError::InvalidInput);
    }

    let mut ret = 0;
    for n in 0..files.len() {
        let mut builder = PollEventBuilder::new();

        let file = files[n];
        let can_read = file.bytes_available_read()?.map(|_| true).unwrap_or(false);
        let can_write = file
            .bytes_available_write()?
            .map(|s| s > 0)
            .unwrap_or(false);
        let is_closed = file.is_open() == false;

        tracing::debug!(
            "poll_evt can_read={} can_write={} is_closed={}",
            can_read,
            can_write,
            is_closed
        );

        for event in iterate_poll_events(events[n]) {
            match event {
                PollEvent::PollIn if can_read => {
                    builder = builder.add(PollEvent::PollIn);
                }
                PollEvent::PollOut if can_write => {
                    builder = builder.add(PollEvent::PollOut);
                }
                PollEvent::PollHangUp if is_closed => {
                    builder = builder.add(PollEvent::PollHangUp);
                }
                PollEvent::PollInvalid if is_closed => {
                    builder = builder.add(PollEvent::PollInvalid);
                }
                PollEvent::PollError if is_closed => {
                    builder = builder.add(PollEvent::PollError);
                }
                _ => {}
            }
        }
        let revents = builder.build();
        if revents != 0 {
            ret += 1;
        }
        seen_events[n] = revents;
    }

    if ret == 0 && timeout > Duration::ZERO {
        return Err(FsError::WouldBlock);
    }

    Ok(ret)
}

pub trait WasiPath {}

/// For piping stdio. Stores all output / input in a byte-vector.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct Pipe {
    buffer: Arc<Mutex<VecDeque<u8>>>,
}

impl Pipe {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Read for Pipe {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut buffer = self.buffer.lock().unwrap();
        let amt = std::cmp::min(buf.len(), buffer.len());
        let buf_iter = buffer.drain(..amt).enumerate();
        for (i, byte) in buf_iter {
            buf[i] = byte;
        }
        Ok(amt)
    }
}

impl Write for Pipe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut buffer = self.buffer.lock().unwrap();
        buffer.extend(buf);
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

//#[cfg_attr(feature = "enable-serde", typetag::serde)]
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
        let buffer = self.buffer.lock().unwrap();
        buffer.len() as u64
    }
    fn set_len(&mut self, len: u64) -> Result<(), FsError> {
        let mut buffer = self.buffer.lock().unwrap();
        buffer.resize(len as usize, 0);
        Ok(())
    }
    fn unlink(&mut self) -> Result<(), FsError> {
        Ok(())
    }
    fn bytes_available_read(&self) -> Result<Option<usize>, FsError> {
        let buffer = self.buffer.lock().unwrap();
        Ok(Some(buffer.len()))
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
