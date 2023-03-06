use crate::access::{RefCow, SliceCow, WasmRefAccess};
use crate::mem_access::{MemoryAccessError, WasmRef, WasmSlice};
use crate::WasmSliceAccess;

impl<'a, T> WasmSliceAccess<'a, T>
where
    T: wasmer_types::ValueType,
{
    pub(crate) fn new(slice: WasmSlice<'a, T>) -> Result<Self, MemoryAccessError> {
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
    pub(crate) fn new(ptr: WasmRef<'a, T>) -> Result<Self, MemoryAccessError> {
        let val = ptr.read()?;
        Ok(Self {
            ptr,
            buf: RefCow::Owned(val, false),
        })
    }
}
