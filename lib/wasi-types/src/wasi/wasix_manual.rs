use std::mem::MaybeUninit;

use wasmer::ValueType;

use super::{
    Errno, EventFdReadwrite, Eventtype, Snapshot0SubscriptionClock, SubscriptionClock,
    SubscriptionFsReadwrite, Userdata,
};

/// Thread local key
pub type TlKey = u32;
/// Thread local value
pub type TlVal = u64;
/// Thread local user data (associated with the value)
pub type TlUser = u64;
/// Long size used by checkpoints
pub type Longsize = u64;

/// The contents of a `subscription`, snapshot0 version.
#[repr(C)]
#[derive(Clone, Copy)]
pub union Snapshot0SubscriptionUnion {
    pub clock: Snapshot0SubscriptionClock,
    pub fd_readwrite: SubscriptionFsReadwrite,
}
/// The contents of a `subscription`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union SubscriptionUnion {
    pub clock: SubscriptionClock,
    pub fd_readwrite: SubscriptionFsReadwrite,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Snapshot0Subscription {
    pub userdata: Userdata,
    pub type_: Eventtype,
    pub u: Snapshot0SubscriptionUnion,
}
impl core::fmt::Debug for Snapshot0Subscription {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Snapshot0Subscription")
            .field("userdata", &self.userdata)
            .field("type", &self.type_)
            .finish()
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Subscription {
    pub userdata: Userdata,
    pub type_: Eventtype,
    pub data: SubscriptionUnion,
}
impl core::fmt::Debug for Subscription {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Subscription")
            .field("userdata", &self.userdata)
            .field("type", &self.type_)
            .finish()
    }
}

impl From<Snapshot0Subscription> for Subscription {
    fn from(other: Snapshot0Subscription) -> Self {
        Self {
            userdata: other.userdata,
            type_: other.type_,
            data: match other.type_ {
                Eventtype::Clock => SubscriptionUnion {
                    clock: unsafe {
                        SubscriptionClock {
                            clock_id: other.u.clock.id.into(),
                            timeout: other.u.clock.timeout,
                            precision: other.u.clock.precision,
                            flags: other.u.clock.flags,
                        }
                    },
                },
                Eventtype::FdRead => SubscriptionUnion {
                    fd_readwrite: unsafe { other.u.fd_readwrite },
                },
                Eventtype::FdWrite => SubscriptionUnion {
                    fd_readwrite: unsafe { other.u.fd_readwrite },
                },
            },
        }
    }
}

/// The contents of an `event`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union EventUnion {
    pub clock: u8,
    pub fd_readwrite: EventFdReadwrite,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(C)]
pub struct StackSnapshot {
    pub user: u64,
    pub hash: u128,
}

/// An event that occurred.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Event {
    /// User-provided value that got attached to `subscription::userdata`.
    pub userdata: Userdata,
    /// If non-zero, an error that occurred while processing the subscription request.
    pub error: Errno,
    /// Type of event that was triggered
    pub type_: Eventtype,
    /// The type of the event that occurred, and the contents of the event
    pub u: EventUnion,
}
impl core::fmt::Debug for Event {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Event")
            .field("userdata", &self.userdata)
            .field("error", &self.error)
            .field("type", &self.type_)
            .finish()
    }
}
/// An event that occurred.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Snapshot0Event {
    /// User-provided value that got attached to `subscription::userdata`.
    pub userdata: Userdata,
    /// If non-zero, an error that occurred while processing the subscription request.
    pub error: Errno,
    /// The type of event that occured
    pub type_: Eventtype,
    /// The contents of the event, if it is an `eventtype::fd_read` or
    /// `eventtype::fd_write`. `eventtype::clock` events ignore this field.
    pub fd_readwrite: EventFdReadwrite,
}
impl core::fmt::Debug for Snapshot0Event {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Snapshot0Event")
            .field("userdata", &self.userdata)
            .field("error", &self.error)
            .field("type", &self.type_)
            .field("fd-readwrite", &self.fd_readwrite)
            .finish()
    }
}

unsafe impl ValueType for Snapshot0Subscription {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

unsafe impl ValueType for Snapshot0Event {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

unsafe impl ValueType for Subscription {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

unsafe impl ValueType for Event {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

unsafe impl ValueType for StackSnapshot {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}
