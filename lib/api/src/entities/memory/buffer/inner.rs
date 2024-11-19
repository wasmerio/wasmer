use std::{marker::PhantomData, mem::MaybeUninit};

use crate::MemoryAccessError;

/// Underlying buffer for a memory.
#[derive(Debug, Copy, Clone, derive_more::From)]
pub(crate) enum RuntimeMemoryBuffer<'a> {
    #[cfg(feature = "sys")]
    Sys(crate::rt::sys::entities::memory::MemoryBuffer<'a>),

    #[cfg(feature = "wamr")]
    Wamr(crate::rt::wamr::entities::memory::MemoryBuffer<'a>),

    #[cfg(feature = "v8")]
    V8(crate::rt::v8::entities::memory::MemoryBuffer<'a>),
    Phantom(PhantomData<&'a ()>),
}

impl<'a> RuntimeMemoryBuffer<'a> {
    #[allow(unused)]
    pub(crate) fn read(&self, offset: u64, buf: &mut [u8]) -> Result<(), MemoryAccessError> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.read(offset, buf),

            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s.read(offset, buf),

            #[cfg(feature = "v8")]
            Self::V8(s) => s.read(offset, buf),
            _ => panic!("No runtime enabled!"),
        }
    }

    #[allow(unused)]
    pub(crate) fn read_uninit<'b>(
        &self,
        offset: u64,
        buf: &'b mut [MaybeUninit<u8>],
    ) -> Result<&'b mut [u8], MemoryAccessError> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.read_uninit(offset, buf),

            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s.read_uninit(offset, buf),

            #[cfg(feature = "v8")]
            Self::V8(s) => s.read_uninit(offset, buf),
            _ => panic!("No runtime enabled!"),
        }
    }

    #[allow(unused)]
    pub(crate) fn write(&self, offset: u64, data: &[u8]) -> Result<(), MemoryAccessError> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.write(offset, data),

            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s.write(offset, data),

            #[cfg(feature = "v8")]
            Self::V8(s) => s.write(offset, data),
            _ => panic!("No runtime enabled!"),
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.len,

            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s.len,

            #[cfg(feature = "v8")]
            Self::V8(s) => s.len,
            _ => panic!("No runtime enabled!"),
        }
    }

    pub(crate) fn base(&self) -> *mut u8 {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.base,

            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s.base,

            #[cfg(feature = "v8")]
            Self::V8(s) => s.base,
            _ => panic!("No runtime enabled!"),
        }
    }
}
