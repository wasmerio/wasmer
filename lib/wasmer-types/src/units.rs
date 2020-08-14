use crate::lib::std::fmt;
use crate::lib::std::ops::{Add, Sub};
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};

/// WebAssembly page sizes are fixed to be 64KiB.
/// Note: large page support may be added in an opt-in manner in the [future].
///
/// [future]: https://webassembly.org/docs/future-features/#large-page-support
pub const WASM_PAGE_SIZE: usize = 0x10000;

/// The number of pages we can have before we run out of byte index space.
pub const WASM_MAX_PAGES: u32 = 0x10000;

/// The minimum number of pages allowed.
pub const WASM_MIN_PAGES: u32 = 0x100;

/// Units of WebAssembly pages (as specified to be 65,536 bytes).
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct Pages(pub u32);

impl Pages {
    /// Returns the largest value that can be represented by the Pages type.
    ///
    /// This is defined by the WebAssembly standard as 65,536 pages.
    #[inline(always)]
    pub const fn max_value() -> Self {
        Self(WASM_MAX_PAGES)
    }

    /// Checked addition. Computes `self + rhs`,
    /// returning `None` if overflow occurred.
    pub fn checked_add(self, rhs: Self) -> Option<Self> {
        let added = (self.0 as usize) + (rhs.0 as usize);
        if added <= (WASM_MAX_PAGES as usize) {
            Some(Self(added as u32))
        } else {
            None
        }
    }

    /// Calculate number of bytes from pages.
    pub fn bytes(self) -> Bytes {
        self.into()
    }
}

impl fmt::Debug for Pages {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} pages", self.0)
    }
}

impl From<u32> for Pages {
    fn from(other: u32) -> Self {
        Self(other)
    }
}

/// Units of WebAssembly memory in terms of 8-bit bytes.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct Bytes(pub usize);

impl fmt::Debug for Bytes {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} bytes", self.0)
    }
}

impl From<Pages> for Bytes {
    fn from(pages: Pages) -> Self {
        Self((pages.0 as usize) * WASM_PAGE_SIZE)
    }
}

impl From<usize> for Bytes {
    fn from(other: usize) -> Self {
        Self(other)
    }
}

impl<T> Sub<T> for Pages
where
    T: Into<Self>,
{
    type Output = Self;
    fn sub(self, rhs: T) -> Self {
        Self(self.0 - rhs.into().0)
    }
}

impl<T> Add<T> for Pages
where
    T: Into<Self>,
{
    type Output = Self;
    fn add(self, rhs: T) -> Self {
        Self(self.0 + rhs.into().0)
    }
}

impl From<Bytes> for Pages {
    fn from(bytes: Bytes) -> Self {
        Self((bytes.0 / WASM_PAGE_SIZE) as u32)
    }
}

impl<T> Sub<T> for Bytes
where
    T: Into<Self>,
{
    type Output = Self;
    fn sub(self, rhs: T) -> Self {
        Self(self.0 - rhs.into().0)
    }
}

impl<T> Add<T> for Bytes
where
    T: Into<Self>,
{
    type Output = Self;
    fn add(self, rhs: T) -> Self {
        Self(self.0 + rhs.into().0)
    }
}
