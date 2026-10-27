//! `ioctl`: the requests every descriptor understands, then the object's own.

use wasixv2_abi::{GuestPtr, GuestPtrMut, IoctlRequest, OpenFlags};
use wasixv2_syscalls::{Caller, Errno, Fd, SysResult, read_value, write_value};
use wasixv2_vfs::FileType;

use crate::{Kernel, file::Backing, fs::FASYNC};

/// `FIGETBSZ` (`_IO(0x00, 2)`, `linux/fs.h`): the filesystem's block size. Not among the ABI's
/// request names.
const FIGETBSZ: IoctlRequest = IoctlRequest(2);

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
        match request {
            IoctlRequest::FIOCLEX | IoctlRequest::FIONCLEX => {
                let cloexec = request == IoctlRequest::FIOCLEX;
                let files = self.files(caller.task());
                files.lock().unwrap().set_cloexec(fd, cloexec);
                Ok(0)
            }
            // `ioctl_fioasync`: no object here has `fasync`, so turning it on (or off, once
            // `O_ASYNC` came from open) is ENOTTY, and asking for what is already so is 0.
            IoctlRequest::FIOASYNC => {
                let on: i32 = read_value(caller.memory(), GuestPtr::new(arg)).await?;
                let flag = if on != 0 { FASYNC } else { 0 };
                match (flag ^ file.flags().0) & FASYNC {
                    0 => Ok(0),
                    _ => Err(Errno::ENOTTY.into()),
                }
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
            _ if crate::net::is_dev_ioctl(request) && self.is_socket(&file) => {
                self.sys_dev_ioctl(caller, request, arg).await
            }
            _ => match file.object() {
                Some(object) => object.ioctl(caller, request, arg).await,
                None => Err(Errno::ENOTTY.into()),
            },
        }
    }
}
