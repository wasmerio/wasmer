use std::{marker::PhantomData, mem::MaybeUninit, ops::Range};

use wasmer_types::Pages;

use super::{Memory, MemoryBuffer};
use crate::{AsStoreRef, MemoryAccessError};

/// A WebAssembly `memory` view.
#[derive(Debug)]
pub struct MemoryView<'a> {
    pub(crate) buffer: MemoryBuffer<'a>,
    pub(crate) size: u32,
}

impl<'a> MemoryView<'a> {
    pub(crate) fn new(memory: &Memory, store: &'a (impl AsStoreRef + ?Sized)) -> Self {
        let store = store.as_store_ref();
        let len = memory.handle.data_size(&store.inner.store.as_wasmi().inner);
        let base = memory.handle.data_ptr(&store.inner.store.as_wasmi().inner);
        let size = memory.handle.size(&store.inner.store.as_wasmi().inner) as u32;

        Self {
            buffer: MemoryBuffer {
                base,
                len,
                marker: PhantomData,
            },
            size,
        }
    }

    #[doc(hidden)]
    pub fn data_ptr(&self) -> *mut u8 {
        self.buffer.base
    }

    pub fn data_size(&self) -> u64 {
        self.buffer.len.try_into().unwrap()
    }

    #[doc(hidden)]
    pub unsafe fn data_unchecked(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.buffer.base as *const u8, self.buffer.len) }
    }

    #[allow(clippy::mut_from_ref)]
    #[doc(hidden)]
    pub unsafe fn data_unchecked_mut(&self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.buffer.base, self.buffer.len) }
    }

    pub fn size(&self) -> Pages {
        Pages(self.size)
    }

    #[inline]
    pub(crate) fn buffer(&'a self) -> MemoryBuffer<'a> {
        self.buffer
    }

    pub fn read(&self, offset: u64, buf: &mut [u8]) -> Result<(), MemoryAccessError> {
        self.buffer.read(offset, buf)
    }

    pub fn read_u8(&self, offset: u64) -> Result<u8, MemoryAccessError> {
        let mut buf = [0u8; 1];
        self.read(offset, &mut buf)?;
        Ok(buf[0])
    }

    pub fn read_uninit<'b>(
        &self,
        offset: u64,
        buf: &'b mut [MaybeUninit<u8>],
    ) -> Result<&'b mut [u8], MemoryAccessError> {
        self.buffer.read_uninit(offset, buf)
    }

    pub fn write(&self, offset: u64, data: &[u8]) -> Result<(), MemoryAccessError> {
        self.buffer.write(offset, data)
    }

    pub fn write_u8(&self, offset: u64, val: u8) -> Result<(), MemoryAccessError> {
        self.write(offset, &[val])
    }

    pub fn copy_to_vec(&self) -> Result<Vec<u8>, MemoryAccessError> {
        self.copy_range_to_vec(0..self.data_size())
    }

    pub fn copy_range_to_vec(&self, range: Range<u64>) -> Result<Vec<u8>, MemoryAccessError> {
        let mut new_memory = Vec::new();
        let mut offset = range.start;
        let end = range.end.min(self.data_size());
        let mut chunk = [0u8; 40960];
        while offset < end {
            let remaining = end - offset;
            let sublen = remaining.min(chunk.len() as u64) as usize;
            self.read(offset, &mut chunk[..sublen])?;
            new_memory.extend_from_slice(&chunk[..sublen]);
            offset += sublen as u64;
        }
        Ok(new_memory)
    }

    pub fn copy_to_memory(&self, amount: u64, new_memory: &Self) -> Result<(), MemoryAccessError> {
        let mut offset = 0;
        let mut chunk = [0u8; 40960];
        while offset < amount {
            let remaining = amount - offset;
            let sublen = remaining.min(chunk.len() as u64) as usize;
            self.read(offset, &mut chunk[..sublen])?;
            new_memory.write(offset, &chunk[..sublen])?;
            offset += sublen as u64;
        }
        Ok(())
    }
}
