//! VFS handle types and OFD semantics.

use crate::flags::OpenFlags;
use crate::mount::MountGuard;
use crate::node::FsHandle;
use crate::{VfsError, VfsErrorKind, VfsFileType, VfsHandleId, VfsInodeId, VfsResult};
use parking_lot::Mutex;
use std::io::SeekFrom;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

#[derive(Debug)]
pub struct OFDState {
    offset: Mutex<u64>,
    status_flags: AtomicU32,
}

impl OFDState {
    pub fn new(flags: OpenFlags) -> Self {
        let status = flags & OpenFlags::STATUS_MASK;
        Self {
            offset: Mutex::new(0),
            status_flags: AtomicU32::new(status.bits()),
        }
    }

    pub fn status_flags(&self) -> OpenFlags {
        OpenFlags::from_bits_truncate(self.status_flags.load(Ordering::Acquire))
    }

    pub fn set_status_flags(&self, flags: OpenFlags) {
        let status = flags & OpenFlags::STATUS_MASK;
        self.status_flags.store(status.bits(), Ordering::Release);
    }
}

#[derive(Clone)]
pub struct VfsHandle {
    id: VfsHandleId,
    mount_guard: MountGuard,
    inode: VfsInodeId,
    file_type: VfsFileType,
    state: Arc<OFDState>,
    inner: Arc<dyn FsHandle>,
}

impl VfsHandle {
    pub fn new(
        id: VfsHandleId,
        mount_guard: MountGuard,
        inode: VfsInodeId,
        file_type: VfsFileType,
        inner: Arc<dyn FsHandle>,
        flags: OpenFlags,
    ) -> Self {
        Self {
            id,
            mount_guard,
            inode,
            file_type,
            state: Arc::new(OFDState::new(flags)),
            inner,
        }
    }

    pub fn id(&self) -> VfsHandleId {
        self.id
    }

    pub fn inode(&self) -> VfsInodeId {
        self.inode
    }

    pub fn file_type(&self) -> VfsFileType {
        self.file_type
    }

    pub fn status_flags(&self) -> OpenFlags {
        self.state.status_flags()
    }

    pub fn set_status_flags(&self, flags: OpenFlags) {
        self.state.set_status_flags(flags);
    }

    pub fn read(&self, buf: &mut [u8]) -> VfsResult<usize> {
        let mut offset = self.state.offset.lock();
        let read = self.inner.read_at(*offset, buf)?;
        *offset = offset.saturating_add(read as u64);
        Ok(read)
    }

    pub fn write(&self, buf: &[u8]) -> VfsResult<usize> {
        if self.status_flags().contains(OpenFlags::APPEND) {
            if let Some(written) = self.inner.append(buf)? {
                let mut offset = self.state.offset.lock();
                if let Ok(len) = self.inner.len() {
                    *offset = len;
                } else {
                    *offset = offset.saturating_add(written as u64);
                }
                return Ok(written);
            }

            let len = self.inner.len()?;
            let written = self.inner.write_at(len, buf)?;
            let mut offset = self.state.offset.lock();
            *offset = len.saturating_add(written as u64);
            return Ok(written);
        }

        let mut offset = self.state.offset.lock();
        let written = self.inner.write_at(*offset, buf)?;
        *offset = offset.saturating_add(written as u64);
        Ok(written)
    }

    pub fn pread(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        self.inner.read_at(offset, buf)
    }

    pub fn pwrite(&self, offset: u64, buf: &[u8]) -> VfsResult<usize> {
        self.inner.write_at(offset, buf)
    }

    pub fn seek(&self, pos: SeekFrom) -> VfsResult<u64> {
        let mut offset = self.state.offset.lock();
        let new = match pos {
            SeekFrom::Start(val) => val,
            SeekFrom::Current(delta) => {
                if delta >= 0 {
                    offset.saturating_add(delta as u64)
                } else {
                    let neg = (-delta) as u64;
                    offset.checked_sub(neg).ok_or_else(|| {
                        VfsError::new(VfsErrorKind::InvalidInput, "handle.seek")
                    })?
                }
            }
            SeekFrom::End(delta) => {
                let len = self.inner.len()?;
                if delta >= 0 {
                    len.saturating_add(delta as u64)
                } else {
                    let neg = (-delta) as u64;
                    len.checked_sub(neg).ok_or_else(|| {
                        VfsError::new(VfsErrorKind::InvalidInput, "handle.seek")
                    })?
                }
            }
        };
        *offset = new;
        Ok(new)
    }

    pub fn flush(&self) -> VfsResult<()> {
        self.inner.flush()
    }

    pub fn fsync(&self) -> VfsResult<()> {
        self.inner.fsync()
    }

    pub fn dup(&self) -> VfsResult<Arc<VfsHandle>> {
        let inner = if let Some(dup) = self.inner.dup()? {
            dup
        } else {
            self.inner.clone()
        };
        Ok(Arc::new(Self {
            id: self.id,
            mount_guard: self.mount_guard.clone(),
            inode: self.inode,
            file_type: self.file_type,
            state: self.state.clone(),
            inner,
        }))
    }
}

#[derive(Clone)]
pub struct VfsDirHandle {
    inner: Arc<DirHandleInner>,
}

struct DirHandleInner {
    id: VfsHandleId,
    mount_guard: MountGuard,
    inode: VfsInodeId,
    node: Arc<dyn crate::node::FsNode>,
    parent: Option<crate::inode::NodeRef>,
}

impl VfsDirHandle {
    pub fn new(
        id: VfsHandleId,
        mount_guard: MountGuard,
        inode: VfsInodeId,
        node: Arc<dyn crate::node::FsNode>,
        parent: Option<crate::inode::NodeRef>,
    ) -> Self {
        Self {
            inner: Arc::new(DirHandleInner {
                id,
                mount_guard,
                inode,
                node,
                parent,
            }),
        }
    }

    pub fn id(&self) -> VfsHandleId {
        self.inner.id
    }

    pub fn inode(&self) -> VfsInodeId {
        self.inner.inode
    }

    pub fn node(&self) -> &Arc<dyn crate::node::FsNode> {
        &self.inner.node
    }

    pub fn parent(&self) -> Option<crate::inode::NodeRef> {
        self.inner.parent.clone()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DirStreamHandle {
    id: VfsHandleId,
}

impl DirStreamHandle {
    pub(crate) fn new(id: VfsHandleId) -> Self {
        Self { id }
    }

    pub fn id(&self) -> VfsHandleId {
        self.id
    }
}
