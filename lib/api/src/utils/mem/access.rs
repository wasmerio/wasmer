use std::mem::{self, MaybeUninit};
use std::slice;

use crate::{
    utils::mem::{WasmRef, WasmSlice},
    MemoryAccessError,
};

pub(crate) enum SliceCow<'a, T> {
    #[allow(dead_code)]
    Borrowed(&'a mut [T]),
    #[allow(dead_code)]
    Owned(Vec<T>, bool),
}

impl<'a, T> AsRef<[T]> for SliceCow<'a, T> {
    fn as_ref(&self) -> &[T] {
        match self {
            Self::Borrowed(buf) => buf,
            Self::Owned(buf, _) => buf,
        }
    }
}

impl<'a, T> AsMut<[T]> for SliceCow<'a, T> {
    fn as_mut(&mut self) -> &mut [T] {
        // Note: Zero padding is not required here as its a typed copy which does
        //       not leak the bytes into the memory
        // https://stackoverflow.com/questions/61114026/does-stdptrwrite-transfer-the-uninitialized-ness-of-the-bytes-it-writes
        match self {
            Self::Borrowed(buf) => buf,
            Self::Owned(buf, modified) => {
                *modified = true;
                buf.as_mut()
            }
        }
    }
}

/// Provides direct memory access to a piece of memory that
/// is owned by WASM
pub struct WasmSliceAccess<'a, T>
where
    T: wasmer_types::ValueType,
{
    pub(crate) slice: WasmSlice<'a, T>,
    pub(crate) buf: SliceCow<'a, T>,
}

impl<'a, T> AsRef<[T]> for WasmSliceAccess<'a, T>
where
    T: wasmer_types::ValueType,
{
    fn as_ref(&self) -> &[T] {
        self.buf.as_ref()
    }
}

impl<'a, T> AsMut<[T]> for WasmSliceAccess<'a, T>
where
    T: wasmer_types::ValueType,
{
    fn as_mut(&mut self) -> &mut [T] {
        self.buf.as_mut()
    }
}

impl<'a, T> WasmSliceAccess<'a, T>
where
    T: wasmer_types::ValueType,
{
    /// Returns an iterator of all the elements in the slice
    pub fn iter(&'a self) -> std::slice::Iter<'a, T> {
        self.as_ref().iter()
    }

    /// Returns an iterator of all the elements in the slice
    pub fn iter_mut(&'a mut self) -> std::slice::IterMut<'a, T> {
        self.buf.as_mut().iter_mut()
    }

    /// Number of elements in this slice
    pub fn len(&self) -> usize {
        self.buf.as_ref().len()
    }

    /// If the slice is empty
    pub fn is_empty(&self) -> bool {
        self.buf.as_ref().is_empty()
    }
}

impl<'a> WasmSliceAccess<'a, u8> {
    /// Writes to the address pointed to by this `WasmPtr` in a memory.
    #[inline]
    pub fn copy_from_slice(&mut self, src: &[u8]) {
        let dst = self.buf.as_mut();
        dst.copy_from_slice(src);
    }

    /// Writes to the address pointed to by this `WasmPtr` in a memory.
    ///
    /// If the source buffer is smaller than the destination buffer
    /// only the correct amount of bytes will be copied
    ///
    /// Returns the number of bytes copied
    #[inline]
    pub fn copy_from_slice_min(&mut self, src: &[u8]) -> usize {
        let dst = self.buf.as_mut();
        let amt = dst.len().min(src.len());
        dst[..amt].copy_from_slice(&src[..amt]);
        amt
    }
}

impl<'a, T> Drop for WasmSliceAccess<'a, T>
where
    T: wasmer_types::ValueType,
{
    fn drop(&mut self) {
        if let SliceCow::Owned(buf, modified) = &self.buf {
            if *modified {
                self.slice.write_slice(buf.as_ref()).ok();
            }
        }
    }
}

pub(crate) enum RefCow<'a, T> {
    #[allow(dead_code)]
    Borrowed(&'a mut T),
    #[allow(dead_code)]
    Owned(T, bool),
}

impl<'a, T> AsRef<T> for RefCow<'a, T> {
    fn as_ref(&self) -> &T {
        match self {
            Self::Borrowed(val) => val,
            Self::Owned(val, _) => val,
        }
    }
}

impl<'a, T> AsMut<T> for RefCow<'a, T> {
    fn as_mut(&mut self) -> &mut T {
        // Note: Zero padding is not required here as its a typed copy which does
        //       not leak the bytes into the memory
        // https://stackoverflow.com/questions/61114026/does-stdptrwrite-transfer-the-uninitialized-ness-of-the-bytes-it-writes
        match self {
            Self::Borrowed(val) => val,
            Self::Owned(val, modified) => {
                *modified = true;
                val
            }
        }
    }
}

/// Provides direct memory access to a piece of memory that
/// is owned by WASM
pub struct WasmRefAccess<'a, T>
where
    T: wasmer_types::ValueType,
{
    pub(crate) is_owned: bool,
    pub(crate) ptr: WasmRef<'a, T>,
    pub(crate) buf: RefCow<'a, T>,
}

impl<'a, T> AsRef<T> for WasmRefAccess<'a, T>
where
    T: wasmer_types::ValueType,
{
    fn as_ref(&self) -> &T {
        self.buf.as_ref()
    }
}

impl<'a, T> AsMut<T> for WasmRefAccess<'a, T>
where
    T: wasmer_types::ValueType,
{
    fn as_mut(&mut self) -> &mut T {
        self.buf.as_mut()
    }
}

impl<'a, T> Drop for WasmRefAccess<'a, T>
where
    T: wasmer_types::ValueType,
{
    fn drop(&mut self) {
        if let RefCow::Owned(val, modified) = &self.buf {
            if *modified {
                self.ptr.write(*val).ok();
            }
        }
    }
}

impl<'a, T> WasmSliceAccess<'a, T>
where
    T: wasmer_types::ValueType,
{
    /// Returns a mutable slice that is not yet initialized
    pub fn as_mut_uninit(&mut self) -> &mut [MaybeUninit<T>] {
        let ret: &mut [T] = self.buf.as_mut();
        let ret: &mut [MaybeUninit<T>] = unsafe { std::mem::transmute(ret) };
        ret
    }
}

impl<'a, T> WasmRefAccess<'a, T>
where
    T: wasmer_types::ValueType,
{
    /// Returns a reference to an unitialized reference to this value
    pub fn as_mut_uninit(&mut self) -> &mut MaybeUninit<T> {
        let ret: &mut T = self.buf.as_mut();
        let ret: &mut MaybeUninit<T> = unsafe { std::mem::transmute(ret) };
        ret
    }
}

impl<'a, T> WasmSliceAccess<'a, T>
where
    T: wasmer_types::ValueType,
{
    pub(crate) fn new(slice: WasmSlice<'a, T>, is_owned: bool) -> Result<Self, MemoryAccessError> {
        if is_owned {
            Self::new_owned(slice)
        } else {
            Self::new_borrowed(slice)
        }
    }

    pub(crate) fn new_borrowed(slice: WasmSlice<'a, T>) -> Result<Self, MemoryAccessError> {
        let total_len = slice
            .len
            .checked_mul(std::mem::size_of::<T>() as u64)
            .ok_or(MemoryAccessError::Overflow)?;
        let end = slice
            .offset
            .checked_add(total_len)
            .ok_or(MemoryAccessError::Overflow)?;
        if end > slice.buffer.len() as u64 {
            tracing::warn!(
                "attempted to read ({} bytes) beyond the bounds of the memory view ({} > {})",
                total_len,
                end,
                slice.buffer.len()
            );
            return Err(MemoryAccessError::HeapOutOfBounds);
        }
        let buf = unsafe {
            let buf_ptr: *mut u8 = slice.buffer.base().add(slice.offset as usize);
            let buf_ptr: *mut T = std::mem::transmute(buf_ptr);
            if !buf_ptr.is_aligned() {
                return Err(MemoryAccessError::UnalignedPointerRead);
            }
            std::slice::from_raw_parts_mut(buf_ptr, slice.len as usize)
        };
        Ok(Self {
            slice,
            buf: SliceCow::Borrowed(buf),
        })
    }

    pub(crate) fn new_owned(slice: WasmSlice<'a, T>) -> Result<Self, MemoryAccessError> {
        let buf = slice.read_to_vec()?;
        Ok(Self {
            slice,
            buf: SliceCow::Owned(buf, false),
        })
    }
}

impl<'a, T> WasmRefAccess<'a, T>
where
    T: wasmer_types::ValueType,
{
    pub(crate) fn new(ptr: WasmRef<'a, T>, is_owned: bool) -> Result<Self, MemoryAccessError> {
        if is_owned {
            Self::new_owned(ptr)
        } else {
            Self::new_borrowed(ptr)
        }
    }

    pub(crate) fn new_borrowed(ptr: WasmRef<'a, T>) -> Result<Self, MemoryAccessError> {
        let total_len = std::mem::size_of::<T>() as u64;
        let end = ptr
            .offset
            .checked_add(total_len)
            .ok_or(MemoryAccessError::Overflow)?;
        if end > ptr.buffer.len() as u64 {
            tracing::warn!(
                "attempted to read ({} bytes) beyond the bounds of the memory view ({} > {})",
                total_len,
                end,
                ptr.buffer.len()
            );
            return Err(MemoryAccessError::HeapOutOfBounds);
        }
        let val = unsafe {
            let val_ptr: *mut u8 = ptr.buffer.base().add(ptr.offset as usize);
            let val_ptr: *mut T = std::mem::transmute(val_ptr);
            &mut *val_ptr
        };
        Ok(Self {
            is_owned: false,
            ptr,
            buf: RefCow::Borrowed(val),
        })
    }

    pub(crate) fn new_owned(ptr: WasmRef<'a, T>) -> Result<Self, MemoryAccessError> {
        let mut out = MaybeUninit::uninit();
        let buf =
            unsafe { slice::from_raw_parts_mut(out.as_mut_ptr() as *mut u8, mem::size_of::<T>()) };
        ptr.buffer.read(ptr.offset, buf)?;
        let val = unsafe { out.assume_init() };

        Ok(Self {
            is_owned: true,
            ptr,
            buf: RefCow::Owned(val, false),
        })
    }
}

impl<'a, T> WasmRefAccess<'a, T>
where
    T: wasmer_types::ValueType,
{
    /// Reads the address pointed to by this `WasmPtr` in a memory.
    #[inline]
    #[allow(clippy::clone_on_copy)]
    pub fn read(&self) -> T
    where
        T: Clone,
    {
        self.as_ref().clone()
    }

    /// Writes to the address pointed to by this `WasmPtr` in a memory.
    #[inline]
    pub fn write(&mut self, val: T) {
        if self.is_owned {
            let mut data = MaybeUninit::new(val);
            let data = unsafe {
                slice::from_raw_parts_mut(
                    data.as_mut_ptr() as *mut MaybeUninit<u8>,
                    mem::size_of::<T>(),
                )
            };
            val.zero_padding_bytes(data);
            let data = unsafe { slice::from_raw_parts(data.as_ptr() as *const _, data.len()) };
            self.ptr.buffer.write(self.ptr.offset, data).unwrap()
        } else {
            // Note: Zero padding is not required here as its a typed copy which does
            //       not leak the bytes into the memory
            // https://stackoverflow.com/questions/61114026/does-stdptrwrite-transfer-the-uninitialized-ness-of-the-bytes-it-writes
            *(self.as_mut()) = val;
        }
    }
}
