//! Named pipes (`fs/pipe.c`'s `fifo_open`): opening a FIFO inode attaches an end of the pipe
//! bound to that inode, allocating it on the first open.

use wasixv2_abi::{IoctlRequest, OpenFlags};
use wasixv2_syscalls::{Caller, Errno, LocalFuture, SysResult};
use wasixv2_vfs::{File, Path};
use wasixv2_wait::{Interrupt, PollMask, PollTable, Pollable};

use crate::{
    file::{FileObject, Io, OpenFile},
    pipe::{Pipe, PipeEnd, fifo_pipe},
};

/// An open FIFO: the pipe ends its access mode asks for, and the VFS file that keeps the inode
/// (and so the pipe binding) alive.
pub(crate) struct FifoFile {
    read: Option<PipeEnd>,
    write: Option<PipeEnd>,
    pub(crate) file: File,
}

impl FifoFile {
    pub(crate) fn path(&self) -> &Path {
        &self.file.path
    }
}

/// `fifo_open`. Blocking opens wait for the other side; a non-blocking writer without readers
/// is `ENXIO`; `O_RDWR` never waits. A signal interrupts the wait.
pub(crate) async fn open(
    file: File,
    flags: OpenFlags,
    interrupt: &dyn Interrupt,
) -> SysResult<FifoFile> {
    let inode = file.path.dentry.inode().clone();
    let pipe = inode
        .bound_or_bind(|| fifo_pipe())
        .downcast::<Pipe>()
        .map_err(|_| Errno::ENXIO)?;
    let nonblock = flags.contains(OpenFlags::O_NONBLOCK);
    let (read, write) = match flags & OpenFlags::O_ACCMODE {
        OpenFlags::O_RDONLY => {
            let end = PipeEnd::attach(&pipe, false);
            if !nonblock {
                pipe.wait_ends(|_, writers| writers > 0, interrupt).await?;
            }
            (Some(end), None)
        }
        OpenFlags::O_WRONLY => {
            if nonblock && pipe.ends().0 == 0 {
                return Err(Errno::ENXIO.into());
            }
            let end = PipeEnd::attach(&pipe, true);
            if !nonblock {
                pipe.wait_ends(|readers, _| readers > 0, interrupt).await?;
            }
            (None, Some(end))
        }
        _ => (
            Some(PipeEnd::attach(&pipe, false)),
            Some(PipeEnd::attach(&pipe, true)),
        ),
    };
    Ok(FifoFile { read, write, file })
}

impl Pollable for FifoFile {
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        let mut mask = PollMask::empty();
        if let Some(end) = &self.read {
            mask |= end.poll(table.as_deref_mut());
        }
        if let Some(end) = &self.write {
            mask |= end.poll(table);
        }
        // `O_RDWR` holds both ends, so neither side ever hangs up.
        if self.read.is_some() && self.write.is_some() {
            mask &= !(PollMask::HUP | PollMask::ERR);
        }
        mask
    }
}

impl FileObject for FifoFile {
    fn read<'a>(&'a self, io: Io<'a>, buf: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        match &self.read {
            Some(end) => end.read(io, buf),
            None => Box::pin(async { Err(Errno::EBADF.into()) }),
        }
    }

    fn write<'a>(&'a self, io: Io<'a>, data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        match &self.write {
            Some(end) => end.write(io, data),
            None => Box::pin(async { Err(Errno::EBADF.into()) }),
        }
    }

    fn ioctl<'a>(
        &'a self,
        caller: &'a Caller<'a>,
        request: IoctlRequest,
        arg: u32,
    ) -> LocalFuture<'a, SysResult<i32>> {
        match self.read.as_ref().or(self.write.as_ref()) {
            Some(end) => end.ioctl(caller, request, arg),
            None => Box::pin(async { Err(Errno::ENOTTY.into()) }),
        }
    }
}

/// The FIFO's own path, for the fd-based calls that act on the inode (`fstat`, `fchmod`).
pub(crate) fn path_of(file: &OpenFile) -> Option<Path> {
    file.downcast::<FifoFile>().map(|fifo| fifo.path().clone())
}
