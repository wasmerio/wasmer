//! File type classification.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VfsFileType {
    Unknown,
    RegularFile,
    Directory,
    Symlink,
    CharDevice,
    BlockDevice,
    Fifo,
    Socket,
}
