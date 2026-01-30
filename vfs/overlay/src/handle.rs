use std::sync::Arc;

use vfs_core::node::FsHandle;
use vfs_core::VfsResult;

pub struct OverlayHandle {
    inner: Arc<dyn FsHandle>,
}

impl OverlayHandle {
    pub fn new(inner: Arc<dyn FsHandle>) -> Self {
        Self { inner }
    }
}

impl FsHandle for OverlayHandle {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        self.inner.read_at(offset, buf)
    }

    fn write_at(&self, offset: u64, buf: &[u8]) -> VfsResult<usize> {
        self.inner.write_at(offset, buf)
    }

    fn flush(&self) -> VfsResult<()> {
        self.inner.flush()
    }

    fn fsync(&self) -> VfsResult<()> {
        self.inner.fsync()
    }

    fn get_metadata(&self) -> VfsResult<vfs_core::VfsMetadata> {
        self.inner.get_metadata()
    }

    fn set_len(&self, len: u64) -> VfsResult<()> {
        self.inner.set_len(len)
    }

    fn len(&self) -> VfsResult<u64> {
        self.inner.len()
    }

    fn append(&self, buf: &[u8]) -> VfsResult<Option<usize>> {
        self.inner.append(buf)
    }

    fn dup(&self) -> VfsResult<Option<Arc<dyn FsHandle>>> {
        self.inner.dup()
    }

    fn is_seekable(&self) -> bool {
        self.inner.is_seekable()
    }
}
