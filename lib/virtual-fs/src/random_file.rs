//! Used for /dev/zero - infinitely returns zero
//! which is useful for commands like `dd if=/dev/zero of=bigfile.img size=1G`

use std::io::{self, *};
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite};

use crate::VirtualFile;

#[derive(Debug, Default)]
pub struct RandomFile {}

impl AsyncSeek for RandomFile {
    fn start_seek(self: Pin<&mut Self>, _position: SeekFrom) -> io::Result<()> {
        Ok(())
    }
    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Ok(0))
    }
}

impl AsyncWrite for RandomFile {
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
        // Bytes written, not the number of slices. Writes here are discarded,
        // so that is the total length.
        Poll::Ready(Ok(bufs.iter().map(|buf| buf.len()).sum()))
    }
    fn is_write_vectored(&self) -> bool {
        false
    }
}

impl AsyncRead for RandomFile {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let mut data = vec![0u8; buf.remaining()];
        getrandom::fill(&mut data).ok();
        buf.put_slice(&data[..]);
        Poll::Ready(Ok(()))
    }
}

impl VirtualFile for RandomFile {
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
        Poll::Ready(Ok(0))
    }
    fn poll_write_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `poll_write_vectored` reports bytes written, not slices. Returning the
    /// slice count made a caller believe a 300-byte write had moved 3 bytes.
    #[tokio::test]
    async fn write_vectored_reports_bytes_not_slices() {
        let mut file = RandomFile::default();
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
}
