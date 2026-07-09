//! A mock networking backend used by the `writev_partial_send_error` test.
//!
//! It hands out a TCP socket whose first `try_send` succeeds in full and whose
//! subsequent `try_send` calls fail with `ConnectionReset`. That deterministically
//! drives fd_write's per-iovec loop down the "a later send errors after an earlier
//! iovec already succeeded" branch, which cannot be triggered reliably over real
//! host sockets (it would depend on an asynchronous RST landing between two
//! back-to-back sends of a single writev - see issue #6785).

use std::mem::MaybeUninit;
use std::net::{Shutdown, SocketAddr};
use std::task::{Context, Poll};
use std::time::Duration;

use wasmer_wasix::virtual_net::{
    InterestHandler, NetworkError, Result as NetResult, SocketStatus, VirtualConnectedSocket,
    VirtualIoSource, VirtualNetworking, VirtualSocket, VirtualTcpSocket,
};

/// A connected TCP socket whose first `try_send` succeeds and whose following
/// `try_send` calls return `ConnectionReset`.
#[derive(Debug)]
struct FailAfterFirstSendSocket {
    local: SocketAddr,
    peer: SocketAddr,
    sends: usize,
}

impl FailAfterFirstSendSocket {
    fn new(local: SocketAddr, peer: SocketAddr) -> Self {
        Self {
            local,
            peer,
            sends: 0,
        }
    }
}

impl VirtualIoSource for FailAfterFirstSendSocket {
    fn remove_handler(&mut self) {}

    fn poll_read_ready(&mut self, _cx: &mut Context<'_>) -> Poll<NetResult<usize>> {
        Poll::Ready(Ok(0))
    }

    fn poll_write_ready(&mut self, _cx: &mut Context<'_>) -> Poll<NetResult<usize>> {
        // Report writable so a blocking connect() completes immediately.
        Poll::Ready(Ok(8192))
    }
}

impl VirtualSocket for FailAfterFirstSendSocket {
    fn set_ttl(&mut self, _ttl: u32) -> NetResult<()> {
        Ok(())
    }

    fn ttl(&self) -> NetResult<u32> {
        Ok(64)
    }

    fn addr_local(&self) -> NetResult<SocketAddr> {
        Ok(self.local)
    }

    fn status(&self) -> NetResult<SocketStatus> {
        Ok(SocketStatus::Opened)
    }

    fn set_handler(&mut self, _handler: Box<dyn InterestHandler + Send + Sync>) -> NetResult<()> {
        Ok(())
    }
}

impl VirtualConnectedSocket for FailAfterFirstSendSocket {
    fn set_linger(&mut self, _linger: Option<Duration>) -> NetResult<()> {
        Ok(())
    }

    fn linger(&self) -> NetResult<Option<Duration>> {
        Ok(None)
    }

    fn try_send(&mut self, data: &[u8]) -> NetResult<usize> {
        self.sends += 1;
        if self.sends == 1 {
            // First iovec is accepted in full.
            Ok(data.len())
        } else {
            // Any later iovec's send fails, exercising the partial-return branch.
            Err(NetworkError::ConnectionReset)
        }
    }

    fn try_flush(&mut self) -> NetResult<()> {
        Ok(())
    }

    fn close(&mut self) -> NetResult<()> {
        Ok(())
    }

    fn try_recv(&mut self, _buf: &mut [MaybeUninit<u8>], _peek: bool) -> NetResult<usize> {
        Err(NetworkError::WouldBlock)
    }
}

impl VirtualTcpSocket for FailAfterFirstSendSocket {
    fn set_recv_buf_size(&mut self, _size: usize) -> NetResult<()> {
        Ok(())
    }

    fn recv_buf_size(&self) -> NetResult<usize> {
        Ok(0)
    }

    fn set_send_buf_size(&mut self, _size: usize) -> NetResult<()> {
        Ok(())
    }

    fn send_buf_size(&self) -> NetResult<usize> {
        Ok(0)
    }

    fn set_nodelay(&mut self, _nodelay: bool) -> NetResult<()> {
        Ok(())
    }

    fn nodelay(&self) -> NetResult<bool> {
        Ok(false)
    }

    fn set_keepalive(&mut self, _keepalive: bool) -> NetResult<()> {
        Ok(())
    }

    fn keepalive(&self) -> NetResult<bool> {
        Ok(false)
    }

    fn set_dontroute(&mut self, _dontroute: bool) -> NetResult<()> {
        Ok(())
    }

    fn dontroute(&self) -> NetResult<bool> {
        Ok(false)
    }

    fn addr_peer(&self) -> NetResult<SocketAddr> {
        Ok(self.peer)
    }

    fn shutdown(&mut self, _how: Shutdown) -> NetResult<()> {
        Ok(())
    }

    fn is_closed(&self) -> bool {
        false
    }
}

/// Networking backend that hands out [`FailAfterFirstSendSocket`]s on connect.
/// Every other operation is left at the `VirtualNetworking` default (unsupported).
#[derive(Debug, Default)]
pub struct FailAfterFirstSendNetworking;

#[async_trait::async_trait]
impl VirtualNetworking for FailAfterFirstSendNetworking {
    async fn connect_tcp(
        &self,
        addr: SocketAddr,
        peer: SocketAddr,
    ) -> NetResult<Box<dyn VirtualTcpSocket + Sync>> {
        Ok(Box::new(FailAfterFirstSendSocket::new(addr, peer)))
    }
}
