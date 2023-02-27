// TODO: review allow..
use cfg_if::cfg_if;

/// types for use in the WASI filesystem
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};

use wasmer_wasi_types::wasi::{BusErrno, Rights};

use crate::VirtualBusError;

cfg_if! {
    if #[cfg(feature = "host-fs")] {
        pub use wasmer_vfs::host_fs::{Stderr, Stdin, Stdout};
    } else {
        pub use wasmer_vfs::mem_fs::{Stderr, Stdin, Stdout};
    }
}

pub fn vbus_error_into_bus_errno(bus_error: VirtualBusError) -> BusErrno {
    use VirtualBusError::*;
    match bus_error {
        Serialization => BusErrno::Ser,
        Deserialization => BusErrno::Des,
        InvalidWapm => BusErrno::Wapm,
        FetchFailed => BusErrno::Fetch,
        CompileError => BusErrno::Compile,
        InvalidABI => BusErrno::Abi,
        Aborted => BusErrno::Aborted,
        BadHandle => BusErrno::Badhandle,
        InvalidTopic => BusErrno::Topic,
        BadCallback => BusErrno::Badcb,
        Unsupported => BusErrno::Unsupported,
        BadRequest => BusErrno::Badrequest,
        AccessDenied => BusErrno::Denied,
        InternalError => BusErrno::Internal,
        MemoryAllocationFailed => BusErrno::Alloc,
        InvokeFailed => BusErrno::Invoke,
        AlreadyConsumed => BusErrno::Consumed,
        MemoryAccessViolation => BusErrno::Memviolation,
        UnknownError => BusErrno::Unknown,
        NotFound => BusErrno::Unknown,
    }
}

pub fn bus_errno_into_vbus_error(bus_error: BusErrno) -> VirtualBusError {
    use VirtualBusError::*;
    match bus_error {
        BusErrno::Ser => Serialization,
        BusErrno::Des => Deserialization,
        BusErrno::Wapm => InvalidWapm,
        BusErrno::Fetch => FetchFailed,
        BusErrno::Compile => CompileError,
        BusErrno::Abi => InvalidABI,
        BusErrno::Aborted => Aborted,
        BusErrno::Badhandle => BadHandle,
        BusErrno::Topic => InvalidTopic,
        BusErrno::Badcb => BadCallback,
        BusErrno::Unsupported => Unsupported,
        BusErrno::Badrequest => BadRequest,
        BusErrno::Denied => AccessDenied,
        BusErrno::Internal => InternalError,
        BusErrno::Alloc => MemoryAllocationFailed,
        BusErrno::Invoke => InvokeFailed,
        BusErrno::Consumed => AlreadyConsumed,
        BusErrno::Memviolation => MemoryAccessViolation,
        BusErrno::Unknown => UnknownError,
        BusErrno::Success => UnknownError,
    }
}

#[allow(dead_code)]
pub(crate) fn bus_read_rights() -> Rights {
    Rights::FD_FDSTAT_SET_FLAGS
        .union(Rights::FD_FILESTAT_GET)
        .union(Rights::FD_READ)
        .union(Rights::POLL_FD_READWRITE)
}

#[allow(dead_code)]
pub(crate) fn bus_write_rights() -> Rights {
    Rights::FD_FDSTAT_SET_FLAGS
        .union(Rights::FD_FILESTAT_GET)
        .union(Rights::FD_WRITE)
        .union(Rights::POLL_FD_READWRITE)
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

pub trait WasiPath {}

/*
TODO: Think about using this
trait WasiFdBacking: std::fmt::Debug {
    fn get_stat(&self) -> &Filestat;
    fn get_stat_mut(&mut self) -> &mut Filestat;
    fn is_preopened(&self) -> bool;
    fn get_name(&self) -> &str;
}
*/
