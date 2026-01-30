use std::fmt::Debug;
use std::future::Future;
use std::io;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use parking_lot::Mutex;
use rustls_tokio_stream::UnderlyingStream;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use virtual_net::{VirtualIoSource, VirtualTcpSocket, net_error_into_io_err};

#[derive(Clone)]
pub struct SharedTcpStream {
    socket: Arc<Mutex<Box<dyn VirtualTcpSocket + Sync>>>,
}

impl SharedTcpStream {
    pub fn new(
        socket: Box<dyn VirtualTcpSocket + Sync>,
    ) -> (Self, Arc<Mutex<Box<dyn VirtualTcpSocket + Sync>>>) {
        let socket = Arc::new(Mutex::new(socket));
        (
            Self {
                socket: socket.clone(),
            },
            socket,
        )
    }
}

impl Debug for SharedTcpStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedTcpStream").finish()
    }
}

impl UnderlyingStream for SharedTcpStream {
    type StdType = std::net::TcpStream;

    fn poll_read_ready(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut socket = self.socket.lock();
        match socket.poll_read_ready(cx) {
            Poll::Ready(Ok(_)) => Poll::Ready(Ok(())),
            Poll::Ready(Err(err)) => Poll::Ready(Err(net_error_into_io_err(err))),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_write_ready(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut socket = self.socket.lock();
        match socket.poll_write_ready(cx) {
            Poll::Ready(Ok(_)) => Poll::Ready(Ok(())),
            Poll::Ready(Err(err)) => Poll::Ready(Err(net_error_into_io_err(err))),
            Poll::Pending => Poll::Pending,
        }
    }

    fn try_read(&self, buf: &mut [u8]) -> io::Result<usize> {
        let mut socket = self.socket.lock();
        let uninit = unsafe {
            std::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut MaybeUninit<u8>, buf.len())
        };
        socket
            .try_recv(uninit, false)
            .map_err(net_error_into_io_err)
    }

    fn try_write(&self, buf: &[u8]) -> io::Result<usize> {
        let mut socket = self.socket.lock();
        socket.try_send(buf).map_err(net_error_into_io_err)
    }

    fn readable(&self) -> impl Future<Output = io::Result<()>> + Send {
        let socket = self.socket.clone();
        async move {
            futures_util::future::poll_fn(|cx| {
                let mut socket = socket.lock();
                match socket.poll_read_ready(cx) {
                    Poll::Ready(Ok(_)) => Poll::Ready(Ok(())),
                    Poll::Ready(Err(err)) => Poll::Ready(Err(net_error_into_io_err(err))),
                    Poll::Pending => Poll::Pending,
                }
            })
            .await
        }
    }

    fn writable(&self) -> impl Future<Output = io::Result<()>> + Send {
        let socket = self.socket.clone();
        async move {
            futures_util::future::poll_fn(|cx| {
                let mut socket = socket.lock();
                match socket.poll_write_ready(cx) {
                    Poll::Ready(Ok(_)) => Poll::Ready(Ok(())),
                    Poll::Ready(Err(err)) => Poll::Ready(Err(net_error_into_io_err(err))),
                    Poll::Pending => Poll::Pending,
                }
            })
            .await
        }
    }

    fn shutdown(&self, how: std::net::Shutdown) -> io::Result<()> {
        let mut socket = self.socket.lock();
        socket.shutdown(how).map_err(net_error_into_io_err)
    }

    fn into_std(self) -> Option<std::io::Result<Self::StdType>> {
        None
    }
}

impl AsyncRead for SharedTcpStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let mut socket = self.socket.lock();
        AsyncRead::poll_read(Pin::new(&mut *socket), cx, buf)
    }
}

impl AsyncWrite for SharedTcpStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut socket = self.socket.lock();
        AsyncWrite::poll_write(Pin::new(&mut *socket), cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut socket = self.socket.lock();
        AsyncWrite::poll_flush(Pin::new(&mut *socket), cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut socket = self.socket.lock();
        AsyncWrite::poll_shutdown(Pin::new(&mut *socket), cx)
    }
}
