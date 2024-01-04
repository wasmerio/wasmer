use std::io;

use hyper::client::connect::Connected;
use virtual_net::tcp_pair::{TcpSocketHalfRx, TcpSocketHalfTx};

use super::*;

#[derive(Debug)]
pub struct HyperProxyStream {
    pub(super) tx: TcpSocketHalfTx,
    pub(super) rx: TcpSocketHalfRx,
}

impl AsyncRead for HyperProxyStream {
    #[inline]
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.rx).poll_read(cx, buf)
    }
}

impl AsyncWrite for HyperProxyStream {
    #[inline]
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        Pin::new(&mut self.tx).poll_write(cx, buf)
    }

    #[inline]
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.tx).poll_flush(cx)
    }

    #[inline]
    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.tx).poll_shutdown(cx)
    }
}

impl hyper::client::connect::Connection for HyperProxyStream {
    fn connected(&self) -> Connected {
        Connected::new().proxy(true)
    }
}
