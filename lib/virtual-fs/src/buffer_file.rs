//! Used for /dev/zero - infinitely returns zero
//! which is useful for commands like `dd if=/dev/zero of=bigfile.img size=1G`

use std::io::{self, *};
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite};

use crate::VirtualFile;

#[derive(Debug, Default)]
pub struct BufferFile {
    pub(crate) data: Cursor<Vec<u8>>,
}

impl AsyncSeek for BufferFile {
    fn start_seek(mut self: Pin<&mut Self>, position: io::SeekFrom) -> io::Result<()> {
        let data = Pin::new(&mut self.data);
        data.start_seek(position)
    }

    fn poll_complete(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        let data = Pin::new(&mut self.data);
        data.poll_complete(cx)
    }
}

impl AsyncWrite for BufferFile {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let data = Pin::new(&mut self.data);
        data.poll_write(cx, buf)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let data = Pin::new(&mut self.data);
        data.poll_write_vectored(cx, bufs)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let data = Pin::new(&mut self.data);
        data.poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let data = Pin::new(&mut self.data);
        data.poll_shutdown(cx)
    }
}

impl AsyncRead for BufferFile {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let data = Pin::new(&mut self.data);
        data.poll_read(cx, buf)
    }
}

impl VirtualFile for BufferFile {
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
        self.data.get_ref().len() as u64
    }
    fn set_len(&mut self, new_size: u64) -> crate::Result<()> {
        self.data.get_mut().resize(new_size as usize, 0);
        Ok(())
    }
    fn unlink(&mut self) -> crate::Result<()> {
        Ok(())
    }
    fn poll_read_ready(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        let cur = self.data.stream_position().unwrap_or_default();
        let len = self.data.seek(SeekFrom::End(0)).unwrap_or_default();
        if cur < len {
            Poll::Ready(Ok((len - cur) as usize))
        } else {
            Poll::Ready(Ok(0))
        }
    }

    fn poll_write_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(8192))
    }
}
