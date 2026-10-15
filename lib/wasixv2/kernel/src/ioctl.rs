//! `ioctl`: the requests every descriptor understands, then the object's own.

use wasixv2_abi::{GuestPtr, GuestPtrMut, IoctlRequest, OpenFlags};
use wasixv2_syscalls::{Caller, Errno, Fd, SysResult, read_value, write_value};
use wasixv2_vfs::FileType;

use crate::{Kernel, file::Backing};

impl Kernel {
    /// Linux's `do_vfs_ioctl` followed by `vfs_ioctl`.
    pub(crate) async fn sys_ioctl(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        request: IoctlRequest,
        arg: u32,
    ) -> SysResult<i32> {
        let file = self.file(caller.task(), fd)?;
        // Regular files answer FIONREAD generically; other files ask their object.
        if request == IoctlRequest::FIONREAD
            && let Backing::Inode(inode) = &file.backing
            && inode.file_type() == FileType::Regular
        {
            let left = inode.size().saturating_sub(file.position()) as i32;
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
            IoctlRequest::FIONBIO => {
                let on: i32 = read_value(caller.memory(), GuestPtr::new(arg)).await?;
                let flags = match on {
                    0 => file.flags() & !OpenFlags::O_NONBLOCK,
                    _ => file.flags() | OpenFlags::O_NONBLOCK,
                };
                file.set_status_flags(flags);
                Ok(0)
            }
            _ => match file.object() {
                Some(object) => object.ioctl(caller, request, arg).await,
                None => Err(Errno::ENOTTY.into()),
            },
        }
    }
}
