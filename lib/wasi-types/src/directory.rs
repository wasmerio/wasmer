use std::mem;
use wasmer_wasi_types_generated::wasi_snapshot0;

pub const __WASI_DIRCOOKIE_START: wasi_snapshot0::Dircookie = 0;

pub fn dirent_to_le_bytes(ent: &wasi_snapshot0::Dirent) -> Vec<u8> {
    let out: Vec<u8> = std::iter::empty()
        .chain(ent.d_next.to_le_bytes())
        .chain(ent.d_ino.to_le_bytes())
        .chain(ent.d_namlen.to_le_bytes())
        .chain(u32::from(ent.d_type as u8).to_le_bytes())
        .collect();

    assert_eq!(out.len(), mem::size_of::<wasi_snapshot0::Dirent>());
    out
}

#[cfg(test)]
mod tests {
    use super::dirent_to_le_bytes;
    use wasmer_wasi_types_generated::wasi_snapshot0;

    #[test]
    fn test_dirent_to_le_bytes() {
        let s = wasi_snapshot0::Dirent {
            d_next: 0x0123456789abcdef,
            d_ino: 0xfedcba9876543210,
            d_namlen: 0xaabbccdd,
            d_type: wasi_snapshot0::Filetype::Directory,
        };

        assert_eq!(
            vec![
                // d_next
                0xef,
                0xcd,
                0xab,
                0x89,
                0x67,
                0x45,
                0x23,
                0x01,
                //
                // d_ino
                0x10,
                0x32,
                0x54,
                0x76,
                0x98,
                0xba,
                0xdc,
                0xfe,
                //
                // d_namelen
                0xdd,
                0xcc,
                0xbb,
                0xaa,
                //
                // d_type
                // plus padding
                wasi_snapshot0::Filetype::Directory as u8,
                0x00,
                0x00,
                0x00,
            ],
            dirent_to_le_bytes(&s)
        );
    }
}
