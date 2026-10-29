//! Inode flags and extended attributes of files (`fs/file_attr.c`): the `FS_IOC_GETFLAGS`,
//! `FS_IOC_SETFLAGS`, `FS_IOC_FSGETXATTR` and `FS_IOC_FSSETXATTR` requests, and
//! `file_getattr`/`file_setattr`. The VFS checks and enforces them.

use wasixv2_abi::{
    AtFlags, ConstFileAttrPtr, Cstr, FileAttr, FileAttrPtr, FileAttrSize, Fsxattr, GuestPtr,
    GuestPtrMut, IoctlRequest,
};
use wasixv2_syscalls::{Caller, Errno, Fd, SysResult, read_value, write_value};
use wasixv2_vfs::{FS_XFLAG_RDONLY_MASK, FS_XFLAGS_MASK, FileKattr, Path};

use super::read_path;
use crate::{Kernel, file::OpenFile};

/// The most `file_getattr`/`file_setattr` take (`PAGE_SIZE`).
const MAX_SIZE: usize = 4096;
const SIZE: usize = FileAttrSize::FILE_ATTR_SIZE_LATEST.0 as usize;

impl Kernel {
    /// The inode-flag requests of `do_vfs_ioctl`, for a file with an inode; `None` for any
    /// other request.
    pub(crate) async fn fileattr_ioctl(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        request: IoctlRequest,
        arg: u32,
    ) -> Option<SysResult<i32>> {
        let path = file.inode().ok()?.path.clone();
        let result = match request {
            IoctlRequest::FS_IOC_GETFLAGS => self.ioctl_getflags(caller, &path, arg).await,
            IoctlRequest::FS_IOC_SETFLAGS => self.ioctl_setflags(caller, &path, arg).await,
            IoctlRequest::FS_IOC_FSGETXATTR => self.ioctl_fsgetxattr(caller, &path, arg).await,
            IoctlRequest::FS_IOC_FSSETXATTR => self.ioctl_fssetxattr(caller, &path, arg).await,
            _ => return None,
        };
        Some(result.map(|()| 0))
    }

    /// `ioctl_getflags`.
    async fn ioctl_getflags(&self, caller: &Caller<'_>, path: &Path, arg: u32) -> SysResult<()> {
        let fa = self.vfs.fileattr_get(path).await?;
        write_value(caller.memory(), GuestPtrMut::new(arg), &fa.flags).await?;
        Ok(())
    }

    /// `ioctl_setflags`.
    async fn ioctl_setflags(&self, caller: &Caller<'_>, path: &Path, arg: u32) -> SysResult<()> {
        let flags: u32 = read_value(caller.memory(), GuestPtr::new(arg)).await?;
        let env = self.env(caller);
        let fa = FileKattr::from_flags(flags);
        self.vfs.fileattr_set(&env.cx(), path, fa).await?;
        Ok(())
    }

    /// `ioctl_fsgetxattr` (`copy_fsxattr_to_user`).
    async fn ioctl_fsgetxattr(&self, caller: &Caller<'_>, path: &Path, arg: u32) -> SysResult<()> {
        let fa = self.vfs.fileattr_get(path).await?;
        let out = Fsxattr {
            fsx_xflags: fa.xflags & FS_XFLAGS_MASK,
            fsx_extsize: fa.extsize,
            fsx_nextents: fa.nextents,
            fsx_projid: fa.projid,
            fsx_cowextsize: fa.cowextsize,
            fsx_pad: [0; 8],
        };
        write_value(caller.memory(), GuestPtrMut::new(arg), &out).await?;
        Ok(())
    }

    /// `ioctl_fssetxattr` (`copy_fsxattr_from_user`): unknown flags are `EOPNOTSUPP`.
    async fn ioctl_fssetxattr(&self, caller: &Caller<'_>, path: &Path, arg: u32) -> SysResult<()> {
        let xfa: Fsxattr = read_value(caller.memory(), GuestPtr::new(arg)).await?;
        if xfa.fsx_xflags & !FS_XFLAGS_MASK != 0 {
            return Err(Errno::EOPNOTSUPP.into());
        }
        let fa = FileKattr {
            extsize: xfa.fsx_extsize,
            nextents: xfa.fsx_nextents,
            projid: xfa.fsx_projid,
            cowextsize: xfa.fsx_cowextsize,
            ..FileKattr::from_xflags(xfa.fsx_xflags & !FS_XFLAG_RDONLY_MASK)
        };
        let env = self.env(caller);
        self.vfs.fileattr_set(&env.cx(), path, fa).await?;
        Ok(())
    }

    /// The object of `file_getattr`/`file_setattr`: `dfd`'s file with a NULL or empty
    /// name and `AT_EMPTY_PATH`, else the name looked up (`filename_maybe_null`).
    async fn fileattr_target(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        at_flags: AtFlags,
        usize: u32,
    ) -> SysResult<Path> {
        let known = AtFlags::AT_SYMLINK_NOFOLLOW | AtFlags::AT_EMPTY_PATH;
        if !(at_flags & !known).is_empty() {
            return Err(Errno::EINVAL.into());
        }
        if usize as usize > MAX_SIZE {
            return Err(Errno::E2BIG.into());
        }
        if (usize as usize) < SIZE {
            return Err(Errno::EINVAL.into());
        }
        let empty_ok = at_flags.contains(AtFlags::AT_EMPTY_PATH);
        let path = match filename.is_null() {
            true if empty_ok => Vec::new(),
            true => return Err(Errno::EFAULT.into()),
            false => read_path(caller.memory(), filename).await?,
        };
        let env = self.env(caller);
        Ok(self
            .resolve_at(caller, &env, dfd, &path, at_flags, true)
            .await?)
    }

    /// `sys_file_getattr`.
    pub(crate) async fn sys_file_getattr(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        ufattr: FileAttrPtr,
        usize: u32,
        at_flags: AtFlags,
    ) -> SysResult<i32> {
        let path = self
            .fileattr_target(caller, dfd, filename, at_flags, usize)
            .await?;
        let fa = match self.vfs.fileattr_get(&path).await {
            Err(Errno::ENOTTY) => return Err(Errno::EOPNOTSUPP.into()),
            result => result?,
        };
        let out = FileAttr {
            fa_xflags: u64::from(fa.xflags & FS_XFLAGS_MASK),
            fa_extsize: fa.extsize,
            fa_nextents: fa.nextents,
            fa_projid: fa.projid,
            fa_cowextsize: fa.cowextsize,
        };
        write_value(caller.memory(), ufattr, &out).await?;
        // `copy_struct_to_user`: a larger structure is zeroed past ours.
        if usize as usize > SIZE {
            let zeros = vec![0; usize as usize - SIZE];
            caller
                .memory()
                .write(ufattr.addr() + SIZE as u32, &zeros)
                .await
                .map_err(|_| Errno::EFAULT)?;
        }
        Ok(0)
    }

    /// `sys_file_setattr`.
    pub(crate) async fn sys_file_setattr(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        ufattr: ConstFileAttrPtr,
        usize: u32,
        at_flags: AtFlags,
    ) -> SysResult<i32> {
        let known = AtFlags::AT_SYMLINK_NOFOLLOW | AtFlags::AT_EMPTY_PATH;
        if !(at_flags & !known).is_empty() {
            return Err(Errno::EINVAL.into());
        }
        if usize as usize > MAX_SIZE {
            return Err(Errno::E2BIG.into());
        }
        if (usize as usize) < SIZE {
            return Err(Errno::EINVAL.into());
        }
        let fattr: FileAttr = read_value(caller.memory(), ufattr).await?;
        // `copy_struct_from_user`: what is past our structure must be zero.
        if usize as usize > SIZE {
            let mut rest = vec![0; usize as usize - SIZE];
            caller
                .memory()
                .read(ufattr.addr() + SIZE as u32, &mut rest)
                .await
                .map_err(|_| Errno::EFAULT)?;
            if rest.iter().any(|&b| b != 0) {
                return Err(Errno::E2BIG.into());
            }
        }
        // `file_attr_to_fileattr`.
        if fattr.fa_xflags & !u64::from(FS_XFLAGS_MASK) != 0 {
            return Err(Errno::EINVAL.into());
        }
        let xflags = fattr.fa_xflags as u32 & !FS_XFLAG_RDONLY_MASK;
        let fa = FileKattr {
            extsize: fattr.fa_extsize,
            projid: fattr.fa_projid,
            cowextsize: fattr.fa_cowextsize,
            ..FileKattr::from_xflags(xflags)
        };
        let path = self
            .fileattr_target(caller, dfd, filename, at_flags, usize)
            .await?;
        let env = self.env(caller);
        match self.vfs.fileattr_set(&env.cx(), &path, fa).await {
            Err(Errno::ENOTTY) => Err(Errno::EOPNOTSUPP.into()),
            result => Ok(result.map(|()| 0)?),
        }
    }
}
