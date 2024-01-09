#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
use std::mem::MaybeUninit;

use wasmer::{FromToNativeWasmType, MemorySize, ValueType};

use super::{
    Errno, ErrnoSignal, EventFdReadwrite, Eventtype, Fd, JoinStatusType, Signal,
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
                Eventtype::Unknown => SubscriptionUnion {
                    fd_readwrite: SubscriptionFsReadwrite {
                        file_descriptor: u32::MAX,
                    },
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
                JoinStatusType::Nothing => f.field("nothing", &self.u.nothing),
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
    pub stack_upper: M::Offset,
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
            .field("stack_upper", &self.stack_upper)
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
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
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

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for EpollType {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

wai_bindgen_rust::bitflags::bitflags! {
    #[doc = " Epoll available event types."]
    #[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
    pub struct EpollType : u32 {
        #[doc = " The associated file is available for read(2) operations."]
        const EPOLLIN = 1 << 0;
        #[doc = " The associated file is available for write(2) operations."]
        const EPOLLOUT = 1 << 1;
        #[doc = " Stream socket peer closed connection, or shut down writing"]
        #[doc = " half of connection.  (This flag is especially useful for"]
        #[doc = " writing simple code to detect peer shutdown when using"]
        #[doc = " edge-triggered monitoring.)"]
        const EPOLLRDHUP = 1 << 2;
        #[doc = " There is an exceptional condition on the file descriptor."]
        #[doc = " See the discussion of POLLPRI in poll(2)."]
        const EPOLLPRI = 1 << 3;
        #[doc = " Error condition happened on the associated file"]
        #[doc = " descriptor.  This event is also reported for the write end"]
        #[doc = " of a pipe when the read end has been closed."]
        const EPOLLERR = 1 << 4;
        #[doc = " Hang up happened on the associated file descriptor."]
        const EPOLLHUP = 1 << 5;
        #[doc = " Requests edge-triggered notification for the associated"]
        #[doc = " file descriptor.  The default behavior for epoll is level-"]
        #[doc = " triggered.  See epoll(7) for more detailed information"]
        #[doc = " about edge-triggered and level-triggered notification."]
        const EPOLLET = 1 << 6;
        #[doc = " Requests one-shot notification for the associated file"]
        #[doc = " descriptor.  This means that after an event notified for"]
        #[doc = " the file descriptor by epoll_wait(2), the file descriptor"]
        #[doc = " is disabled in the interest list and no other events will"]
        #[doc = " be reported by the epoll interface.  The user must call"]
        #[doc = " epoll_ctl() with EPOLL_CTL_MOD to rearm the file"]
        #[doc = " descriptor with a new event mask."]
        const EPOLLONESHOT = 1 << 7;
    }
}

#[doc = " Epoll operation."]
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, num_enum :: TryFromPrimitive, Hash)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub enum EpollCtl {
    #[doc = " Add an entry to the interest list of the epoll file descriptor, epfd."]
    Add,
    #[doc = " Change the settings associated with fd in the interest list to the new settings specified in event."]
    Mod,
    #[doc = " Remove (deregister) the target file descriptor fd from the interest list."]
    Del,
    #[doc = " Unknown."]
    Unknown,
}
impl core::fmt::Debug for EpollCtl {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            EpollCtl::Add => f.debug_tuple("EPOLL_CTL_ADD").finish(),
            EpollCtl::Mod => f.debug_tuple("EPOLL_CTL_MOD").finish(),
            EpollCtl::Del => f.debug_tuple("EPOLL_CTL_DEL").finish(),
            EpollCtl::Unknown => f.debug_tuple("Unknown").finish(),
        }
    }
}
// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for EpollCtl {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

unsafe impl wasmer::FromToNativeWasmType for EpollCtl {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }

    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Add,
            1 => Self::Mod,
            2 => Self::Del,

            q => {
                tracing::debug!("could not serialize number {q} to enum EpollCtl");
                Self::Unknown
            }
        }
    }

    fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct EpollEventCtl {
    pub events: EpollType,
    pub ptr: u64,
    pub fd: Fd,
    pub data1: u32,
    pub data2: u64,
}

/// An event that can be triggered
#[repr(C)]
#[derive(Copy, Clone)]
pub struct EpollData<M: MemorySize> {
    /// Pointer to the data
    pub ptr: M::Offset,
    /// File descriptor
    pub fd: Fd,
    /// Associated user data
    pub data1: u32,
    /// Associated user data
    pub data2: u64,
}
impl<M> core::fmt::Debug for EpollData<M>
where
    M: MemorySize,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EpollData")
            .field("ptr", &self.ptr)
            .field("fd", &self.fd)
            .field("data1", &self.data1)
            .field("data2", &self.data2)
            .finish()
    }
}

/// An event that can be triggered
#[repr(C)]
#[derive(Copy, Clone)]
pub struct EpollEvent<M: MemorySize> {
    /// Pointer to the data
    pub events: EpollType,
    /// File descriptor
    pub data: EpollData<M>,
}
impl<M> core::fmt::Debug for EpollEvent<M>
where
    M: MemorySize,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EpollData")
            .field("events", &self.events)
            .field("data", &self.data)
            .finish()
    }
}
unsafe impl<M> ValueType for EpollEvent<M>
where
    M: MemorySize,
{
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}
