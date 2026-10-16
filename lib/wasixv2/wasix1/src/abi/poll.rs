//! `poll_oneoff` subscriptions and events.

use super::{Errno, Timestamp, Userdata, layout, newtype_layout};

/// `eventtype` (`u8`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Eventtype(pub u8);

impl Eventtype {
    pub const CLOCK: Self = Self(0);
    pub const FD_READ: Self = Self(1);
    pub const FD_WRITE: Self = Self(2);
}

/// `subclockflags` (`u16`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Subclockflags(pub u16);

impl Subclockflags {
    pub const ABSTIME: Self = Self(1 << 0);
}

/// `eventrwflags` (`u16`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Eventrwflags(pub u16);

impl Eventrwflags {
    pub const FD_READWRITE_HANGUP: Self = Self(1 << 0);
}

newtype_layout!(Eventtype(u8), Subclockflags(u16), Eventrwflags(u16));

layout! {
    /// `subscription_clock`.
    pub struct SubscriptionClock (size 32, align 8) {
        pub id: u32 = 0,
        pub timeout: Timestamp = 8,
        pub precision: Timestamp = 16,
        pub flags: Subclockflags = 24,
    }
}

layout! {
    /// `subscription_clock` of `wasi_unstable`, which carries its own identifier.
    pub struct Snapshot0SubscriptionClock (size 40, align 8) {
        pub identifier: Userdata = 0,
        pub id: u32 = 8,
        pub timeout: Timestamp = 16,
        pub precision: Timestamp = 24,
        pub flags: Subclockflags = 32,
    }
}

layout! {
    /// `subscription`: `userdata` and a union tagged by `tag`. The payload starts at 16 and is
    /// a [`SubscriptionClock`] or, for the fd types, the descriptor as a `u32`; decode it with
    /// [`Subscription::clock`] or [`Subscription::fd`].
    pub struct Subscription (size 48, align 8) {
        pub userdata: Userdata = 0,
        pub tag: Eventtype = 8,
        pub payload: [u8; 32] = 16,
    }
}

layout! {
    /// `subscription` of `wasi_unstable`; the payload is a [`Snapshot0SubscriptionClock`] or a
    /// descriptor.
    pub struct Snapshot0Subscription (size 56, align 8) {
        pub userdata: Userdata = 0,
        pub tag: Eventtype = 8,
        pub payload: [u8; 40] = 16,
    }
}

impl Subscription {
    pub fn clock(&self) -> Option<SubscriptionClock> {
        use super::GuestLayout;
        (self.tag == Eventtype::CLOCK)
            .then(|| SubscriptionClock::read_from(&self.payload))
            .flatten()
    }

    pub fn fd(&self) -> Option<u32> {
        (self.tag != Eventtype::CLOCK)
            .then(|| u32::from_le_bytes(self.payload[..4].try_into().unwrap()))
    }
}

impl Snapshot0Subscription {
    pub fn clock(&self) -> Option<Snapshot0SubscriptionClock> {
        use super::GuestLayout;
        (self.tag == Eventtype::CLOCK)
            .then(|| Snapshot0SubscriptionClock::read_from(&self.payload))
            .flatten()
    }

    pub fn fd(&self) -> Option<u32> {
        (self.tag != Eventtype::CLOCK)
            .then(|| u32::from_le_bytes(self.payload[..4].try_into().unwrap()))
    }
}

layout! {
    /// `event_fd_readwrite`.
    pub struct EventFdReadwrite (size 16, align 8) {
        pub nbytes: u64 = 0,
        pub flags: Eventrwflags = 8,
    }
}

layout! {
    /// `event`, the same in every v1 module.
    pub struct Event (size 32, align 8) {
        pub userdata: Userdata = 0,
        pub error: Errno = 8,
        pub type_: Eventtype = 10,
        pub fd_readwrite: EventFdReadwrite = 16,
    }
}
