use crate::{WASI_DIRENT_ALIGN, WASI_DIRENT_HEADER_SIZE, encode_wasi_dirents};
use vfs_core::{BackendInodeId, VfsDirEntry, VfsFileType, VfsNameBuf};

#[test]
fn encodes_dirents_without_partial_write() {
    let entry = VfsDirEntry {
        name: VfsNameBuf::new(b"a".to_vec()).expect("valid name"),
        inode: Some(BackendInodeId::new(2).expect("non-zero inode")),
        file_type: Some(VfsFileType::Directory),
    };

    let entry2 = VfsDirEntry {
        name: VfsNameBuf::new(b"bb".to_vec()).expect("valid name"),
        inode: None,
        file_type: None,
    };

    let record_len = align_up(WASI_DIRENT_HEADER_SIZE + 1, WASI_DIRENT_ALIGN);
    let mut buf = vec![0u8; record_len];
    let result = encode_wasi_dirents(vec![entry.clone(), entry2.clone()].into_iter(), 0, &mut buf);

    assert_eq!(result.bytes_written, record_len);
    assert_eq!(result.entries_written, 1);
    assert_eq!(result.next_cookie, 1);

    let mut expected = vec![0u8; record_len];
    write_u64_le(&mut expected, 0, 1);
    write_u64_le(&mut expected, 8, 2);
    write_u32_le(&mut expected, 16, 1);
    expected[20] = wasmer_wasix_types::wasi::Filetype::Directory as u8;
    expected[WASI_DIRENT_HEADER_SIZE] = b'a';

    assert_eq!(buf, expected);
}

#[test]
fn honors_start_cookie() {
    let entry = VfsDirEntry {
        name: VfsNameBuf::new(b"a".to_vec()).expect("valid name"),
        inode: Some(BackendInodeId::new(2).expect("non-zero inode")),
        file_type: Some(VfsFileType::Directory),
    };

    let entry2 = VfsDirEntry {
        name: VfsNameBuf::new(b"bb".to_vec()).expect("valid name"),
        inode: None,
        file_type: None,
    };

    let record_len = align_up(WASI_DIRENT_HEADER_SIZE + 2, WASI_DIRENT_ALIGN);
    let mut buf = vec![0u8; record_len];
    let result = encode_wasi_dirents(vec![entry, entry2].into_iter(), 1, &mut buf);

    assert_eq!(result.entries_written, 1);
    assert_eq!(result.next_cookie, 2);
}

fn align_up(value: usize, align: usize) -> usize {
    if value % align == 0 {
        value
    } else {
        value + (align - (value % align))
    }
}

fn write_u64_le(dst: &mut [u8], offset: usize, value: u64) {
    dst[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn write_u32_le(dst: &mut [u8], offset: usize, value: u32) {
    dst[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
