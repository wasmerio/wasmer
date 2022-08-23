/* TODO: if required, move to generated wasi::Event type
use crate::*;
use std::{
    fmt,
    mem::{self, MaybeUninit},
};
use wasmer_derive::ValueType;
use wasmer_types::ValueType;
use wasmer_wasi_types_generated::wasi;

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_event_fd_readwrite_t {
    pub nbytes: __wasi_filesize_t,
    pub flags: wasi::Eventrwflags,
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
        flags: wasi::Eventrwflags,
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

impl __wasi_event_t {
    pub fn tagged(&self) -> Option<EventEnum> {
        match self.type_ {
            wasi::Eventtype::FdRead | wasi::Eventtype::FdWrite => Some(EventEnum::FdReadWrite {
                nbytes: unsafe { self.u.fd_readwrite.nbytes },
                flags: unsafe { self.u.fd_readwrite.flags },
            }),
            _ => None,
        }
    }
}

unsafe impl ValueType for __wasi_event_t {
    fn zero_padding_bytes(&self, bytes: &mut [MaybeUninit<u8>]) {
        macro_rules! field {
            ($($f:tt)*) => {
                &self.$($f)* as *const _ as usize - self as *const _ as usize
            };
        }
        macro_rules! field_end {
            ($($f:tt)*) => {
                field!($($f)*) + mem::size_of_val(&self.$($f)*)
            };
        }
        macro_rules! zero {
            ($start:expr, $end:expr) => {
                for i in $start..$end {
                    bytes[i] = MaybeUninit::new(0);
                }
            };
        }
        self.userdata
            .zero_padding_bytes(&mut bytes[field!(userdata)..field_end!(userdata)]);
        zero!(field_end!(userdata), field!(error));
        self.error
            .zero_padding_bytes(&mut bytes[field!(error)..field_end!(error)]);
        zero!(field_end!(error), field!(type_));
        self.type_
            .zero_padding_bytes(&mut bytes[field!(type_)..field_end!(type_)]);
        zero!(field_end!(type_), field!(u));
        match self.type_ {
            wasi::Eventtype::FdRead | wasi::Eventtype::FdWrite => unsafe {
                self.u.fd_readwrite.zero_padding_bytes(
                    &mut bytes[field!(u.fd_readwrite)..field_end!(u.fd_readwrite)],
                );
                zero!(field_end!(u.fd_readwrite), field_end!(u));
            },
            _ => zero!(field!(u), field_end!(u)),
        }
        zero!(field_end!(u), mem::size_of_val(self));
    }
}
*/
