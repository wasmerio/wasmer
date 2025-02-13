use std::{mem::MaybeUninit, ops::Range};
use wasmer_types::Pages;

use crate::{
    buffer::{BackendMemoryBuffer, MemoryBuffer},
    macros::backend::{gen_rt_ty, match_rt},
    AsStoreRef, Memory, MemoryAccessError,
};

/// A WebAssembly `memory` view.
///
/// A memory view is used to read and write to the linear memory.
///
/// After a memory is grown a view must not be used anymore. Views are
/// created using the Memory.view() method.
gen_rt_ty!(MemoryView<'a> @derives Debug, derive_more::From ; @path memory::view);

impl<'a> BackendMemoryView<'a> {
    #[inline]
    pub(crate) fn new(memory: &Memory, store: &'a (impl AsStoreRef + ?Sized)) -> Self {
        match &store.as_store_ref().inner.store {
            #[cfg(feature = "sys")]
            crate::BackendStore::Sys(s) => {
                return Self::Sys(
                    crate::backend::sys::entities::memory::view::MemoryView::new(
                        memory.as_sys(),
                        store,
                    ),
                )
            }
            #[cfg(feature = "wamr")]
            crate::BackendStore::Wamr(s) => {
                return Self::Wamr(
                    crate::backend::wamr::entities::memory::view::MemoryView::new(
                        memory.as_wamr(),
                        store,
                    ),
                )
            }
            #[cfg(feature = "wasmi")]
            crate::BackendStore::Wasmi(s) => {
                return Self::Wasmi(
                    crate::backend::wasmi::entities::memory::view::MemoryView::new(
                        memory.as_wasmi(),
                        store,
                    ),
                )
            }
            #[cfg(feature = "v8")]
            crate::BackendStore::V8(s) => {
                return Self::V8(crate::backend::v8::entities::memory::view::MemoryView::new(
                    memory.as_v8(),
                    store,
                ))
            }
            #[cfg(feature = "js")]
            crate::BackendStore::Js(s) => {
                return Self::Js(crate::backend::js::entities::memory::view::MemoryView::new(
                    memory.as_js(),
                    store,
                ))
            }
            #[cfg(feature = "jsc")]
            crate::BackendStore::Jsc(s) => {
                return Self::Jsc(
                    crate::backend::jsc::entities::memory::view::MemoryView::new(
                        memory.as_jsc(),
                        store,
                    ),
                )
            }
        }
    }

    /// Returns the pointer to the raw bytes of the `Memory`.
    //
    // This used by wasmer-c-api, but should be treated
    // as deprecated and not used in future code.
    #[doc(hidden)]
    #[inline]
    pub fn data_ptr(&self) -> *mut u8 {
        match_rt!(on self => s {
            s.data_ptr()
        })
    }

    /// Returns the size (in bytes) of the `Memory`.
    #[inline]
    pub fn data_size(&self) -> u64 {
        match_rt!(on self => s {
            s.data_size()
        })
    }

    /// Retrieve a slice of the memory contents.
    ///
    /// # Safety
    ///
    /// Until the returned slice is dropped, it is undefined behaviour to
    /// modify the memory contents in any way including by calling a wasm
    /// function that writes to the memory or by resizing the memory.
    #[doc(hidden)]
    #[inline]
    pub unsafe fn data_unchecked(&self) -> &[u8] {
        match_rt!(on self => s {
            s.data_unchecked()
        })
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
    #[inline]
    pub unsafe fn data_unchecked_mut(&self) -> &mut [u8] {
        match_rt!(on self => s {
            s.data_unchecked_mut()
        })
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
    #[inline]
    pub fn size(&self) -> Pages {
        match_rt!(on self => s {
            s.size()
        })
    }

    #[inline]
    pub(crate) fn buffer(&'a self) -> MemoryBuffer<'a> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => MemoryBuffer(BackendMemoryBuffer::Sys(s.buffer())),
            #[cfg(feature = "wamr")]
            Self::Wamr(s) => MemoryBuffer(BackendMemoryBuffer::Wamr(s.buffer())),
            #[cfg(feature = "wasmi")]
            Self::Wasmi(s) => MemoryBuffer(BackendMemoryBuffer::Wasmi(s.buffer())),
            #[cfg(feature = "v8")]
            Self::V8(s) => MemoryBuffer(BackendMemoryBuffer::V8(s.buffer())),
            #[cfg(feature = "js")]
            Self::Js(s) => MemoryBuffer(BackendMemoryBuffer::Js(s.buffer())),
            #[cfg(feature = "jsc")]
            Self::Jsc(s) => MemoryBuffer(BackendMemoryBuffer::Jsc(s.buffer())),
        }
    }

    /// Safely reads bytes from the memory at the given offset.
    ///
    /// The full buffer will be filled, otherwise a `MemoryAccessError` is returned
    /// to indicate an out-of-bounds access.
    ///
    /// This method is guaranteed to be safe (from the host side) in the face of
    /// concurrent writes.
    #[inline]
    pub fn read(&self, offset: u64, buf: &mut [u8]) -> Result<(), MemoryAccessError> {
        match_rt!(on self => s {
            s.read(offset, buf)
        })
    }

    /// Safely reads a single byte from memory at the given offset
    ///
    /// This method is guaranteed to be safe (from the host side) in the face of
    /// concurrent writes.
    #[inline]
    pub fn read_u8(&self, offset: u64) -> Result<u8, MemoryAccessError> {
        match_rt!(on self => s {
            s.read_u8(offset)
        })
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
    #[inline]
    pub fn read_uninit<'b>(
        &self,
        offset: u64,
        buf: &'b mut [MaybeUninit<u8>],
    ) -> Result<&'b mut [u8], MemoryAccessError> {
        match_rt!(on self => s {
            s.read_uninit(offset, buf)
        })
    }

    /// Safely writes bytes to the memory at the given offset.
    ///
    /// If the write exceeds the bounds of the memory then a `MemoryAccessError` is
    /// returned.
    ///
    /// This method is guaranteed to be safe (from the host side) in the face of
    /// concurrent reads/writes.
    #[inline]
    pub fn write(&self, offset: u64, data: &[u8]) -> Result<(), MemoryAccessError> {
        match_rt!(on self => s {
            s.write(offset, data)
        })
    }

    /// Safely writes a single byte from memory at the given offset
    ///
    /// This method is guaranteed to be safe (from the host side) in the face of
    /// concurrent writes.
    #[inline]
    pub fn write_u8(&self, offset: u64, val: u8) -> Result<(), MemoryAccessError> {
        match_rt!(on self => s {
            s.write_u8(offset, val)
        })
    }

    /// Copies the memory and returns it as a vector of bytes
    #[inline]
    pub fn copy_to_vec(&self) -> Result<Vec<u8>, MemoryAccessError> {
        self.copy_range_to_vec(0..self.data_size())
    }

    /// Copies a range of the memory and returns it as a vector of bytes
    #[inline]
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
    #[inline]
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
