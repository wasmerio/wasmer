//! VFS file type → WASI filetype translation.
//!
//! Keep all file type mapping in this single module.

use vfs_core::VfsFileType;
use wasmer_wasix_types::wasi::Filetype;

/// Convert a VFS file type to a WASI file type.
pub fn vfs_filetype_to_wasi(ft: VfsFileType) -> Filetype {
    match ft {
        VfsFileType::Unknown => Filetype::Unknown,
        VfsFileType::RegularFile => Filetype::RegularFile,
        VfsFileType::Directory => Filetype::Directory,
        VfsFileType::Symlink => Filetype::SymbolicLink,
        VfsFileType::CharDevice => Filetype::CharacterDevice,
        VfsFileType::BlockDevice => Filetype::BlockDevice,
        VfsFileType::Fifo => Filetype::Unknown,
        VfsFileType::Socket => Filetype::SocketStream,
    }
}
