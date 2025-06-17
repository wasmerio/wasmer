//! Used for /dev/zero - infinitely returns zero
//! which is useful for commands like `dd if=/dev/zero of=bigfile.img size=1G`

use derive_more::Debug;
use replace_with::replace_with_or_abort;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::{
    future::Future,
    io::{self, *},
};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt, AsyncWrite};

use crate::{BufferFile, VirtualFile};

#[derive(Debug)]
enum CowState {
    ReadOnly(Box<dyn VirtualFile + Send + Sync>),
    Copying {
        #[debug(skip)]
        future: Pin<Box<dyn Future<Output = io::Result<BufferFile>> + Send + Sync>>,
        requested_size: Option<u64>,
        requested_position: Option<SeekFrom>,
        cached_size: u64,
    },
    Copied(BufferFile),
}

impl CowState {
    fn inner_mut(&mut self) -> &mut (dyn VirtualFile + Send + Sync) {
        match self {
            Self::ReadOnly(inner) => inner.as_mut(),
            Self::Copying { .. } => panic!("Cannot access inner file while copying"),
            Self::Copied(inner) => inner,
        }
    }
}

#[derive(Debug)]
pub struct CopyOnWriteFile {
    last_accessed: u64,
    last_modified: u64,
    created_time: u64,
    state: CowState,
}

impl CopyOnWriteFile {
    pub fn new(inner: Box<dyn VirtualFile + Send + Sync>) -> Self {
        Self {
            last_accessed: inner.last_accessed(),
            last_modified: inner.last_modified(),
            created_time: inner.created_time(),
            state: CowState::ReadOnly(inner),
        }
    }

    async fn copy(mut inner: Box<dyn VirtualFile + Send + Sync>) -> io::Result<BufferFile> {
        let initial_position = inner.seek(SeekFrom::Current(0)).await?;
        inner.seek(SeekFrom::Start(0)).await?;

        let mut buffer = [0u8; 8192];
        let mut buffer_file = BufferFile::default();
        loop {
            let read_bytes = inner.read_buf(&mut &mut buffer[..]).await?;
            if read_bytes == 0 {
                break;
            }
            buffer_file.data.write_all(&buffer[0..read_bytes])?;
        }

        buffer_file.seek(SeekFrom::Start(initial_position)).await?;

        Ok(buffer_file)
    }

    fn poll_copy_progress(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        match self.state {
            CowState::Copying {
                ref mut future,
                requested_size,
                requested_position,
                ..
            } => match future.as_mut().poll(cx) {
                Poll::Ready(Ok(mut buf)) => {
                    if let Some(requested_size) = requested_size {
                        buf.set_len(requested_size)?;
                    }
                    if let Some(requested_position) = requested_position {
                        Pin::new(&mut buf).start_seek(requested_position)?;
                    }
                    self.state = CowState::Copied(buf);
                    Poll::Ready(Ok(()))
                }
                Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                Poll::Pending => Poll::Pending,
            },
            _ => Poll::Ready(Ok(())),
        }
    }

    fn start_copy(&mut self) {
        replace_with_or_abort(&mut self.state, |state| match state {
            CowState::ReadOnly(inner) => CowState::Copying {
                cached_size: inner.size(),
                requested_size: None,
                requested_position: None,
                future: Box::pin(Self::copy(inner)),
            },
            state => state,
        });
    }

    fn poll_copy_start_and_progress(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        self.start_copy();
        self.poll_copy_progress(cx)
    }
}

impl AsyncSeek for CopyOnWriteFile {
    fn start_seek(mut self: Pin<&mut Self>, position: io::SeekFrom) -> io::Result<()> {
        match self.state {
            CowState::Copying {
                ref mut requested_position,
                ..
            } => {
                *requested_position = Some(position);
                Ok(())
            }

            _ => Pin::new(self.state.inner_mut()).start_seek(position),
        }
    }

    fn poll_complete(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        match self.poll_copy_progress(cx) {
            Poll::Ready(Ok(())) => {}
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
        }

        Pin::new(self.state.inner_mut()).poll_complete(cx)
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
        Pin::new(self.state.inner_mut()).poll_write(cx, buf)
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
        Pin::new(self.state.inner_mut()).poll_write_vectored(cx, bufs)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.poll_copy_start_and_progress(cx) {
            Poll::Ready(Ok(())) => {}
            p => return p,
        }
        Pin::new(self.state.inner_mut()).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.poll_copy_start_and_progress(cx) {
            Poll::Ready(Ok(())) => {}
            p => return p,
        }
        Pin::new(self.state.inner_mut()).poll_shutdown(cx)
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
        Pin::new(self.state.inner_mut()).poll_read(cx, buf)
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
        match &self.state {
            CowState::ReadOnly(inner) => inner.size(),
            CowState::Copying {
                requested_size: Some(size),
                ..
            } => *size,
            CowState::Copying { cached_size, .. } => *cached_size,
            CowState::Copied(buffer_file) => buffer_file.size(),
        }
    }

    fn set_len(&mut self, new_size: u64) -> crate::Result<()> {
        match self.state {
            CowState::ReadOnly(_) => {
                self.start_copy();
                let CowState::Copying {
                    ref mut requested_size,
                    ..
                } = self.state
                else {
                    unreachable!()
                };
                *requested_size = Some(new_size);
            }

            CowState::Copying {
                ref mut requested_size,
                ..
            } => {
                *requested_size = Some(new_size);
            }

            CowState::Copied(ref mut buf) => {
                buf.set_len(new_size)?;
            }
        }

        Ok(())
    }

    fn unlink(&mut self) -> crate::Result<()> {
        // TODO: one can imagine interrupting an in-progress copy here
        self.set_len(0)
    }

    fn poll_read_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        match self.poll_copy_progress(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            Poll::Ready(Ok(())) => {}
        }
        Pin::new(self.state.inner_mut()).poll_read_ready(cx)
    }

    fn poll_write_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        self.poll_copy_progress(cx).map_ok(|_| 8192)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // This is as weird a test as it gets, yes, but I'm (unashamedly!) cramming
    // everything we know was wrong with the impl into this one test to save time.
    #[tokio::test]
    async fn cow_file_works() {
        let mut data = Vec::with_capacity(16385);
        for i in 0..16385 {
            data.push(i as u8);
        }
        let inner = BufferFile {
            data: Cursor::new(data),
        };
        let mut file = CopyOnWriteFile::new(Box::new(inner));

        assert!(matches!(file.state, CowState::ReadOnly(_)));
        assert_eq!(file.size(), 16385);
        assert_ne!(file.created_time(), 0);
        assert_ne!(file.last_accessed(), 0);
        assert_ne!(file.last_modified(), 0);

        let mut buf = [0u8; 4];
        let read = file.read_exact(buf.as_mut()).await.unwrap();
        assert_eq!(read, 4);
        assert_eq!(buf, [0, 1, 2, 3]);
        assert_eq!(file.seek(SeekFrom::Current(0)).await.unwrap(), 4);
        assert!(matches!(file.state, CowState::ReadOnly { .. }));

        // After this call, the file will "start" copying, but the actual
        // future won't be polled until we try to read or write.
        file.start_copy();
        assert!(matches!(file.state, CowState::Copying { .. }));
        assert_eq!(file.size(), 16385);

        // The cached length should be returned while copying
        file.set_len(16400).unwrap();
        assert!(matches!(file.state, CowState::Copying { .. }));
        assert_eq!(file.size(), 16400);

        // Now try to read from the file, which will trigger the copy
        let read = file.read_exact(buf.as_mut()).await.unwrap();
        assert!(matches!(file.state, CowState::Copied { .. }));
        assert_eq!(read, 4);
        assert_eq!(buf, [4, 5, 6, 7]);
        assert_eq!(file.seek(SeekFrom::Current(0)).await.unwrap(), 8);
        assert_eq!(file.size(), 16400);

        file.seek(SeekFrom::Start(16383)).await.unwrap();
        let read = file.read_exact(buf.as_mut()).await.unwrap();
        assert_eq!(read, 4);
        // set_len should have filled the rest with zeroes
        assert_eq!(buf, [(16383 % 256) as u8, (16384 % 256) as u8, 0, 0]);
        assert_eq!(file.seek(SeekFrom::Current(0)).await.unwrap(), 16387);
        assert!(matches!(file.state, CowState::Copied { .. }));
    }
}
