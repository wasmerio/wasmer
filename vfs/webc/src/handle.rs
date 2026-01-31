
use std::io::SeekFrom;

use vfs_core::traits_sync::FsHandleSync;
use vfs_core::{VfsError, VfsErrorKind, VfsResult};
use webc::compat::SharedBytes;

#[derive(Debug)]
pub struct WebcFileHandle {
    data: SharedBytes,
}

impl WebcFileHandle {
    pub fn new(data: SharedBytes) -> Self {
        Self { data }
    }
}

impl FsHandleSync for WebcFileHandle {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        let start = offset as usize;
        if start >= self.data.len() {
            return Ok(0);
        }
        let remaining = &self.data[start..];
        let to_copy = remaining.len().min(buf.len());
        buf[..to_copy].copy_from_slice(&remaining[..to_copy]);
        Ok(to_copy)
    }

    fn write_at(&self, _offset: u64, _buf: &[u8]) -> VfsResult<usize> {
        Err(VfsError::new(VfsErrorKind::ReadOnlyFs, "webc.write_at"))
    }

    fn flush(&self) -> VfsResult<()> {
        Ok(())
    }

    fn fsync(&self) -> VfsResult<()> {
        Ok(())
    }

    fn get_metadata(&self) -> VfsResult<vfs_core::VfsMetadata> {
        Err(VfsError::new(
            VfsErrorKind::NotSupported,
            "webc.handle.metadata",
        ))
    }

    fn set_len(&self, _len: u64) -> VfsResult<()> {
        Err(VfsError::new(VfsErrorKind::ReadOnlyFs, "webc.set_len"))
    }

    fn len(&self) -> VfsResult<u64> {
        Ok(self.data.len() as u64)
    }

    fn is_seekable(&self) -> bool {
        true
    }
}
