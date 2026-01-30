//! VFS handle types and OFD semantics.

use crate::flags::{HandleStatusFlags, OpenFlags};
use crate::mount::MountGuard;
use crate::node::FsHandle;
use crate::{VfsError, VfsErrorKind, VfsFileType, VfsHandleId, VfsInodeId, VfsResult};
use bitflags::bitflags;
use parking_lot::Mutex;
use std::io::SeekFrom;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use vfs_ratelimit::rt::StdSyncWait;
use vfs_ratelimit::{AcquireError, AcquireOptions, IoClass, IoCost, LimiterChain, LimiterKey};

pub struct OFDState {
    backend: Arc<dyn FsHandle>,
    offset: AtomicU64,
    status_flags: AtomicU32,
    io_lock: Mutex<()>,
}

impl OFDState {
    pub fn new(backend: Arc<dyn FsHandle>, flags: OpenFlags) -> Self {
        let status = flags.status_flags();
        Self {
            backend,
            offset: AtomicU64::new(0),
            status_flags: AtomicU32::new(status.bits()),
            io_lock: Mutex::new(()),
        }
    }

    pub fn status_flags(&self) -> HandleStatusFlags {
        HandleStatusFlags::from_bits_truncate(self.status_flags.load(Ordering::Acquire))
    }

    pub fn set_status_flags(&self, flags: HandleStatusFlags) {
        self.status_flags.store(flags.bits(), Ordering::Release);
    }

    pub fn offset(&self) -> u64 {
        self.offset.load(Ordering::Acquire)
    }
}

#[derive(Clone)]
pub struct VfsHandle {
    id: VfsHandleId,
    _mount_guard: MountGuard,
    inode: VfsInodeId,
    file_type: VfsFileType,
    access: HandleAccess,
    state: Arc<OFDState>,
    rate_limiters: LimiterChain,
}

impl VfsHandle {
    pub fn new(
        id: VfsHandleId,
        mount_guard: MountGuard,
        inode: VfsInodeId,
        file_type: VfsFileType,
        inner: Arc<dyn FsHandle>,
        flags: OpenFlags,
        rate_limiters: LimiterChain,
    ) -> Self {
        let access = HandleAccess::from_open_flags(flags);
        Self {
            id,
            _mount_guard: mount_guard,
            inode,
            file_type,
            access,
            state: Arc::new(OFDState::new(inner, flags)),
            rate_limiters,
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

    pub fn status_flags(&self) -> HandleStatusFlags {
        self.state.status_flags()
    }

    pub fn set_status_flags(&self, flags: HandleStatusFlags) -> VfsResult<()> {
        self.state.set_status_flags(flags);
        Ok(())
    }

    pub fn tell(&self) -> u64 {
        self.state.offset()
    }

    pub fn get_metadata(&self) -> VfsResult<crate::VfsMetadata> {
        self.state.backend.get_metadata()
    }

    pub fn set_len(&self, new_len: u64) -> VfsResult<()> {
        self.require_access(HandleAccess::WRITE, "handle.set_len")?;
        if self.file_type == VfsFileType::Directory {
            return Err(VfsError::new(VfsErrorKind::IsDir, "handle.set_len"));
        }
        self.state.backend.set_len(new_len)
    }

    pub fn read(&self, buf: &mut [u8]) -> VfsResult<usize> {
        self.require_access(HandleAccess::READ, "handle.read")?;
        if self.file_type == VfsFileType::Directory {
            return Err(VfsError::new(VfsErrorKind::IsDir, "handle.read"));
        }
        self.apply_rate_limit(IoClass::Read, buf.len() as u64)?;

        let _guard = self.state.io_lock.lock();
        let offset = self.state.offset.load(Ordering::Acquire);
        let read = self.state.backend.read_at(offset, buf)?;
        self.state
            .offset
            .store(offset.saturating_add(read as u64), Ordering::Release);
        Ok(read)
    }

    pub fn write(&self, buf: &[u8]) -> VfsResult<usize> {
        self.require_access(HandleAccess::WRITE, "handle.write")?;
        if self.file_type == VfsFileType::Directory {
            return Err(VfsError::new(VfsErrorKind::IsDir, "handle.write"));
        }
        self.apply_rate_limit(IoClass::Write, buf.len() as u64)?;

        let _guard = self.state.io_lock.lock();
        if self.status_flags().contains(HandleStatusFlags::APPEND) {
            let end = self.state.backend.get_metadata()?.size;
            let written = self.state.backend.write_at(end, buf)?;
            self.state
                .offset
                .store(end.saturating_add(written as u64), Ordering::Release);
            return Ok(written);
        }

        let offset = self.state.offset.load(Ordering::Acquire);
        let written = self.state.backend.write_at(offset, buf)?;
        self.state
            .offset
            .store(offset.saturating_add(written as u64), Ordering::Release);
        Ok(written)
    }

    pub fn pread_at(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        self.require_access(HandleAccess::READ, "handle.pread")?;
        if self.file_type == VfsFileType::Directory {
            return Err(VfsError::new(VfsErrorKind::IsDir, "handle.pread"));
        }
        self.apply_rate_limit(IoClass::Read, buf.len() as u64)?;
        self.state.backend.read_at(offset, buf)
    }

    pub fn pwrite_at(&self, offset: u64, buf: &[u8]) -> VfsResult<usize> {
        self.require_access(HandleAccess::WRITE, "handle.pwrite")?;
        if self.file_type == VfsFileType::Directory {
            return Err(VfsError::new(VfsErrorKind::IsDir, "handle.pwrite"));
        }
        self.apply_rate_limit(IoClass::Write, buf.len() as u64)?;
        self.state.backend.write_at(offset, buf)
    }

    pub fn pread(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        self.pread_at(offset, buf)
    }

    pub fn pwrite(&self, offset: u64, buf: &[u8]) -> VfsResult<usize> {
        self.pwrite_at(offset, buf)
    }

    pub fn seek(&self, pos: SeekFrom) -> VfsResult<u64> {
        if !self.state.backend.is_seekable() {
            return Err(VfsError::new(VfsErrorKind::NotSupported, "handle.seek"));
        }

        let _guard = self.state.io_lock.lock();
        let current = self.state.offset.load(Ordering::Acquire);
        let new = match pos {
            SeekFrom::Start(val) => val,
            SeekFrom::Current(delta) => {
                if delta >= 0 {
                    current.saturating_add(delta as u64)
                } else {
                    let neg = (-delta) as u64;
                    current
                        .checked_sub(neg)
                        .ok_or_else(|| VfsError::new(VfsErrorKind::InvalidInput, "handle.seek"))?
                }
            }
            SeekFrom::End(delta) => {
                let len = self.state.backend.get_metadata()?.size;
                if delta >= 0 {
                    len.saturating_add(delta as u64)
                } else {
                    let neg = (-delta) as u64;
                    len.checked_sub(neg)
                        .ok_or_else(|| VfsError::new(VfsErrorKind::InvalidInput, "handle.seek"))?
                }
            }
        };
        self.state.offset.store(new, Ordering::Release);
        Ok(new)
    }

    pub fn flush(&self) -> VfsResult<()> {
        self.state.backend.flush()
    }

    pub fn fsync(&self) -> VfsResult<()> {
        self.state.backend.fsync()
    }

    pub fn dup(&self) -> VfsResult<Arc<VfsHandle>> {
        Ok(Arc::new(self.clone()))
    }

    fn require_access(&self, access: HandleAccess, context: &'static str) -> VfsResult<()> {
        if self.access.contains(access) {
            Ok(())
        } else {
            Err(VfsError::new(VfsErrorKind::BadHandle, context))
        }
    }

    fn apply_rate_limit(&self, class: IoClass, bytes: u64) -> VfsResult<()> {
        if self.rate_limiters.is_empty() {
            return Ok(());
        }
        let opts = AcquireOptions {
            nonblocking: self.status_flags().contains(HandleStatusFlags::NONBLOCK),
            timeout: None,
            key: Some(LimiterKey::Handle(self.id.0)),
        };
        let cost = IoCost {
            class,
            ops: 1,
            bytes,
        };
        let wait = StdSyncWait;
        self.rate_limiters
            .acquire_blocking(cost, &opts, &wait)
            .map_err(|err| VfsError::new(map_acquire_error(err), "handle.ratelimit"))
    }
}

fn map_acquire_error(err: AcquireError) -> VfsErrorKind {
    match err {
        AcquireError::WouldBlock => VfsErrorKind::WouldBlock,
        AcquireError::TimedOut => VfsErrorKind::WouldBlock,
        AcquireError::Cancelled => VfsErrorKind::Interrupted,
        AcquireError::Misconfigured => VfsErrorKind::InvalidInput,
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct HandleAccess: u8 {
        const READ = 1 << 0;
        const WRITE = 1 << 1;
    }
}

impl HandleAccess {
    pub fn from_open_flags(flags: OpenFlags) -> Self {
        let mut access = HandleAccess::empty();
        if flags.contains(OpenFlags::READ) {
            access |= HandleAccess::READ;
        }
        if flags.contains(OpenFlags::WRITE) {
            access |= HandleAccess::WRITE;
        }
        access
    }
}

#[derive(Clone)]
pub struct VfsDirHandle {
    inner: Arc<DirHandleInner>,
}

struct DirHandleInner {
    id: VfsHandleId,
    _mount_guard: MountGuard,
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
                _mount_guard: mount_guard,
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
    #[allow(dead_code)]
    pub(crate) fn new(id: VfsHandleId) -> Self {
        Self { id }
    }

    pub fn id(&self) -> VfsHandleId {
        self.id
    }
}
