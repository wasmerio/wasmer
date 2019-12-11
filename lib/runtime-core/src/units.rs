//! The units module provides common WebAssembly units like `Pages` and conversion functions into
//! other units.
use crate::error::PageError;
use std::{
    fmt,
    ops::{Add, Sub},
};

/// The page size in bytes of a wasm page.
pub const WASM_PAGE_SIZE: usize = 65_536;
/// Tbe max number of wasm pages allowed.
pub const WASM_MAX_PAGES: usize = 65_536;
// From emscripten resize_heap implementation
/// The minimum number of wasm pages allowed.
pub const WASM_MIN_PAGES: usize = 256;

/// Units of WebAssembly pages (as specified to be 65,536 bytes).
#[derive(Serialize, Deserialize, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pages(pub u32);

impl Pages {
    /// Checked add of Pages to Pages.
    pub fn checked_add(self, rhs: Pages) -> Result<Pages, PageError> {
        let added = (self.0 as usize) + (rhs.0 as usize);
        if added <= WASM_MAX_PAGES {
            Ok(Pages(added as u32))
        } else {
            Err(PageError::ExceededMaxPages(
                self.0 as usize,
                rhs.0 as usize,
                added,
            ))
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

/// Units of WebAssembly memory in terms of 8-bit bytes.
#[derive(Serialize, Deserialize, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Bytes(pub usize);

impl fmt::Debug for Bytes {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} bytes", self.0)
    }
}

impl From<Pages> for Bytes {
    fn from(pages: Pages) -> Bytes {
        Bytes((pages.0 as usize) * WASM_PAGE_SIZE)
    }
}

impl<T> Sub<T> for Pages
where
    T: Into<Pages>,
{
    type Output = Pages;
    fn sub(self, rhs: T) -> Pages {
        Pages(self.0 - rhs.into().0)
    }
}

impl<T> Add<T> for Pages
where
    T: Into<Pages>,
{
    type Output = Pages;
    fn add(self, rhs: T) -> Pages {
        Pages(self.0 + rhs.into().0)
    }
}

impl From<Bytes> for Pages {
    fn from(bytes: Bytes) -> Pages {
        Pages((bytes.0 / WASM_PAGE_SIZE) as u32)
    }
}

impl<T> Sub<T> for Bytes
where
    T: Into<Bytes>,
{
    type Output = Bytes;
    fn sub(self, rhs: T) -> Bytes {
        Bytes(self.0 - rhs.into().0)
    }
}

impl<T> Add<T> for Bytes
where
    T: Into<Bytes>,
{
    type Output = Bytes;
    fn add(self, rhs: T) -> Bytes {
        Bytes(self.0 + rhs.into().0)
    }
}
