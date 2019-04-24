use crate::syscalls::types::{__wasi_errno_t, __WASI_EFAULT};
use std::{cell::Cell, fmt, marker::PhantomData, mem};
use wasmer_runtime_core::{
    memory::Memory,
    types::{ValueType, WasmExternType},
};

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

impl<T: Copy + ValueType> WasmPtr<T, Item> {
    #[inline]
    pub fn deref<'a>(self, memory: &'a Memory) -> Result<&'a Cell<T>, __wasi_errno_t> {
        if (self.offset as usize) + mem::size_of::<T>() >= memory.size().bytes().0 {
            return Err(__WASI_EFAULT);
        }
        unsafe {
            // clears bits below aligment amount (assumes power of 2) to align pointer
            let aligner = |ptr: usize, align: usize| ptr & !(align - 1);
            let cell_ptr = aligner(
                memory.view::<u8>().as_ptr().add(self.offset as usize) as usize,
                mem::align_of::<T>(),
            ) as *const Cell<T>;
            Ok(&*cell_ptr)
        }
    }
}

impl<T: Copy + ValueType> WasmPtr<T, Array> {
    #[inline]
    pub fn deref<'a>(
        self,
        memory: &'a Memory,
        index: u32,
        length: u32,
    ) -> Result<&'a [Cell<T>], __wasi_errno_t> {
        if (self.offset as usize) + (mem::size_of::<T>() * ((index + length) as usize))
            >= memory.size().bytes().0
        {
            return Err(__WASI_EFAULT);
        }

        unsafe {
            let cell_ptrs = memory.view::<T>().get_unchecked(
                ((self.offset as usize) / mem::size_of::<T>()) + (index as usize)
                    ..((self.offset() as usize) / mem::size_of::<T>())
                        + ((index + length) as usize),
            ) as *const _;
            Ok(&*cell_ptrs)
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
