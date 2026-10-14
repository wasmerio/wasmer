//! Guest-visible ABI: constants, errno values, flag bits and struct layouts for wasm32.
//!
//! Values and layouts match Linux's generic architecture so libc and applications port unchanged.

/// A Linux errno value. Syscalls hand it to the guest negated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Errno(pub i32);

impl Errno {
    pub const EINTR: Self = Self(4);
    pub const EIO: Self = Self(5);
    pub const EBADF: Self = Self(9);
    pub const EAGAIN: Self = Self(11);
    pub const EFAULT: Self = Self(14);
    pub const EINVAL: Self = Self(22);
    pub const EPIPE: Self = Self(32);
    pub const ENOSYS: Self = Self(38);
}

/// A file descriptor number as the guest sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Fd(pub i32);
