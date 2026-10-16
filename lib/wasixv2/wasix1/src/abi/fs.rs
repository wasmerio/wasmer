//! Files, directories and descriptors.

use super::{Timestamp, layout, newtype_layout};

/// `filetype` (`u8`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Filetype(pub u8);

impl Filetype {
    pub const UNKNOWN: Self = Self(0);
    pub const BLOCK_DEVICE: Self = Self(1);
    pub const CHARACTER_DEVICE: Self = Self(2);
    pub const DIRECTORY: Self = Self(3);
    pub const REGULAR_FILE: Self = Self(4);
    pub const SOCKET_DGRAM: Self = Self(5);
    pub const SOCKET_STREAM: Self = Self(6);
    pub const SYMBOLIC_LINK: Self = Self(7);
    /// WASIX additions.
    pub const SOCKET_RAW: Self = Self(8);
    pub const SOCKET_SEQPACKET: Self = Self(9);
}

/// `fdflags` (`u16`): file status flags, which Linux keeps per open file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fdflags(pub u16);

impl Fdflags {
    pub const APPEND: Self = Self(1 << 0);
    pub const DSYNC: Self = Self(1 << 1);
    pub const NONBLOCK: Self = Self(1 << 2);
    pub const RSYNC: Self = Self(1 << 3);
    pub const SYNC: Self = Self(1 << 4);
}

/// `fdflagsext` (`u16`, WASIX): descriptor flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fdflagsext(pub u16);

impl Fdflagsext {
    pub const CLOEXEC: Self = Self(1 << 0);
}

/// `oflags` (`u16`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Oflags(pub u16);

impl Oflags {
    pub const CREAT: Self = Self(1 << 0);
    pub const DIRECTORY: Self = Self(1 << 1);
    pub const EXCL: Self = Self(1 << 2);
    pub const TRUNC: Self = Self(1 << 3);
}

/// `lookupflags` (`u32`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lookupflags(pub u32);

impl Lookupflags {
    pub const SYMLINK_FOLLOW: Self = Self(1 << 0);
}

/// `fstflags` (`u16`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fstflags(pub u16);

impl Fstflags {
    pub const SET_ATIM: Self = Self(1 << 0);
    pub const SET_ATIM_NOW: Self = Self(1 << 1);
    pub const SET_MTIM: Self = Self(1 << 2);
    pub const SET_MTIM_NOW: Self = Self(1 << 3);
}

/// `whence` (`u8`). `wasi_unstable` numbers it differently: see [`Snapshot0Whence`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Whence(pub u8);

impl Whence {
    pub const SET: Self = Self(0);
    pub const CUR: Self = Self(1);
    pub const END: Self = Self(2);
}

/// `whence` of `wasi_unstable`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot0Whence(pub u8);

impl Snapshot0Whence {
    pub const CUR: Self = Self(0);
    pub const END: Self = Self(1);
    pub const SET: Self = Self(2);
}

/// `advice` (`u8`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Advice(pub u8);

impl Advice {
    pub const NORMAL: Self = Self(0);
    pub const SEQUENTIAL: Self = Self(1);
    pub const RANDOM: Self = Self(2);
    pub const WILLNEED: Self = Self(3);
    pub const DONTNEED: Self = Self(4);
    pub const NOREUSE: Self = Self(5);
}

/// `preopentype` (`u8`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preopentype(pub u8);

impl Preopentype {
    pub const DIR: Self = Self(0);
}

/// `rights` (`u64`). Reported, never enforced (see DESIGN.md, "WASIX v1 compatibility").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rights(pub u64);

impl Rights {
    pub const FD_DATASYNC: Self = Self(1 << 0);
    pub const FD_READ: Self = Self(1 << 1);
    pub const FD_SEEK: Self = Self(1 << 2);
    pub const FD_FDSTAT_SET_FLAGS: Self = Self(1 << 3);
    pub const FD_SYNC: Self = Self(1 << 4);
    pub const FD_TELL: Self = Self(1 << 5);
    pub const FD_WRITE: Self = Self(1 << 6);
    pub const FD_ADVISE: Self = Self(1 << 7);
    pub const FD_ALLOCATE: Self = Self(1 << 8);
    pub const PATH_CREATE_DIRECTORY: Self = Self(1 << 9);
    pub const PATH_CREATE_FILE: Self = Self(1 << 10);
    pub const PATH_LINK_SOURCE: Self = Self(1 << 11);
    pub const PATH_LINK_TARGET: Self = Self(1 << 12);
    pub const PATH_OPEN: Self = Self(1 << 13);
    pub const FD_READDIR: Self = Self(1 << 14);
    pub const PATH_READLINK: Self = Self(1 << 15);
    pub const PATH_RENAME_SOURCE: Self = Self(1 << 16);
    pub const PATH_RENAME_TARGET: Self = Self(1 << 17);
    pub const PATH_FILESTAT_GET: Self = Self(1 << 18);
    pub const PATH_FILESTAT_SET_SIZE: Self = Self(1 << 19);
    pub const PATH_FILESTAT_SET_TIMES: Self = Self(1 << 20);
    pub const FD_FILESTAT_GET: Self = Self(1 << 21);
    pub const FD_FILESTAT_SET_SIZE: Self = Self(1 << 22);
    pub const FD_FILESTAT_SET_TIMES: Self = Self(1 << 23);
    pub const PATH_SYMLINK: Self = Self(1 << 24);
    pub const PATH_REMOVE_DIRECTORY: Self = Self(1 << 25);
    pub const PATH_UNLINK_FILE: Self = Self(1 << 26);
    pub const POLL_FD_READWRITE: Self = Self(1 << 27);
    pub const SOCK_SHUTDOWN: Self = Self(1 << 28);
    pub const SOCK_ACCEPT: Self = Self(1 << 29);
    pub const SOCK_CONNECT: Self = Self(1 << 30);
    pub const SOCK_LISTEN: Self = Self(1 << 31);
    pub const SOCK_BIND: Self = Self(1 << 32);
    pub const SOCK_RECV: Self = Self(1 << 33);
    pub const SOCK_SEND: Self = Self(1 << 34);
    pub const SOCK_ADDR_LOCAL: Self = Self(1 << 35);
    pub const SOCK_ADDR_REMOTE: Self = Self(1 << 36);
    pub const SOCK_RECV_FROM: Self = Self(1 << 37);
    pub const SOCK_SEND_TO: Self = Self(1 << 38);
    /// Every right.
    pub const ALL: Self = Self((1 << 39) - 1);
}

/// Bit operations for the v1 flag sets.
macro_rules! flag_ops {
    ($($name:ident),* $(,)?) => {$(
        impl $name {
            pub const fn empty() -> Self {
                Self(0)
            }

            /// Whether every bit of `other` is set.
            pub const fn contains(self, other: Self) -> bool {
                self.0 & other.0 == other.0
            }

            /// Whether any bit of `other` is set.
            pub const fn intersects(self, other: Self) -> bool {
                self.0 & other.0 != 0
            }
        }

        impl core::ops::BitOr for $name {
            type Output = Self;
            fn bitor(self, other: Self) -> Self {
                Self(self.0 | other.0)
            }
        }

        impl core::ops::BitOrAssign for $name {
            fn bitor_assign(&mut self, other: Self) {
                self.0 |= other.0;
            }
        }

        impl core::ops::BitAnd for $name {
            type Output = Self;
            fn bitand(self, other: Self) -> Self {
                Self(self.0 & other.0)
            }
        }

        impl core::ops::Not for $name {
            type Output = Self;
            fn not(self) -> Self {
                Self(!self.0)
            }
        }
    )*};
}

flag_ops!(Fdflags, Fdflagsext, Oflags, Lookupflags, Fstflags, Rights);

impl Fdflags {
    pub const ALL: Self = Self(0x1f);
}

impl Oflags {
    pub const ALL: Self = Self(0xf);
}

impl Fstflags {
    pub const ALL: Self = Self(0xf);
}

impl Rights {
    /// What a directory descriptor may do to the names below it.
    pub const PATH_ALL: Self = Self(
        Self::PATH_CREATE_DIRECTORY.0
            | Self::PATH_CREATE_FILE.0
            | Self::PATH_LINK_SOURCE.0
            | Self::PATH_LINK_TARGET.0
            | Self::PATH_OPEN.0
            | Self::PATH_READLINK.0
            | Self::PATH_RENAME_SOURCE.0
            | Self::PATH_RENAME_TARGET.0
            | Self::PATH_FILESTAT_GET.0
            | Self::PATH_FILESTAT_SET_SIZE.0
            | Self::PATH_FILESTAT_SET_TIMES.0
            | Self::PATH_SYMLINK.0
            | Self::PATH_REMOVE_DIRECTORY.0
            | Self::PATH_UNLINK_FILE.0,
    );
    /// Every socket operation.
    pub const SOCK_ALL: Self = Self(
        Self::SOCK_SHUTDOWN.0
            | Self::SOCK_ACCEPT.0
            | Self::SOCK_CONNECT.0
            | Self::SOCK_LISTEN.0
            | Self::SOCK_BIND.0
            | Self::SOCK_RECV.0
            | Self::SOCK_SEND.0
            | Self::SOCK_ADDR_LOCAL.0
            | Self::SOCK_ADDR_REMOTE.0
            | Self::SOCK_RECV_FROM.0
            | Self::SOCK_SEND_TO.0,
    );
}

newtype_layout!(
    Filetype(u8),
    Fdflags(u16),
    Fdflagsext(u16),
    Oflags(u16),
    Lookupflags(u32),
    Fstflags(u16),
    Whence(u8),
    Snapshot0Whence(u8),
    Advice(u8),
    Preopentype(u8),
    Rights(u64),
);

layout! {
    /// `fdstat`.
    pub struct Fdstat (size 24, align 8) {
        pub fs_filetype: Filetype = 0,
        pub fs_flags: Fdflags = 2,
        pub fs_rights_base: Rights = 8,
        pub fs_rights_inheriting: Rights = 16,
    }
}

layout! {
    /// `filestat` of `wasi_snapshot_preview1` and `wasix_32v1`.
    pub struct Filestat (size 64, align 8) {
        pub st_dev: u64 = 0,
        pub st_ino: u64 = 8,
        pub st_filetype: Filetype = 16,
        pub st_nlink: u64 = 24,
        pub st_size: u64 = 32,
        pub st_atim: Timestamp = 40,
        pub st_mtim: Timestamp = 48,
        pub st_ctim: Timestamp = 56,
    }
}

layout! {
    /// `filestat` of `wasi_unstable`: a 32-bit link count.
    pub struct Snapshot0Filestat (size 56, align 8) {
        pub st_dev: u64 = 0,
        pub st_ino: u64 = 8,
        pub st_filetype: Filetype = 16,
        pub st_nlink: u32 = 20,
        pub st_size: u64 = 24,
        pub st_atim: Timestamp = 32,
        pub st_mtim: Timestamp = 40,
        pub st_ctim: Timestamp = 48,
    }
}

layout! {
    /// `prestat`: a tagged union whose only variant is a directory with its name's length.
    pub struct Prestat (size 8, align 4) {
        pub pr_type: Preopentype = 0,
        pub pr_name_len: u32 = 4,
    }
}

layout! {
    /// `dirent`, followed in the buffer by `d_namlen` bytes of name. The same layout serves
    /// `wasi_unstable`.
    pub struct Dirent (size 24, align 8) {
        /// The cookie of the next entry.
        pub d_next: u64 = 0,
        pub d_ino: u64 = 8,
        pub d_namlen: u32 = 16,
        pub d_type: Filetype = 20,
    }
}
