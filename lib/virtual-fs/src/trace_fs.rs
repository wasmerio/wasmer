use std::{
    path::PathBuf,
    pin::Pin,
    task::{Context, Poll},
};

use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite, ReadBuf};

use crate::{FileSystem, OpenOptionsConfig, VirtualFile};

/// A [`FileSystem`] wrapper that will automatically log all operations at the
/// `trace` level.
///
/// To see these logs, you will typically need to set the `$RUST_LOG`
/// environment variable to `virtual_fs::trace_fs=trace`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceFileSystem<F>(pub F);

impl<F> TraceFileSystem<F> {
    pub fn new(filesystem: F) -> Self {
        TraceFileSystem(filesystem)
    }

    pub fn inner(&self) -> &F {
        &self.0
    }

    pub fn inner_mut(&mut self) -> &mut F {
        &mut self.0
    }

    pub fn into_inner(self) -> F {
        self.0
    }
}

#[async_trait::async_trait]
impl<F> FileSystem for TraceFileSystem<F>
where
    F: FileSystem,
{
    #[tracing::instrument(level = "trace", skip(self), err)]
    async fn readlink(&self, path: &std::path::Path) -> crate::Result<PathBuf> {
        self.0.readlink(path).await
    }

    #[tracing::instrument(level = "trace", skip(self), err)]
    async fn read_dir(&self, path: &std::path::Path) -> crate::Result<crate::ReadDir> {
        self.0.read_dir(path).await
    }

    #[tracing::instrument(level = "trace", skip(self), err)]
    async fn create_dir(&self, path: &std::path::Path) -> crate::Result<()> {
        self.0.create_dir(path).await
    }

    #[tracing::instrument(level = "trace", skip(self), err)]
    async fn remove_dir(&self, path: &std::path::Path) -> crate::Result<()> {
        self.0.remove_dir(path).await
    }

    #[tracing::instrument(level = "trace", skip(self), err)]
    async fn rename(&self, from: &std::path::Path, to: &std::path::Path) -> crate::Result<()> {
        self.0.rename(from, to).await
    }

    #[tracing::instrument(level = "trace", skip(self), err)]
    async fn metadata(&self, path: &std::path::Path) -> crate::Result<crate::Metadata> {
        self.0.metadata(path).await
    }

    #[tracing::instrument(level = "trace", skip(self), err)]
    async fn symlink_metadata(&self, path: &std::path::Path) -> crate::Result<crate::Metadata> {
        self.0.symlink_metadata(path).await
    }

    #[tracing::instrument(level = "trace", skip(self), err)]
    async fn remove_file(&self, path: &std::path::Path) -> crate::Result<()> {
        self.0.remove_file(path).await
    }

    #[tracing::instrument(level = "trace", skip(self))]
    fn new_open_options(&self) -> crate::OpenOptions<'_> {
        crate::OpenOptions::new(self)
    }
    #[tracing::instrument(level = "trace", skip(self))]
    async fn open(
        &self,
        path: &std::path::Path,
        conf: &OpenOptionsConfig,
    ) -> crate::Result<Box<dyn crate::VirtualFile + Send + Sync + 'static>> {
        let file = self.0.open(path, conf).await?;
        Ok(Box::new(TraceFile {
            file,
            path: path.to_owned(),
        }))
    }
}

#[derive(Debug)]
struct TraceFile {
    path: PathBuf,
    file: Box<dyn crate::VirtualFile + Send + Sync + 'static>,
}

#[async_trait::async_trait]
impl VirtualFile for TraceFile {
    #[tracing::instrument(level = "trace", skip(self), fields(path=%self.path.display()))]
    async fn last_accessed(&self) -> u64 {
        self.file.last_accessed().await
    }

    #[tracing::instrument(level = "trace", skip(self), fields(path=%self.path.display()))]
    async fn last_modified(&self) -> u64 {
        self.file.last_modified().await
    }

    #[tracing::instrument(level = "trace", skip(self), fields(path=%self.path.display()))]
    async fn created_time(&self) -> u64 {
        self.file.created_time().await
    }

    #[tracing::instrument(level = "trace", skip(self), fields(path=%self.path.display()))]
    async fn set_times(&mut self, atime: Option<u64>, mtime: Option<u64>) -> crate::Result<()> {
        self.file.set_times(atime, mtime).await
    }

    #[tracing::instrument(level = "trace", skip(self), fields(path=%self.path.display()))]
    async fn size(&self) -> u64 {
        self.file.size().await
    }

    #[tracing::instrument(level = "trace", skip(self), fields(path=%self.path.display()), err)]
    async fn set_len(&mut self, new_size: u64) -> crate::Result<()> {
        self.file.set_len(new_size).await
    }

    async fn unlink(&mut self) -> crate::Result<()> {
        self.file.unlink().await
    }

    #[tracing::instrument(level = "trace", skip_all, fields(path=%self.path.display()))]
    fn poll_read_ready(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<usize>> {
        let result = Pin::new(&mut *self.file).poll_read_ready(cx);

        if let Poll::Ready(Err(e)) = &result {
            tracing::trace!(error = e as &dyn std::error::Error);
        }

        result
    }

    #[tracing::instrument(level = "trace", skip_all, fields(path=%self.path.display()))]
    fn poll_write_ready(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<usize>> {
        let result = Pin::new(&mut *self.file).poll_write_ready(cx);

        if let Poll::Ready(Err(e)) = &result {
            tracing::trace!(error = e as &dyn std::error::Error);
        }

        result
    }
}

impl AsyncRead for TraceFile {
    #[tracing::instrument(level = "trace", skip_all, fields(path=%self.path.display()))]
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let result = Pin::new(&mut *self.file).poll_read(cx, buf);

        if let Poll::Ready(Err(e)) = &result {
            tracing::trace!(error = e as &dyn std::error::Error);
        }

        result
    }
}

impl AsyncWrite for TraceFile {
    #[tracing::instrument(level = "trace", skip_all, fields(path=%self.path.display()))]
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        let result = Pin::new(&mut *self.file).poll_write(cx, buf);

        if let Poll::Ready(Err(e)) = &result {
            tracing::trace!(error = e as &dyn std::error::Error);
        }

        result
    }

    #[tracing::instrument(level = "trace", skip_all, fields(path=%self.path.display()))]
    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        let result = Pin::new(&mut *self.file).poll_flush(cx);

        if let Poll::Ready(Err(e)) = &result {
            tracing::trace!(error = e as &dyn std::error::Error);
        }

        result
    }

    #[tracing::instrument(level = "trace", skip_all, fields(path=%self.path.display()))]
    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        let result = Pin::new(&mut *self.file).poll_shutdown(cx);

        if let Poll::Ready(Err(e)) = &result {
            tracing::trace!(error = e as &dyn std::error::Error);
        }

        result
    }
}

impl AsyncSeek for TraceFile {
    #[tracing::instrument(level = "trace", skip_all, fields(path=%self.path.display()), err)]
    fn start_seek(mut self: Pin<&mut Self>, position: std::io::SeekFrom) -> std::io::Result<()> {
        Pin::new(&mut *self.file).start_seek(position)
    }

    #[tracing::instrument(level = "trace", skip_all, fields(path=%self.path.display()))]
    fn poll_complete(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<u64>> {
        let result = Pin::new(&mut *self.file).poll_complete(cx);

        if let Poll::Ready(Err(e)) = &result {
            tracing::trace!(error = e as &dyn std::error::Error);
        }

        result
    }
}
