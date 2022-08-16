mod bindings;

pub use bindings::*;

use std::mem::MaybeUninit;
use wasmer_types::ValueType;

unsafe impl ValueType for wasi_snapshot0::Errno {
    #[inline]
    fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {}
}
