//! `ioctl`: the requests every descriptor understands, then the object's own.

use wasixv2_abi::{GuestPtr, GuestPtrMut, IoctlRequest, OpenFlags};
use wasixv2_syscalls::{Caller, Errno, Fd, SysResult, read_value, write_value};
use wasixv2_vfs::FileType;

use crate::{
    Kernel,
    file::{Backing, OpenFile},
    fs::FASYNC,
    nsfs::NsFile,
    sync::MutexExt,
    tty::TtyFile,
};

/// `_IOC_DIR`, `_IOC_TYPE`, `_IOC_NR` and `_IOC_SIZE` (`asm-generic/ioctl.h`): the fields of
/// a request number.
pub(crate) fn ioc_dir(request: IoctlRequest) -> u32 {
    request.0 >> 30
}

pub(crate) fn ioc_type(request: IoctlRequest) -> u32 {
    (request.0 >> 8) & 0xff
}

pub(crate) fn ioc_nr(request: IoctlRequest) -> u32 {
    request.0 & 0xff
}

pub(crate) fn ioc_size(request: IoctlRequest) -> u32 {
    (request.0 >> 16) & 0x3fff
}

/// `FIGETBSZ` (`_IO(0x00, 2)`, `linux/fs.h`): the filesystem's block size. Not among the ABI's
/// request names.
pub(crate) const FIGETBSZ: IoctlRequest = IoctlRequest(2);
/// `FICLONE` (`_IOW(0x94, 9, int)`), `FICLONERANGE` (`_IOW(0x94, 13, struct
/// file_clone_range)`) and `FIDEDUPERANGE` (`_IOWR(0x94, 54, struct file_dedupe_range)`),
/// from `linux/fs.h`; not among the ABI's request names.
const FICLONE: IoctlRequest = IoctlRequest(0x4004_9409);
const FICLONERANGE: IoctlRequest = IoctlRequest(0x4020_940d);
const FIDEDUPERANGE: IoctlRequest = IoctlRequest(0xc018_9436);
/// `sizeof(struct file_clone_range)`: `src_fd`, `src_offset`, `src_length`, `dest_offset`.
const FILE_CLONE_RANGE: usize = 32;
/// `sizeof(struct file_dedupe_range)` without its `info` array, and of one element.
const FILE_DEDUPE_RANGE: usize = 24;
const FILE_DEDUPE_RANGE_INFO: usize = 32;
/// The most a `FIDEDUPERANGE` request may take (`PAGE_SIZE`).
const DEDUPE_MAX: usize = 4096;

impl Kernel {
    /// Linux's `do_vfs_ioctl` followed by `vfs_ioctl`.
    pub(crate) async fn sys_ioctl(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        request: IoctlRequest,
        arg: u32,
    ) -> SysResult<i32> {
        // `fdget`: an `O_PATH` descriptor has no operations.
        let file = self.io_file(caller, fd)?;
        // `security_file_ioctl`: Landlock's `LANDLOCK_ACCESS_FS_IOCTL_DEV`.
        if !file.landlock_allows(crate::landlock::IOCTL_DEV)
            && matches!(
                file.path().dentry.file_type(),
                FileType::CharDevice | FileType::BlockDevice
            )
            && !crate::landlock::device_ioctl_masked(request)
        {
            return Err(Errno::EACCES.into());
        }
        if request == IoctlRequest::FUSE_DEV_IOC_CLONE
            && let Some(device) = file.downcast::<crate::fuse::Device>()
        {
            return self.clone_fuse(caller, device, arg).await;
        }
        // Regular files answer FIONREAD generically; other files ask their object.
        if request == IoctlRequest::FIONREAD
            && let Backing::Inode(inode) = &file.backing
            && inode.file_type() == FileType::Regular
        {
            let size = inode.node().getattr().await?.size;
            let left = size.saturating_sub(file.position()) as i32;
            write_value(caller.memory(), GuestPtrMut::new(arg), &left).await?;
            return Ok(0);
        }
        if request == IoctlRequest::IOC_WATCH_QUEUE_SET_SIZE
            && let Some(end) = file.downcast::<crate::pipe::PipeEnd>()
        {
            return Ok(end.set_watch_size(arg, self.pipe_user_unprivileged(caller))?);
        }
        if let Some(result) = self.fileattr_ioctl(caller, &file, request, arg).await {
            return result;
        }
        match request {
            IoctlRequest::FIOCLEX | IoctlRequest::FIONCLEX => {
                let cloexec = request == IoctlRequest::FIOCLEX;
                let files = self.files(caller.task());
                files.lock_or_recover().set_cloexec(fd, cloexec);
                Ok(0)
            }
            // `ioctl_fioasync`: a change goes to the object's `fasync`, and is ENOTTY for an
            // object without one; asking for what is already so is 0.
            IoctlRequest::FIOASYNC => {
                let on: i32 = read_value(caller.memory(), GuestPtr::new(arg)).await?;
                let flag = if on != 0 { FASYNC } else { 0 };
                if (flag ^ file.flags().0) & FASYNC == 0 {
                    return Ok(0);
                }
                self.fasync(caller, fd, &file, on != 0)
                    .unwrap_or(Err(Errno::ENOTTY))?;
                Ok(0)
            }
            // `inode_get_bytes` of a directory, regular file or symlink.
            IoctlRequest::FIOQSIZE => {
                let Backing::Inode(inode) = &file.backing else {
                    return Err(Errno::ENOTTY.into());
                };
                if !matches!(
                    inode.file_type(),
                    FileType::Directory | FileType::Regular | FileType::Symlink
                ) {
                    return Err(Errno::ENOTTY.into());
                }
                let bytes = inode.node().getattr().await?.blocks * 512;
                write_value(caller.memory(), GuestPtrMut::new(arg), &(bytes as i64)).await?;
                Ok(0)
            }
            // `s_blocksize`, for any file; EINVAL for a filesystem without one.
            FIGETBSZ => {
                let size = self.vfs.statfs(file.path()).await?.block_size;
                if size == 0 {
                    return Err(Errno::EINVAL.into());
                }
                write_value(caller.memory(), GuestPtrMut::new(arg), &(size as i32)).await?;
                Ok(0)
            }
            FICLONE => self.clone_file(caller, &file, arg as i32).await,
            FICLONERANGE => {
                let mut raw = [0; FILE_CLONE_RANGE];
                caller
                    .memory()
                    .read(arg, &mut raw)
                    .await
                    .map_err(|_| Errno::EFAULT)?;
                // `CLASS(fd, ...)` takes the descriptor as an `unsigned int`.
                let src_fd = i64::from_le_bytes(raw[..8].try_into().unwrap()) as i32;
                self.clone_file(caller, &file, src_fd).await
            }
            FIDEDUPERANGE => self.dedupe_file(caller, &file, arg).await,
            IoctlRequest::FIONBIO => {
                let on: i32 = read_value(caller.memory(), GuestPtr::new(arg)).await?;
                let flags = match on {
                    0 => file.flags() & !OpenFlags::O_NONBLOCK,
                    _ => file.flags() | OpenFlags::O_NONBLOCK,
                };
                file.set_status_flags(flags);
                Ok(0)
            }
            // ---- net ----
            // `sock_ioctl`: a socket's owner.
            IoctlRequest::FIOSETOWN | IoctlRequest::SIOCSPGRP
                if self.socket_net(&file).is_some() =>
            {
                let who: i32 = read_value(caller.memory(), GuestPtr::new(arg)).await?;
                self.f_setown(caller, &file, who)
            }
            IoctlRequest::FIOGETOWN | IoctlRequest::SIOCGPGRP
                if self.socket_net(&file).is_some() =>
            {
                let who = self.f_getown(caller, &file);
                write_value(caller.memory(), GuestPtrMut::new(arg), &who).await?;
                Ok(0)
            }
            _ if crate::net::is_dev_ioctl(request)
                && let Some(net) = self.socket_net(&file) =>
            {
                self.sys_dev_ioctl(caller, &file, &net, request, arg).await
            }
            _ => match file.object() {
                Some(_) if let Some(tty) = file.downcast::<TtyFile>() => {
                    self.tty_ioctl(caller, &file, tty, request, arg).await
                }
                Some(_) if let Some(ns) = file.downcast::<NsFile>() => {
                    self.ns_ioctl(caller, ns, request, arg).await
                }
                Some(_) if let Some(pidfd) = file.downcast::<crate::process::PidFd>() => {
                    self.pidfd_ioctl(caller, pidfd, request, arg).await
                }
                Some(_) if let Some(listener) = file.downcast::<crate::seccomp::Listener>() => {
                    self.seccomp_notify_ioctl(caller, listener, request, arg)
                        .await
                }
                Some(object) => object.ioctl(caller, request, arg).await,
                None => Err(Errno::ENOTTY.into()),
            },
        }
    }
}

impl Kernel {
    /// `ioctl_file_clone` into `dst` from descriptor `src_fd`: `vfs_clone_file_range`'s checks
    /// (`EXDEV` across filesystems, then `generic_file_rw_checks`), then `EOPNOTSUPP`, since
    /// no filesystem here shares extents between files (`remap_file_range`). The range only
    /// matters past that point, so it is not looked at.
    async fn clone_file(&self, caller: &Caller<'_>, dst: &OpenFile, src_fd: i32) -> SysResult<i32> {
        let src = self.io_file(caller, Fd(src_fd))?;
        if src.path().dentry.sb().id != dst.path().dentry.sb().id {
            return Err(Errno::EXDEV.into());
        }
        let types = [src.path().dentry.file_type(), dst.path().dentry.file_type()];
        if types.contains(&FileType::Directory) {
            return Err(Errno::EISDIR.into());
        }
        if types.iter().any(|&t| t != FileType::Regular) {
            return Err(Errno::EINVAL.into());
        }
        src.check_readable()?;
        dst.check_writable()?;
        if dst.flags().contains(OpenFlags::O_APPEND) {
            return Err(Errno::EBADF.into());
        }
        Err(Errno::EOPNOTSUPP.into())
    }

    /// `ioctl_file_dedupe_range` from `src`: the request is read whole (at most a page, else
    /// `ENOMEM`), then `vfs_dedupe_file_range`'s checks on the source, then `EOPNOTSUPP`,
    /// since no filesystem here can share extents (`remap_file_range`).
    async fn dedupe_file(&self, caller: &Caller<'_>, src: &OpenFile, arg: u32) -> SysResult<i32> {
        let memory = caller.memory();
        let mut count = [0; 2];
        let count_at = arg.checked_add(16).ok_or(Errno::EFAULT)?;
        memory
            .read(count_at, &mut count)
            .await
            .map_err(|_| Errno::EFAULT)?;
        let size = FILE_DEDUPE_RANGE + FILE_DEDUPE_RANGE_INFO * u16::from_le_bytes(count) as usize;
        if size > DEDUPE_MAX {
            return Err(Errno::ENOMEM.into());
        }
        let mut raw = vec![0; size];
        memory
            .read(arg, &mut raw)
            .await
            .map_err(|_| Errno::EFAULT)?;
        if src.check_readable().is_err() {
            return Err(Errno::EINVAL.into());
        }
        // `reserved1` and `reserved2`.
        if raw[18..24].iter().any(|&b| b != 0) {
            return Err(Errno::EINVAL.into());
        }
        match src.path().dentry.file_type() {
            FileType::Directory => Err(Errno::EISDIR.into()),
            FileType::Regular => Err(Errno::EOPNOTSUPP.into()),
            _ => Err(Errno::EINVAL.into()),
        }
    }
}
