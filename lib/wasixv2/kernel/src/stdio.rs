use std::{io, sync::Arc};

use wasixv2_host::{ByteSink, ByteSource};
use wasixv2_syscalls::{Errno, LocalFuture};

use crate::file::FileObject;

/// The host streams a new process gets as descriptors 0, 1 and 2.
pub struct Stdio {
    pub stdin: Arc<dyn ByteSource>,
    pub stdout: Arc<dyn ByteSink>,
    pub stderr: Arc<dyn ByteSink>,
}

impl Stdio {
    pub(crate) fn into_objects(self) -> [Arc<dyn FileObject>; 3] {
        [
            Arc::new(HostInput(self.stdin)),
            Arc::new(HostOutput(self.stdout)),
            Arc::new(HostOutput(self.stderr)),
        ]
    }
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
