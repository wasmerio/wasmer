//! This is a wrapper around the `WasmPtr` abstraction that does not allow deref of address 0
//! This is a common assumption in Emscripten code

// this is a wrapper with extra logic around the runtime-core `WasmPtr`, so we
// don't want to warn about unusued code here
#![allow(dead_code)]

use std::fmt;
pub use wasmer::{Array, FromToNativeWasmType, Memory, ValueType, WasmCell};

#[repr(transparent)]
pub struct WasmPtr<T: Copy, Ty = wasmer::Item>(wasmer::WasmPtr<T, Ty>);

unsafe impl<T: Copy, Ty> ValueType for WasmPtr<T, Ty> {}
impl<T: Copy, Ty> Copy for WasmPtr<T, Ty> {}

impl<T: Copy, Ty> Clone for WasmPtr<T, Ty> {
    fn clone(&self) -> Self {
        Self(self.0)
    }
}

impl<T: Copy, Ty> fmt::Debug for WasmPtr<T, Ty> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

unsafe impl<T: Copy, Ty> FromToNativeWasmType for WasmPtr<T, Ty> {
    type Native = <wasmer::WasmPtr<T, Ty> as FromToNativeWasmType>::Native;

    fn to_native(self) -> Self::Native {
        self.0.to_native()
    }
    fn from_native(n: Self::Native) -> Self {
        Self(wasmer::WasmPtr::from_native(n))
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
        Self(wasmer::WasmPtr::new(offset))
    }

    #[inline(always)]
    pub fn offset(self) -> u32 {
        self.0.offset()
    }
}

impl<T: Copy + ValueType> WasmPtr<T, wasmer::Item> {
    #[inline(always)]
    pub fn deref(self, memory: &'_ Memory) -> Option<WasmCell<'_, T>> {
        if self.0.offset() == 0 {
            None
        } else {
            self.0.deref(memory)
        }
    }
}

impl<T: Copy + ValueType> WasmPtr<T, wasmer::Array> {
    #[inline(always)]
    pub fn deref(
        self,
        memory: &'_ Memory,
        index: u32,
        length: u32,
    ) -> Option<Vec<WasmCell<'_, T>>> {
        if self.0.offset() == 0 {
            None
        } else {
            self.0.deref(memory, index, length)
        }
    }

    #[inline(always)]
    pub unsafe fn get_utf8_str(self, memory: &'_ Memory, str_len: u32) -> Option<&'_ str> {
        if self.0.offset() == 0 {
            None
        } else {
            self.0.get_utf8_str(memory, str_len)
        }
    }

    #[inline(always)]
    pub fn get_utf8_string(self, memory: &Memory, str_len: u32) -> Option<String> {
        if self.0.offset() == 0 {
            None
        } else {
            self.0.get_utf8_string(memory, str_len)
        }
    }
}
