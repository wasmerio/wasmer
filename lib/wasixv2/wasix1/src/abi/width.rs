//! The import namespace chooses argument and guest-word widths independently of memory.

use crate::{abi::Errno, errno::V1Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbiWidth {
    Wasm32,
    Wasm64,
}

impl AbiWidth {
    pub const fn bytes(self) -> u32 {
        match self {
            Self::Wasm32 => 4,
            Self::Wasm64 => 8,
        }
    }
}

/// An untranslated guest pointer. Narrow it only when the operation uses it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pointer(pub u64);

impl Pointer {
    pub const NULL: Self = Self(0);

    pub fn narrow(self) -> V1Result<u32> {
        u32::try_from(self.0).map_err(|_| Errno::MEMVIOLATION.into())
    }

    pub fn offset(self, bytes: u64) -> V1Result<Self> {
        self.0
            .checked_add(bytes)
            .map(Self)
            .ok_or(Errno::OVERFLOW.into())
    }
}

impl From<u32> for Pointer {
    fn from(value: u32) -> Self {
        Self(u64::from(value))
    }
}

/// An untranslated `M::Offset` count or capacity. Its meaning determines when it narrows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Size(pub u64);

impl Size {
    pub fn narrow(self) -> V1Result<u32> {
        u32::try_from(self.0).map_err(|_| Errno::MEMVIOLATION.into())
    }
}

impl From<u32> for Size {
    fn from(value: u32) -> Self {
        Self(u64::from(value))
    }
}
