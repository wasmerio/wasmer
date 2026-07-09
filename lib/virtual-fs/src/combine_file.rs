use super::*;

use crate::VirtualFile;

#[derive(Debug)]
pub struct CombineFile {
    tx: Box<dyn VirtualFile + Send + Sync + 'static>,
    rx: Box<dyn VirtualFile + Send + Sync + 'static>,
}

impl CombineFile {
    pub fn new(
        tx: Box<dyn VirtualFile + Send + Sync + 'static>,
        rx: Box<dyn VirtualFile + Send + Sync + 'static>,
    ) -> Self {
        Self { tx, rx }
    }
}

#[async_trait::async_trait]
impl VirtualFile for CombineFile {
    async fn last_accessed(&self) -> u64 {
        self.rx.last_accessed().await
    }

    async fn last_modified(&self) -> u64 {
        self.tx.last_modified().await
    }

    async fn created_time(&self) -> u64 {
        self.tx.created_time().await
    }

    async fn set_times(&mut self, atime: Option<u64>, mtime: Option<u64>) -> crate::Result<()> {
        self.tx.set_times(atime, mtime).await
    }

    async fn size(&self) -> u64 {
        self.rx.size().await
    }

    async fn set_len(&mut self, new_size: u64) -> Result<()> {
        self.tx.set_len(new_size).await
    }

    async fn unlink(&mut self) -> Result<()> {
        self.tx.unlink().await
    }

    fn poll_read_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Pin::new(self.rx.as_mut()).poll_read_ready(cx)
    }

    fn poll_write_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Pin::new(self.tx.as_mut()).poll_write_ready(cx)
    }
}

impl AsyncWrite for CombineFile {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.tx).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.tx).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.tx).poll_shutdown(cx)
    }
}

impl AsyncRead for CombineFile {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.rx).poll_read(cx, buf)
    }
}

impl AsyncSeek for CombineFile {
    fn start_seek(mut self: Pin<&mut Self>, position: io::SeekFrom) -> io::Result<()> {
        Pin::new(&mut self.tx).start_seek(position)?;
        Pin::new(&mut self.rx).start_seek(position)
    }

    fn poll_complete(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<io::Result<u64>> {
        if Pin::new(&mut self.tx).poll_complete(cx).is_pending() {
            return Poll::Pending;
        }
        Pin::new(&mut self.rx).poll_complete(cx)
    }
}
