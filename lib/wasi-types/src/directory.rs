use crate::*;
use std::mem;
use wasmer_types::ValueType;

pub type __wasi_dircookie_t = u64;
pub const __WASI_DIRCOOKIE_START: u64 = 0;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct __wasi_dirent_t {
    pub d_next: __wasi_dircookie_t,
    pub d_ino: __wasi_inode_t,
    pub d_namlen: u32,
    pub d_type: __wasi_filetype_t,
}

unsafe impl ValueType for __wasi_dirent_t {}

pub fn dirent_to_le_bytes(ent: &__wasi_dirent_t) -> Vec<u8> {
    use mem::transmute;

    let mut out = Vec::with_capacity(mem::size_of::<__wasi_dirent_t>());
    let bytes: [u8; 8] = unsafe { transmute(ent.d_next.to_le()) };
    for &b in &bytes {
        out.push(b);
    }
    let bytes: [u8; 8] = unsafe { transmute(ent.d_ino.to_le()) };
    for &b in &bytes {
        out.push(b);
    }
    let bytes: [u8; 4] = unsafe { transmute(ent.d_namlen.to_le()) };
    for &b in &bytes {
        out.push(b);
    }
    out.push(ent.d_type);
    out.push(0);
    out.push(0);
    out.push(0);
    assert_eq!(out.len(), mem::size_of::<__wasi_dirent_t>());
    out
}
