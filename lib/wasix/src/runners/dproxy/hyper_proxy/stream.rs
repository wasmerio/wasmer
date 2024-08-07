use std::io;

use futures::Stream;
use hyper_util::{client::legacy::connect::Connected, rt::TokioIo};
use tokio_stream::wrappers::BroadcastStream;
use virtual_net::tcp_pair::{TcpSocketHalfRx, TcpSocketHalfTx};

use super::*;

#[derive(Debug)]
pub struct HyperProxyStream {
    pub(super) tx: TcpSocketHalfTx,
    pub(super) rx: TokioIo<TcpSocketHalfRx>,
    pub(super) terminate: BroadcastStream<()>,
    pub(super) terminated: bool,
}

impl hyper_util::client::legacy::connect::Connection for HyperProxyStream {
    fn connected(&self) -> Connected {
        Connected::new().proxy(true)
    }
}

impl hyper::rt::Read for HyperProxyStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: hyper::rt::ReadBufCursor<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        if let Poll::Ready(ret) = Pin::new(&mut self.rx).poll_read(cx, buf) {
            return Poll::Ready(ret);
        }
        if self.terminated {
            return Poll::Ready(Err(io::ErrorKind::ConnectionReset.into()));
        }
        if let Poll::Ready(Some(_)) = Pin::new(&mut self.terminate).poll_next(cx) {
            return Poll::Ready(Err(io::ErrorKind::ConnectionReset.into()));
        }
        Poll::Pending
    }
}

impl hyper::rt::Write for HyperProxyStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        if let Poll::Ready(ret) = Pin::new(&mut self.tx).poll_write(cx, buf) {
            return Poll::Ready(ret);
        }
        if self.terminated {
            return Poll::Ready(Err(io::ErrorKind::ConnectionReset.into()));
        }
        if let Poll::Ready(Some(_)) = Pin::new(&mut self.terminate).poll_next(cx) {
            return Poll::Ready(Err(io::ErrorKind::ConnectionReset.into()));
        }
        Poll::Pending
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        if let Poll::Ready(ret) = Pin::new(&mut self.tx).poll_flush(cx) {
            return Poll::Ready(ret);
        }
        if self.terminated {
            return Poll::Ready(Err(io::ErrorKind::ConnectionReset.into()));
        }
        if let Poll::Ready(Some(_)) = Pin::new(&mut self.terminate).poll_next(cx) {
            return Poll::Ready(Err(io::ErrorKind::ConnectionReset.into()));
        }
        Poll::Pending
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        if let Poll::Ready(ret) = Pin::new(&mut self.tx).poll_shutdown(cx) {
            return Poll::Ready(ret);
        }
        if self.terminated {
            return Poll::Ready(Err(io::ErrorKind::ConnectionReset.into()));
        }
        if let Poll::Ready(Some(_)) = Pin::new(&mut self.terminate).poll_next(cx) {
            return Poll::Ready(Err(io::ErrorKind::ConnectionReset.into()));
        }
        Poll::Pending
    }
}
