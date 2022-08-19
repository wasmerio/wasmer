mod bindings;
pub use bindings::wasi_snapshot0::*;

use std::mem::MaybeUninit;
use wasmer_types::ValueType;

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Errno {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Filetype {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Eventtype {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Rights {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Eventrwflags {
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

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Fdflags {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Preopentype {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for Fdstat {
    fn zero_padding_bytes(&self, bytes: &mut [MaybeUninit<u8>]) {
        macro_rules! field {
            ($($f:tt)*) => {
                &self.$($f)* as *const _ as usize - self as *const _ as usize
            };
        }
        macro_rules! field_end {
            ($($f:tt)*) => {
                field!($($f)*) + std::mem::size_of_val(&self.$($f)*)
            };
        }
        macro_rules! zero {
            ($start:expr, $end:expr) => {
                for i in $start..$end {
                    bytes[i] = MaybeUninit::new(0);
                }
            };
        }

        self.fs_filetype
            .zero_padding_bytes(&mut bytes[field!(fs_filetype)..field_end!(fs_filetype)]);
        zero!(field_end!(fs_filetype), field!(fs_flags));

        self.fs_flags
            .zero_padding_bytes(&mut bytes[field!(fs_flags)..field_end!(fs_flags)]);
        zero!(field_end!(fs_flags), field!(fs_rights_base));

        self.fs_rights_base
            .zero_padding_bytes(&mut bytes[field!(fs_rights_base)..field_end!(fs_rights_base)]);
        zero!(
            field_end!(fs_rights_inheriting),
            std::mem::size_of_val(self)
        );
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl ValueType for SubscriptionClock {
    fn zero_padding_bytes(&self, bytes: &mut [MaybeUninit<u8>]) {
        macro_rules! field {
            ($($f:tt)*) => {
                &self.$($f)* as *const _ as usize - self as *const _ as usize
            };
        }
        macro_rules! field_end {
            ($($f:tt)*) => {
                field!($($f)*) + std::mem::size_of_val(&self.$($f)*)
            };
        }
        macro_rules! zero {
            ($start:expr, $end:expr) => {
                for i in $start..$end {
                    bytes[i] = MaybeUninit::new(0);
                }
            };
        }

        self.identifier
            .zero_padding_bytes(&mut bytes[field!(identifier)..field_end!(identifier)]);
        zero!(field_end!(identifier), field!(id));

        self.id
            .zero_padding_bytes(&mut bytes[field!(id)..field_end!(id)]);
        zero!(field_end!(id), field!(timeout));

        self.timeout
            .zero_padding_bytes(&mut bytes[field!(timeout)..field_end!(timeout)]);
        zero!(field_end!(timeout), field!(precision));

        self.precision
            .zero_padding_bytes(&mut bytes[field!(precision)..field_end!(precision)]);
        zero!(field_end!(precision), field!(flags));

        self.flags
            .zero_padding_bytes(&mut bytes[field!(flags)..field_end!(flags)]);
        zero!(field_end!(flags), std::mem::size_of_val(self));
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wit_bindgen_wasmer::wasmer::FromToNativeWasmType for Errno {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }
    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Success,
            1 => Self::Toobig,
            2 => Self::Acces,
            3 => Self::Addrinuse,
            4 => Self::Addrnotavail,
            5 => Self::Afnosupport,
            6 => Self::Again,
            7 => Self::Already,
            8 => Self::Badf,
            9 => Self::Badmsg,
            10 => Self::Busy,
            11 => Self::Canceled,
            12 => Self::Child,
            13 => Self::Connaborted,
            14 => Self::Connrefused,
            15 => Self::Connreset,
            16 => Self::Deadlk,
            17 => Self::Destaddrreq,
            18 => Self::Dom,
            19 => Self::Dquot,
            20 => Self::Exist,
            21 => Self::Fault,
            22 => Self::Fbig,
            23 => Self::Hostunreach,
            24 => Self::Idrm,
            25 => Self::Ilseq,
            26 => Self::Inprogress,
            27 => Self::Intr,
            28 => Self::Inval,
            29 => Self::Io,
            30 => Self::Isconn,
            31 => Self::Isdir,
            32 => Self::Loop,
            33 => Self::Mfile,
            34 => Self::Mlink,
            35 => Self::Msgsize,
            36 => Self::Multihop,
            37 => Self::Nametoolong,
            38 => Self::Netdown,
            39 => Self::Netreset,
            40 => Self::Netunreach,
            41 => Self::Nfile,
            42 => Self::Nobufs,
            43 => Self::Nodev,
            44 => Self::Noent,
            45 => Self::Noexec,
            46 => Self::Nolck,
            47 => Self::Nolink,
            48 => Self::Nomem,
            49 => Self::Nomsg,
            50 => Self::Noprotoopt,
            51 => Self::Nospc,
            52 => Self::Nosys,
            53 => Self::Notconn,
            54 => Self::Notdir,
            55 => Self::Notempty,
            56 => Self::Notrecoverable,
            57 => Self::Notsock,
            58 => Self::Notsup,
            59 => Self::Notty,
            60 => Self::Nxio,
            61 => Self::Overflow,
            62 => Self::Ownerdead,
            63 => Self::Perm,
            64 => Self::Pipe,
            65 => Self::Proto,
            66 => Self::Protonosupport,
            67 => Self::Prototype,
            68 => Self::Range,
            69 => Self::Rofs,
            70 => Self::Spipe,
            71 => Self::Srch,
            72 => Self::Stale,
            73 => Self::Timedout,
            74 => Self::Txtbsy,
            75 => Self::Xdev,
            76 => Self::Notcapable,
            // TODO: What should we map invalid native values to?
            _ => Self::Inval,
        }
    }

    #[cfg(feature = "sys")]
    fn is_from_store(&self, _store: &impl wit_bindgen_wasmer::wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wit_bindgen_wasmer::wasmer::FromToNativeWasmType for Advice {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }
    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Normal,
            1 => Self::Sequential,
            2 => Self::Random,
            3 => Self::Willneed,
            4 => Self::Dontneed,
            5 => Self::Noreuse,
            // TODO: What should we map invalid native values to?
            _ => todo!("Need to decide what to do here…"),
        }
    }

    #[cfg(feature = "sys")]
    fn is_from_store(&self, _store: &impl wit_bindgen_wasmer::wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wit_bindgen_wasmer::wasmer::FromToNativeWasmType for Rights {
    type Native = i64;

    fn to_native(self) -> Self::Native {
        self.bits() as i64
    }
    fn from_native(n: Self::Native) -> Self {
        Self::from_bits_truncate(n as u64)
    }

    #[cfg(feature = "sys")]
    fn is_from_store(&self, _store: &impl wit_bindgen_wasmer::wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wit_bindgen_wasmer::wasmer::FromToNativeWasmType for Eventrwflags {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self.bits() as i32
    }
    fn from_native(n: Self::Native) -> Self {
        Self::from_bits_truncate(n as u8)
    }

    #[cfg(feature = "sys")]
    fn is_from_store(&self, _store: &impl wit_bindgen_wasmer::wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wit_bindgen_wasmer::wasmer::FromToNativeWasmType for Subclockflags {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self.bits() as i32
    }
    fn from_native(n: Self::Native) -> Self {
        Self::from_bits_truncate(n as u8)
    }

    #[cfg(feature = "sys")]
    fn is_from_store(&self, _store: &impl wit_bindgen_wasmer::wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wit_bindgen_wasmer::wasmer::FromToNativeWasmType for Clockid {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }
    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Realtime,
            1 => Self::Monotonic,
            2 => Self::ProcessCputimeId,
            3 => Self::ThreadCputimeId,
            // TODO: What should we map invalid native values to?
            _ => todo!("Need to decide what to do here…"),
        }
    }

    #[cfg(feature = "sys")]
    fn is_from_store(&self, _store: &impl wit_bindgen_wasmer::wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wit_bindgen_wasmer::wasmer::FromToNativeWasmType for Fdflags {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self.bits() as i32
    }
    fn from_native(n: Self::Native) -> Self {
        Self::from_bits_truncate(n as u8)
    }

    #[cfg(feature = "sys")]
    fn is_from_store(&self, _store: &impl wit_bindgen_wasmer::wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
}

// TODO: if necessary, must be implemented in wit-bindgen
unsafe impl wit_bindgen_wasmer::wasmer::FromToNativeWasmType for Preopentype {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self as i32
    }
    fn from_native(n: Self::Native) -> Self {
        match n {
            0 => Self::Dir,
            // TODO: What should we map invalid native values to?
            _ => todo!("Need to decide what to do here…"),
        }
    }

    #[cfg(feature = "sys")]
    fn is_from_store(&self, _store: &impl wit_bindgen_wasmer::wasmer::AsStoreRef) -> bool {
        // TODO: find correct implementation
        false
    }
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
impl From<super::wasi_io_typenames::Eventtype> for Eventtype {
    fn from(other: super::wasi_io_typenames::Eventtype) -> Self {
        match other {
            super::wasi_io_typenames::Eventtype::Clock => Eventtype::Clock,
            super::wasi_io_typenames::Eventtype::FdRead => Eventtype::FdRead,
            super::wasi_io_typenames::Eventtype::FdWrite => Eventtype::FdWrite,
        }
    }
}

/// Workaround implementation because `wit-bindgen` does not generate
/// type aliases, but instead creates the same filetype in each module
/// for `use` statements in the `.wit` file.
impl From<super::wasi_io_typenames::Errno> for Errno {
    fn from(other: super::wasi_io_typenames::Errno) -> Self {
        match other {
            super::wasi_io_typenames::Errno::Success => Errno::Success,
            super::wasi_io_typenames::Errno::Toobig => Errno::Toobig,
            super::wasi_io_typenames::Errno::Access => Errno::Acces,
            super::wasi_io_typenames::Errno::Addrinuse => Errno::Addrinuse,
            super::wasi_io_typenames::Errno::Addrnotavail => Errno::Addrnotavail,
            super::wasi_io_typenames::Errno::Afnosupport => Errno::Afnosupport,
            super::wasi_io_typenames::Errno::Again => Errno::Again,
            super::wasi_io_typenames::Errno::Already => Errno::Already,
            super::wasi_io_typenames::Errno::Badf => Errno::Badf,
            super::wasi_io_typenames::Errno::Badmsg => Errno::Badmsg,
            super::wasi_io_typenames::Errno::Busy => Errno::Busy,
            super::wasi_io_typenames::Errno::Canceled => Errno::Canceled,
            super::wasi_io_typenames::Errno::Child => Errno::Child,
            super::wasi_io_typenames::Errno::Connaborted => Errno::Connaborted,
            super::wasi_io_typenames::Errno::Connrefused => Errno::Connrefused,
            super::wasi_io_typenames::Errno::Connreset => Errno::Connreset,
            super::wasi_io_typenames::Errno::Deadlk => Errno::Deadlk,
            super::wasi_io_typenames::Errno::Destaddrreq => Errno::Destaddrreq,
            super::wasi_io_typenames::Errno::Dom => Errno::Dom,
            super::wasi_io_typenames::Errno::Dquot => Errno::Dquot,
            super::wasi_io_typenames::Errno::Exist => Errno::Exist,
            super::wasi_io_typenames::Errno::Fault => Errno::Fault,
            super::wasi_io_typenames::Errno::Fbig => Errno::Fbig,
            super::wasi_io_typenames::Errno::Hostunreach => Errno::Hostunreach,
            super::wasi_io_typenames::Errno::Idrm => Errno::Idrm,
            super::wasi_io_typenames::Errno::Ilseq => Errno::Ilseq,
            super::wasi_io_typenames::Errno::Inprogress => Errno::Inprogress,
            super::wasi_io_typenames::Errno::Intr => Errno::Intr,
            super::wasi_io_typenames::Errno::Inval => Errno::Inval,
            super::wasi_io_typenames::Errno::Io => Errno::Io,
            super::wasi_io_typenames::Errno::Isconn => Errno::Isconn,
            super::wasi_io_typenames::Errno::Isdir => Errno::Isdir,
            super::wasi_io_typenames::Errno::Loop => Errno::Loop,
            super::wasi_io_typenames::Errno::Mfile => Errno::Mfile,
            super::wasi_io_typenames::Errno::Mlink => Errno::Mlink,
            super::wasi_io_typenames::Errno::Msgsize => Errno::Msgsize,
            super::wasi_io_typenames::Errno::Multihop => Errno::Multihop,
            super::wasi_io_typenames::Errno::Nametoolong => Errno::Nametoolong,
            super::wasi_io_typenames::Errno::Netdown => Errno::Netdown,
            super::wasi_io_typenames::Errno::Netreset => Errno::Netreset,
            super::wasi_io_typenames::Errno::Netunreach => Errno::Netunreach,
            super::wasi_io_typenames::Errno::Nfile => Errno::Nfile,
            super::wasi_io_typenames::Errno::Nobufs => Errno::Nobufs,
            super::wasi_io_typenames::Errno::Nodev => Errno::Nodev,
            super::wasi_io_typenames::Errno::Noent => Errno::Noent,
            super::wasi_io_typenames::Errno::Noexec => Errno::Noexec,
            super::wasi_io_typenames::Errno::Nolck => Errno::Nolck,
            super::wasi_io_typenames::Errno::Nolink => Errno::Nolink,
            super::wasi_io_typenames::Errno::Nomem => Errno::Nomem,
            super::wasi_io_typenames::Errno::Nomsg => Errno::Nomsg,
            super::wasi_io_typenames::Errno::Noprotoopt => Errno::Noprotoopt,
            super::wasi_io_typenames::Errno::Nospc => Errno::Nospc,
            super::wasi_io_typenames::Errno::Nosys => Errno::Nosys,
            super::wasi_io_typenames::Errno::Notconn => Errno::Notconn,
            super::wasi_io_typenames::Errno::Notdir => Errno::Notdir,
            super::wasi_io_typenames::Errno::Notempty => Errno::Notempty,
            super::wasi_io_typenames::Errno::Notrecoverable => Errno::Notrecoverable,
            super::wasi_io_typenames::Errno::Notsock => Errno::Notsock,
            super::wasi_io_typenames::Errno::Notsup => Errno::Notsup,
            super::wasi_io_typenames::Errno::Notty => Errno::Notty,
            super::wasi_io_typenames::Errno::Nxio => Errno::Nxio,
            super::wasi_io_typenames::Errno::Overflow => Errno::Overflow,
            super::wasi_io_typenames::Errno::Ownerdead => Errno::Ownerdead,
            super::wasi_io_typenames::Errno::Perm => Errno::Perm,
            super::wasi_io_typenames::Errno::Pipe => Errno::Pipe,
            super::wasi_io_typenames::Errno::Proto => Errno::Proto,
            super::wasi_io_typenames::Errno::Protonosupport => Errno::Protonosupport,
            super::wasi_io_typenames::Errno::Prototype => Errno::Prototype,
            super::wasi_io_typenames::Errno::Range => Errno::Range,
            super::wasi_io_typenames::Errno::Rofs => Errno::Rofs,
            super::wasi_io_typenames::Errno::Spipe => Errno::Spipe,
            super::wasi_io_typenames::Errno::Srch => Errno::Srch,
            super::wasi_io_typenames::Errno::Stale => Errno::Stale,
            super::wasi_io_typenames::Errno::Timedout => Errno::Timedout,
            super::wasi_io_typenames::Errno::Txtbsy => Errno::Txtbsy,
            super::wasi_io_typenames::Errno::Xdev => Errno::Xdev,
            super::wasi_io_typenames::Errno::Notcapable => Errno::Notcapable,
        }
    }
}
