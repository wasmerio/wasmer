use futures::future::BoxFuture;
use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite};

use std::borrow::Cow;
use std::convert::TryInto;
use std::io::{self, SeekFrom};
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::{FsError, VirtualFile};

#[derive(Debug)]
pub struct StaticFile {
    bytes: Cow<'static, [u8]>,
    cursor: u64,
    len: u64,
}
impl StaticFile {
    pub fn new(bytes: Cow<'static, [u8]>) -> Self {
        Self {
            len: bytes.len() as u64,
            bytes,
            cursor: 0,
        }
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
        self.len
    }
    fn set_len(&mut self, _new_size: u64) -> crate::Result<()> {
        Ok(())
    }
    fn unlink(&mut self) -> BoxFuture<'static, Result<(), FsError>> {
        Box::pin(async { Ok(()) })
    }
    fn poll_read_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        let remaining = self.len - self.cursor;
        Poll::Ready(Ok(remaining as usize))
    }
    fn poll_write_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(0))
    }
}

impl AsyncRead for StaticFile {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let bytes = self.bytes.as_ref();

        let cursor: usize = self.cursor.try_into().unwrap_or(u32::MAX as usize);
        let _start = cursor.min(bytes.len());
        let bytes = &bytes[cursor..];

        if bytes.len() > buf.remaining() {
            let remaining = buf.remaining();
            buf.put_slice(&bytes[..remaining]);
        } else {
            buf.put_slice(bytes);
        }
        Poll::Ready(Ok(()))
    }
}

// WebC file is not writable, the FileOpener will return a MemoryFile for writing instead
// This code should never be executed (since writes are redirected to memory instead).
impl AsyncWrite for StaticFile {
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
    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }
}

impl AsyncSeek for StaticFile {
    fn start_seek(mut self: Pin<&mut Self>, pos: io::SeekFrom) -> io::Result<()> {
        let self_size = self.size();
        match pos {
            SeekFrom::Start(s) => {
                self.cursor = s.min(self_size);
            }
            SeekFrom::End(e) => {
                let self_size_i64 = self_size.try_into().unwrap_or(i64::MAX);
                self.cursor = ((self_size_i64).saturating_add(e))
                    .min(self_size_i64)
                    .try_into()
                    .unwrap_or(i64::MAX as u64);
            }
            SeekFrom::Current(c) => {
                self.cursor = (self
                    .cursor
                    .saturating_add(c.try_into().unwrap_or(i64::MAX as u64)))
                .min(self_size);
            }
        }
        Ok(())
    }
    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Ok(self.cursor))
    }
}
