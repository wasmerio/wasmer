use std::{io, sync::Arc};

use wasixv2_abi::OpenFlags;
use wasixv2_host::{ByteSink, ByteSource, Stdio};
use wasixv2_syscalls::{Errno, LocalFuture};

use crate::file::{Backing, FileObject, OpenFile};

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

struct HostInput(Arc<dyn ByteSource>);

impl FileObject for HostInput {
    fn read<'a>(&'a self, buf: &'a mut [u8]) -> LocalFuture<'a, Result<usize, Errno>> {
        Box::pin(async move { self.0.read(buf).await.map_err(io_errno) })
    }

    fn write<'a>(&'a self, _: &'a [u8]) -> LocalFuture<'a, Result<usize, Errno>> {
        // Linux reports writes to a descriptor opened read-only as EBADF.
        Box::pin(async { Err(Errno::EBADF) })
    }
}

struct HostOutput(Arc<dyn ByteSink>);

impl FileObject for HostOutput {
    fn read<'a>(&'a self, _: &'a mut [u8]) -> LocalFuture<'a, Result<usize, Errno>> {
        Box::pin(async { Err(Errno::EBADF) })
    }

    fn write<'a>(&'a self, data: &'a [u8]) -> LocalFuture<'a, Result<usize, Errno>> {
        Box::pin(async move { self.0.write(data).await.map_err(io_errno) })
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
