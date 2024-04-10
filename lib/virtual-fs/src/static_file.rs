use std::{
    convert::TryInto,
    io::{self, Cursor},
    pin::Pin,
    task::{Context, Poll},
};

use shared_buffer::OwnedBuffer;
use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite};

use crate::{FsError, VirtualFile};

/// An immutable file backed by an [`OwnedBuffer`].
#[derive(Debug, Clone, PartialEq)]
pub struct StaticFile(Cursor<OwnedBuffer>);

impl StaticFile {
    pub fn new(bytes: impl Into<OwnedBuffer>) -> Self {
        StaticFile(Cursor::new(bytes.into()))
    }

    /// Access the underlying buffer.
    pub fn contents(&self) -> &OwnedBuffer {
        self.0.get_ref()
    }
}

#[async_trait::async_trait]
impl VirtualFile for StaticFile {
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
        self.0.get_ref().len().try_into().unwrap()
    }

    fn set_len(&mut self, _new_size: u64) -> Result<(), FsError> {
        Err(FsError::PermissionDenied)
    }

    fn unlink(&mut self) -> Result<(), FsError> {
        Err(FsError::PermissionDenied)
    }

    fn poll_read_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        let remaining = self.size() - self.0.position();
        Poll::Ready(Ok(remaining.try_into().unwrap()))
    }

    fn poll_write_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Err(std::io::ErrorKind::PermissionDenied.into()))
    }
}

impl AsyncRead for StaticFile {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

// WebC file is not writable, the FileOpener will return a MemoryFile for writing instead
// This code should never be executed (since writes are redirected to memory instead).
impl AsyncWrite for StaticFile {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(Err(std::io::ErrorKind::PermissionDenied.into()))
    }
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }
}

impl AsyncSeek for StaticFile {
    fn start_seek(mut self: Pin<&mut Self>, pos: io::SeekFrom) -> io::Result<()> {
        Pin::new(&mut self.0).start_seek(pos)
    }

    fn poll_complete(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Pin::new(&mut self.0).poll_complete(cx)
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::AsyncReadExt;

    use super::*;

    #[tokio::test]
    async fn read_a_static_file_to_end() {
        let mut file = StaticFile::new(OwnedBuffer::from_static(b"Hello, World!"));
        let mut buffer = [0; 5];

        let bytes_read = file.read(&mut buffer).await.unwrap();
        assert_eq!(bytes_read, 5);
        assert_eq!(&buffer[..bytes_read], b"Hello");
        assert_eq!(file.0.position(), 5);

        let bytes_read = file.read(&mut buffer).await.unwrap();
        assert_eq!(bytes_read, 5);
        assert_eq!(&buffer[..bytes_read], b", Wor");
        assert_eq!(file.0.position(), 10);

        let bytes_read = file.read(&mut buffer).await.unwrap();
        assert_eq!(bytes_read, 3);
        assert_eq!(&buffer[..bytes_read], b"ld!");
        assert_eq!(file.0.position(), 13);

        let bytes_read = file.read(&mut buffer).await.unwrap();
        assert_eq!(bytes_read, 0);
        assert_eq!(&buffer[..bytes_read], b"");
        assert_eq!(file.0.position(), 13);
    }
}
