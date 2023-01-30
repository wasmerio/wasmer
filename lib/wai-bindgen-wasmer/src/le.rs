use crate::AllBytesValid;
use std::cmp::Ordering;
use std::fmt;
use std::mem;
use std::slice;

/// Helper type representing a 1-byte-aligned little-endian value in memory.
///
/// This type is used in slice types for Wasmer host bindings. Guest types are
/// not guaranteed to be either aligned or in the native endianness. This type
/// wraps these types and provides explicit getters/setters to interact with the
/// underlying value in a safe host-agnostic manner.
#[repr(packed)]
pub struct Le<T>(T);

impl<T> Le<T>
where
    T: Endian,
{
    /// Creates a new `Le<T>` value where the internals are stored in a way
    /// that's safe to copy into wasm linear memory.
    pub fn new(t: T) -> Le<T> {
        Le(t.into_le())
    }

    /// Reads the value stored in this `Le<T>`.
    ///
    /// This will perform a correct read even if the underlying memory is
    /// unaligned, and it will also convert to the host's endianness for the
    /// right representation of `T`.
    pub fn get(&self) -> T {
        self.0.from_le()
    }

    /// Writes the `val` to this slot.
    ///
    /// This will work correctly even if the underlying memory is unaligned and
    /// it will also automatically convert the `val` provided to an endianness
    /// appropriate for WebAssembly (little-endian).
    pub fn set(&mut self, val: T) {
        self.0 = val.into_le();
    }

    pub(crate) fn from_slice(bytes: &[u8]) -> &[Le<T>] {
        // SAFETY: The invariants we uphold here are:
        //
        // * the lifetime of the input is the same as the output, so we're only
        //   dealing with valid memory.
        // * the alignment of the input is the same as the output (1)
        // * the input isn't being truncated and we're consuming all of it (it
        //   must be a multiple of the size of `Le<T>`)
        // * all byte-patterns for `Le<T>` are valid. This is guaranteed by the
        //   `AllBytesValid` supertrait of `Endian`.
        unsafe {
            assert_eq!(mem::align_of::<Le<T>>(), 1);
            assert!(bytes.len() % mem::size_of::<Le<T>>() == 0);
            fn all_bytes_valid<T: AllBytesValid>() {}
            all_bytes_valid::<Le<T>>();

            slice::from_raw_parts(
                bytes.as_ptr().cast::<Le<T>>(),
                bytes.len() / mem::size_of::<Le<T>>(),
            )
        }
    }

    pub(crate) fn from_slice_mut(bytes: &mut [u8]) -> &mut [Le<T>] {
        // SAFETY: see `from_slice` above
        //
        // Note that both the input and the output are `mut`, helping to
        // maintain the guarantee of uniqueness.
        unsafe {
            assert_eq!(mem::align_of::<Le<T>>(), 1);
            assert!(bytes.len() % mem::size_of::<Le<T>>() == 0);
            slice::from_raw_parts_mut(
                bytes.as_mut_ptr().cast::<Le<T>>(),
                bytes.len() / mem::size_of::<Le<T>>(),
            )
        }
    }
}

impl<T: Copy> Clone for Le<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Copy> Copy for Le<T> {}

impl<T: Endian + PartialEq> PartialEq for Le<T> {
    fn eq(&self, other: &Le<T>) -> bool {
        self.get() == other.get()
    }
}

impl<T: Endian + PartialEq> PartialEq<T> for Le<T> {
    fn eq(&self, other: &T) -> bool {
        self.get() == *other
    }
}

impl<T: Endian + Eq> Eq for Le<T> {}

impl<T: Endian + PartialOrd> PartialOrd for Le<T> {
    fn partial_cmp(&self, other: &Le<T>) -> Option<Ordering> {
        self.get().partial_cmp(&other.get())
    }
}

impl<T: Endian + Ord> Ord for Le<T> {
    fn cmp(&self, other: &Le<T>) -> Ordering {
        self.get().cmp(&other.get())
    }
}

impl<T: Endian + fmt::Debug> fmt::Debug for Le<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get().fmt(f)
    }
}

impl<T: Endian> From<T> for Le<T> {
    fn from(t: T) -> Le<T> {
        Le::new(t)
    }
}

unsafe impl<T: AllBytesValid> AllBytesValid for Le<T> {}

/// Trait used for the implementation of the `Le` type.
pub trait Endian: AllBytesValid + Copy + Sized {
    /// Converts this value and any aggregate fields (if any) into little-endian
    /// byte order
    fn into_le(self) -> Self;
    /// Converts this value and any aggregate fields (if any) from
    /// little-endian byte order
    #[allow(clippy::wrong_self_convention)]
    fn from_le(self) -> Self;
}

macro_rules! primitives {
    ($($t:ident)*) => ($(
        impl Endian for $t {
            #[inline]
            fn into_le(self) -> Self {
                Self::from_ne_bytes(self.to_le_bytes())
            }

            #[inline]
            fn from_le(self) -> Self {
                Self::from_le_bytes(self.to_ne_bytes())
            }
        }
    )*)
}

primitives! {
    u8 i8
    u16 i16
    u32 i32
    u64 i64
    f32 f64
}

#[allow(clippy::unused_unit)]
macro_rules! tuples {
    ($(($($t:ident)*))*) => ($(
        #[allow(non_snake_case)]
        impl <$($t:Endian,)*> Endian for ($($t,)*) {
            #[allow(clippy::unused_unit)]
            fn into_le(self) -> Self {
                let ($($t,)*) = self;
                // Needed for single element "tuples".
                ($($t.into_le(),)*)
            }

            #[allow(clippy::unused_unit)]
            fn from_le(self) -> Self {
                let ($($t,)*) = self;
                // Needed for single element "tuples".
                ($($t.from_le(),)*)
            }
        }
    )*)
}

tuples! {
    ()
    (T1)
    (T1 T2)
    (T1 T2 T3)
    (T1 T2 T3 T4)
    (T1 T2 T3 T4 T5)
    (T1 T2 T3 T4 T5 T6)
    (T1 T2 T3 T4 T5 T6 T7)
    (T1 T2 T3 T4 T5 T6 T7 T8)
    (T1 T2 T3 T4 T5 T6 T7 T8 T9)
    (T1 T2 T3 T4 T5 T6 T7 T8 T9 T10)
}
