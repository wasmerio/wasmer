//! This is a wrapper around the `WasmPtr` abstraction that returns __WASI_EFAULT
//! if memory access failed

use crate::syscalls::types::{__wasi_errno_t, __WASI_EFAULT};
use std::{cell::Cell, fmt};
pub use wasmer_runtime_core::memory::ptr::Array;
use wasmer_runtime_core::{
    memory::{ptr, Memory},
    types::{ValueType, WasmExternType},
};

#[repr(transparent)]
pub struct WasmPtr<T: Copy, Ty = ptr::Item>(ptr::WasmPtr<T, Ty>);

unsafe impl<T: Copy, Ty> ValueType for WasmPtr<T, Ty> {}
impl<T: Copy, Ty> Copy for WasmPtr<T, Ty> {}

impl<T: Copy, Ty> Clone for WasmPtr<T, Ty> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Copy, Ty> fmt::Debug for WasmPtr<T, Ty> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

unsafe impl<T: Copy, Ty> WasmExternType for WasmPtr<T, Ty> {
    type Native = <ptr::WasmPtr<T, Ty> as WasmExternType>::Native;

    fn to_native(self) -> Self::Native {
        self.0.to_native()
    }
    fn from_native(n: Self::Native) -> Self {
        Self(ptr::WasmPtr::from_native(n))
    }
}

impl<T: Copy, Ty> PartialEq for WasmPtr<T, Ty> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<T: Copy, Ty> Eq for WasmPtr<T, Ty> {}

impl<T: Copy, Ty> WasmPtr<T, Ty> {
    #[inline(always)]
    pub fn new(offset: u32) -> Self {
        Self(ptr::WasmPtr::new(offset))
    }

    #[inline(always)]
    pub fn offset(self) -> u32 {
        self.0.offset()
    }
}

impl<T: Copy + ValueType> WasmPtr<T, ptr::Item> {
    #[inline(always)]
    pub fn deref<'a>(self, memory: &'a Memory) -> Result<&'a Cell<T>, __wasi_errno_t> {
        self.0.deref(memory).ok_or(__WASI_EFAULT)
    }
}

impl<T: Copy + ValueType> WasmPtr<T, ptr::Array> {
    #[inline(always)]
    pub fn deref<'a>(
        self,
        memory: &'a Memory,
        index: u32,
        length: u32,
    ) -> Result<&'a [Cell<T>], __wasi_errno_t> {
        self.0.deref(memory, index, length).ok_or(__WASI_EFAULT)
    }

    #[inline(always)]
    pub fn get_utf8_string<'a>(self, memory: &'a Memory, str_len: u32) -> Option<&'a str> {
        self.0.get_utf8_string(memory, str_len)
    }
}
