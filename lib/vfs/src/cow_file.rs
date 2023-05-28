//! Used for /dev/zero - infinitely returns zero
//! which is useful for commands like `dd if=/dev/zero of=bigfile.img size=1G`

use std::io::{self, *};
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite, ReadBuf};

use crate::{BufferFile, VirtualFile};

#[derive(Debug)]
pub struct CopyOnWriteFile {
    inner: Option<Box<dyn VirtualFile + Send + Sync>>,
    buf: BufferFile,
}

impl CopyOnWriteFile {
    pub fn new(inner: Box<dyn VirtualFile + Send + Sync>) -> Self {
        Self {
            inner: Some(inner),
            buf: BufferFile::default(),
        }
    }
    fn poll_copy(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        if let Some(inner) = self.inner.as_mut() {
            let mut temp = [0u8; 8192];
            while self.buf.size() < inner.size() {
                let mut read_temp = ReadBuf::new(&mut temp);

                let inner = Pin::new(inner.as_mut());
                match inner.poll_read(cx, &mut read_temp) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                    Poll::Ready(Ok(())) => {}
                }
                if read_temp.remaining() <= 0 {
                    return Poll::Pending;
                }

                self.buf.data.write_all(read_temp.filled()).unwrap();
            }

            drop(inner);
            self.inner.take();
        }
        Poll::Ready(Ok(()))
    }
}

impl AsyncSeek for CopyOnWriteFile {
    fn start_seek(mut self: Pin<&mut Self>, position: io::SeekFrom) -> io::Result<()> {
        let data = Pin::new(&mut self.buf);
        data.start_seek(position)?;

        if let Some(inner) = self.inner.as_mut() {
            let data = Pin::new(inner.as_mut());
            data.start_seek(position)?;
        }

        Ok(())
    }

    fn poll_complete(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        match self.inner.as_mut() {
            Some(inner) => {
                let data = Pin::new(inner.as_mut());
                data.poll_complete(cx)
            }
            None => {
                let data = Pin::new(&mut self.buf);
                data.poll_complete(cx)
            }
        }
    }
}

impl AsyncWrite for CopyOnWriteFile {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.poll_copy(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            Poll::Ready(Ok(())) => {}
        }
        let data = Pin::new(&mut self.buf);
        data.poll_write(cx, buf)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        match self.poll_copy(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            Poll::Ready(Ok(())) => {}
        }
        let data = Pin::new(&mut self.buf);
        data.poll_write_vectored(cx, bufs)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.poll_copy(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            Poll::Ready(Ok(())) => {}
        }
        let data = Pin::new(&mut self.buf);
        data.poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.poll_copy(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            Poll::Ready(Ok(())) => {}
        }
        let data = Pin::new(&mut self.buf);
        data.poll_shutdown(cx)
    }
}

impl AsyncRead for CopyOnWriteFile {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.inner.as_mut() {
            Some(inner) => {
                let data = Pin::new(inner.as_mut());
                data.poll_read(cx, buf)
            }
            None => {
                let data = Pin::new(&mut self.buf);
                data.poll_read(cx, buf)
            }
        }
    }
}

impl VirtualFile for CopyOnWriteFile {
    fn last_accessed(&self) -> u64 {
        0
    }
    fn last_modified(&self) -> u64 {
        0
    }
    fn created_time(&self) -> u64 {
        0
    }
    fn size(&self) -> u64 {
        match self.inner.as_ref() {
            Some(inner) => inner.size(),
            None => self.buf.size(),
        }
    }
    fn set_len(&mut self, new_size: u64) -> crate::Result<()> {
        match self.inner.as_mut() {
            Some(inner) => inner.set_len(new_size),
            None => self.buf.set_len(new_size),
        }
    }
    fn unlink(&mut self) -> crate::Result<()> {
        Ok(())
    }
    fn poll_read_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        match self.inner.as_mut() {
            Some(inner) => {
                let data = Pin::new(inner.as_mut());
                data.poll_read_ready(cx)
            }
            None => {
                let data: Pin<&mut BufferFile> = Pin::new(&mut self.buf);
                data.poll_read_ready(cx)
            }
        }
    }

    fn poll_write_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(8192))
    }
}
