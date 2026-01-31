use std::sync::Arc;

use vfs_core::flags::OpenFlags;
use vfs_core::node::FsHandle;
use vfs_core::{VfsError, VfsErrorKind, VfsResult};

use crate::fs::MemFsInner;
use crate::inode::MemInode;

#[derive(Debug, Clone)]
pub(crate) struct MemHandle {
    fs: Arc<MemFsInner>,
    inode: Arc<MemInode>,
    flags: OpenFlags,
}

impl MemHandle {
    pub(crate) fn new(fs: Arc<MemFsInner>, inode: Arc<MemInode>, flags: OpenFlags) -> Self {
        Self { fs, inode, flags }
    }

    fn can_write(&self) -> bool {
        self.flags.contains(OpenFlags::WRITE) || self.flags.contains(OpenFlags::APPEND)
    }
}

impl FsHandle for MemHandle {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        let data = self.inode.file_data()?;
        let data = data.read().expect("lock");
        let offset = offset as usize;
        if offset >= data.len() {
            return Ok(0);
        }
        let end = (offset + buf.len()).min(data.len());
        buf[..end - offset].copy_from_slice(&data[offset..end]);
        Ok(end - offset)
    }

    fn write_at(&self, offset: u64, buf: &[u8]) -> VfsResult<usize> {
        self.fs.check_writable()?;
        if !self.can_write() {
            return Err(VfsError::new(
                VfsErrorKind::PermissionDenied,
                "memfs.write_at",
            ));
        }
        let data = self.inode.file_data()?;
        let mut data = data.write().expect("lock");
        let offset = offset as usize;
        let old_len = data.len();
        let new_len = offset.saturating_add(buf.len()).max(old_len);
        if new_len > old_len {
            let delta = (new_len - old_len) as u64;
            self.fs.try_reserve_bytes(delta)?;
            if offset > old_len {
                data.resize(offset, 0);
            }
            data.resize(new_len, 0);
        }
        data[offset..offset + buf.len()].copy_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&self) -> VfsResult<()> {
        Ok(())
    }

    fn fsync(&self) -> VfsResult<()> {
        Ok(())
    }

    fn get_metadata(&self) -> VfsResult<vfs_core::VfsMetadata> {
        Ok(self.inode.metadata())
    }

    fn set_len(&self, len: u64) -> VfsResult<()> {
        self.fs.check_writable()?;
        if !self.can_write() {
            return Err(VfsError::new(
                VfsErrorKind::PermissionDenied,
                "memfs.set_len",
            ));
        }
        let data = self.inode.file_data()?;
        let mut data = data.write().expect("lock");
        let old_len = data.len();
        let new_len = len as usize;
        if new_len > old_len {
            let delta = (new_len - old_len) as u64;
            self.fs.try_reserve_bytes(delta)?;
            data.resize(new_len, 0);
        } else if new_len < old_len {
            let delta = (old_len - new_len) as u64;
            data.truncate(new_len);
            self.fs.release_bytes(delta);
        }
        Ok(())
    }

    fn len(&self) -> VfsResult<u64> {
        let data = self.inode.file_data()?;
        Ok(data.read().expect("lock").len() as u64)
    }

    fn dup(&self) -> VfsResult<Option<Arc<dyn FsHandle>>> {
        Ok(Some(Arc::new(self.clone())))
    }
}
