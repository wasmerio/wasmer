use super::Memory;
use crate::{js::store::StoreObjects, MemoryAccessError};
use std::{marker::PhantomData, mem::MaybeUninit, slice};
use tracing::warn;

/// Underlying buffer for a memory.
#[derive(Copy, Clone, Debug)]
pub(crate) struct MemoryBuffer<'a> {
    pub(crate) base: *mut js_sys::Uint8Array,
    pub(crate) marker: PhantomData<(&'a Memory, &'a StoreObjects)>,
}

impl<'a> MemoryBuffer<'a> {
    pub(crate) fn read(&self, offset: u64, buf: &mut [u8]) -> Result<(), MemoryAccessError> {
        let end = offset
            .checked_add(buf.len() as u64)
            .ok_or(MemoryAccessError::Overflow)?;
        let view = unsafe { &*(self.base) };
        if end > view.length().into() {
            warn!(
                "attempted to read ({} bytes) beyond the bounds of the memory view ({} > {})",
                buf.len(),
                end,
                view.length()
            );
            return Err(MemoryAccessError::HeapOutOfBounds);
        }
        view.subarray(offset as _, end as _)
            .copy_to(unsafe { &mut slice::from_raw_parts_mut(buf.as_mut_ptr(), buf.len()) });
        Ok(())
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        let view = unsafe { &*(self.base) };
        view.length() as usize
    }

    pub(crate) fn read_uninit<'b>(
        &self,
        offset: u64,
        buf: &'b mut [MaybeUninit<u8>],
    ) -> Result<&'b mut [u8], MemoryAccessError> {
        let end = offset
            .checked_add(buf.len() as u64)
            .ok_or(MemoryAccessError::Overflow)?;
        let view = unsafe { &*(self.base) };
        if end > view.length().into() {
            warn!(
                "attempted to read ({} bytes) beyond the bounds of the memory view ({} > {})",
                buf.len(),
                end,
                view.length()
            );
            return Err(MemoryAccessError::HeapOutOfBounds);
        }
        let buf_ptr = buf.as_mut_ptr() as *mut u8;
        view.subarray(offset as _, end as _)
            .copy_to(unsafe { &mut slice::from_raw_parts_mut(buf_ptr, buf.len()) });

        Ok(unsafe { slice::from_raw_parts_mut(buf_ptr, buf.len()) })
    }

    pub(crate) fn write(&self, offset: u64, data: &[u8]) -> Result<(), MemoryAccessError> {
        let end = offset
            .checked_add(data.len() as u64)
            .ok_or(MemoryAccessError::Overflow)?;
        let view = unsafe { &mut *(self.base) };
        if end > view.length().into() {
            warn!(
                "attempted to write ({} bytes) beyond the bounds of the memory view ({} > {})",
                data.len(),
                end,
                view.length()
            );
            return Err(MemoryAccessError::HeapOutOfBounds);
        }
        view.subarray(offset as _, end as _).copy_from(data);

        Ok(())
    }
}
