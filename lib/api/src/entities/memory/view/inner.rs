use std::{mem::MaybeUninit, ops::Range};
use wasmer_types::Pages;

use crate::{
    buffer::{MemoryBuffer, RuntimeMemoryBuffer},
    AsStoreRef, Memory, MemoryAccessError,
};

/// A WebAssembly `memory` view.
///
/// A memory view is used to read and write to the linear memory.
///
/// After a memory is grown a view must not be used anymore. Views are
/// created using the Memory.view() method.
#[derive(Debug, derive_more::From)]
pub enum RuntimeMemoryView<'a> {
    #[cfg(feature = "sys")]
    /// The memory view for the `sys` runtime.
    Sys(crate::rt::sys::entities::memory::view::MemoryView<'a>),

    #[cfg(feature = "wamr")]
    /// The memory view for the `wamr` runtime.
    Wamr(crate::rt::wamr::entities::memory::view::MemoryView<'a>),

    #[cfg(feature = "v8")]
    /// The memory view for the `v8` runtime.
    V8(crate::rt::v8::entities::memory::view::MemoryView<'a>),

    #[doc(hidden)]
    Phantom(&'a ()),
}

impl<'a> RuntimeMemoryView<'a> {
    pub(crate) fn new(memory: &Memory, store: &'a (impl AsStoreRef + ?Sized)) -> Self {
        match &store.as_store_ref().inner.store {
            #[cfg(feature = "sys")]
            crate::RuntimeStore::Sys(s) => {
                return Self::Sys(crate::rt::sys::entities::memory::view::MemoryView::new(
                    memory.as_sys(),
                    store,
                ))
            }
            #[cfg(feature = "wamr")]
            crate::RuntimeStore::Wamr(s) => {
                return Self::Wamr(crate::rt::wamr::entities::memory::view::MemoryView::new(
                    memory.as_wamr(),
                    store,
                ))
            }
            #[cfg(feature = "v8")]
            crate::RuntimeStore::V8(s) => {
                return Self::V8(crate::rt::v8::entities::memory::view::MemoryView::new(
                    memory.as_v8(),
                    store,
                ))
            }
            _ => panic!("No runtime enabled!"),
        }
    }

    /// Returns the pointer to the raw bytes of the `Memory`.
    //
    // This used by wasmer-c-api, but should be treated
    // as deprecated and not used in future code.
    #[doc(hidden)]
    pub fn data_ptr(&self) -> *mut u8 {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.data_ptr(),
            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s.data_ptr(),
            #[cfg(feature = "v8")]
            Self::V8(s) => s.data_ptr(),
            _ => panic!("No runtime enabled!"),
        }
    }

    /// Returns the size (in bytes) of the `Memory`.
    pub fn data_size(&self) -> u64 {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.data_size(),

            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s.data_size(),

            #[cfg(feature = "v8")]
            Self::V8(s) => s.data_size(),
            _ => panic!("No runtime enabled!"),
        }
    }

    /// Retrieve a slice of the memory contents.
    ///
    /// # Safety
    ///
    /// Until the returned slice is dropped, it is undefined behaviour to
    /// modify the memory contents in any way including by calling a wasm
    /// function that writes to the memory or by resizing the memory.
    #[doc(hidden)]
    pub unsafe fn data_unchecked(&self) -> &[u8] {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.data_unchecked(),
            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s.data_unchecked(),
            #[cfg(feature = "v8")]
            Self::V8(s) => s.data_unchecked(),
            _ => panic!("No runtime enabled!"),
        }
    }

    /// Retrieve a mutable slice of the memory contents.
    ///
    /// # Safety
    ///
    /// This method provides interior mutability without an UnsafeCell. Until
    /// the returned value is dropped, it is undefined behaviour to read or
    /// write to the pointed-to memory in any way except through this slice,
    /// including by calling a wasm function that reads the memory contents or
    /// by resizing this Memory.
    #[allow(clippy::mut_from_ref)]
    #[doc(hidden)]
    pub unsafe fn data_unchecked_mut(&self) -> &mut [u8] {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.data_unchecked_mut(),

            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s.data_unchecked_mut(),

            #[cfg(feature = "v8")]
            Self::V8(s) => s.data_unchecked_mut(),
            _ => panic!("No runtime enabled!"),
        }
    }

    /// Returns the size (in [`Pages`]) of the `Memory`.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Memory, MemoryType, Pages, Store, Type, Value};
    /// # let mut store = Store::default();
    /// #
    /// let m = Memory::new(&mut store, MemoryType::new(1, None, false)).unwrap();
    ///
    /// assert_eq!(m.view(&mut store).size(), Pages(1));
    /// ```
    pub fn size(&self) -> Pages {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.size(),

            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s.size(),

            #[cfg(feature = "v8")]
            Self::V8(s) => s.size(),
            _ => panic!("No runtime enabled!"),
        }
    }

    #[inline]
    pub(crate) fn buffer(&'a self) -> MemoryBuffer<'a> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => MemoryBuffer(RuntimeMemoryBuffer::Sys(s.buffer())),
            #[cfg(feature = "wamr")]
            Self::Wamr(s) => MemoryBuffer(RuntimeMemoryBuffer::Wamr(s.buffer())),
            #[cfg(feature = "v8")]
            Self::V8(s) => MemoryBuffer(RuntimeMemoryBuffer::V8(s.buffer())),
            _ => panic!("No runtime enabled!"),
        }
    }

    /// Safely reads bytes from the memory at the given offset.
    ///
    /// The full buffer will be filled, otherwise a `MemoryAccessError` is returned
    /// to indicate an out-of-bounds access.
    ///
    /// This method is guaranteed to be safe (from the host side) in the face of
    /// concurrent writes.
    pub fn read(&self, offset: u64, buf: &mut [u8]) -> Result<(), MemoryAccessError> {
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

    /// Safely reads a single byte from memory at the given offset
    ///
    /// This method is guaranteed to be safe (from the host side) in the face of
    /// concurrent writes.
    pub fn read_u8(&self, offset: u64) -> Result<u8, MemoryAccessError> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.read_u8(offset),

            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s.read_u8(offset),

            #[cfg(feature = "v8")]
            Self::V8(s) => s.read_u8(offset),
            _ => panic!("No runtime enabled!"),
        }
    }

    /// Safely reads bytes from the memory at the given offset.
    ///
    /// This method is similar to `read` but allows reading into an
    /// uninitialized buffer. An initialized view of the buffer is returned.
    ///
    /// The full buffer will be filled, otherwise a `MemoryAccessError` is returned
    /// to indicate an out-of-bounds access.
    ///
    /// This method is guaranteed to be safe (from the host side) in the face of
    /// concurrent writes.
    pub fn read_uninit<'b>(
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

    /// Safely writes bytes to the memory at the given offset.
    ///
    /// If the write exceeds the bounds of the memory then a `MemoryAccessError` is
    /// returned.
    ///
    /// This method is guaranteed to be safe (from the host side) in the face of
    /// concurrent reads/writes.
    pub fn write(&self, offset: u64, data: &[u8]) -> Result<(), MemoryAccessError> {
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

    /// Safely writes a single byte from memory at the given offset
    ///
    /// This method is guaranteed to be safe (from the host side) in the face of
    /// concurrent writes.
    pub fn write_u8(&self, offset: u64, val: u8) -> Result<(), MemoryAccessError> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.write_u8(offset, val),

            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s.write_u8(offset, val),

            #[cfg(feature = "v8")]
            Self::V8(s) => s.write_u8(offset, val),
            _ => panic!("No runtime enabled!"),
        }
    }

    /// Copies the memory and returns it as a vector of bytes
    pub fn copy_to_vec(&self) -> Result<Vec<u8>, MemoryAccessError> {
        self.copy_range_to_vec(0..self.data_size())
    }

    /// Copies a range of the memory and returns it as a vector of bytes
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

    /// Copies the memory to another new memory object
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
