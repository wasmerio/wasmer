use wasmer_types::ValueType;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct __wasi_ciovec_t {
    pub buf: u32,
    pub buf_len: u32,
}

unsafe impl ValueType for __wasi_ciovec_t {}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct __wasi_iovec_t {
    pub buf: u32,
    pub buf_len: u32,
}

unsafe impl ValueType for __wasi_iovec_t {}
