use super::*;

use crate::VirtualFile;

/// Wraps a [`VirtualFile`], and also invokes a provided function for each write.
///
/// Useful for debugging.
#[derive(derive_more::Debug)]
pub struct DualWriteFile {
    inner: Box<dyn VirtualFile + Send + Sync + 'static>,
    #[allow(clippy::type_complexity)]
    #[debug(ignore)]
    extra_write: Box<dyn FnMut(&[u8]) + Send + Sync + 'static>,
}

impl DualWriteFile {
    pub fn new(
        inner: Box<dyn VirtualFile + Send + Sync + 'static>,
        funct: impl FnMut(&[u8]) + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner,
            extra_write: Box::new(funct),
        }
    }
}

impl VirtualFile for DualWriteFile {
    fn last_accessed(&self) -> u64 {
        self.inner.last_accessed()
    }

    fn last_modified(&self) -> u64 {
        self.inner.last_modified()
    }

    fn created_time(&self) -> u64 {
        self.inner.created_time()
    }

    fn set_times(&mut self, atime: Option<u64>, mtime: Option<u64>) -> crate::Result<()> {
        self.inner.set_times(atime, mtime)
    }

    fn size(&self) -> u64 {
        self.inner.size()
    }

    fn set_len(&mut self, new_size: u64) -> crate::Result<()> {
        self.inner.set_len(new_size)
    }

    fn unlink(&mut self) -> Result<()> {
        self.inner.unlink()
    }

    fn poll_read_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Pin::new(self.inner.as_mut()).poll_read_ready(cx)
    }

    fn poll_write_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Pin::new(self.inner.as_mut()).poll_write_ready(cx)
    }
}

impl AsyncWrite for DualWriteFile {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match Pin::new(&mut self.inner).poll_write(cx, buf) {
            Poll::Ready(Ok(amt)) => {
                if amt > 0 {
                    (self.extra_write)(&buf[..amt]);
                }
                Poll::Ready(Ok(amt))
            }
            res => res,
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

impl AsyncRead for DualWriteFile {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncSeek for DualWriteFile {
    fn start_seek(mut self: Pin<&mut Self>, position: io::SeekFrom) -> io::Result<()> {
        Pin::new(&mut self.inner).start_seek(position)
    }

    fn poll_complete(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<io::Result<u64>> {
        Pin::new(&mut self.inner).poll_complete(cx)
    }
}
