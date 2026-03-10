/// types for use in the WASI filesystem
use crate::syscalls::types::*;
use serde::{de, Deserialize, Serialize};
#[cfg(unix)]
use std::convert::TryInto;
use std::{
    fs,
    io::{self, Read, Seek, Write},
    path::PathBuf,
    time::SystemTime,
};

/// Error type for external users
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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
    /// The address was in use
    AddressInUse,
    /// The address could not be found
    AddressNotAvailable,
    /// A pipe was closed
    BrokenPipe,
    /// The connection was aborted
    ConnectionAborted,
    /// The connection request was refused
    ConnectionRefused,
    /// The connection was reset
    ConnectionReset,
    /// The operation was interrupted before it could finish
    Interrupted,
    /// Invalid internal data, if the argument data is invalid, use `InvalidInput`
    InvalidData,
    /// The provided data is invalid
    InvalidInput,
    /// Could not perform the operation because there was not an open connection
    NotConnected,
    /// The requested file or directory could not be found
    EntityNotFound,
    /// The requested device couldn't be accessed
    NoDevice,
    /// Caller was not allowed to perform this operation
    PermissionDenied,
    /// The operation did not complete within the given amount of time
    TimedOut,
    /// Found EOF when EOF was not expected
    UnexpectedEof,
    /// Operation would block, this error lets the caller know that they can try again
    WouldBlock,
    /// A call to write returned 0
    WriteZero,
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
            __WASI_EADDRINUSE => WasiFsError::AddressInUse,
            __WASI_EADDRNOTAVAIL => WasiFsError::AddressNotAvailable,
            __WASI_EPIPE => WasiFsError::BrokenPipe,
            __WASI_ECONNABORTED => WasiFsError::ConnectionAborted,
            __WASI_ECONNREFUSED => WasiFsError::ConnectionRefused,
            __WASI_ECONNRESET => WasiFsError::ConnectionReset,
            __WASI_EINTR => WasiFsError::Interrupted,
            __WASI_EINVAL => WasiFsError::InvalidInput,
            __WASI_ENOTCONN => WasiFsError::NotConnected,
            __WASI_ENODEV => WasiFsError::NoDevice,
            __WASI_ENOENT => WasiFsError::EntityNotFound,
            __WASI_EPERM => WasiFsError::PermissionDenied,
            __WASI_ETIMEDOUT => WasiFsError::TimedOut,
            __WASI_EPROTO => WasiFsError::UnexpectedEof,
            __WASI_EAGAIN => WasiFsError::WouldBlock,
            __WASI_ENOSPC => WasiFsError::WriteZero,
            _ => WasiFsError::UnknownError(err),
        }
    }

    pub fn into_wasi_err(self) -> __wasi_errno_t {
        match self {
            WasiFsError::AlreadyExists => __WASI_EEXIST,
            WasiFsError::AddressInUse => __WASI_EADDRINUSE,
            WasiFsError::AddressNotAvailable => __WASI_EADDRNOTAVAIL,
            WasiFsError::BaseNotDirectory => __WASI_ENOTDIR,
            WasiFsError::BrokenPipe => __WASI_EPIPE,
            WasiFsError::ConnectionAborted => __WASI_ECONNABORTED,
            WasiFsError::ConnectionRefused => __WASI_ECONNREFUSED,
            WasiFsError::ConnectionReset => __WASI_ECONNRESET,
            WasiFsError::Interrupted => __WASI_EINTR,
            WasiFsError::InvalidData => __WASI_EIO,
            WasiFsError::InvalidFd => __WASI_EBADF,
            WasiFsError::InvalidInput => __WASI_EINVAL,
            WasiFsError::IOError => __WASI_EIO,
            WasiFsError::NoDevice => __WASI_ENODEV,
            WasiFsError::NotAFile => __WASI_EINVAL,
            WasiFsError::NotConnected => __WASI_ENOTCONN,
            WasiFsError::EntityNotFound => __WASI_ENOENT,
            WasiFsError::PermissionDenied => __WASI_EPERM,
            WasiFsError::TimedOut => __WASI_ETIMEDOUT,
            WasiFsError::UnexpectedEof => __WASI_EPROTO,
            WasiFsError::WouldBlock => __WASI_EAGAIN,
            WasiFsError::WriteZero => __WASI_ENOSPC,
            WasiFsError::UnknownError(ec) => ec,
        }
    }
}

/// This trait relies on your file closing when it goes out of scope via `Drop`
#[typetag::serde(tag = "type")]
pub trait WasiFile: std::fmt::Debug + Send + Write + Read + Seek {
    /// the last time the file was accessed in nanoseconds as a UNIX timestamp
    fn last_accessed(&self) -> __wasi_timestamp_t;

    /// the last time the file was modified in nanoseconds as a UNIX timestamp
    fn last_modified(&self) -> __wasi_timestamp_t;

    /// the time at which the file was created in nanoseconds as a UNIX timestamp
    fn created_time(&self) -> __wasi_timestamp_t;

    /// set the last time the file was accessed in nanoseconds as a UNIX timestamp
    fn set_last_accessed(&self, _last_accessed: __wasi_timestamp_t) {
        debug!("{:?} did nothing in WasiFile::set_last_accessed due to using the default implementation", self);
    }

    /// set the last time the file was modified in nanoseconds as a UNIX timestamp
    fn set_last_modified(&self, _last_modified: __wasi_timestamp_t) {
        debug!("{:?} did nothing in WasiFile::set_last_modified due to using the default implementation", self);
    }

    /// set the time at which the file was created in nanoseconds as a UNIX timestamp
    fn set_created_time(&self, _created_time: __wasi_timestamp_t) {
        debug!(
            "{:?} did nothing in WasiFile::set_created_time to using the default implementation",
            self
        );
    }

    /// the size of the file in bytes
    fn size(&self) -> u64;

    /// Change the size of the file, if the `new_size` is greater than the current size
    /// the extra bytes will be allocated and zeroed
    fn set_len(&mut self, _new_size: __wasi_filesize_t) -> Result<(), WasiFsError>;

    /// Request deletion of the file
    fn unlink(&mut self) -> Result<(), WasiFsError>;

    /// Store file contents and metadata to disk
    /// Default implementation returns `Ok(())`.  You should implement this method if you care
    /// about flushing your cache to permanent storage
    fn sync_to_disk(&self) -> Result<(), WasiFsError> {
        Ok(())
    }

    /// Moves the file to a new location
    /// NOTE: the signature of this function will change before stabilization
    // TODO: stablizie this in 0.7.0 or 0.8.0 by removing default impl
    fn rename_file(&self, _new_name: &std::path::Path) -> Result<(), WasiFsError> {
        panic!("Default implementation for now as this method is unstable; this default implementation or this entire method may be removed in a future release.");
    }

    /// Returns the number of bytes available.  This function must not block
    fn bytes_available(&self) -> Result<usize, WasiFsError>;

    /// Used for polling.  Default returns `None` because this method cannot be implemented for most types
    /// Returns the underlying host fd
    fn get_raw_fd(&self) -> Option<i32> {
        None
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
    selfs: &[&dyn WasiFile],
    events: &[PollEventSet],
    seen_events: &mut [PollEventSet],
) -> Result<u32, WasiFsError> {
    if !(selfs.len() == events.len() && events.len() == seen_events.len()) {
        return Err(WasiFsError::InvalidInput);
    }
    let mut fds = selfs
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.get_raw_fd().map(|rfd| (i, rfd)))
        .map(|(i, host_fd)| libc::pollfd {
            fd: host_fd,
            events: poll_event_set_to_platform_poll_events(events[i]),
            revents: 0,
        })
        .collect::<Vec<_>>();
    let result = unsafe { libc::poll(fds.as_mut_ptr(), selfs.len() as _, 1) };

    if result < 0 {
        // TODO: check errno and return value
        return Err(WasiFsError::IOError);
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
    _selfs: &[&dyn WasiFile],
    _events: &[PollEventSet],
    _seen_events: &mut [PollEventSet],
) -> Result<(), WasiFsError> {
    unimplemented!("HostFile::poll in WasiFile is not implemented for non-Unix-like targets yet");
}

pub trait WasiPath {}

/// A thin wrapper around `std::fs::File`
#[derive(Debug, Serialize)]
pub struct HostFile {
    #[serde(skip_serializing)]
    pub inner: fs::File,
    pub host_path: PathBuf,
    flags: u16,
}

impl<'de> Deserialize<'de> for HostFile {
    fn deserialize<D>(deserializer: D) -> Result<HostFile, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "snake_case")]
        enum Field {
            HostPath,
            Flags,
        }

        struct HostFileVisitor;

        impl<'de> de::Visitor<'de> for HostFileVisitor {
            type Value = HostFile;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("struct HostFile")
            }

            fn visit_seq<V>(self, mut seq: V) -> Result<Self::Value, V::Error>
            where
                V: de::SeqAccess<'de>,
            {
                let host_path = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let flags = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?;
                let inner = std::fs::OpenOptions::new()
                    .read(flags & HostFile::READ != 0)
                    .write(flags & HostFile::WRITE != 0)
                    .append(flags & HostFile::APPEND != 0)
                    .open(&host_path)
                    .map_err(|_| de::Error::custom("Could not open file on this system"))?;
                Ok(HostFile {
                    inner,
                    host_path,
                    flags,
                })
            }

            fn visit_map<V>(self, mut map: V) -> Result<Self::Value, V::Error>
            where
                V: de::MapAccess<'de>,
            {
                let mut host_path = None;
                let mut flags = None;
                while let Some(key) = map.next_key()? {
                    match key {
                        Field::HostPath => {
                            if host_path.is_some() {
                                return Err(de::Error::duplicate_field("host_path"));
                            }
                            host_path = Some(map.next_value()?);
                        }
                        Field::Flags => {
                            if flags.is_some() {
                                return Err(de::Error::duplicate_field("flags"));
                            }
                            flags = Some(map.next_value()?);
                        }
                    }
                }
                let host_path = host_path.ok_or_else(|| de::Error::missing_field("host_path"))?;
                let flags = flags.ok_or_else(|| de::Error::missing_field("flags"))?;
                let inner = std::fs::OpenOptions::new()
                    .read(flags & HostFile::READ != 0)
                    .write(flags & HostFile::WRITE != 0)
                    .append(flags & HostFile::APPEND != 0)
                    .open(&host_path)
                    .map_err(|_| de::Error::custom("Could not open file on this system"))?;
                Ok(HostFile {
                    inner,
                    host_path,
                    flags,
                })
            }
        }

        const FIELDS: &[&str] = &["host_path", "flags"];
        deserializer.deserialize_struct("HostFile", FIELDS, HostFileVisitor)
    }
}

impl HostFile {
    const READ: u16 = 1;
    const WRITE: u16 = 2;
    const APPEND: u16 = 4;

    /// creates a new host file from a `std::fs::File` and a path
    pub fn new(file: fs::File, host_path: PathBuf, read: bool, write: bool, append: bool) -> Self {
        let mut flags = 0;
        if read {
            flags |= Self::READ;
        }
        if write {
            flags |= Self::WRITE;
        }
        if append {
            flags |= Self::APPEND;
        }
        Self {
            inner: file,
            host_path,
            flags,
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

#[typetag::serde]
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

    fn set_len(&mut self, new_size: __wasi_filesize_t) -> Result<(), WasiFsError> {
        fs::File::set_len(&self.inner, new_size).map_err(Into::into)
    }

    fn unlink(&mut self) -> Result<(), WasiFsError> {
        std::fs::remove_file(&self.host_path).map_err(Into::into)
    }
    fn sync_to_disk(&self) -> Result<(), WasiFsError> {
        self.inner.sync_all().map_err(Into::into)
    }

    fn rename_file(&self, new_name: &std::path::Path) -> Result<(), WasiFsError> {
        std::fs::rename(&self.host_path, new_name).map_err(Into::into)
    }

    fn bytes_available(&self) -> Result<usize, WasiFsError> {
        // unwrap is safe because of get_raw_fd implementation
        let host_fd = self.get_raw_fd().unwrap();

        host_file_bytes_available(host_fd)
    }

    #[cfg(unix)]
    fn get_raw_fd(&self) -> Option<i32> {
        use std::os::unix::io::AsRawFd;
        Some(self.inner.as_raw_fd())
    }
    #[cfg(not(unix))]
    fn get_raw_fd(&self) -> Option<i32> {
        unimplemented!(
            "HostFile::get_raw_fd in WasiFile is not implemented for non-Unix-like targets yet"
        );
    }
}

impl From<io::Error> for WasiFsError {
    fn from(io_error: io::Error) -> Self {
        match io_error.kind() {
            io::ErrorKind::AddrInUse => WasiFsError::AddressInUse,
            io::ErrorKind::AddrNotAvailable => WasiFsError::AddressNotAvailable,
            io::ErrorKind::AlreadyExists => WasiFsError::AlreadyExists,
            io::ErrorKind::BrokenPipe => WasiFsError::BrokenPipe,
            io::ErrorKind::ConnectionAborted => WasiFsError::ConnectionAborted,
            io::ErrorKind::ConnectionRefused => WasiFsError::ConnectionRefused,
            io::ErrorKind::ConnectionReset => WasiFsError::ConnectionReset,
            io::ErrorKind::Interrupted => WasiFsError::Interrupted,
            io::ErrorKind::InvalidData => WasiFsError::InvalidData,
            io::ErrorKind::InvalidInput => WasiFsError::InvalidInput,
            io::ErrorKind::NotConnected => WasiFsError::NotConnected,
            io::ErrorKind::NotFound => WasiFsError::EntityNotFound,
            io::ErrorKind::PermissionDenied => WasiFsError::PermissionDenied,
            io::ErrorKind::TimedOut => WasiFsError::TimedOut,
            io::ErrorKind::UnexpectedEof => WasiFsError::UnexpectedEof,
            io::ErrorKind::WouldBlock => WasiFsError::WouldBlock,
            io::ErrorKind::WriteZero => WasiFsError::WriteZero,
            io::ErrorKind::Other => WasiFsError::IOError,
            // if the following triggers, a new error type was added to this non-exhaustive enum
            _ => WasiFsError::UnknownError(__WASI_EIO),
        }
    }
}

#[cfg(unix)]
fn host_file_bytes_available(host_fd: i32) -> Result<usize, WasiFsError> {
    let mut bytes_found = 0 as libc::c_int;
    let result = unsafe { libc::ioctl(host_fd, libc::FIONREAD, &mut bytes_found) };

    match result {
        // success
        0 => Ok(bytes_found.try_into().unwrap_or(0)),
        libc::EBADF => Err(WasiFsError::InvalidFd),
        libc::EFAULT => Err(WasiFsError::InvalidData),
        libc::EINVAL => Err(WasiFsError::InvalidInput),
        _ => Err(WasiFsError::IOError),
    }
}

#[cfg(not(unix))]
fn host_file_bytes_available(_raw_fd: i32) -> Result<usize, WasiFsError> {
    unimplemented!("host_file_bytes_available not yet implemented for non-Unix-like targets.  This probably means the program tried to use wasi::poll_oneoff")
}

/// A wrapper type around Stdout that implements `WasiFile` and
/// `Serialize` + `Deserialize`.
#[derive(Debug, Serialize, Deserialize)]
pub struct Stdout;
impl Read for Stdout {
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
impl Seek for Stdout {
    fn seek(&mut self, _pos: io::SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(io::ErrorKind::Other, "can not seek stdout"))
    }
}
impl Write for Stdout {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        io::stdout().write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        io::stdout().flush()
    }
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        io::stdout().write_all(buf)
    }
    fn write_fmt(&mut self, fmt: ::std::fmt::Arguments) -> io::Result<()> {
        io::stdout().write_fmt(fmt)
    }
}

#[typetag::serde]
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
    fn set_len(&mut self, _new_size: __wasi_filesize_t) -> Result<(), WasiFsError> {
        debug!("Calling WasiFile::set_len on stdout; this is probably a bug");
        Err(WasiFsError::PermissionDenied)
    }
    fn unlink(&mut self) -> Result<(), WasiFsError> {
        Ok(())
    }

    fn bytes_available(&self) -> Result<usize, WasiFsError> {
        // unwrap is safe because of get_raw_fd implementation
        let host_fd = self.get_raw_fd().unwrap();

        host_file_bytes_available(host_fd)
    }

    #[cfg(unix)]
    fn get_raw_fd(&self) -> Option<i32> {
        use std::os::unix::io::AsRawFd;
        Some(io::stdout().as_raw_fd())
    }

    #[cfg(not(unix))]
    fn get_raw_fd(&self) -> Option<i32> {
        unimplemented!(
            "Stdout::get_raw_fd in WasiFile is not implemented for non-Unix-like targets yet"
        );
    }
}

/// A wrapper type around Stderr that implements `WasiFile` and
/// `Serialize` + `Deserialize`.
#[derive(Debug, Serialize, Deserialize)]
pub struct Stderr;
impl Read for Stderr {
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
impl Seek for Stderr {
    fn seek(&mut self, _pos: io::SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(io::ErrorKind::Other, "can not seek stderr"))
    }
}
impl Write for Stderr {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        io::stderr().write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        io::stderr().flush()
    }
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        io::stderr().write_all(buf)
    }
    fn write_fmt(&mut self, fmt: ::std::fmt::Arguments) -> io::Result<()> {
        io::stderr().write_fmt(fmt)
    }
}

#[typetag::serde]
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
    fn set_len(&mut self, _new_size: __wasi_filesize_t) -> Result<(), WasiFsError> {
        debug!("Calling WasiFile::set_len on stderr; this is probably a bug");
        Err(WasiFsError::PermissionDenied)
    }
    fn unlink(&mut self) -> Result<(), WasiFsError> {
        Ok(())
    }

    fn bytes_available(&self) -> Result<usize, WasiFsError> {
        // unwrap is safe because of get_raw_fd implementation
        let host_fd = self.get_raw_fd().unwrap();

        host_file_bytes_available(host_fd)
    }

    #[cfg(unix)]
    fn get_raw_fd(&self) -> Option<i32> {
        use std::os::unix::io::AsRawFd;
        Some(io::stderr().as_raw_fd())
    }

    #[cfg(not(unix))]
    fn get_raw_fd(&self) -> Option<i32> {
        unimplemented!(
            "Stderr::get_raw_fd in WasiFile is not implemented for non-Unix-like targets yet"
        );
    }
}

/// A wrapper type around Stdin that implements `WasiFile` and
/// `Serialize` + `Deserialize`.
#[derive(Debug, Serialize, Deserialize)]
pub struct Stdin;
impl Read for Stdin {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        io::stdin().read(buf)
    }
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        io::stdin().read_to_end(buf)
    }
    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        io::stdin().read_to_string(buf)
    }
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        io::stdin().read_exact(buf)
    }
}
impl Seek for Stdin {
    fn seek(&mut self, _pos: io::SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(io::ErrorKind::Other, "can not seek stdin"))
    }
}
impl Write for Stdin {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not write to stdin",
        ))
    }
    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not write to stdin",
        ))
    }
    fn write_all(&mut self, _buf: &[u8]) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not write to stdin",
        ))
    }
    fn write_fmt(&mut self, _fmt: ::std::fmt::Arguments) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not write to stdin",
        ))
    }
}

#[typetag::serde]
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
    fn set_len(&mut self, _new_size: __wasi_filesize_t) -> Result<(), WasiFsError> {
        debug!("Calling WasiFile::set_len on stdin; this is probably a bug");
        Err(WasiFsError::PermissionDenied)
    }

    fn unlink(&mut self) -> Result<(), WasiFsError> {
        Ok(())
    }

    fn bytes_available(&self) -> Result<usize, WasiFsError> {
        // unwrap is safe because of get_raw_fd implementation
        let host_fd = self.get_raw_fd().unwrap();

        host_file_bytes_available(host_fd)
    }

    #[cfg(unix)]
    fn get_raw_fd(&self) -> Option<i32> {
        use std::os::unix::io::AsRawFd;
        Some(io::stdin().as_raw_fd())
    }

    #[cfg(not(unix))]
    fn get_raw_fd(&self) -> Option<i32> {
        unimplemented!(
            "Stdin::get_raw_fd in WasiFile is not implemented for non-Unix-like targets yet"
        );
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
