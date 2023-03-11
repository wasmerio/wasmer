use std::mem::MaybeUninit;

use wasmer::{FromToNativeWasmType, MemorySize, ValueType};

use super::{
    Errno, ErrnoSignal, EventFdReadwrite, Eventtype, JoinStatusType, Signal,
    Snapshot0SubscriptionClock, SubscriptionClock, SubscriptionFsReadwrite, Userdata,
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

#[repr(C)]
#[derive(Clone, Copy)]
pub union JoinStatusUnion {
    pub nothing: u8,
    pub exit_normal: Errno,
    pub exit_signal: ErrnoSignal,
    pub stopped: Signal,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct JoinStatus {
    pub tag: JoinStatusType,
    pub u: JoinStatusUnion,
}
impl core::fmt::Debug for JoinStatus {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut binding = f.debug_struct("JoinStatus");
        let mut f = binding.field("tag", &self.tag);
        f = unsafe {
            match self.tag {
                JoinStatusType::Nothing => f.field("pid", &self.u.nothing),
                JoinStatusType::ExitNormal => f.field("exit_normal", &self.u.exit_normal),
                JoinStatusType::ExitSignal => f.field("exit_signal", &self.u.exit_signal),
                JoinStatusType::Stopped => f.field("stopped", &self.u.stopped),
            }
        };
        f.finish()
    }
}
unsafe impl ValueType for JoinStatus {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

#[doc = " Represents the thread start object"]
#[repr(C)]
#[derive(Copy, Clone)]
pub struct ThreadStart<M: MemorySize> {
    pub stack_start: M::Offset,
    pub tls_base: M::Offset,
    pub start_funct: M::Offset,
    pub start_args: M::Offset,
    pub reserved: [M::Offset; 10],
    pub stack_size: M::Offset,
    pub guard_size: M::Offset,
}
impl<M: MemorySize> core::fmt::Debug for ThreadStart<M> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ThreadStart")
            .field("stack_start", &self.stack_start)
            .field("tls-base", &self.tls_base)
            .field("start-funct", &self.start_funct)
            .field("start-args", &self.start_args)
            .field("stack_size", &self.stack_size)
            .field("guard_size", &self.guard_size)
            .finish()
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl<M: MemorySize> ValueType for ThreadStart<M> {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExitCode {
    Errno(Errno),
    Other(i32),
}
impl ExitCode {
    pub fn raw(&self) -> i32 {
        match self {
            ExitCode::Errno(err) => err.to_native(),
            ExitCode::Other(code) => *code,
        }
    }

    pub fn is_success(&self) -> bool {
        self.raw() == 0
    }
}
impl core::fmt::Debug for ExitCode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ExitCode::Errno(a) => write!(f, "ExitCode::{}", a),
            ExitCode::Other(a) => write!(f, "ExitCode::{}", a),
        }
    }
}
impl core::fmt::Display for ExitCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        core::fmt::Debug::fmt(&self, f)
    }
}

unsafe impl wasmer::FromToNativeWasmType for ExitCode {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self.into()
    }

    fn from_native(n: Self::Native) -> Self {
        n.into()
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        false
    }
}

impl From<Errno> for ExitCode {
    fn from(val: Errno) -> Self {
        Self::Errno(val)
    }
}

impl From<i32> for ExitCode {
    fn from(val: i32) -> Self {
        let err = Errno::from_native(val);
        match err {
            Errno::Unknown => Self::Other(val),
            err => Self::Errno(err),
        }
    }
}

impl From<ExitCode> for Errno {
    fn from(code: ExitCode) -> Self {
        match code {
            ExitCode::Errno(err) => err,
            ExitCode::Other(code) => Errno::from_native(code),
        }
    }
}

impl From<ExitCode> for i32 {
    fn from(val: ExitCode) -> Self {
        match val {
            ExitCode::Errno(err) => err.to_native(),
            ExitCode::Other(code) => code,
        }
    }
}
