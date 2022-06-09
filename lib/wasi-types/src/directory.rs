use crate::*;
use std::mem;
use wasmer_derive::ValueType;

pub type __wasi_dircookie_t = u64;
pub const __WASI_DIRCOOKIE_START: u64 = 0;

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_dirent_t {
    pub d_next: __wasi_dircookie_t,
    pub d_ino: __wasi_inode_t,
    pub d_namlen: u32,
    pub d_type: __wasi_filetype_t,
}

pub fn dirent_to_le_bytes(ent: &__wasi_dirent_t) -> Vec<u8> {
    let out: Vec<u8> = std::iter::empty()
        .chain(ent.d_next.to_le_bytes())
        .chain(ent.d_ino.to_le_bytes())
        .chain(ent.d_namlen.to_le_bytes())
        .chain(u32::from(ent.d_type).to_le_bytes())
        .collect();

    assert_eq!(out.len(), mem::size_of::<__wasi_dirent_t>());
    out
}

#[cfg(test)]
mod tests {
    use super::{__wasi_dirent_t, dirent_to_le_bytes};

    #[test]
    fn test_dirent_to_le_bytes() {
        let s = __wasi_dirent_t {
            d_next: 0x0123456789abcdef,
            d_ino: 0xfedcba9876543210,
            d_namlen: 0xaabbccdd,
            d_type: 0x99,
        };

        assert_eq!(
            vec![
                // d_next
                0xef, 0xcd, 0xab, 0x89, 0x67, 0x45, 0x23, 0x01,
                //
                // d_ino
                0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe,
                //
                // d_namelen
                0xdd, 0xcc, 0xbb, 0xaa,
                //
                // d_type
                // plus padding
                0x99, 0x00, 0x00, 0x00,
            ],
            dirent_to_le_bytes(&s)
        );
    }
}
