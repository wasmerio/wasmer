//! Used for sharing references to the same file across multiple file systems,
//! effectively this is a symbolic link without all the complex path redirection

use std::{
    io::{self, *},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures::lock::Mutex;
use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite};

use crate::VirtualFile;

#[derive(Debug, Clone)]
pub struct ArcBoxFile {
    inner: Arc<Mutex<Box<dyn VirtualFile + Send + Sync + 'static>>>,
}

impl ArcBoxFile {
    pub fn new(inner: Box<dyn VirtualFile + Send + Sync + 'static>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(inner)),
        }
    }
}

impl AsyncSeek for ArcBoxFile {
    fn start_seek(self: Pin<&mut Self>, position: SeekFrom) -> io::Result<()> {
        let mut guard = self.inner.try_lock().ok_or_else(lock_would_block)?;
        let file = Pin::new(guard.as_mut());
        file.start_seek(position)
    }
    fn poll_complete(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        poll_with_inner(&self.inner, cx, |file, cx| file.poll_complete(cx))
    }
}

impl AsyncWrite for ArcBoxFile {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        poll_with_inner(&self.inner, cx, |file, cx| file.poll_write(cx, buf))
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        poll_with_inner(&self.inner, cx, |file, cx| file.poll_flush(cx))
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        poll_with_inner(&self.inner, cx, |file, cx| file.poll_shutdown(cx))
    }
    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        poll_with_inner(&self.inner, cx, |file, cx| {
            file.poll_write_vectored(cx, bufs)
        })
    }
    fn is_write_vectored(&self) -> bool {
        let mut guard = match self.inner.try_lock() {
            Some(guard) => guard,
            None => return false,
        };
        let file = Pin::new(guard.as_mut());
        file.is_write_vectored()
    }
}

impl AsyncRead for ArcBoxFile {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        poll_with_inner(&self.inner, cx, |file, cx| file.poll_read(cx, buf))
    }
}

#[async_trait::async_trait]
impl VirtualFile for ArcBoxFile {
    async fn last_accessed(&self) -> u64 {
        let inner = self.inner.lock().await;
        inner.last_accessed().await
    }
    async fn last_modified(&self) -> u64 {
        let inner = self.inner.lock().await;
        inner.last_modified().await
    }
    async fn created_time(&self) -> u64 {
        let inner = self.inner.lock().await;
        inner.created_time().await
    }
    async fn set_times(&mut self, atime: Option<u64>, mtime: Option<u64>) -> crate::Result<()> {
        let mut inner = self.inner.lock().await;
        inner.set_times(atime, mtime).await
    }
    async fn size(&self) -> u64 {
        let inner = self.inner.lock().await;
        inner.size().await
    }
    async fn set_len(&mut self, new_size: u64) -> crate::Result<()> {
        let mut inner = self.inner.lock().await;
        inner.set_len(new_size).await
    }
    async fn unlink(&mut self) -> crate::Result<()> {
        let mut inner = self.inner.lock().await;
        inner.unlink().await
    }
    async fn is_open(&self) -> bool {
        let inner = self.inner.lock().await;
        inner.is_open().await
    }
    async fn get_special_fd(&self) -> Option<u32> {
        let inner = self.inner.lock().await;
        inner.get_special_fd().await
    }
    fn poll_read_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        poll_with_inner(&self.inner, cx, |file, cx| file.poll_read_ready(cx))
    }
    fn poll_write_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        poll_with_inner(&self.inner, cx, |file, cx| file.poll_write_ready(cx))
    }
}

fn lock_would_block() -> io::Error {
    io::Error::new(io::ErrorKind::WouldBlock, "shared file is locked")
}

fn poll_with_inner<R>(
    inner: &Mutex<Box<dyn VirtualFile + Send + Sync + 'static>>,
    cx: &mut Context<'_>,
    f: impl FnOnce(
        Pin<&mut (dyn VirtualFile + Send + Sync + 'static)>,
        &mut Context<'_>,
    ) -> Poll<io::Result<R>>,
) -> Poll<io::Result<R>> {
    let Some(mut guard) = inner.try_lock() else {
        cx.waker().wake_by_ref();
        return Poll::Pending;
    };
    f(Pin::new(guard.as_mut()), cx)
}

impl From<Box<dyn VirtualFile + Send + Sync + 'static>> for ArcBoxFile {
    fn from(val: Box<dyn VirtualFile + Send + Sync + 'static>) -> Self {
        ArcBoxFile::new(val)
    }
}
