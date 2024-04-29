//! Used for /dev/zero - infinitely returns zero
//! which is useful for commands like `dd if=/dev/zero of=bigfile.img size=1G`

use replace_with::replace_with_or_abort;
use std::io::{self, *};
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite, ReadBuf};

use crate::{BufferFile, VirtualFile};

#[derive(Debug)]
enum CowState {
    ReadOnly(Box<dyn VirtualFile + Send + Sync>),
    Copying {
        pos: u64,
        inner: Box<dyn VirtualFile + Send + Sync>,
    },
    Copied,
}
impl CowState {
    fn as_ref(&self) -> Option<&(dyn VirtualFile + Send + Sync)> {
        match self {
            Self::ReadOnly(inner) => Some(inner.as_ref()),
            Self::Copying { inner, .. } => Some(inner.as_ref()),
            _ => None,
        }
    }
    fn as_mut(&mut self) -> Option<&mut Box<dyn VirtualFile + Send + Sync>> {
        match self {
            Self::ReadOnly(inner) => Some(inner),
            Self::Copying { inner, .. } => Some(inner),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct CopyOnWriteFile {
    last_accessed: u64,
    last_modified: u64,
    created_time: u64,
    state: CowState,
    buf: BufferFile,
}

impl CopyOnWriteFile {
    pub fn new(inner: Box<dyn VirtualFile + Send + Sync>) -> Self {
        Self {
            last_accessed: inner.last_accessed(),
            last_modified: inner.last_modified(),
            created_time: inner.created_time(),
            state: CowState::ReadOnly(inner),
            buf: BufferFile::default(),
        }
    }
    fn poll_copy_progress(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        if let CowState::Copying { ref mut inner, pos } = &mut self.state {
            let mut temp = [0u8; 8192];

            while *pos < inner.size() {
                let mut read_temp = ReadBuf::new(&mut temp);

                if let Err(err) = Pin::new(inner.as_mut()).start_seek(SeekFrom::Start(*pos)) {
                    return Poll::Ready(Err(err));
                }
                match Pin::new(inner.as_mut()).poll_complete(cx).map_ok(|_| ()) {
                    Poll::Ready(Ok(())) => {}
                    p => return p,
                }
                match Pin::new(inner.as_mut()).poll_read(cx, &mut read_temp) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                    Poll::Ready(Ok(())) => {}
                }
                if read_temp.remaining() == 0 {
                    return Poll::Pending;
                }
                *pos += read_temp.remaining() as u64;

                self.buf.data.write_all(read_temp.filled()).unwrap();
            }
            self.state = CowState::Copied;
        }
        Poll::Ready(Ok(()))
    }
    fn poll_copy_start_and_progress(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        replace_with_or_abort(&mut self.state, |state| match state {
            CowState::ReadOnly(inner) => CowState::Copying { pos: 0, inner },
            state => state,
        });
        self.poll_copy_progress(cx)
    }
}

impl AsyncSeek for CopyOnWriteFile {
    fn start_seek(mut self: Pin<&mut Self>, position: io::SeekFrom) -> io::Result<()> {
        let data = Pin::new(&mut self.buf);
        data.start_seek(position)?;

        if let Some(inner) = self.state.as_mut() {
            Pin::new(inner.as_mut()).start_seek(position)?;
        }

        Ok(())
    }

    fn poll_complete(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        match self.state.as_mut() {
            Some(inner) => Pin::new(inner.as_mut()).poll_complete(cx),
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
        match self.poll_copy_start_and_progress(cx) {
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
        match self.poll_copy_start_and_progress(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            Poll::Ready(Ok(())) => {}
        }
        let data = Pin::new(&mut self.buf);
        data.poll_write_vectored(cx, bufs)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.poll_copy_start_and_progress(cx) {
            Poll::Ready(Ok(())) => {}
            p => return p,
        }
        let data = Pin::new(&mut self.buf);
        data.poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.poll_copy_start_and_progress(cx) {
            Poll::Ready(Ok(())) => {}
            p => return p,
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
        match self.poll_copy_progress(cx) {
            Poll::Ready(Ok(())) => {}
            p => return p,
        }
        match self.state.as_mut() {
            Some(inner) => Pin::new(inner.as_mut()).poll_read(cx, buf),
            None => {
                let data = Pin::new(&mut self.buf);
                data.poll_read(cx, buf)
            }
        }
    }
}

impl VirtualFile for CopyOnWriteFile {
    fn last_accessed(&self) -> u64 {
        self.last_accessed
    }
    fn last_modified(&self) -> u64 {
        self.last_modified
    }
    fn created_time(&self) -> u64 {
        self.created_time
    }
    fn set_times(&mut self, atime: Option<u64>, mtime: Option<u64>) -> crate::Result<()> {
        if let Some(atime) = atime {
            self.last_accessed = atime;
        }
        if let Some(mtime) = mtime {
            self.last_modified = mtime;
        }

        Ok(())
    }
    fn size(&self) -> u64 {
        match self.state.as_ref() {
            Some(inner) => inner.size(),
            None => self.buf.size(),
        }
    }
    fn set_len(&mut self, new_size: u64) -> crate::Result<()> {
        self.buf.set_len(new_size)
    }
    fn unlink(&mut self) -> crate::Result<()> {
        self.buf.set_len(0)
    }
    fn poll_read_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        match self.poll_copy_progress(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            Poll::Ready(Ok(())) => {}
        }
        match self.state.as_mut() {
            Some(inner) => Pin::new(inner.as_mut()).poll_read_ready(cx),
            None => {
                let data: Pin<&mut BufferFile> = Pin::new(&mut self.buf);
                data.poll_read_ready(cx)
            }
        }
    }

    fn poll_write_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        self.poll_copy_progress(cx).map_ok(|_| 8192)
    }
}
