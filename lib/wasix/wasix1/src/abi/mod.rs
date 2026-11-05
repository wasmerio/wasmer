//! The WASIX v1 guest ABI for wasm32: errno values, flag bits, enumerations and struct layouts,
//! as `wasi_snapshot_preview1`, `wasi_unstable` and `wasix_32v1` define them.
//!
//! Layouts implement [`GuestLayout`] with explicit offsets, like the v2 ABI, so they move through
//! [`wasixv2_syscalls::UserMemory`] the same way. `tests/layout.rs` checks every size and offset
//! against `wasmer-wasix-types`.

pub mod errno;
pub mod fs;
pub mod poll;
pub mod proc;
pub mod sock;

pub use errno::Errno;
pub use wasixv2_abi::GuestLayout;

/// A guest pointer. Every pointer-sized field and argument goes through this alias, so a
/// wasm64 flavour would only touch this module.
pub type Ptr = u32;
/// A guest `size_t`.
pub type Size = u32;
/// Nanoseconds.
pub type Timestamp = u64;
pub type Fd = u32;
pub type Pid = u32;
pub type Tid = u32;
pub type Userdata = u64;

/// `clockid`: the same numbers as Linux's first four clocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clockid(pub u32);

impl Clockid {
    pub const REALTIME: Self = Self(0);
    pub const MONOTONIC: Self = Self(1);
    pub const PROCESS_CPUTIME_ID: Self = Self(2);
    pub const THREAD_CPUTIME_ID: Self = Self(3);

    /// The Linux clock, or `None` for an id v1 does not define.
    pub fn to_linux(self) -> Option<wasixv2_abi::ClockId> {
        use wasixv2_abi::ClockId as L;
        Some(match self {
            Self::REALTIME => L::CLOCK_REALTIME,
            Self::MONOTONIC => L::CLOCK_MONOTONIC,
            Self::PROCESS_CPUTIME_ID => L::CLOCK_PROCESS_CPUTIME_ID,
            Self::THREAD_CPUTIME_ID => L::CLOCK_THREAD_CPUTIME_ID,
            _ => return None,
        })
    }
}

/// `bool` as v1 passes it: a `u8`, 0 or 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bool(pub u8);

impl Bool {
    pub const FALSE: Self = Self(0);
    pub const TRUE: Self = Self(1);
}

/// Whether a field at `offset` of `size` bytes fits a struct of `total` bytes.
pub(crate) const fn field_fits(offset: usize, size: usize, total: usize) -> bool {
    offset + size <= total
}

pub(crate) const fn field_aligned(offset: usize, align: usize) -> bool {
    offset.is_multiple_of(align)
}

/// Declares a v1 struct with explicit field offsets and its [`GuestLayout`] impl. Padding is
/// zeroed on write; `tests/layout.rs` checks the offsets.
macro_rules! layout {
    (
        $(#[$meta:meta])*
        pub struct $name:ident (size $size:literal, align $align:literal) {
            $( $(#[$fmeta:meta])* pub $field:ident : $ty:ty = $offset:literal ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $name {
            $( $(#[$fmeta])* pub $field: $ty, )*
        }

        impl $name {
            /// Field offsets, for the layout conformance test.
            pub const OFFSETS: &'static [(&'static str, usize)] = &[$((stringify!($field), $offset)),*];
        }

        const _: () = {
            $( assert!($crate::abi::field_fits($offset, <$ty as $crate::abi::GuestLayout>::SIZE, $size)); )*
            $( assert!($crate::abi::field_aligned($offset, <$ty as $crate::abi::GuestLayout>::ALIGN)); )*
        };

        impl $crate::abi::GuestLayout for $name {
            const SIZE: usize = $size;
            const ALIGN: usize = $align;
            const ZERO: Self = Self {
                $( $field: <$ty as $crate::abi::GuestLayout>::ZERO, )*
            };
            fn read_from(bytes: &[u8]) -> Option<Self> {
                let bytes = bytes.get(..$size)?;
                Some(Self {
                    $( $field: $crate::abi::GuestLayout::read_from(&bytes[$offset..])?, )*
                })
            }
            fn write_to(&self, out: &mut [u8]) {
                let out = &mut out[..$size];
                out.fill(0);
                $( $crate::abi::GuestLayout::write_to(&self.$field, &mut out[$offset..]); )*
            }
        }
    };
}
pub(crate) use layout;

/// Implements [`GuestLayout`] for a newtype over an integer.
macro_rules! newtype_layout {
    ($($name:ident($inner:ty)),* $(,)?) => {$(
        impl $crate::abi::GuestLayout for $name {
            const SIZE: usize = <$inner as $crate::abi::GuestLayout>::SIZE;
            const ALIGN: usize = <$inner as $crate::abi::GuestLayout>::ALIGN;
            const ZERO: Self = Self(0);
            fn read_from(bytes: &[u8]) -> Option<Self> {
                <$inner as $crate::abi::GuestLayout>::read_from(bytes).map(Self)
            }
            fn write_to(&self, out: &mut [u8]) {
                self.0.write_to(out);
            }
        }
    )*};
}
pub(crate) use newtype_layout;

newtype_layout!(Clockid(u32), Bool(u8));

/// `ciovec`/`iovec`: `{buf, buf_len}`, identical to Linux's wasm32 `struct iovec`, so iovec
/// arrays pass to the kernel untranslated.
pub type Iovec = wasixv2_abi::Iovec;
