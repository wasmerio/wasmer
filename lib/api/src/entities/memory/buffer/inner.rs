use std::{marker::PhantomData, mem::MaybeUninit};

use crate::{
    macros::backend::{gen_rt_ty, match_rt},
    MemoryAccessError,
};

/// Underlying buffer for a memory.
gen_rt_ty!(MemoryBuffer<'a>
    @derives Debug, Copy, Clone, derive_more::From;
    @path memory
);

impl<'a> BackendMemoryBuffer<'a> {
    #[allow(unused)]
    #[inline]
    pub(crate) fn read(&self, offset: u64, buf: &mut [u8]) -> Result<(), MemoryAccessError> {
        match_rt!(on self => s {
            s.read(offset, buf)
        })
    }

    #[allow(unused)]
    #[inline]
    pub(crate) fn read_uninit<'b>(
        &self,
        offset: u64,
        buf: &'b mut [MaybeUninit<u8>],
    ) -> Result<&'b mut [u8], MemoryAccessError> {
        match_rt!(on self => s {
            s.read_uninit(offset, buf)
        })
    }

    #[allow(unused)]
    #[inline]
    pub(crate) fn write(&self, offset: u64, data: &[u8]) -> Result<(), MemoryAccessError> {
        match_rt!(on self => s {
            s.write(offset, data)
        })
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.len,

            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s.len,

            #[cfg(feature = "wasmi")]
            Self::Wasmi(s) => s.len,

            #[cfg(feature = "v8")]
            Self::V8(s) => s.len,

            #[cfg(feature = "js")]
            Self::Js(s) => s.len(),

            #[cfg(feature = "jsc")]
            Self::Jsc(s) => s.len,
        }
    }

    #[inline]
    pub(crate) fn is_owned(&self) -> bool {
        match self {
            #[cfg(feature = "js")]
            Self::Js(_) => true,
            _ => false,
        }
    }

    #[inline]
    pub(crate) fn base(&self) -> *mut u8 {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.base,
            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s.base,
            #[cfg(feature = "wasmi")]
            Self::Wasmi(s) => s.base,
            #[cfg(feature = "v8")]
            Self::V8(s) => s.base,
            #[cfg(feature = "js")]
            Self::Js(s) => panic!("js memory buffers do not support the `base` function!"),
            #[cfg(feature = "jsc")]
            Self::Jsc(s) => s.base,
        }
    }
}
