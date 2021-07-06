use crate::*;
use std::convert::TryFrom;
use std::fmt;
use wasmer_types::ValueType;

pub type __wasi_subclockflags_t = u16;
pub const __WASI_SUBSCRIPTION_CLOCK_ABSTIME: u16 = 1 << 0;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct __wasi_subscription_clock_t {
    pub clock_id: __wasi_clockid_t,
    pub timeout: __wasi_timestamp_t,
    pub precision: __wasi_timestamp_t,
    pub flags: __wasi_subclockflags_t,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct __wasi_subscription_fs_readwrite_t {
    pub fd: __wasi_fd_t,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub union __wasi_subscription_u {
    pub clock: __wasi_subscription_clock_t,
    pub fd_readwrite: __wasi_subscription_fs_readwrite_t,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct __wasi_subscription_t {
    pub userdata: __wasi_userdata_t,
    pub type_: __wasi_eventtype_t,
    pub u: __wasi_subscription_u,
}

/// Safe Rust wrapper around `__wasi_subscription_t::type_` and `__wasi_subscription_t::u`
#[derive(Debug, Clone)]
pub enum EventType {
    Clock(__wasi_subscription_clock_t),
    Read(__wasi_subscription_fs_readwrite_t),
    Write(__wasi_subscription_fs_readwrite_t),
}

impl EventType {
    pub fn raw_tag(&self) -> __wasi_eventtype_t {
        match self {
            EventType::Clock(_) => __WASI_EVENTTYPE_CLOCK,
            EventType::Read(_) => __WASI_EVENTTYPE_FD_READ,
            EventType::Write(_) => __WASI_EVENTTYPE_FD_WRITE,
        }
    }
}

/// Safe Rust wrapper around `__wasi_subscription_t`
#[derive(Debug, Clone)]
pub struct WasiSubscription {
    pub user_data: __wasi_userdata_t,
    pub event_type: EventType,
}

impl TryFrom<__wasi_subscription_t> for WasiSubscription {
    type Error = __wasi_errno_t;

    fn try_from(ws: __wasi_subscription_t) -> Result<Self, Self::Error> {
        Ok(Self {
            user_data: ws.userdata,
            event_type: match ws.type_ {
                __WASI_EVENTTYPE_CLOCK => EventType::Clock(unsafe { ws.u.clock }),
                __WASI_EVENTTYPE_FD_READ => EventType::Read(unsafe { ws.u.fd_readwrite }),
                __WASI_EVENTTYPE_FD_WRITE => EventType::Write(unsafe { ws.u.fd_readwrite }),
                _ => return Err(__WASI_EINVAL),
            },
        })
    }
}

impl TryFrom<WasiSubscription> for __wasi_subscription_t {
    type Error = __wasi_errno_t;

    fn try_from(ws: WasiSubscription) -> Result<Self, Self::Error> {
        #[allow(unreachable_patterns)]
        let (type_, u) = match ws.event_type {
            EventType::Clock(c) => (__WASI_EVENTTYPE_CLOCK, __wasi_subscription_u { clock: c }),
            EventType::Read(rw) => (
                __WASI_EVENTTYPE_FD_READ,
                __wasi_subscription_u { fd_readwrite: rw },
            ),
            EventType::Write(rw) => (
                __WASI_EVENTTYPE_FD_WRITE,
                __wasi_subscription_u { fd_readwrite: rw },
            ),
            _ => return Err(__WASI_EINVAL),
        };

        Ok(Self {
            userdata: ws.user_data,
            type_,
            u,
        })
    }
}

impl fmt::Debug for __wasi_subscription_t {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("__wasi_subscription_t")
            .field("userdata", &self.userdata)
            .field("type", &eventtype_to_str(self.type_))
            .field(
                "u",
                match self.type_ {
                    __WASI_EVENTTYPE_CLOCK => unsafe { &self.u.clock },
                    __WASI_EVENTTYPE_FD_READ | __WASI_EVENTTYPE_FD_WRITE => unsafe {
                        &self.u.fd_readwrite
                    },
                    _ => &"INVALID EVENTTYPE",
                },
            )
            .finish()
    }
}

unsafe impl ValueType for __wasi_subscription_t {}

pub enum SubscriptionEnum {
    Clock(__wasi_subscription_clock_t),
    FdReadWrite(__wasi_subscription_fs_readwrite_t),
}

impl __wasi_subscription_t {
    pub fn tagged(&self) -> Option<SubscriptionEnum> {
        match self.type_ {
            __WASI_EVENTTYPE_CLOCK => Some(SubscriptionEnum::Clock(unsafe { self.u.clock })),
            __WASI_EVENTTYPE_FD_READ | __WASI_EVENTTYPE_FD_WRITE => {
                Some(SubscriptionEnum::FdReadWrite(unsafe {
                    self.u.fd_readwrite
                }))
            }
            _ => None,
        }
    }
}
