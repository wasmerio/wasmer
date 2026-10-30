//! NullFile is a special file for `/dev/null`, which returns 0 for all
//! operations except writing.

use std::io::{self, *};
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite};

use crate::{CloneableVirtualFile, VirtualFile};

#[derive(Debug, Clone, Default)]
pub struct NullFile {}

impl AsyncSeek for NullFile {
    fn start_seek(self: Pin<&mut Self>, _position: SeekFrom) -> io::Result<()> {
        Ok(())
    }
    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Ok(0))
    }
}

impl AsyncWrite for NullFile {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_write_vectored(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        // Bytes written, not the number of slices. `/dev/null` swallows all of
        // them, so that is the total length.
        Poll::Ready(Ok(bufs.iter().map(|buf| buf.len()).sum()))
    }
    fn is_write_vectored(&self) -> bool {
        false
    }
}

impl AsyncRead for NullFile {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl VirtualFile for NullFile {
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
        0
    }
    fn set_len(&mut self, _new_size: u64) -> crate::Result<()> {
        Ok(())
    }
    fn unlink(&mut self) -> crate::Result<()> {
        Ok(())
    }
    fn poll_read_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(8192))
    }
    fn poll_write_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(8192))
    }
}

impl CloneableVirtualFile for NullFile {}

#[cfg(test)]
mod tests {
    use tokio::io::AsyncWriteExt;

    use super::*;

    /// `poll_write_vectored` reports bytes written, not slices. Returning the
    /// slice count made a caller believe a 300-byte write had moved 3 bytes.
    #[tokio::test]
    async fn write_vectored_reports_bytes_not_slices() {
        let mut file = NullFile::default();
        let bufs = [
            IoSlice::new(&[0u8; 100]),
            IoSlice::new(&[0u8; 100]),
            IoSlice::new(&[0u8; 100]),
        ];

        let written = std::future::poll_fn(|cx| Pin::new(&mut file).poll_write_vectored(cx, &bufs))
            .await
            .unwrap();

        assert_eq!(written, 300);
    }

    /// The unvectored path already agreed; keep the two consistent.
    #[tokio::test]
    async fn write_reports_bytes() {
        let mut file = NullFile::default();
        assert_eq!(file.write(&[0u8; 100]).await.unwrap(), 100);
    }
}
