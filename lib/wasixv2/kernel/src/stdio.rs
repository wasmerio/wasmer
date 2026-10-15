use std::{io, sync::Arc};

use wasixv2_abi::OpenFlags;
use wasixv2_host::{ByteSink, ByteSource, Stdio};
use wasixv2_syscalls::{Errno, LocalFuture, SysResult};
use wasixv2_wait::{PollMask, PollTable, Pollable};

use crate::file::{Backing, FileObject, Io, OpenFile};

/// The open files behind descriptors 0, 1 and 2.
pub(crate) fn open_files(stdio: Stdio) -> [OpenFile; 3] {
    let input = |source| Backing::Stream(Arc::new(HostInput(source)));
    let output = |sink| Backing::Stream(Arc::new(HostOutput(sink)));
    [
        OpenFile::new(input(stdio.stdin), OpenFlags::O_RDONLY),
        OpenFile::new(output(stdio.stdout), OpenFlags::O_WRONLY),
        OpenFile::new(output(stdio.stderr), OpenFlags::O_WRONLY),
    ]
}

// Host byte streams have no readiness to report, so they always look ready, as a file without
// a poll operation does on Linux, and a read may then still wait for input.
// TODO: readiness once `ByteSource` can report it; O_NONBLOCK reads still block until then.

struct HostInput(Arc<dyn ByteSource>);

impl Pollable for HostInput {
    fn poll(&self, _: Option<&mut PollTable<'_>>) -> PollMask {
        PollMask::DEFAULT
    }
}

impl FileObject for HostInput {
    fn read<'a>(&'a self, _: Io<'a>, buf: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move { Ok(self.0.read(buf).await.map_err(io_errno)?) })
    }

    fn write<'a>(&'a self, _: Io<'a>, _: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        // Linux reports writes to a descriptor opened read-only as EBADF.
        Box::pin(async { Err(Errno::EBADF.into()) })
    }
}

struct HostOutput(Arc<dyn ByteSink>);

impl Pollable for HostOutput {
    fn poll(&self, _: Option<&mut PollTable<'_>>) -> PollMask {
        PollMask::DEFAULT
    }
}

impl FileObject for HostOutput {
    fn read<'a>(&'a self, _: Io<'a>, _: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async { Err(Errno::EBADF.into()) })
    }

    fn write<'a>(&'a self, _: Io<'a>, data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move { Ok(self.0.write(data).await.map_err(io_errno)?) })
    }
}

fn io_errno(err: io::Error) -> Errno {
    match err.kind() {
        io::ErrorKind::BrokenPipe => Errno::EPIPE,
        io::ErrorKind::WouldBlock => Errno::EAGAIN,
        io::ErrorKind::Interrupted => Errno::EINTR,
        _ => Errno::EIO,
    }
}
