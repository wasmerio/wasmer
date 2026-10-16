//! Metadata syscalls: `statx`, `statfs`, access checks, ownership, modes, times and extended
//! attributes (`fs/stat.c`, `fs/statfs.c`, `fs/open.c`, `fs/utimes.c`, `fs/xattr.c`).

use wasixv2_abi::{
    AccessMode, AtFlags, ConstKernelTimespecArray, ConstXattrArgsPtr, Cstr, GuestPtr, KernelFsid,
    KernelTimespec, Statfs64, Statfs64Ptr, Statx, StatxAttributes, StatxMask, StatxPtr,
    StatxTimestamp, UtimeSpecial, XattrArgs, XattrArgsPtr, XattrFlags,
};
use wasixv2_syscalls::{
    Caller, CstrError, Errno, Fd, SysResult, UserMemory, UserSlice, UserSliceMut, read_cstr,
    read_value, write_value,
};
use wasixv2_vfs::{
    Attr, FileType, Path, TimeSet, Timestamp, XATTR_NAME_MAX, XATTR_SIZE_MAX, XattrSet,
};

use super::{Env, read_path};
use crate::Kernel;

/// `ST_*` bits of `statfs.f_flags`.
const ST_RDONLY: u32 = 1;
const ST_NOSUID: u32 = 2;
const ST_NODEV: u32 = 4;
const ST_NOEXEC: u32 = 8;
const ST_NOATIME: u32 = 1024;
const ST_VALID: u32 = 0x20;

/// Which ids an operation acts on: `-1` leaves one unchanged.
fn id(value: u32) -> Option<u32> {
    (value != u32::MAX).then_some(value)
}

fn timestamp(time: Timestamp) -> StatxTimestamp {
    StatxTimestamp {
        tv_sec: time.sec,
        tv_nsec: time.nsec,
        reserved: 0,
    }
}

impl Kernel {
    /// `statx`.
    pub(crate) async fn sys_statx(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        flags: AtFlags,
        mask: StatxMask,
        buffer: StatxPtr,
    ) -> SysResult<i32> {
        let known = AtFlags::AT_SYMLINK_NOFOLLOW
            | AtFlags::AT_NO_AUTOMOUNT
            | AtFlags::AT_EMPTY_PATH
            | AtFlags::AT_STATX_SYNC_TYPE;
        if !(flags & !known).is_empty()
            || flags & AtFlags::AT_STATX_SYNC_TYPE == AtFlags::AT_STATX_SYNC_TYPE
            || mask.contains(StatxMask::STATX__RESERVED)
        {
            return Err(Errno::EINVAL.into());
        }
        let path = read_path(caller.memory(), filename).await?;
        let env = self.env(caller);
        let target = self
            .resolve_at(caller, &env, dfd, &path, flags, true)
            .await?;
        let attr = self.vfs.stat(&target).await?;
        write_value(caller.memory(), buffer, &statx(&target, &attr)).await?;
        Ok(0)
    }

    pub(crate) async fn sys_statfs(
        &self,
        caller: &Caller<'_>,
        pathname: Cstr,
        size: u32,
        buf: Statfs64Ptr,
    ) -> SysResult<i32> {
        if size as usize != size_of::<Statfs64>() {
            return Err(Errno::EINVAL.into());
        }
        let path = read_path(caller.memory(), pathname).await?;
        let env = self.env(caller);
        let target = self
            .resolve_at(
                caller,
                &env,
                wasixv2_abi::Fd(wasixv2_abi::AtFd::AT_FDCWD.0),
                &path,
                AtFlags(0),
                true,
            )
            .await?;
        self.write_statfs(caller, &target, buf).await
    }

    pub(crate) async fn sys_fstatfs(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        size: u32,
        buf: Statfs64Ptr,
    ) -> SysResult<i32> {
        if size as usize != size_of::<Statfs64>() {
            return Err(Errno::EINVAL.into());
        }
        let target = self.fd_path(caller, fd)?;
        self.write_statfs(caller, &target, buf).await
    }

    async fn write_statfs(
        &self,
        caller: &Caller<'_>,
        path: &Path,
        buf: Statfs64Ptr,
    ) -> SysResult<i32> {
        let sb = path.dentry.sb();
        let stat = self.vfs.statfs(path).await?;
        let flags = &path.mnt.flags;
        let mut f_flags = ST_VALID;
        for (set, bit) in [
            (path.mnt.is_read_only(), ST_RDONLY),
            (flags.nosuid, ST_NOSUID),
            (flags.nodev, ST_NODEV),
            (flags.noexec, ST_NOEXEC),
            (flags.noatime, ST_NOATIME),
        ] {
            if set {
                f_flags |= bit;
            }
        }
        let out = Statfs64 {
            f_type: sb.info.magic,
            f_bsize: stat.block_size,
            f_blocks: stat.blocks,
            f_bfree: stat.blocks_free,
            f_bavail: stat.blocks_avail,
            f_files: stat.files,
            f_ffree: stat.files_free,
            f_fsid: KernelFsid {
                val: [(sb.dev.major << 20 | sb.dev.minor) as i32, 0],
            },
            f_namelen: stat.name_max,
            f_frsize: stat.fragment_size,
            f_flags,
            f_spare: [0; 4],
        };
        write_value(caller.memory(), buf, &out).await?;
        Ok(0)
    }

    /// `faccessat2`; `faccessat` passes no flags.
    pub(crate) async fn sys_faccessat2(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        mode: AccessMode,
        flags: AtFlags,
    ) -> SysResult<i32> {
        if mode.0 & !0o7 != 0 {
            return Err(Errno::EINVAL.into());
        }
        let known = AtFlags::AT_EACCESS | AtFlags::AT_SYMLINK_NOFOLLOW | AtFlags::AT_EMPTY_PATH;
        if !(flags & !known).is_empty() {
            return Err(Errno::EINVAL.into());
        }
        let path = read_path(caller.memory(), filename).await?;
        let mut env = self.env(caller);
        if !flags.contains(AtFlags::AT_EACCESS) {
            env.cred = self.cred(caller).fs_for_access();
        }
        let lookup_flags = flags & (AtFlags::AT_SYMLINK_NOFOLLOW | AtFlags::AT_EMPTY_PATH);
        let target = self
            .resolve_at(caller, &env, dfd, &path, lookup_flags, true)
            .await?;
        // R_OK, W_OK and X_OK are MAY_READ, MAY_WRITE and MAY_EXEC.
        self.vfs.access(&env.cx(), &target, mode.0).await?;
        Ok(0)
    }

    pub(crate) async fn sys_fchmod(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        mode: u32,
    ) -> SysResult<i32> {
        self.io_file(caller, fd)?;
        let target = self.fd_path(caller, fd)?;
        let env = self.env(caller);
        self.vfs.chmod(&env.cx(), &target, mode).await?;
        Ok(0)
    }

    pub(crate) async fn sys_fchmodat2(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        mode: u32,
        flags: AtFlags,
    ) -> SysResult<i32> {
        if !(flags & !(AtFlags::AT_SYMLINK_NOFOLLOW | AtFlags::AT_EMPTY_PATH)).is_empty() {
            return Err(Errno::EINVAL.into());
        }
        let path = read_path(caller.memory(), filename).await?;
        let env = self.env(caller);
        let target = self
            .resolve_at(caller, &env, dfd, &path, flags, true)
            .await?;
        // Symlink modes cannot change on Linux.
        if target.dentry.file_type() == FileType::Symlink {
            return Err(Errno::EOPNOTSUPP.into());
        }
        self.vfs.chmod(&env.cx(), &target, mode).await?;
        Ok(0)
    }

    pub(crate) async fn sys_fchown(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        user: u32,
        group: u32,
    ) -> SysResult<i32> {
        self.io_file(caller, fd)?;
        let target = self.fd_path(caller, fd)?;
        let env = self.env(caller);
        self.vfs
            .chown(&env.cx(), &target, id(user), id(group))
            .await?;
        Ok(0)
    }

    pub(crate) async fn sys_fchownat(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        user: u32,
        group: u32,
        flags: AtFlags,
    ) -> SysResult<i32> {
        if !(flags & !(AtFlags::AT_SYMLINK_NOFOLLOW | AtFlags::AT_EMPTY_PATH)).is_empty() {
            return Err(Errno::EINVAL.into());
        }
        let path = read_path(caller.memory(), filename).await?;
        let env = self.env(caller);
        let target = self
            .resolve_at(caller, &env, dfd, &path, flags, true)
            .await?;
        self.vfs
            .chown(&env.cx(), &target, id(user), id(group))
            .await?;
        Ok(0)
    }

    /// `utimensat`: a NULL path acts on `dfd` itself (`futimens`).
    pub(crate) async fn sys_utimensat(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        utimes: ConstKernelTimespecArray,
        flags: AtFlags,
    ) -> SysResult<i32> {
        let times = if utimes.is_null() {
            None
        } else {
            let atime: KernelTimespec = read_value(caller.memory(), utimes).await?;
            let second = utimes.addr().checked_add(16).ok_or(Errno::EFAULT)?;
            let mtime: KernelTimespec = read_value(caller.memory(), GuestPtr::new(second)).await?;
            Some([atime, mtime])
        };
        let env = self.env(caller);
        let target = if filename.is_null() {
            if !flags.is_empty() {
                return Err(Errno::EINVAL.into());
            }
            self.fd_path(caller, dfd)?
        } else {
            if !(flags & !(AtFlags::AT_SYMLINK_NOFOLLOW | AtFlags::AT_EMPTY_PATH)).is_empty() {
                return Err(Errno::EINVAL.into());
            }
            let path = read_path(caller.memory(), filename).await?;
            self.resolve_at(caller, &env, dfd, &path, flags, true)
                .await?
        };
        let (atime, mtime) = match times {
            None => (Some(TimeSet::Now), Some(TimeSet::Now)),
            Some([atime, mtime]) => (time_set(atime)?, time_set(mtime)?),
        };
        self.vfs.utimes(&env.cx(), &target, atime, mtime).await?;
        Ok(0)
    }

    // ---- extended attributes ----

    async fn xattr_target(
        &self,
        caller: &Caller<'_>,
        env: &Env,
        dfd: Fd,
        pathname: Cstr,
        flags: AtFlags,
        follow: bool,
    ) -> SysResult<Path> {
        if !(flags & !(AtFlags::AT_SYMLINK_NOFOLLOW | AtFlags::AT_EMPTY_PATH)).is_empty() {
            return Err(Errno::EINVAL.into());
        }
        let path = read_path(caller.memory(), pathname).await?;
        Ok(self
            .resolve_at(caller, env, dfd, &path, flags, follow)
            .await?)
    }

    fn xattr_fd(&self, caller: &Caller<'_>, fd: Fd) -> SysResult<Path> {
        self.io_file(caller, fd)?;
        Ok(self.fd_path(caller, fd)?)
    }

    pub(crate) async fn xattr_set(
        &self,
        caller: &Caller<'_>,
        target: Path,
        name: Cstr,
        value: UserSlice,
        flags: XattrFlags,
    ) -> SysResult<i32> {
        if !(flags & !(XattrFlags::XATTR_CREATE | XattrFlags::XATTR_REPLACE)).is_empty() {
            return Err(Errno::EINVAL.into());
        }
        let name = read_xattr_name(caller.memory(), name).await?;
        if value.len as usize > XATTR_SIZE_MAX {
            return Err(Errno::E2BIG.into());
        }
        let data = value.copy_in(caller.memory(), XATTR_SIZE_MAX).await?;
        let env = self.env(caller);
        let how = match (
            flags.contains(XattrFlags::XATTR_CREATE),
            flags.contains(XattrFlags::XATTR_REPLACE),
        ) {
            (true, true) => {
                // Both can never hold: it either exists (EEXIST) or not (ENODATA).
                return match self.vfs.getxattr(&env.cx(), &target, &name).await {
                    Ok(_) => Err(Errno::EEXIST.into()),
                    Err(errno) => Err(errno.into()),
                };
            }
            (true, false) => XattrSet::Create,
            (false, true) => XattrSet::Replace,
            (false, false) => XattrSet::Any,
        };
        self.vfs
            .setxattr(&env.cx(), &target, &name, &data, how)
            .await?;
        Ok(0)
    }

    pub(crate) async fn xattr_get(
        &self,
        caller: &Caller<'_>,
        target: Path,
        name: Cstr,
        value: UserSliceMut,
    ) -> SysResult<u32> {
        let name = read_xattr_name(caller.memory(), name).await?;
        let env = self.env(caller);
        let data = self.vfs.getxattr(&env.cx(), &target, &name).await?;
        copy_sized(caller, &data, value).await
    }

    pub(crate) async fn xattr_list(
        &self,
        caller: &Caller<'_>,
        target: Path,
        list: UserSliceMut,
    ) -> SysResult<u32> {
        let env = self.env(caller);
        let data = self.vfs.listxattr(&env.cx(), &target).await?;
        copy_sized(caller, &data, list).await
    }

    pub(crate) async fn xattr_remove(
        &self,
        caller: &Caller<'_>,
        target: Path,
        name: Cstr,
    ) -> SysResult<i32> {
        let name = read_xattr_name(caller.memory(), name).await?;
        let env = self.env(caller);
        self.vfs.removexattr(&env.cx(), &target, &name).await?;
        Ok(0)
    }

    pub(crate) async fn xattr_path(
        &self,
        caller: &Caller<'_>,
        pathname: Cstr,
        follow: bool,
    ) -> SysResult<Path> {
        let env = self.env(caller);
        let flags = if follow {
            AtFlags(0)
        } else {
            AtFlags::AT_SYMLINK_NOFOLLOW
        };
        self.xattr_target(
            caller,
            &env,
            wasixv2_abi::Fd(wasixv2_abi::AtFd::AT_FDCWD.0),
            pathname,
            flags,
            true,
        )
        .await
    }

    pub(crate) async fn xattr_at(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        pathname: Cstr,
        flags: AtFlags,
    ) -> SysResult<Path> {
        let env = self.env(caller);
        self.xattr_target(caller, &env, dfd, pathname, flags, true)
            .await
    }

    pub(crate) fn xattr_of_fd(&self, caller: &Caller<'_>, fd: Fd) -> SysResult<Path> {
        self.xattr_fd(caller, fd)
    }

    /// Reads `struct xattr_args` for `setxattrat`/`getxattrat`.
    pub(crate) async fn xattr_args(
        &self,
        caller: &Caller<'_>,
        addr: u32,
        size: u32,
    ) -> SysResult<XattrArgs> {
        if (size as usize) < size_of::<XattrArgs>() {
            return Err(Errno::EINVAL.into());
        }
        let args: XattrArgs = read_value(caller.memory(), ConstXattrArgsPtr::new(addr)).await?;
        if args.value > u64::from(u32::MAX) {
            return Err(Errno::EFAULT.into());
        }
        Ok(args)
    }
}

impl Kernel {
    #[allow(clippy::too_many_arguments)] // mirrors the syscall
    pub(crate) async fn sys_getxattrat(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        pathname: Cstr,
        at_flags: AtFlags,
        name: Cstr,
        uargs: XattrArgsPtr,
        size: u32,
    ) -> SysResult<i32> {
        let args = self.xattr_args(caller, uargs.addr(), size).await?;
        if args.flags != 0 {
            return Err(Errno::EINVAL.into());
        }
        let target = self.xattr_at(caller, dfd, pathname, at_flags).await?;
        let value = UserSliceMut::new(args.value as u32, args.size);
        Ok(self.xattr_get(caller, target, name, value).await? as i32)
    }

    #[allow(clippy::too_many_arguments)] // mirrors the syscall
    pub(crate) async fn sys_setxattrat(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        pathname: Cstr,
        at_flags: AtFlags,
        name: Cstr,
        uargs: ConstXattrArgsPtr,
        size: u32,
    ) -> SysResult<i32> {
        let args = self.xattr_args(caller, uargs.addr(), size).await?;
        let target = self.xattr_at(caller, dfd, pathname, at_flags).await?;
        let value = UserSlice::new(args.value as u32, args.size);
        self.xattr_set(caller, target, name, value, XattrFlags(args.flags))
            .await
    }
}

/// `st_mode`. Anonymous inodes have no file type (`anon_inode_getattr`).
fn st_mode(path: &Path, attr: &Attr) -> u32 {
    match path.dentry.sb().info.magic {
        crate::pseudo::ANON_INODE_FS_MAGIC => attr.mode,
        _ => attr.st_mode(),
    }
}

fn statx(path: &Path, attr: &Attr) -> Statx {
    let dev = attr.dev.unwrap_or_default();
    let mut mask = StatxMask::STATX_BASIC_STATS | StatxMask::STATX_MNT_ID;
    if attr.btime.is_some() {
        mask |= StatxMask::STATX_BTIME;
    }
    let mut attributes = StatxAttributes(0);
    if path.is_mount_root() {
        attributes = StatxAttributes(StatxAttributes::STATX_ATTR_MOUNT_ROOT.0);
    }
    Statx {
        stx_mask: mask,
        stx_blksize: attr.blksize,
        stx_attributes: attributes,
        stx_nlink: attr.nlink,
        stx_uid: attr.uid,
        stx_gid: attr.gid,
        stx_mode: st_mode(path, attr) as u16,
        stx_ino: attr.ino,
        stx_size: attr.size,
        stx_blocks: attr.blocks,
        stx_attributes_mask: StatxAttributes(
            StatxAttributes::STATX_ATTR_MOUNT_ROOT.0 | StatxAttributes::STATX_ATTR_AUTOMOUNT.0,
        ),
        stx_atime: timestamp(attr.atime),
        stx_btime: attr.btime.map(timestamp).unwrap_or_default(),
        stx_ctime: timestamp(attr.ctime),
        stx_mtime: timestamp(attr.mtime),
        stx_rdev_major: attr.rdev.major,
        stx_rdev_minor: attr.rdev.minor,
        stx_dev_major: dev.major,
        stx_dev_minor: dev.minor,
        stx_mnt_id: path.mnt.id,
        ..Statx::default()
    }
}

/// One `utimensat` time: `UTIME_OMIT` is `None`.
fn time_set(time: KernelTimespec) -> Result<Option<TimeSet>, Errno> {
    match time.tv_nsec {
        n if n == i64::from(UtimeSpecial::UTIME_OMIT.0) => Ok(None),
        n if n == i64::from(UtimeSpecial::UTIME_NOW.0) => Ok(Some(TimeSet::Now)),
        n if (0..1_000_000_000).contains(&n) => {
            Ok(Some(TimeSet::At(Timestamp::new(time.tv_sec, n as u32))))
        }
        _ => Err(Errno::EINVAL),
    }
}

async fn read_xattr_name(memory: &dyn UserMemory, name: Cstr) -> Result<Vec<u8>, Errno> {
    let name = read_cstr(memory, name, XATTR_NAME_MAX + 1)
        .await
        .map_err(|err| match err {
            CstrError::Fault => Errno::EFAULT,
            CstrError::TooLong => Errno::ERANGE,
        })?;
    if name.is_empty() {
        return Err(Errno::ERANGE);
    }
    Ok(name)
}

/// The `getxattr`/`listxattr` convention: size 0 asks for the length; a short buffer is `ERANGE`.
async fn copy_sized(caller: &Caller<'_>, data: &[u8], out: UserSliceMut) -> SysResult<u32> {
    if out.len == 0 {
        return Ok(data.len() as u32);
    }
    if (out.len as usize) < data.len() {
        return Err(Errno::ERANGE.into());
    }
    out.copy_out(caller.memory(), data).await?;
    Ok(data.len() as u32)
}
