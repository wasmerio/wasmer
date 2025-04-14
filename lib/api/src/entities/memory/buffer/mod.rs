use std::{marker::PhantomData, mem::MaybeUninit};

use crate::MemoryAccessError;

pub(crate) mod inner;
pub(crate) use inner::*;

/// Underlying buffer for a memory.
#[derive(Debug, Copy, Clone, derive_more::From)]
pub(crate) struct MemoryBuffer<'a>(pub(crate) BackendMemoryBuffer<'a>);

impl<'a> MemoryBuffer<'a> {
    #[allow(unused)]
    pub(crate) fn read(&self, offset: u64, buf: &mut [u8]) -> Result<(), MemoryAccessError> {
        self.0.read(offset, buf)
    }

    #[allow(unused)]
    pub(crate) fn read_uninit<'b>(
        &self,
        offset: u64,
        buf: &'b mut [MaybeUninit<u8>],
    ) -> Result<&'b mut [u8], MemoryAccessError> {
        self.0.read_uninit(offset, buf)
    }

    #[allow(unused)]
    pub(crate) fn write(&self, offset: u64, data: &[u8]) -> Result<(), MemoryAccessError> {
        self.0.write(offset, data)
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    pub(crate) fn base(&self) -> *mut u8 {
        self.0.base()
    }

    /// This is used to determine if the buffer is owned by the memory.
    ///
    /// This is only useful for JS memory buffers, as they will slowdown
    /// if we always need to own the memory in other engines.
    pub(crate) fn is_owned(&self) -> bool {
        self.0.is_owned()
    }
}
