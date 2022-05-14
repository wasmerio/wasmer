use wasmer_derive::ValueType;
use wasmer_types::MemorySize;

use crate::__wasi_fd_t;

pub type __wasi_count_t = u32;

pub type __wasi_option_t = u8;
pub const __WASI_OPTION_NONE: __wasi_option_t = 0;
pub const __WASI_OPTION_SOME: __wasi_option_t = 1;

pub type __wasi_bool_t = u8;
pub const __WASI_BOOL_FALSE: __wasi_bool_t = 0;
pub const __WASI_BOOL_TRUE: __wasi_bool_t = 1;

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_ciovec_t<M: MemorySize> {
    pub buf: M::Offset,
    pub buf_len: M::Offset,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_iovec_t<M: MemorySize> {
    pub buf: M::Offset,
    pub buf_len: M::Offset,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_tty_t {
    pub cols: u32,
    pub rows: u32,
    pub width: u32,
    pub height: u32,
    pub stdin_tty: __wasi_bool_t,
    pub stdout_tty: __wasi_bool_t,
    pub stderr_tty: __wasi_bool_t,
    pub echo: __wasi_bool_t,
    pub line_buffered: __wasi_bool_t,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_pipe_handles_t {
    pub pipe: __wasi_fd_t,
    pub other: __wasi_fd_t,
}

pub type __wasi_stdiomode_t = u8;
pub const __WASI_STDIO_MODE_PIPED: __wasi_stdiomode_t = 1;
pub const __WASI_STDIO_MODE_INHERIT: __wasi_stdiomode_t = 2;
pub const __WASI_STDIO_MODE_NULL: __wasi_stdiomode_t = 3;
pub const __WASI_STDIO_MODE_LOG: __wasi_stdiomode_t = 4;
