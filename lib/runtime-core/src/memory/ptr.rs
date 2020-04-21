//! Types for a reusable pointer abstraction for accessing Wasm linear memory.
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

/// The `Array` marker type. This type can be used like `WasmPtr<T, Array>`
/// to get access to methods
pub struct Array;
/// The `Item` marker type. This is the default and does not usually need to be
/// specified.
pub struct Item;

/// A zero-cost type that represents a pointer to something in Wasm linear
/// memory.
///
/// This type can be used directly in the host function arguments:
/// ```
/// # use wasmer_runtime_core::vm::Ctx;
/// # use wasmer_runtime_core::memory::ptr::WasmPtr;
/// pub fn host_import(ctx: &mut Ctx, ptr: WasmPtr<u32>) {
///     let memory = ctx.memory(0);
///     let derefed_ptr = ptr.deref(memory).expect("pointer in bounds");
///     let inner_val: u32 = derefed_ptr.get();
///     println!("Got {} from Wasm memory address 0x{:X}", inner_val, ptr.offset());
///     // update the value being pointed to
///     derefed_ptr.set(inner_val + 1);
/// }
/// ```
#[repr(transparent)]
pub struct WasmPtr<T: Copy, Ty = Item> {
    offset: u32,
    _phantom: PhantomData<(T, Ty)>,
}

/// Methods relevant to all types of `WasmPtr`.
impl<T: Copy, Ty> WasmPtr<T, Ty> {
    /// Create a new `WasmPtr` at the given offset.
    #[inline]
    pub fn new(offset: u32) -> Self {
        Self {
            offset,
            _phantom: PhantomData,
        }
    }

    /// Get the offset into Wasm linear memory for this `WasmPtr`.
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

/// Methods for `WasmPtr`s to data that can be dereferenced, namely to types
/// that implement [`ValueType`], meaning that they're valid for all possible
/// bit patterns.
impl<T: Copy + ValueType> WasmPtr<T, Item> {
    /// Dereference the `WasmPtr` getting access to a `&Cell<T>` allowing for
    /// reading and mutating of the inner value.
    ///
    /// This method is unsound if used with unsynchronized shared memory.
    /// If you're unsure what that means, it likely does not apply to you.
    /// This invariant will be enforced in the future.
    #[inline]
    pub fn deref<'a>(self, memory: &'a Memory) -> Option<&'a Cell<T>> {
        if (self.offset as usize) + mem::size_of::<T>() > memory.size().bytes().0
            || mem::size_of::<T>() == 0
        {
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

    /// Mutably dereference this `WasmPtr` getting a `&mut Cell<T>` allowing for
    /// direct access to a `&mut T`.
    ///
    /// # Safety
    /// - This method does not do any aliasing checks: it's possible to create
    ///  `&mut T` that point to the same memory. You should ensure that you have
    ///   exclusive access to Wasm linear memory before calling this method.
    #[inline]
    pub unsafe fn deref_mut<'a>(self, memory: &'a Memory) -> Option<&'a mut Cell<T>> {
        if (self.offset as usize) + mem::size_of::<T>() > memory.size().bytes().0
            || mem::size_of::<T>() == 0
        {
            return None;
        }
        let cell_ptr = align_pointer(
            memory.view::<u8>().as_ptr().add(self.offset as usize) as usize,
            mem::align_of::<T>(),
        ) as *mut Cell<T>;
        Some(&mut *cell_ptr)
    }
}

/// Methods for `WasmPtr`s to arrays of data that can be dereferenced, namely to
/// types that implement [`ValueType`], meaning that they're valid for all
/// possible bit patterns.
impl<T: Copy + ValueType> WasmPtr<T, Array> {
    /// Dereference the `WasmPtr` getting access to a `&[Cell<T>]` allowing for
    /// reading and mutating of the inner values.
    ///
    /// This method is unsound if used with unsynchronized shared memory.
    /// If you're unsure what that means, it likely does not apply to you.
    /// This invariant will be enforced in the future.
    #[inline]
    pub fn deref(self, memory: &Memory, index: u32, length: u32) -> Option<&[Cell<T>]> {
        // gets the size of the item in the array with padding added such that
        // for any index, we will always result an aligned memory access
        let item_size = mem::size_of::<T>() + (mem::size_of::<T>() % mem::align_of::<T>());
        let slice_full_len = index as usize + length as usize;
        let memory_size = memory.size().bytes().0;

        if (self.offset as usize) + (item_size * slice_full_len) > memory_size
            || self.offset as usize >= memory_size
            || mem::size_of::<T>() == 0
        {
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

    /// Mutably dereference this `WasmPtr` getting a `&mut [Cell<T>]` allowing for
    /// direct access to a `&mut [T]`.
    ///
    /// # Safety
    /// - This method does not do any aliasing checks: it's possible to create
    ///  `&mut T` that point to the same memory. You should ensure that you have
    ///   exclusive access to Wasm linear memory before calling this method.
    #[inline]
    pub unsafe fn deref_mut(
        self,
        memory: &Memory,
        index: u32,
        length: u32,
    ) -> Option<&mut [Cell<T>]> {
        // gets the size of the item in the array with padding added such that
        // for any index, we will always result an aligned memory access
        let item_size = mem::size_of::<T>() + (mem::size_of::<T>() % mem::align_of::<T>());
        let slice_full_len = index as usize + length as usize;
        let memory_size = memory.size().bytes().0;

        if (self.offset as usize) + (item_size * slice_full_len) > memory.size().bytes().0
            || self.offset as usize >= memory_size
            || mem::size_of::<T>() == 0
        {
            return None;
        }

        let cell_ptr = align_pointer(
            memory.view::<u8>().as_ptr().add(self.offset as usize) as usize,
            mem::align_of::<T>(),
        ) as *mut Cell<T>;
        let cell_ptrs = &mut std::slice::from_raw_parts_mut(cell_ptr, slice_full_len)
            [index as usize..slice_full_len];
        Some(cell_ptrs)
    }

    /// Get a UTF-8 string from the `WasmPtr` with the given length.
    ///
    /// Note that this method returns a reference to Wasm linear memory. The
    /// underlying data can be mutated if the Wasm is allowed to execute or
    /// an aliasing `WasmPtr` is used to mutate memory.
    pub fn get_utf8_string(self, memory: &Memory, str_len: u32) -> Option<&str> {
        let memory_size = memory.size().bytes().0;

        if self.offset as usize + str_len as usize > memory.size().bytes().0
            || self.offset as usize >= memory_size
        {
            return None;
        }
        let ptr = unsafe { memory.view::<u8>().as_ptr().add(self.offset as usize) as *const u8 };
        let slice: &[u8] = unsafe { std::slice::from_raw_parts(ptr, str_len as usize) };
        std::str::from_utf8(slice).ok()
    }

    /// Get a UTF-8 string from the `WasmPtr`, where the string is nul-terminated.
    ///
    /// Note that this does not account for UTF-8 strings that _contain_ nul themselves,
    /// [`get_utf8_string`] has to be used for those.
    ///
    /// Also note that this method returns a reference to Wasm linear memory. The
    /// underlying data can be mutated if the Wasm is allowed to execute or
    /// an aliasing `WasmPtr` is used to mutate memory.
    pub fn get_utf8_string_with_nul(self, memory: &Memory) -> Option<&str> {
        memory.view::<u8>()[(self.offset as usize)..]
            .iter()
            .map(|cell| cell.get())
            .position(|byte| byte == 0)
            .and_then(|length| self.get_utf8_string(memory, length as u32))
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

#[cfg(test)]
mod test {
    use super::*;
    use crate::memory;
    use crate::types;
    use crate::units::Pages;

    /// Ensure that memory accesses work on the edges of memory and that out of
    /// bounds errors are caught with both `deref` and `deref_mut`.
    #[test]
    fn wasm_ptr_memory_bounds_checks_hold() {
        // create a memory
        let memory_descriptor = types::MemoryType::new(Pages(1), Some(Pages(1)), false).unwrap();
        let memory = memory::Memory::new(memory_descriptor).unwrap();

        // test that basic access works and that len = 0 works, but oob does not
        let start_wasm_ptr: WasmPtr<u8> = WasmPtr::new(0);
        let start_wasm_ptr_array: WasmPtr<u8, Array> = WasmPtr::new(0);

        assert!(start_wasm_ptr.deref(&memory).is_some());
        assert!(unsafe { start_wasm_ptr.deref_mut(&memory).is_some() });
        assert!(start_wasm_ptr_array.deref(&memory, 0, 0).is_some());
        assert!(start_wasm_ptr_array.get_utf8_string(&memory, 0).is_some());
        assert!(unsafe { start_wasm_ptr_array.deref_mut(&memory, 0, 0).is_some() });
        assert!(start_wasm_ptr_array.deref(&memory, 0, 1).is_some());
        assert!(unsafe { start_wasm_ptr_array.deref_mut(&memory, 0, 1).is_some() });

        // test that accessing the last valid memory address works correctly and OOB is caught
        let last_valid_address_for_u8 = (memory.size().bytes().0 - 1) as u32;
        let end_wasm_ptr: WasmPtr<u8> = WasmPtr::new(last_valid_address_for_u8);
        assert!(end_wasm_ptr.deref(&memory).is_some());
        assert!(unsafe { end_wasm_ptr.deref_mut(&memory).is_some() });

        let end_wasm_ptr_array: WasmPtr<u8, Array> = WasmPtr::new(last_valid_address_for_u8);

        assert!(end_wasm_ptr_array.deref(&memory, 0, 1).is_some());
        assert!(unsafe { end_wasm_ptr_array.deref_mut(&memory, 0, 1).is_some() });
        let invalid_idx_len_combos: [(u32, u32); 3] =
            [(last_valid_address_for_u8 + 1, 0), (0, 2), (1, 1)];
        for &(idx, len) in invalid_idx_len_combos.iter() {
            assert!(end_wasm_ptr_array.deref(&memory, idx, len).is_none());
            assert!(unsafe { end_wasm_ptr_array.deref_mut(&memory, idx, len).is_none() });
        }
        assert!(end_wasm_ptr_array.get_utf8_string(&memory, 2).is_none());

        // test that accesing the last valid memory address for a u32 is valid
        // (same as above test but with more edge cases to assert on)
        let last_valid_address_for_u32 = (memory.size().bytes().0 - 4) as u32;
        let end_wasm_ptr: WasmPtr<u32> = WasmPtr::new(last_valid_address_for_u32);
        assert!(end_wasm_ptr.deref(&memory).is_some());
        assert!(unsafe { end_wasm_ptr.deref_mut(&memory).is_some() });
        assert!(end_wasm_ptr.deref(&memory).is_some());
        assert!(unsafe { end_wasm_ptr.deref_mut(&memory).is_some() });

        let end_wasm_ptr_oob_array: [WasmPtr<u32>; 4] = [
            WasmPtr::new(last_valid_address_for_u32 + 1),
            WasmPtr::new(last_valid_address_for_u32 + 2),
            WasmPtr::new(last_valid_address_for_u32 + 3),
            WasmPtr::new(last_valid_address_for_u32 + 4),
        ];
        for oob_end_ptr in end_wasm_ptr_oob_array.iter() {
            assert!(oob_end_ptr.deref(&memory).is_none());
            assert!(unsafe { oob_end_ptr.deref_mut(&memory).is_none() });
        }
        let end_wasm_ptr_array: WasmPtr<u32, Array> = WasmPtr::new(last_valid_address_for_u32);
        assert!(end_wasm_ptr_array.deref(&memory, 0, 1).is_some());
        assert!(unsafe { end_wasm_ptr_array.deref_mut(&memory, 0, 1).is_some() });

        let invalid_idx_len_combos: [(u32, u32); 3] =
            [(last_valid_address_for_u32 + 1, 0), (0, 2), (1, 1)];
        for &(idx, len) in invalid_idx_len_combos.iter() {
            assert!(end_wasm_ptr_array.deref(&memory, idx, len).is_none());
            assert!(unsafe { end_wasm_ptr_array.deref_mut(&memory, idx, len).is_none() });
        }

        let end_wasm_ptr_array_oob_array: [WasmPtr<u32, Array>; 4] = [
            WasmPtr::new(last_valid_address_for_u32 + 1),
            WasmPtr::new(last_valid_address_for_u32 + 2),
            WasmPtr::new(last_valid_address_for_u32 + 3),
            WasmPtr::new(last_valid_address_for_u32 + 4),
        ];

        for oob_end_array_ptr in end_wasm_ptr_array_oob_array.iter() {
            assert!(oob_end_array_ptr.deref(&memory, 0, 1).is_none());
            assert!(unsafe { oob_end_array_ptr.deref_mut(&memory, 0, 1).is_none() });
            assert!(oob_end_array_ptr.deref(&memory, 1, 0).is_none());
            assert!(unsafe { oob_end_array_ptr.deref_mut(&memory, 1, 0).is_none() });
        }
    }
}
