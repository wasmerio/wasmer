use std::fmt;
use wasmer_derive::ValueType;
use wasmer_wasi_types_generated::wasi::{
    Device, Filesize, Filetype, Inode, Snapshot0Linkcount, Timestamp,
};

pub type __wasi_whence_t = u8;
pub const __WASI_WHENCE_CUR: u8 = 0;
pub const __WASI_WHENCE_END: u8 = 1;
pub const __WASI_WHENCE_SET: u8 = 2;
