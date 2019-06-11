//! A reusable pointer abstraction for getting memory from the guest's memory.
//!
//! This abstraction is safe: it ensures the memory is in bounds and that the pointer
//! is aligned (avoiding undefined behavior).
//!
//! Therefore, you should use this abstraction whenever possible to avoid memory
//! related bugs when implementing an ABI.

use crate::{
    memory::Memory,
    types::{ValueType, WasmExternType},
};
use std::{cell::Cell, fmt, marker::PhantomData, mem};

pub struct Array;
pub struct Item;

#[repr(transparent)]
pub struct WasmPtr<T: Copy, Ty = Item> {
    offset: u32,
    _phantom: PhantomData<(T, Ty)>,
}

impl<T: Copy, Ty> WasmPtr<T, Ty> {
    #[inline]
    pub fn new(offset: u32) -> Self {
        Self {
            offset,
            _phantom: PhantomData,
        }
    }

    #[inline]
    pub fn offset(self) -> u32 {
        self.offset
    }
}

#[inline(always)]
fn align_pointer(ptr: usize, align: usize) -> usize {
    // clears bits below aligment amount (assumes power of 2) to align pointer
    debug_assert!(align.count_ones() == 1);
    ptr & !(align - 1)
}

impl<T: Copy + ValueType> WasmPtr<T, Item> {
    #[inline]
    pub fn deref<'a>(self, memory: &'a Memory) -> Option<&'a Cell<T>> {
        if (self.offset as usize) + mem::size_of::<T>() >= memory.size().bytes().0 {
            return None;
        }
        unsafe {
            let cell_ptr = align_pointer(
                memory.view::<u8>().as_ptr().add(self.offset as usize) as usize,
                mem::align_of::<T>(),
            ) as *const Cell<T>;
            Some(&*cell_ptr)
        }
    }
}

impl<T: Copy + ValueType> WasmPtr<T, Array> {
    #[inline]
    pub fn deref<'a>(self, memory: &'a Memory, index: u32, length: u32) -> Option<&'a [Cell<T>]> {
        // gets the size of the item in the array with padding added such that
        // for any index, we will always result an aligned memory access
        let item_size = mem::size_of::<T>() + (mem::size_of::<T>() % mem::align_of::<T>());
        let slice_full_len = index as usize + length as usize;

        if (self.offset as usize) + (item_size * slice_full_len) >= memory.size().bytes().0 {
            return None;
        }

        unsafe {
            let cell_ptr = align_pointer(
                memory.view::<u8>().as_ptr().add(self.offset as usize) as usize,
                mem::align_of::<T>(),
            ) as *const Cell<T>;
            let cell_ptrs = &std::slice::from_raw_parts(cell_ptr, slice_full_len)
                [index as usize..slice_full_len];
            Some(cell_ptrs)
        }
    }
}

unsafe impl<T: Copy, Ty> WasmExternType for WasmPtr<T, Ty> {
    type Native = i32;

    fn to_native(self) -> Self::Native {
        self.offset as i32
    }
    fn from_native(n: Self::Native) -> Self {
        Self {
            offset: n as u32,
            _phantom: PhantomData,
        }
    }
}

unsafe impl<T: Copy, Ty> ValueType for WasmPtr<T, Ty> {}

impl<T: Copy, Ty> Clone for WasmPtr<T, Ty> {
    fn clone(&self) -> Self {
        Self {
            offset: self.offset,
            _phantom: PhantomData,
        }
    }
}

impl<T: Copy, Ty> Copy for WasmPtr<T, Ty> {}

impl<T: Copy, Ty> PartialEq for WasmPtr<T, Ty> {
    fn eq(&self, other: &Self) -> bool {
        self.offset == other.offset
    }
}

impl<T: Copy, Ty> Eq for WasmPtr<T, Ty> {}

impl<T: Copy, Ty> fmt::Debug for WasmPtr<T, Ty> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "WasmPtr({:#x})", self.offset)
    }
}
