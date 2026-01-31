use std::fs::File;

use vfs_core::{VfsMetadata, VfsResult};

use crate::platform;

#[derive(Debug)]
pub struct HostHandle {
    file: File,
}

impl HostHandle {
    pub fn new(file: File) -> Self {
        Self { file }
    }
}

impl vfs_core::traits_sync::FsHandleSync for HostHandle {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileExt;
            return crate::io_result("host.handle.read_at", self.file.read_at(buf, offset));
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileExt;
            return crate::io_result("host.handle.read_at", self.file.seek_read(buf, offset));
        }
    }

    fn write_at(&self, offset: u64, buf: &[u8]) -> VfsResult<usize> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileExt;
            return crate::io_result("host.handle.write_at", self.file.write_at(buf, offset));
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileExt;
            return crate::io_result("host.handle.write_at", self.file.seek_write(buf, offset));
        }
    }

    fn flush(&self) -> VfsResult<()> {
        crate::io_result("host.handle.flush", self.file.sync_data())
    }

    fn fsync(&self) -> VfsResult<()> {
        crate::io_result("host.handle.fsync", self.file.sync_all())
    }

    fn get_metadata(&self) -> VfsResult<VfsMetadata> {
        let stat = crate::io_result("host.handle.stat", platform::stat_file(&self.file))?;
        Ok(crate::node::metadata_from_stat(&stat))
    }

    fn set_len(&self, len: u64) -> VfsResult<()> {
        crate::io_result("host.handle.set_len", self.file.set_len(len))
    }

    fn len(&self) -> VfsResult<u64> {
        let meta = crate::io_result("host.handle.meta", self.file.metadata())?;
        Ok(meta.len())
    }

    fn dup(&self) -> VfsResult<Option<std::sync::Arc<dyn vfs_core::traits_sync::FsHandleSync>>> {
        let cloned =
            crate::io_result("host.handle.dup", self.file.try_clone()).map(HostHandle::new)?;
        Ok(Some(std::sync::Arc::new(cloned)))
    }

    fn is_seekable(&self) -> bool {
        true
    }
}
