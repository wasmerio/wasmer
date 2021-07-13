use crate::*;
use std::fmt;
use wasmer_types::ValueType;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct __wasi_event_fd_readwrite_t {
    pub nbytes: __wasi_filesize_t,
    pub flags: __wasi_eventrwflags_t,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub union __wasi_event_u {
    pub fd_readwrite: __wasi_event_fd_readwrite_t,
}

// TODO: remove this implementation of Debug when `__wasi_event_u` gets more than 1 variant
impl fmt::Debug for __wasi_event_u {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("__wasi_event_u")
            .field("fd_readwrite", unsafe { &self.fd_readwrite })
            .finish()
    }
}

#[derive(Debug, Copy, Clone)]
pub enum EventEnum {
    FdReadWrite {
        nbytes: __wasi_filesize_t,
        flags: __wasi_eventrwflags_t,
    },
}

impl EventEnum {
    pub fn untagged(self) -> __wasi_event_u {
        match self {
            EventEnum::FdReadWrite { nbytes, flags } => __wasi_event_u {
                fd_readwrite: __wasi_event_fd_readwrite_t { nbytes, flags },
            },
        }
    }
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct __wasi_event_t {
    pub userdata: __wasi_userdata_t,
    pub error: __wasi_errno_t,
    pub type_: __wasi_eventtype_t,
    pub u: __wasi_event_u,
}

impl __wasi_event_t {
    pub fn tagged(&self) -> Option<EventEnum> {
        match self.type_ {
            __WASI_EVENTTYPE_FD_READ | __WASI_EVENTTYPE_FD_WRITE => Some(EventEnum::FdReadWrite {
                nbytes: unsafe { self.u.fd_readwrite.nbytes },
                flags: unsafe { self.u.fd_readwrite.flags },
            }),
            _ => None,
        }
    }
}

unsafe impl ValueType for __wasi_event_t {}

pub type __wasi_eventrwflags_t = u16;
pub const __WASI_EVENT_FD_READWRITE_HANGUP: u16 = 1 << 0;

pub type __wasi_eventtype_t = u8;
pub const __WASI_EVENTTYPE_CLOCK: u8 = 0;
pub const __WASI_EVENTTYPE_FD_READ: u8 = 1;
pub const __WASI_EVENTTYPE_FD_WRITE: u8 = 2;

pub fn eventtype_to_str(event_type: __wasi_eventtype_t) -> &'static str {
    match event_type {
        __WASI_EVENTTYPE_CLOCK => "__WASI_EVENTTYPE_CLOCK",
        __WASI_EVENTTYPE_FD_READ => "__WASI_EVENTTYPE_FD_READ",
        __WASI_EVENTTYPE_FD_WRITE => "__WASI_EVENTTYPE_FD_WRITE",
        _ => "INVALID EVENTTYPE",
    }
}
