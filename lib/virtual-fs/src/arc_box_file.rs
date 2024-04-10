//! Used for sharing references to the same file across multiple file systems,
//! effectively this is a symbolic link without all the complex path redirection

use std::{
    io::{self, *},
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

use derivative::Derivative;
use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite};

use crate::VirtualFile;

#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct ArcBoxFile {
    #[derivative(Debug = "ignore")]
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
        let mut guard = self.inner.lock().unwrap();
        let file = Pin::new(guard.as_mut());
        file.start_seek(position)
    }
    fn poll_complete(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        let mut guard = self.inner.lock().unwrap();
        let file = Pin::new(guard.as_mut());
        file.poll_complete(cx)
    }
}

impl AsyncWrite for ArcBoxFile {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut guard = self.inner.lock().unwrap();
        let file = Pin::new(guard.as_mut());
        file.poll_write(cx, buf)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut guard = self.inner.lock().unwrap();
        let file = Pin::new(guard.as_mut());
        file.poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut guard = self.inner.lock().unwrap();
        let file = Pin::new(guard.as_mut());
        file.poll_shutdown(cx)
    }
    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let mut guard = self.inner.lock().unwrap();
        let file = Pin::new(guard.as_mut());
        file.poll_write_vectored(cx, bufs)
    }
    fn is_write_vectored(&self) -> bool {
        let mut guard = self.inner.lock().unwrap();
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
        let mut guard = self.inner.lock().unwrap();
        let file = Pin::new(guard.as_mut());
        file.poll_read(cx, buf)
    }
}

impl VirtualFile for ArcBoxFile {
    fn last_accessed(&self) -> u64 {
        let inner = self.inner.lock().unwrap();
        inner.last_accessed()
    }
    fn last_modified(&self) -> u64 {
        let inner = self.inner.lock().unwrap();
        inner.last_modified()
    }
    fn created_time(&self) -> u64 {
        let inner = self.inner.lock().unwrap();
        inner.created_time()
    }
    fn size(&self) -> u64 {
        let inner = self.inner.lock().unwrap();
        inner.size()
    }
    fn set_len(&mut self, new_size: u64) -> crate::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.set_len(new_size)
    }
    fn unlink(&mut self) -> crate::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.unlink()
    }
    fn is_open(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.is_open()
    }
    fn get_special_fd(&self) -> Option<u32> {
        let inner = self.inner.lock().unwrap();
        inner.get_special_fd()
    }
    fn poll_read_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        let mut inner = self.inner.lock().unwrap();
        let inner = Pin::new(inner.as_mut());
        inner.poll_read_ready(cx)
    }
    fn poll_write_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        let mut inner = self.inner.lock().unwrap();
        let inner = Pin::new(inner.as_mut());
        inner.poll_write_ready(cx)
    }
}

impl From<Box<dyn VirtualFile + Send + Sync + 'static>> for ArcBoxFile {
    fn from(val: Box<dyn VirtualFile + Send + Sync + 'static>) -> Self {
        ArcBoxFile::new(val)
    }
}
