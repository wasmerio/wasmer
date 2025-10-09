use std::{mem::MaybeUninit, ops::Range};
use wasmer_types::Pages;

use crate::{AsStoreRef, Memory, MemoryAccessError, buffer::MemoryBuffer};

pub(crate) mod inner;
pub(crate) use inner::*;

/// A WebAssembly `memory` view.
///
/// A memory view is used to read and write to the linear memory.
///
/// After a memory is grown a view must not be used anymore. Views are
/// created using the Memory.view() method.
#[derive(Debug, derive_more::From)]
pub struct MemoryView<'a>(pub(crate) BackendMemoryView<'a>);

impl<'a> MemoryView<'a> {
    pub(crate) fn new(memory: &Memory, store: &'a (impl AsStoreRef + ?Sized)) -> Self {
        Self(BackendMemoryView::new(memory, store))
    }

    /// Returns the pointer to the raw bytes of the `Memory`.
    //
    // This used by wasmer-c-api, but should be treated
    // as deprecated and not used in future code.
    #[doc(hidden)]
    pub fn data_ptr(&self) -> *mut u8 {
        self.0.data_ptr()
    }

    /// Returns the size (in bytes) of the `Memory`.
    pub fn data_size(&self) -> u64 {
        self.0.data_size()
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
        unsafe { self.0.data_unchecked() }
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
        unsafe { self.0.data_unchecked_mut() }
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
        self.0.size()
    }

    #[inline]
    pub(crate) fn buffer(&'a self) -> MemoryBuffer<'a> {
        self.0.buffer()
    }

    /// Safely reads bytes from the memory at the given offset.
    ///
    /// The full buffer will be filled, otherwise a `MemoryAccessError` is returned
    /// to indicate an out-of-bounds access.
    ///
    /// This method is guaranteed to be safe (from the host side) in the face of
    /// concurrent writes.
    pub fn read(&self, offset: u64, buf: &mut [u8]) -> Result<(), MemoryAccessError> {
        self.0.read(offset, buf)
    }

    /// Safely reads a single byte from memory at the given offset
    ///
    /// This method is guaranteed to be safe (from the host side) in the face of
    /// concurrent writes.
    pub fn read_u8(&self, offset: u64) -> Result<u8, MemoryAccessError> {
        self.0.read_u8(offset)
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
        self.0.read_uninit(offset, buf)
    }

    /// Safely writes bytes to the memory at the given offset.
    ///
    /// If the write exceeds the bounds of the memory then a `MemoryAccessError` is
    /// returned.
    ///
    /// This method is guaranteed to be safe (from the host side) in the face of
    /// concurrent reads/writes.
    pub fn write(&self, offset: u64, data: &[u8]) -> Result<(), MemoryAccessError> {
        self.0.write(offset, data)
    }

    /// Safely writes a single byte from memory at the given offset
    ///
    /// This method is guaranteed to be safe (from the host side) in the face of
    /// concurrent writes.
    pub fn write_u8(&self, offset: u64, val: u8) -> Result<(), MemoryAccessError> {
        self.0.write_u8(offset, val)
    }

    /// Copies the memory and returns it as a vector of bytes
    pub fn copy_to_vec(&self) -> Result<Vec<u8>, MemoryAccessError> {
        self.0.copy_to_vec()
    }

    /// Copies a range of the memory and returns it as a vector of bytes
    pub fn copy_range_to_vec(&self, range: Range<u64>) -> Result<Vec<u8>, MemoryAccessError> {
        self.0.copy_range_to_vec(range)
    }

    /// Copies the memory to another new memory object
    pub fn copy_to_memory(&self, amount: u64, new_memory: &Self) -> Result<(), MemoryAccessError> {
        self.0.copy_to_memory(amount, &new_memory.0)
    }
}
