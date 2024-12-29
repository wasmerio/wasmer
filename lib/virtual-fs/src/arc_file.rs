//! Used for sharing references to the same file across multiple file systems,
//! effectively this is a symbolic link without all the complex path redirection

use crate::{ClonableVirtualFile, VirtualFile};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::{
    io::{self, *},
    sync::{Arc, Mutex},
};
use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite};

#[derive(Debug)]
pub struct ArcFile<T>
where
    T: VirtualFile + Send + Sync + 'static,
{
    inner: Arc<Mutex<Box<T>>>,
}

impl<T> ArcFile<T>
where
    T: VirtualFile + Send + Sync + 'static,
{
    pub fn new(inner: Box<T>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(inner)),
        }
    }
}

impl<T> AsyncSeek for ArcFile<T>
where
    T: VirtualFile + Send + Sync + 'static,
{
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

impl<T> AsyncWrite for ArcFile<T>
where
    T: VirtualFile + Send + Sync + 'static,
{
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

impl<T> AsyncRead for ArcFile<T>
where
    T: VirtualFile + Send + Sync + 'static,
{
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

impl<T> VirtualFile for ArcFile<T>
where
    T: VirtualFile + Send + Sync + 'static,
{
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
    fn set_times(&mut self, atime: Option<u64>, mtime: Option<u64>) -> crate::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.set_times(atime, mtime)
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

impl<T> Clone for ArcFile<T>
where
    T: VirtualFile + Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        ArcFile {
            inner: self.inner.clone(),
        }
    }
}

impl<T> ClonableVirtualFile for ArcFile<T>
where
    T: VirtualFile + Send + Sync + 'static,
    T: Clone,
{
}

impl<T> Default for ArcFile<T>
where
    T: VirtualFile + Send + Sync + 'static,
    T: Default,
{
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Box::default())),
        }
    }
}
