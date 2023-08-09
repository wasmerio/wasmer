use std::{io, pin::Pin, task::Poll};

use futures::future::BoxFuture;
use tokio::{
    io::{AsyncRead, AsyncSeek, AsyncWrite},
    sync::mpsc,
};
use wasmer_wasix::{os::TtyOptions, VirtualFile};

#[derive(Debug)]
pub(crate) enum TerminalCommandRx {
    Print(String),
    #[allow(dead_code)]
    Cls,
}

#[derive(Debug, Clone)]
pub struct TermStdout {
    term_tx: mpsc::UnboundedSender<TerminalCommandRx>,
    tty: TtyOptions,
}

impl TermStdout {
    pub(crate) fn new(tx: mpsc::UnboundedSender<TerminalCommandRx>, tty: TtyOptions) -> Self {
        Self { term_tx: tx, tty }
    }

    fn term_write(&self, data: &[u8]) {
        let data = match self.tty.line_feeds() {
            true => data
                .iter()
                .copied()
                .flat_map(|a| match a {
                    b'\n' => vec![b'\r', b'\n'].into_iter(),
                    a => vec![a].into_iter(),
                })
                .collect::<Vec<_>>(),
            false => data.to_vec(),
        };
        if let Ok(text) = String::from_utf8(data) {
            self.term_tx.send(TerminalCommandRx::Print(text)).unwrap();
        }
    }
}

impl AsyncRead for TermStdout {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        _buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Pending
    }
}

impl AsyncWrite for TermStdout {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        self.term_write(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }
}

impl AsyncSeek for TermStdout {
    fn start_seek(self: Pin<&mut Self>, _position: io::SeekFrom) -> io::Result<()> {
        Ok(())
    }

    fn poll_complete(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<io::Result<u64>> {
        Poll::Ready(Ok(0))
    }
}

impl VirtualFile for TermStdout {
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

    fn set_len(&mut self, _new_size: u64) -> wasmer_wasix::virtual_fs::Result<()> {
        Ok(())
    }

    fn unlink(&mut self) -> BoxFuture<'static, wasmer_wasix::virtual_fs::Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn poll_read_ready(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<usize>> {
        Poll::Pending
    }

    fn poll_write_ready(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<usize>> {
        Poll::Ready(Ok(8192))
    }
}
