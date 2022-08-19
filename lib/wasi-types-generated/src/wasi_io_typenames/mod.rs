mod bindings;
pub use bindings::wasi_io_typenames::*;

use std::mem::MaybeUninit;
use wasmer_types::ValueType;

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Eventtype {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Subclockflags {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Clockid {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

impl Eventtype {
    pub fn to_str(self) -> &'static str {
        match self {
            Eventtype::Clock => "Wasi::Eventtype::Clock",
            Eventtype::FdRead => "Wasi::Eventtype::FdRead",
            Eventtype::FdWrite => "Wasi::Eventtype::FdWrite",
        }
    }
}

/// Workaround implementation because `wit-bindgen` does not generate
/// type aliases, but instead creates the same filetype in each module
/// for `use` statements in the `.wit` file.
impl From<super::wasi_snapshot0::Eventtype> for Eventtype {
    fn from(other: super::wasi_snapshot0::Eventtype) -> Self {
        match other {
            super::wasi_snapshot0::Eventtype::Clock => Eventtype::Clock,
            super::wasi_snapshot0::Eventtype::FdRead => Eventtype::FdRead,
            super::wasi_snapshot0::Eventtype::FdWrite => Eventtype::FdWrite,
        }
    }
}

/// Workaround implementation because `wit-bindgen` does not generate
/// type aliases, but instead creates the same filetype in each module
/// for `use` statements in the `.wit` file.
impl From<super::wasi_snapshot0::Clockid> for Clockid {
    fn from(other: super::wasi_snapshot0::Clockid) -> Self {
        match other {
            super::wasi_snapshot0::Clockid::Realtime => Clockid::Realtime,
            super::wasi_snapshot0::Clockid::Monotonic => Clockid::Monotonic,
            // TODO: no idea what we should be doing here in the end
            _ => panic!("unsupported clock wasi_snapshot0 clockid"),
        }
    }
}

/// Workaround implementation because `wit-bindgen` does not generate
/// type aliases, but instead creates the same filetype in each module
/// for `use` statements in the `.wit` file.
impl From<super::wasi_snapshot0::Subclockflags> for Subclockflags {
    fn from(other: super::wasi_snapshot0::Subclockflags) -> Self {
        Subclockflags::from_bits_truncate(other.bits())
    }
}

/// Workaround implementation because `wit-bindgen` does not generate
/// type aliases, but instead creates the same filetype in each module
/// for `use` statements in the `.wit` file.
impl From<super::wasi_snapshot0::Errno> for Errno {
    fn from(other: super::wasi_snapshot0::Errno) -> Self {
        match other {
            super::wasi_snapshot0::Errno::Success => Errno::Success,
            super::wasi_snapshot0::Errno::Toobig => Errno::Toobig,
            super::wasi_snapshot0::Errno::Acces => Errno::Access,
            super::wasi_snapshot0::Errno::Addrinuse => Errno::Addrinuse,
            super::wasi_snapshot0::Errno::Addrnotavail => Errno::Addrnotavail,
            super::wasi_snapshot0::Errno::Afnosupport => Errno::Afnosupport,
            super::wasi_snapshot0::Errno::Again => Errno::Again,
            super::wasi_snapshot0::Errno::Already => Errno::Already,
            super::wasi_snapshot0::Errno::Badf => Errno::Badf,
            super::wasi_snapshot0::Errno::Badmsg => Errno::Badmsg,
            super::wasi_snapshot0::Errno::Busy => Errno::Busy,
            super::wasi_snapshot0::Errno::Canceled => Errno::Canceled,
            super::wasi_snapshot0::Errno::Child => Errno::Child,
            super::wasi_snapshot0::Errno::Connaborted => Errno::Connaborted,
            super::wasi_snapshot0::Errno::Connrefused => Errno::Connrefused,
            super::wasi_snapshot0::Errno::Connreset => Errno::Connreset,
            super::wasi_snapshot0::Errno::Deadlk => Errno::Deadlk,
            super::wasi_snapshot0::Errno::Destaddrreq => Errno::Destaddrreq,
            super::wasi_snapshot0::Errno::Dom => Errno::Dom,
            super::wasi_snapshot0::Errno::Dquot => Errno::Dquot,
            super::wasi_snapshot0::Errno::Exist => Errno::Exist,
            super::wasi_snapshot0::Errno::Fault => Errno::Fault,
            super::wasi_snapshot0::Errno::Fbig => Errno::Fbig,
            super::wasi_snapshot0::Errno::Hostunreach => Errno::Hostunreach,
            super::wasi_snapshot0::Errno::Idrm => Errno::Idrm,
            super::wasi_snapshot0::Errno::Ilseq => Errno::Ilseq,
            super::wasi_snapshot0::Errno::Inprogress => Errno::Inprogress,
            super::wasi_snapshot0::Errno::Intr => Errno::Intr,
            super::wasi_snapshot0::Errno::Inval => Errno::Inval,
            super::wasi_snapshot0::Errno::Io => Errno::Io,
            super::wasi_snapshot0::Errno::Isconn => Errno::Isconn,
            super::wasi_snapshot0::Errno::Isdir => Errno::Isdir,
            super::wasi_snapshot0::Errno::Loop => Errno::Loop,
            super::wasi_snapshot0::Errno::Mfile => Errno::Mfile,
            super::wasi_snapshot0::Errno::Mlink => Errno::Mlink,
            super::wasi_snapshot0::Errno::Msgsize => Errno::Msgsize,
            super::wasi_snapshot0::Errno::Multihop => Errno::Multihop,
            super::wasi_snapshot0::Errno::Nametoolong => Errno::Nametoolong,
            super::wasi_snapshot0::Errno::Netdown => Errno::Netdown,
            super::wasi_snapshot0::Errno::Netreset => Errno::Netreset,
            super::wasi_snapshot0::Errno::Netunreach => Errno::Netunreach,
            super::wasi_snapshot0::Errno::Nfile => Errno::Nfile,
            super::wasi_snapshot0::Errno::Nobufs => Errno::Nobufs,
            super::wasi_snapshot0::Errno::Nodev => Errno::Nodev,
            super::wasi_snapshot0::Errno::Noent => Errno::Noent,
            super::wasi_snapshot0::Errno::Noexec => Errno::Noexec,
            super::wasi_snapshot0::Errno::Nolck => Errno::Nolck,
            super::wasi_snapshot0::Errno::Nolink => Errno::Nolink,
            super::wasi_snapshot0::Errno::Nomem => Errno::Nomem,
            super::wasi_snapshot0::Errno::Nomsg => Errno::Nomsg,
            super::wasi_snapshot0::Errno::Noprotoopt => Errno::Noprotoopt,
            super::wasi_snapshot0::Errno::Nospc => Errno::Nospc,
            super::wasi_snapshot0::Errno::Nosys => Errno::Nosys,
            super::wasi_snapshot0::Errno::Notconn => Errno::Notconn,
            super::wasi_snapshot0::Errno::Notdir => Errno::Notdir,
            super::wasi_snapshot0::Errno::Notempty => Errno::Notempty,
            super::wasi_snapshot0::Errno::Notrecoverable => Errno::Notrecoverable,
            super::wasi_snapshot0::Errno::Notsock => Errno::Notsock,
            super::wasi_snapshot0::Errno::Notsup => Errno::Notsup,
            super::wasi_snapshot0::Errno::Notty => Errno::Notty,
            super::wasi_snapshot0::Errno::Nxio => Errno::Nxio,
            super::wasi_snapshot0::Errno::Overflow => Errno::Overflow,
            super::wasi_snapshot0::Errno::Ownerdead => Errno::Ownerdead,
            super::wasi_snapshot0::Errno::Perm => Errno::Perm,
            super::wasi_snapshot0::Errno::Pipe => Errno::Pipe,
            super::wasi_snapshot0::Errno::Proto => Errno::Proto,
            super::wasi_snapshot0::Errno::Protonosupport => Errno::Protonosupport,
            super::wasi_snapshot0::Errno::Prototype => Errno::Prototype,
            super::wasi_snapshot0::Errno::Range => Errno::Range,
            super::wasi_snapshot0::Errno::Rofs => Errno::Rofs,
            super::wasi_snapshot0::Errno::Spipe => Errno::Spipe,
            super::wasi_snapshot0::Errno::Srch => Errno::Srch,
            super::wasi_snapshot0::Errno::Stale => Errno::Stale,
            super::wasi_snapshot0::Errno::Timedout => Errno::Timedout,
            super::wasi_snapshot0::Errno::Txtbsy => Errno::Txtbsy,
            super::wasi_snapshot0::Errno::Xdev => Errno::Xdev,
            super::wasi_snapshot0::Errno::Notcapable => Errno::Notcapable,
        }
    }
}
