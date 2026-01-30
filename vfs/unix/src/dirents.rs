//! WASI dirent encoding helpers.
//!
//! This module owns the dirent byte layout used by Wasix readdir/getdents.
//! Callers must not reimplement this packing logic elsewhere.

use crate::filetype::vfs_filetype_to_wasi;
use vfs_core::{BackendInodeId, VfsDirEntry, VfsFileType};

/// WASI dirent header alignment in bytes.
pub const WASI_DIRENT_ALIGN: usize = 8;
/// WASI dirent header size (d_next + d_ino + d_namlen + d_type + padding).
pub const WASI_DIRENT_HEADER_SIZE: usize = 8 + 8 + 4 + 1 + 3;

/// Result of `encode_wasi_dirents`.
pub struct EncodeDirentsResult {
    pub bytes_written: usize,
    pub next_cookie: u64,
    pub entries_written: u32,
}

/// Encodes directory entries into `dst` in WASI dirent format.
///
/// The cookie model used here is a simple entry index (0-based). `start_cookie`
/// skips entries until the iterator index is >= `start_cookie`. The `d_next`
/// field is set to `index + 1`, and `next_cookie` returns the last `d_next`
/// written (or `start_cookie` if nothing is written).
pub fn encode_wasi_dirents(
    entries: impl Iterator<Item = VfsDirEntry>,
    start_cookie: u64,
    dst: &mut [u8],
) -> EncodeDirentsResult {
    let mut offset = 0usize;
    let mut next_cookie = start_cookie;
    let mut entries_written: u32 = 0;

    for (index, entry) in entries.enumerate() {
        let index = index as u64;
        if index < start_cookie {
            continue;
        }

        let name = entry.name.as_bytes();
        let name_len_u32 = match u32::try_from(name.len()) {
            Ok(len) => len,
            Err(_) => break,
        };
        let record_len = align_up(WASI_DIRENT_HEADER_SIZE + name.len(), WASI_DIRENT_ALIGN);
        if offset + record_len > dst.len() {
            break;
        }

        let d_next = index + 1;
        let d_ino = vfs_inode_to_u64(entry.inode);
        let d_type = vfs_dirent_type(entry.file_type);

        write_u64_le(dst, offset, d_next);
        write_u64_le(dst, offset + 8, d_ino);
        write_u32_le(dst, offset + 16, name_len_u32);
        dst[offset + 20] = d_type;
        dst[offset + 21] = 0;
        dst[offset + 22] = 0;
        dst[offset + 23] = 0;

        let name_start = offset + WASI_DIRENT_HEADER_SIZE;
        dst[name_start..name_start + name.len()].copy_from_slice(name);
        let padding_start = name_start + name.len();
        for byte in &mut dst[padding_start..offset + record_len] {
            *byte = 0;
        }

        offset += record_len;
        next_cookie = d_next;
        entries_written = entries_written.saturating_add(1);
    }

    EncodeDirentsResult {
        bytes_written: offset,
        next_cookie,
        entries_written,
    }
}

fn vfs_dirent_type(ft: Option<VfsFileType>) -> u8 {
    ft.map(vfs_filetype_to_wasi)
        .unwrap_or(wasmer_wasix_types::wasi::Filetype::Unknown) as u8
}

fn vfs_inode_to_u64(inode: Option<BackendInodeId>) -> u64 {
    inode.map(inode_backend_to_u64).unwrap_or(0)
}

fn inode_backend_to_u64(id: BackendInodeId) -> u64 {
    id.get()
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
