//! File seals (`F_ADD_SEALS`), after Linux's `mm/memfd.c` and the checks in `mm/shmem.c`.
//!
//! Only regular files of tmpfs can have seals, as only shmem files can on Linux. Every such file
//! starts with `F_SEAL_SEAL`, so seals can only be added to one made unsealed on purpose
//! (`memfd_create` with `MFD_ALLOW_SEALING`). Seals belong to the inode, which a sealable file
//! never outlives: it has no name to come back through. The VFS enforces them on every write,
//! size change, allocation and mode change; the kernel enforces them on `mmap`.

use std::sync::atomic::Ordering;

use wasixv2_abi::{Errno, SealFlags};

use crate::{
    AllocMode, File, FsResult, SetAttr, Vfs,
    inode::{DirLock, Inode},
    types::S_IXUGO,
};

/// Seals that forbid writes.
pub const WRITE_SEALS: SealFlags =
    SealFlags(SealFlags::F_SEAL_WRITE.0 | SealFlags::F_SEAL_FUTURE_WRITE.0);

/// `F_ALL_SEALS`.
const ALL_SEALS: SealFlags = SealFlags(
    SealFlags::F_SEAL_SEAL.0
        | SealFlags::F_SEAL_SHRINK.0
        | SealFlags::F_SEAL_GROW.0
        | WRITE_SEALS.0
        | SealFlags::F_SEAL_EXEC.0,
);

impl Inode {
    /// `mapping_map_writable`: counts a new writable shared mapping of the file, which
    /// `F_SEAL_WRITE` forbids (`EPERM`).
    pub fn map_writable(&self) -> FsResult<()> {
        self.mmap_writable
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n >= 0).then_some(n + 1)
            })
            .map(drop)
            .map_err(|_| Errno::EPERM)
    }

    /// `mapping_allow_writable`: counts a copy of a writable shared mapping, made by splitting
    /// one or by `fork`, which an existing mapping makes allowed.
    pub fn allow_writable(&self) {
        self.mmap_writable.fetch_add(1, Ordering::AcqRel);
    }

    /// `mapping_unmap_writable`: a writable shared mapping of the file is gone.
    pub fn unmap_writable(&self) {
        self.mmap_writable.fetch_sub(1, Ordering::AcqRel);
    }

    /// `mapping_deny_writable`: forbids writable shared mappings from now on, `EBUSY` while one
    /// exists.
    fn deny_writable(&self) -> FsResult<()> {
        self.mmap_writable
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n <= 0).then_some(n - 1)
            })
            .map(drop)
            .map_err(|_| Errno::EBUSY)
    }

    /// The file's seals; `None` for a file that cannot have any (`memfd_file_seals_ptr`).
    pub fn seals(&self) -> Option<SealFlags> {
        self.sealable
            .then(|| SealFlags(self.seals.load(Ordering::Acquire)))
    }

    /// Replaces the seals of a file nothing else can reach yet (`memfd_alloc_file`).
    pub(crate) fn init_seals(&self, seals: SealFlags) {
        self.seals.store(seals.0, Ordering::Release);
    }

    /// Takes the inode lock (`i_rwsem`) for a change seals may forbid, so the check and the
    /// change are one step against `F_ADD_SEALS`. Not taken where the seals are final and
    /// forbid nothing, as for every tmpfs file not made sealable.
    pub(crate) async fn seal_lock(&self) -> Option<DirLock> {
        match self.seals() {
            Some(seals) if seals != SealFlags::F_SEAL_SEAL => Some(self.lock().await),
            _ => None,
        }
    }

    /// `shmem_write_begin`'s checks: the part of `data`, to be written at `offset` (the end of
    /// the file for `None`), that the seals allow. `F_SEAL_GROW` cuts a write short at the end
    /// of the file, and fails one that starts there. Called under `seal_lock`.
    pub(crate) async fn sealed_write<'a>(
        &self,
        data: &'a [u8],
        offset: Option<u64>,
    ) -> FsResult<&'a [u8]> {
        let seals = self.seals().unwrap_or_default();
        if data.is_empty() || !seals.intersects(WRITE_SEALS | SealFlags::F_SEAL_GROW) {
            return Ok(data);
        }
        if seals.intersects(WRITE_SEALS) {
            return Err(Errno::EPERM);
        }
        let size = self.node().getattr().await?.size;
        let room = size.saturating_sub(offset.unwrap_or(size));
        match usize::try_from(room).map_or(data.len(), |room| room.min(data.len())) {
            0 => Err(Errno::EPERM),
            fits => Ok(&data[..fits]),
        }
    }

    /// `shmem_fallocate`'s checks. Zeroing a range changes data, as punching a hole does.
    /// Called under `seal_lock`.
    pub(crate) async fn sealed_allocate(
        &self,
        mode: AllocMode,
        offset: u64,
        len: u64,
    ) -> FsResult<()> {
        let seals = self.seals().unwrap_or_default();
        let changes_data = matches!(mode, AllocMode::PunchHole | AllocMode::ZeroRange { .. });
        if changes_data && seals.intersects(WRITE_SEALS) {
            return Err(Errno::EPERM);
        }
        // Even with FALLOC_FL_KEEP_SIZE.
        if mode != AllocMode::PunchHole && seals.contains(SealFlags::F_SEAL_GROW) {
            let size = self.node().getattr().await?.size;
            if offset.saturating_add(len) > size {
                return Err(Errno::EPERM);
            }
        }
        Ok(())
    }

    /// `shmem_setattr`'s checks: no change to the execute bits under `F_SEAL_EXEC`, and no
    /// size change the seals forbid. Called under `seal_lock`.
    pub(crate) async fn sealed_setattr(&self, attr: &SetAttr) -> FsResult<()> {
        let seals = self.seals().unwrap_or_default();
        let guarded = SealFlags::F_SEAL_EXEC | SealFlags::F_SEAL_SHRINK | SealFlags::F_SEAL_GROW;
        if !seals.intersects(guarded) || (attr.mode.is_none() && attr.size.is_none()) {
            return Ok(());
        }
        let current = self.node().getattr().await?;
        if let Some(mode) = attr.mode
            && seals.contains(SealFlags::F_SEAL_EXEC)
            && (current.mode ^ mode) & S_IXUGO != 0
        {
            return Err(Errno::EPERM);
        }
        if let Some(size) = attr.size
            && ((size < current.size && seals.contains(SealFlags::F_SEAL_SHRINK))
                || (size > current.size && seals.contains(SealFlags::F_SEAL_GROW)))
        {
            return Err(Errno::EPERM);
        }
        Ok(())
    }
}

impl Vfs {
    /// `memfd_add_seals`: adds `seals` to the file `file` is open on.
    pub async fn add_seals(&self, file: &File, seals: SealFlags) -> FsResult<()> {
        if !file.writable {
            return Err(Errno::EPERM);
        }
        if seals.0 & !ALL_SEALS.0 != 0 {
            return Err(Errno::EINVAL);
        }
        let inode = file.path.dentry.inode();
        let _lock = inode.lock().await;
        let current = inode.seals().ok_or(Errno::EINVAL)?;
        if current.contains(SealFlags::F_SEAL_SEAL) {
            return Err(Errno::EPERM);
        }
        let mut seals = seals;
        // F_SEAL_EXEC on an executable file makes it W^X from the start.
        if seals.contains(SealFlags::F_SEAL_EXEC)
            && inode.node().getattr().await?.mode & S_IXUGO != 0
        {
            seals = seals | SealFlags::F_SEAL_SHRINK | SealFlags::F_SEAL_GROW | WRITE_SEALS;
        }
        // No writable shared mapping may exist or appear once the file is write-sealed
        // (`memfd_wait_for_pins` has nothing to wait for: no page is ever pinned).
        if seals.contains(SealFlags::F_SEAL_WRITE) && !current.contains(SealFlags::F_SEAL_WRITE) {
            inode.deny_writable()?;
        }
        inode.seals.fetch_or(seals.0, Ordering::AcqRel);
        Ok(())
    }
}
