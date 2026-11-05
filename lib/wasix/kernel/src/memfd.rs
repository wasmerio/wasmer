//! Anonymous memory files (`memfd_create`) and file seals, as Linux's `mm/memfd.c`.
//!
//! A memfd is an unnamed regular file on the machine's internal shmem filesystem, which the
//! embedder supplies with [`Kernel::with_shm`]. The VFS keeps the seals and enforces them on
//! writes, size changes and allocations; mappings are checked here.

use std::sync::Arc;

use wasixv2_abi::{
    Cstr, FcntlCmd, HugetlbEncoding, MapFlags, MemfdFlags, OpenFlags, ProtFlags, SealFlags,
};
use wasixv2_syscalls::{Caller, CstrError, Errno, Fd, SysResult, read_cstr};
use wasixv2_vfs::{File, NAME_MAX, NewKind, NewNode, WRITE_SEALS};

use crate::{
    Kernel,
    file::{Backing, OpenFile},
};

/// What every memfd's name starts with, in `/proc/<pid>/fd` and `/proc/<pid>/maps`.
const NAME_PREFIX: &[u8] = b"memfd:";
/// `MFD_NAME_MAX_LEN`: the longest name, so the prefixed one fits a dentry.
const NAME_MAX_LEN: usize = NAME_MAX - NAME_PREFIX.len();

/// `MFD_ALL_FLAGS`.
const ALL_FLAGS: MemfdFlags = MemfdFlags(
    MemfdFlags::MFD_CLOEXEC.0
        | MemfdFlags::MFD_ALLOW_SEALING.0
        | MemfdFlags::MFD_HUGETLB.0
        | MemfdFlags::MFD_NOEXEC_SEAL.0
        | MemfdFlags::MFD_EXEC.0,
);

/// `sanitize_flags`, with `vm.memfd_noexec` at its default of 0: a memfd is executable unless
/// asked otherwise.
fn sanitize_flags(flags: MemfdFlags) -> Result<MemfdFlags, Errno> {
    let mut valid = ALL_FLAGS.0;
    if flags.contains(MemfdFlags::MFD_HUGETLB) {
        // The huge page size may be encoded in the flags.
        valid |= HugetlbEncoding::MFD_HUGE_MASK.0 << HugetlbEncoding::MFD_HUGE_SHIFT.0;
    }
    if flags.0 & !valid != 0 {
        return Err(Errno::EINVAL);
    }
    if flags.contains(MemfdFlags::MFD_EXEC | MemfdFlags::MFD_NOEXEC_SEAL) {
        return Err(Errno::EINVAL);
    }
    if !flags.intersects(MemfdFlags::MFD_EXEC | MemfdFlags::MFD_NOEXEC_SEAL) {
        return Ok(flags | MemfdFlags::MFD_EXEC);
    }
    Ok(flags)
}

/// `alloc_name`: `memfd:` and the guest's name, at most `NAME_MAX_LEN` bytes of it.
async fn alloc_name(caller: &Caller<'_>, uname: Cstr) -> Result<Vec<u8>, Errno> {
    // Linux never maps page 0 (`mmap_min_addr`); wasm memory starts there.
    if uname.is_null() {
        return Err(Errno::EFAULT);
    }
    let name = read_cstr(caller.memory(), uname, NAME_MAX_LEN + 1)
        .await
        .map_err(|err| match err {
            CstrError::Fault => Errno::EFAULT,
            CstrError::TooLong => Errno::EINVAL,
        })?;
    Ok([NAME_PREFIX, &name].concat())
}

impl Kernel {
    /// `memfd_create`.
    pub(crate) async fn sys_memfd_create(
        &self,
        caller: &Caller<'_>,
        uname: Cstr,
        flags: MemfdFlags,
    ) -> SysResult<Fd> {
        let flags = sanitize_flags(flags)?;
        let name = alloc_name(caller, uname).await?;
        let cloexec = flags.contains(MemfdFlags::MFD_CLOEXEC);
        let [fd] = self.reserve_fds(caller, cloexec)?;
        let file = self.memfd_alloc_file(caller, &name, flags).await?;
        let flags = OpenFlags::O_RDWR | OpenFlags::O_LARGEFILE;
        Ok(fd.install(Arc::new(OpenFile::new(Backing::Inode(file), flags))))
    }

    /// `memfd_alloc_file`: an empty file owned by the caller with mode 0777, or 0666 and
    /// `F_SEAL_EXEC` for `MFD_NOEXEC_SEAL`. Sealing is allowed with `MFD_ALLOW_SEALING` or
    /// `MFD_NOEXEC_SEAL`.
    async fn memfd_alloc_file(
        &self,
        caller: &Caller<'_>,
        name: &[u8],
        flags: MemfdFlags,
    ) -> Result<File, Errno> {
        // There is no hugetlbfs (`hugetlb_file_setup` without `CONFIG_HUGETLBFS`).
        if flags.contains(MemfdFlags::MFD_HUGETLB) {
            return Err(Errno::ENOSYS);
        }
        let shm = self.shm.as_ref().ok_or(Errno::ENOSYS)?;
        let (mode, seals) = if flags.contains(MemfdFlags::MFD_NOEXEC_SEAL) {
            (0o666, SealFlags::F_SEAL_EXEC)
        } else if flags.contains(MemfdFlags::MFD_ALLOW_SEALING) {
            (0o777, SealFlags::empty())
        } else {
            (0o777, SealFlags::F_SEAL_SEAL)
        };
        let cred = self.env(caller).cred;
        let new = NewNode {
            umask: 0,
            kind: NewKind::Regular,
            mode,
            uid: cred.fsuid,
            gid: cred.fsgid,
            task: caller.task().0,
            quota_exempt: cred.capable_init(wasixv2_vfs::Cap::SysResource),
        };
        self.vfs.open_unnamed(shm, name, &new, seals).await
    }

    /// `memfd_fcntl`: `F_ADD_SEALS` and `F_GET_SEALS`, `EINVAL` for a file without seals.
    pub(crate) async fn memfd_fcntl(
        &self,
        file: &OpenFile,
        cmd: FcntlCmd,
        arg: u32,
    ) -> SysResult<i32> {
        let Backing::Inode(inner) = &file.backing else {
            return Err(Errno::EINVAL.into());
        };
        if cmd == FcntlCmd::F_ADD_SEALS {
            self.vfs.add_seals(inner, SealFlags(arg)).await?;
            return Ok(0);
        }
        let seals = inner.path.dentry.inode().seals().ok_or(Errno::EINVAL)?;
        Ok(seals.0 as i32)
    }
}

/// `memfd_check_seals_mmap`: no new writable shared mapping of a write-sealed file.
pub(crate) fn check_seals_mmap(file: &File, prot: ProtFlags, flags: MapFlags) -> Result<(), Errno> {
    let seals = file.path.dentry.inode().seals().unwrap_or_default();
    // A shared mapping of a file not open for writing is private as far as seals go.
    let shared = matches!(
        flags & MapFlags::MAP_TYPE,
        MapFlags::MAP_SHARED | MapFlags::MAP_SHARED_VALIDATE
    ) && file.writable;
    if seals.intersects(WRITE_SEALS) && shared && prot.contains(ProtFlags::PROT_WRITE) {
        return Err(Errno::EPERM);
    }
    Ok(())
}
