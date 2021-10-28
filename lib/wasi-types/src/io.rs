use wasmer_derive::ValueType;

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_ciovec_t {
    pub buf: u32,
    pub buf_len: u32,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_iovec_t {
    pub buf: u32,
    pub buf_len: u32,
}
