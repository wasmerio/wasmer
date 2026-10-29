//! Named pipes (`fs/pipe.c`'s `fifo_open`): opening a FIFO inode attaches an end of the pipe
//! bound to that inode, allocating it on the first open. A pipe's own inode on pipefs, opened
//! again through `/proc/<pid>/fd`, attaches the same way but never waits for a partner.

use std::sync::Arc;

use wasixv2_abi::{IoctlRequest, OpenFlags};
use wasixv2_syscalls::{Caller, Errno, LocalFuture, SysResult};
use wasixv2_vfs::File;
use wasixv2_wait::{Interrupt, PollMask, PollTable, Pollable};

use crate::{
    fasync::Fasync,
    file::{FileObject, Io, ReadSink},
    pipe::{Pipe, PipeEnd, fifo_pipe},
    pseudo::PIPEFS_MAGIC,
};

/// An open FIFO: the pipe ends its access mode asks for, and the VFS file that keeps the inode
/// (and so the pipe binding) alive.
pub(crate) struct FifoFile {
    pipe: Arc<Pipe>,
    read: Option<PipeEnd>,
    write: Option<PipeEnd>,
    _file: File,
}

/// `fifo_open`. Blocking opens wait for the other side; a non-blocking writer without readers
/// is `ENXIO`; `O_RDWR` never waits. A signal interrupts the wait. None of this applies to a
/// pipe's inode (`is_pipe`): its other side is whatever the pipe has now, so a reader after
/// the last writer sees end of file and a writer after the last reader `EPIPE`.
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
    let is_pipe = file.path.dentry.sb().info.magic == PIPEFS_MAGIC;
    let nonblock = flags.contains(OpenFlags::O_NONBLOCK);
    let (read, write) = match flags & OpenFlags::O_ACCMODE {
        OpenFlags::O_RDONLY => {
            let (mut end, writers) = PipeEnd::attach(&pipe, false);
            if !is_pipe && writers.open == 0 {
                match nonblock {
                    true => end.hup_after_writer(writers),
                    false => pipe.wait_for_partner(true, writers, interrupt).await?,
                }
            }
            (Some(end), None)
        }
        OpenFlags::O_WRONLY => {
            if !is_pipe && nonblock && pipe.readers() == 0 {
                return Err(Errno::ENXIO.into());
            }
            let (end, readers) = PipeEnd::attach(&pipe, true);
            if !is_pipe && !nonblock && readers.open == 0 {
                pipe.wait_for_partner(false, readers, interrupt).await?;
            }
            (None, Some(end))
        }
        _ => (
            Some(PipeEnd::attach(&pipe, false).0),
            Some(PipeEnd::attach(&pipe, true).0),
        ),
    };
    Ok(FifoFile {
        pipe,
        read,
        write,
        _file: file,
    })
}

impl FifoFile {
    /// The pipe bound to the FIFO.
    pub(crate) fn pipe(&self) -> &Arc<Pipe> {
        &self.pipe
    }
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

    fn read_to<'a>(
        &'a self,
        io: Io<'a>,
        buf: &'a mut [u8],
        to: &'a mut dyn ReadSink,
    ) -> LocalFuture<'a, SysResult<usize>> {
        match &self.read {
            Some(end) => end.read_to(io, buf, to),
            None => Box::pin(async { Err(Errno::EBADF.into()) }),
        }
    }

    fn write<'a>(&'a self, io: Io<'a>, data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        match &self.write {
            Some(end) => end.write(io, data),
            None => Box::pin(async { Err(Errno::EBADF.into()) }),
        }
    }

    fn buffered(&self) -> usize {
        self.read.as_ref().map_or(0, |end| end.buffered())
    }

    /// `pipe_fasync`: on the readers' list with a read end, the writers' with a write end.
    fn fasync(&self, fasync: &Fasync<'_>) -> Option<Result<(), Errno>> {
        let (read, write) = (self.read.is_some(), self.write.is_some());
        self.pipe.fasync(fasync, read, write);
        Some(Ok(()))
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
