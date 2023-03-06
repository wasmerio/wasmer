use crate::access::{RefCow, SliceCow, WasmRefAccess};
use crate::mem_access::MemoryAccessError;
use crate::WasmSlice;
use crate::{WasmRef, WasmSliceAccess};
use std::mem;

impl<'a, T> WasmSliceAccess<'a, T>
where
    T: wasmer_types::ValueType,
{
    pub(crate) fn new(slice: WasmSlice<'a, T>) -> Result<Self, MemoryAccessError> {
        let total_len = slice
            .len
            .checked_mul(mem::size_of::<T>() as u64)
            .ok_or(MemoryAccessError::Overflow)?;
        let end = slice
            .offset
            .checked_add(total_len)
            .ok_or(MemoryAccessError::Overflow)?;
        if end > slice.buffer.0.len as u64 {
            #[cfg(feature = "tracing")]
            warn!(
                "attempted to read ({} bytes) beyond the bounds of the memory view ({} > {})",
                total_len, end, slice.buffer.len
            );
            return Err(MemoryAccessError::HeapOutOfBounds);
        }
        let buf = unsafe {
            let buf_ptr: *mut u8 = slice.buffer.0.base.add(slice.offset as usize);
            let buf_ptr: *mut T = std::mem::transmute(buf_ptr);
            std::slice::from_raw_parts_mut(buf_ptr, slice.len as usize)
        };
        Ok(Self {
            slice,
            buf: SliceCow::Borrowed(buf),
        })
    }
}

impl<'a, T> WasmRefAccess<'a, T>
where
    T: wasmer_types::ValueType,
{
    pub(crate) fn new(ptr: WasmRef<'a, T>) -> Result<Self, MemoryAccessError> {
        let total_len = mem::size_of::<T>() as u64;
        let end = ptr
            .offset
            .checked_add(total_len)
            .ok_or(MemoryAccessError::Overflow)?;
        if end > ptr.buffer.0.len as u64 {
            #[cfg(feature = "tracing")]
            warn!(
                "attempted to read ({} bytes) beyond the bounds of the memory view ({} > {})",
                total_len, end, ptr.buffer.len
            );
            return Err(MemoryAccessError::HeapOutOfBounds);
        }
        let val = unsafe {
            let val_ptr: *mut u8 = ptr.buffer.0.base.add(ptr.offset as usize);
            let val_ptr: *mut T = std::mem::transmute(val_ptr);
            &mut *val_ptr
        };
        Ok(Self {
            ptr,
            buf: RefCow::Borrowed(val),
        })
    }
}
