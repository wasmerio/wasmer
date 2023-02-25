#![allow(unused_variables)]
use std::future::Future;
use std::mem::MaybeUninit;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr};
use std::pin::Pin;
use std::ptr;
use std::sync::Mutex;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use std::time::Duration;
use tokio::sync::mpsc;
#[allow(unused_imports, dead_code)]
use tracing::{debug, error, info, trace, warn};
#[allow(unused_imports)]
use wasmer_vnet::{
    IpCidr, IpRoute, NetworkError, Result, SocketStatus, StreamSecurity, VirtualConnectedSocket,
    VirtualConnectionlessSocket, VirtualIcmpSocket, VirtualNetworking, VirtualRawSocket,
    VirtualSocket, VirtualTcpListener, VirtualTcpSocket, VirtualUdpSocket,
};

#[derive(Debug)]
pub struct LocalNetworking {
    // Make struct internals private.
    // Can be removed once some fields are added (like permissions).
    _private: (),
}

impl LocalNetworking {
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for LocalNetworking {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
#[allow(unused_variables)]
impl VirtualNetworking for LocalNetworking {
    async fn listen_tcp(
        &self,
        addr: SocketAddr,
        only_v6: bool,
        reuse_port: bool,
        reuse_addr: bool,
    ) -> Result<Box<dyn VirtualTcpListener + Sync>> {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map(|sock| {
                Box::new(LocalTcpListener {
                    stream: sock,
                    backlog: Mutex::new(Vec::new()),
                })
            })
            .map_err(io_err_into_net_error)?;
        Ok(listener)
    }

    async fn bind_udp(
        &self,
        addr: SocketAddr,
        _reuse_port: bool,
        _reuse_addr: bool,
    ) -> Result<Box<dyn VirtualUdpSocket + Sync>> {
        let socket = tokio::net::UdpSocket::bind(addr)
            .await
            .map_err(io_err_into_net_error)?;
        Ok(Box::new(LocalUdpSocket {
            socket,
            addr,
            nonblocking: false,
        }))
    }

    async fn connect_tcp(
        &self,
        _addr: SocketAddr,
        peer: SocketAddr,
    ) -> Result<Box<dyn VirtualTcpSocket + Sync>> {
        let stream = tokio::net::TcpStream::connect(peer)
            .await
            .map_err(io_err_into_net_error)?;
        let peer = stream.peer_addr().map_err(io_err_into_net_error)?;
        Ok(Box::new(LocalTcpStream::new(stream, peer)))
    }

    async fn resolve(
        &self,
        host: &str,
        port: Option<u16>,
        dns_server: Option<IpAddr>,
    ) -> Result<Vec<IpAddr>> {
        tokio::net::lookup_host(host)
            .await
            .map(|a| a.map(|a| a.ip()).collect::<Vec<_>>())
            .map_err(io_err_into_net_error)
    }
}

#[derive(Debug)]
pub struct LocalTcpListener {
    stream: tokio::net::TcpListener,
    backlog: Mutex<Vec<(Box<LocalTcpStream>, SocketAddr)>>,
}

#[async_trait::async_trait]
impl VirtualTcpListener for LocalTcpListener {
    fn try_accept(&mut self) -> Option<Result<(Box<dyn VirtualTcpSocket + Sync>, SocketAddr)>> {
        {
            let mut backlog = self.backlog.lock().unwrap();
            if let Some((sock, addr)) = backlog.pop() {
                return Some(Ok((sock, addr)));
            }
        }

        let waker = unsafe { Waker::from_raw(RawWaker::new(ptr::null(), &NOOP_WAKER_VTABLE)) };
        let mut cx = Context::from_waker(&waker);
        match self
            .stream
            .poll_accept(&mut cx)
            .map_err(io_err_into_net_error)
        {
            Poll::Ready(Ok((stream, addr))) => {
                Some(Ok((Box::new(LocalTcpStream::new(stream, addr)), addr)))
            }
            Poll::Ready(Err(NetworkError::WouldBlock)) => None,
            Poll::Ready(Err(err)) => Some(Err(err)),
            Poll::Pending => None,
        }
    }

    fn poll_accept(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(Box<dyn VirtualTcpSocket + Sync>, SocketAddr)>> {
        {
            let mut backlog = self.backlog.lock().unwrap();
            if let Some((sock, addr)) = backlog.pop() {
                return Poll::Ready(Ok((sock, addr)));
            }
        }

        // We poll the socket
        let (sock, addr) = match self.stream.poll_accept(cx).map_err(io_err_into_net_error) {
            Poll::Ready(Ok((sock, addr))) => (Box::new(LocalTcpStream::new(sock, addr)), addr),
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            Poll::Pending => return Poll::Pending,
        };
        Poll::Ready(Ok((sock, addr)))
    }

    fn poll_accept_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<usize>> {
        {
            let backlog = self.backlog.lock().unwrap();
            if backlog.len() > 10 {
                return Poll::Ready(Ok(backlog.len()));
            }
        }
        self.stream
            .poll_accept(cx)
            .map_err(io_err_into_net_error)
            .map_ok(|(sock, addr)| {
                let mut backlog = self.backlog.lock().unwrap();
                backlog.push((Box::new(LocalTcpStream::new(sock, addr)), addr));
                backlog.len()
            })
    }

    fn addr_local(&self) -> Result<SocketAddr> {
        self.stream.local_addr().map_err(io_err_into_net_error)
    }

    fn set_ttl(&mut self, ttl: u8) -> Result<()> {
        self.stream
            .set_ttl(ttl as u32)
            .map_err(io_err_into_net_error)
    }

    fn ttl(&self) -> Result<u8> {
        self.stream
            .ttl()
            .map(|ttl| ttl as u8)
            .map_err(io_err_into_net_error)
    }
}

#[derive(Debug)]
pub struct LocalTcpStream {
    stream: tokio::net::TcpStream,
    addr: SocketAddr,
    shutdown: Option<Shutdown>,
    tx_write_ready: mpsc::Sender<()>,
    rx_write_ready: mpsc::Receiver<()>,
    tx_write_poll_ready: mpsc::Sender<()>,
    rx_write_poll_ready: mpsc::Receiver<()>,
}

impl LocalTcpStream {
    pub fn new(stream: tokio::net::TcpStream, addr: SocketAddr) -> Self {
        let (tx_write_ready, rx_write_ready) = mpsc::channel(1);
        let (tx_write_poll_ready, rx_write_poll_ready) = mpsc::channel(1);
        Self {
            stream,
            addr,
            shutdown: None,
            tx_write_ready,
            rx_write_ready,
            tx_write_poll_ready,
            rx_write_poll_ready,
        }
    }
}

#[async_trait::async_trait]
impl VirtualTcpSocket for LocalTcpStream {
    fn set_recv_buf_size(&mut self, size: usize) -> Result<()> {
        Ok(())
    }

    fn recv_buf_size(&self) -> Result<usize> {
        Err(NetworkError::Unsupported)
    }

    fn set_send_buf_size(&mut self, size: usize) -> Result<()> {
        Ok(())
    }

    fn send_buf_size(&self) -> Result<usize> {
        Err(NetworkError::Unsupported)
    }

    fn set_nodelay(&mut self, nodelay: bool) -> Result<()> {
        self.stream
            .set_nodelay(nodelay)
            .map_err(io_err_into_net_error)
    }

    fn nodelay(&self) -> Result<bool> {
        self.stream.nodelay().map_err(io_err_into_net_error)
    }

    fn addr_peer(&self) -> Result<SocketAddr> {
        Ok(self.addr)
    }

    fn shutdown(&mut self, how: Shutdown) -> Result<()> {
        self.shutdown = Some(how);
        Ok(())
    }

    fn is_closed(&self) -> bool {
        false
    }
}

impl VirtualConnectedSocket for LocalTcpStream {
    fn set_linger(&mut self, linger: Option<Duration>) -> Result<()> {
        self.stream
            .set_linger(linger)
            .map_err(io_err_into_net_error)?;
        Ok(())
    }

    fn linger(&self) -> Result<Option<Duration>> {
        self.stream.linger().map_err(io_err_into_net_error)
    }

    fn try_send(&mut self, data: &[u8]) -> Result<usize> {
        self.stream.try_write(data).map_err(io_err_into_net_error)
    }

    fn poll_send(&mut self, cx: &mut Context<'_>, data: &[u8]) -> Poll<Result<usize>> {
        use tokio::io::AsyncWrite;
        Pin::new(&mut self.stream)
            .poll_write(cx, data)
            .map_err(io_err_into_net_error)
    }

    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<()>> {
        while self.rx_write_ready.try_recv().is_ok() {}
        self.tx_write_poll_ready.try_send(()).ok();
        use tokio::io::AsyncWrite;
        Pin::new(&mut self.stream)
            .poll_flush(cx)
            .map_err(io_err_into_net_error)
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn poll_recv<'a>(
        &mut self,
        cx: &mut Context<'_>,
        buf: &'a mut [MaybeUninit<u8>],
    ) -> Poll<Result<usize>> {
        use tokio::io::AsyncRead;
        let mut read_buf = tokio::io::ReadBuf::uninit(buf);
        let res = Pin::new(&mut self.stream)
            .poll_read(cx, &mut read_buf)
            .map_err(io_err_into_net_error);
        match res {
            Poll::Ready(Ok(_)) => {
                let amt = read_buf.filled().len();
                let data: &[u8] = unsafe { std::mem::transmute(&buf[..amt]) };
                Poll::Ready(Ok(amt))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            Poll::Pending => Poll::Pending,
        }
    }

    fn try_recv(&mut self, buf: &mut [MaybeUninit<u8>]) -> Result<usize> {
        let buf: &mut [u8] = unsafe { std::mem::transmute(buf) };
        self.stream.try_read(buf).map_err(io_err_into_net_error)
    }
}

#[async_trait::async_trait]
impl VirtualSocket for LocalTcpStream {
    fn set_ttl(&mut self, ttl: u32) -> Result<()> {
        self.stream.set_ttl(ttl).map_err(io_err_into_net_error)
    }

    fn ttl(&self) -> Result<u32> {
        self.stream.ttl().map_err(io_err_into_net_error)
    }

    fn addr_local(&self) -> Result<SocketAddr> {
        self.stream.local_addr().map_err(io_err_into_net_error)
    }

    fn status(&self) -> Result<SocketStatus> {
        Ok(SocketStatus::Opened)
    }

    fn poll_read_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<usize>> {
        self.stream
            .poll_read_ready(cx)
            .map_ok(|_| 1)
            .map_err(io_err_into_net_error)
    }

    fn poll_write_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<usize>> {
        loop {
            // this wakes this polling ready call whenever the `rx_write_poll_ready` is triggerd
            // (which is triggered whenever a send operation is transmitted)
            let mut rx = Pin::new(&mut self.rx_write_poll_ready);
            if rx.poll_recv(cx).is_pending() {
                break;
            }
        }
        match self
            .stream
            .poll_write_ready(cx)
            .map_err(io_err_into_net_error)
        {
            Poll::Ready(Ok(())) => {
                if self.tx_write_ready.try_send(()).is_ok() {
                    Poll::Ready(Ok(1))
                } else {
                    Poll::Pending
                }
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            Poll::Pending => Poll::Pending,
        }
    }
}

struct LocalTcpStreamReadReady<'a> {
    inner: &'a mut LocalTcpStream,
}
impl<'a> Future for LocalTcpStreamReadReady<'a> {
    type Output = Result<usize>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        self.inner.poll_read_ready(cx)
    }
}

struct LocalTcpStreamWriteReady<'a> {
    inner: &'a mut LocalTcpStream,
}
impl<'a> Future for LocalTcpStreamWriteReady<'a> {
    type Output = Result<usize>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        self.inner.poll_write_ready(cx)
    }
}

#[derive(Debug)]
pub struct LocalUdpSocket {
    socket: tokio::net::UdpSocket,
    #[allow(dead_code)]
    addr: SocketAddr,
    nonblocking: bool,
}

#[async_trait::async_trait]
impl VirtualUdpSocket for LocalUdpSocket {
    fn set_broadcast(&mut self, broadcast: bool) -> Result<()> {
        self.socket
            .set_broadcast(broadcast)
            .map_err(io_err_into_net_error)
    }

    fn broadcast(&self) -> Result<bool> {
        self.socket.broadcast().map_err(io_err_into_net_error)
    }

    fn set_multicast_loop_v4(&mut self, val: bool) -> Result<()> {
        self.socket
            .set_multicast_loop_v4(val)
            .map_err(io_err_into_net_error)
    }

    fn multicast_loop_v4(&self) -> Result<bool> {
        self.socket
            .multicast_loop_v4()
            .map_err(io_err_into_net_error)
    }

    fn set_multicast_loop_v6(&mut self, val: bool) -> Result<()> {
        self.socket
            .set_multicast_loop_v6(val)
            .map_err(io_err_into_net_error)
    }

    fn multicast_loop_v6(&self) -> Result<bool> {
        self.socket
            .multicast_loop_v6()
            .map_err(io_err_into_net_error)
    }

    fn set_multicast_ttl_v4(&mut self, ttl: u32) -> Result<()> {
        self.socket
            .set_multicast_ttl_v4(ttl)
            .map_err(io_err_into_net_error)
    }

    fn multicast_ttl_v4(&self) -> Result<u32> {
        self.socket
            .multicast_ttl_v4()
            .map_err(io_err_into_net_error)
    }

    fn join_multicast_v4(&mut self, multiaddr: Ipv4Addr, iface: Ipv4Addr) -> Result<()> {
        self.socket
            .join_multicast_v4(multiaddr, iface)
            .map_err(io_err_into_net_error)
    }

    fn leave_multicast_v4(&mut self, multiaddr: Ipv4Addr, iface: Ipv4Addr) -> Result<()> {
        self.socket
            .leave_multicast_v4(multiaddr, iface)
            .map_err(io_err_into_net_error)
    }

    fn join_multicast_v6(&mut self, multiaddr: Ipv6Addr, iface: u32) -> Result<()> {
        self.socket
            .join_multicast_v6(&multiaddr, iface)
            .map_err(io_err_into_net_error)
    }

    fn leave_multicast_v6(&mut self, multiaddr: Ipv6Addr, iface: u32) -> Result<()> {
        self.socket
            .leave_multicast_v6(&multiaddr, iface)
            .map_err(io_err_into_net_error)
    }

    fn addr_peer(&self) -> Result<Option<SocketAddr>> {
        self.socket
            .peer_addr()
            .map(Some)
            .map_err(io_err_into_net_error)
    }
}

impl VirtualConnectionlessSocket for LocalUdpSocket {
    fn poll_send_to(
        &mut self,
        cx: &mut Context<'_>,
        data: &[u8],
        addr: SocketAddr,
    ) -> Poll<Result<usize>> {
        self.socket
            .poll_send_to(cx, data, addr)
            .map_err(io_err_into_net_error)
    }

    fn try_send_to(&mut self, data: &[u8], addr: SocketAddr) -> Result<usize> {
        self.socket
            .try_send_to(data, addr)
            .map_err(io_err_into_net_error)
    }

    fn poll_recv_from(
        &mut self,
        cx: &mut Context<'_>,
        buf: &mut [MaybeUninit<u8>],
    ) -> Poll<Result<(usize, SocketAddr)>> {
        let mut read_buf = tokio::io::ReadBuf::uninit(buf);
        let res = self
            .socket
            .poll_recv_from(cx, &mut read_buf)
            .map_err(io_err_into_net_error);
        match res {
            Poll::Ready(Ok(addr)) => {
                let amt = read_buf.filled().len();
                Poll::Ready(Ok((amt, addr)))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            Poll::Pending if self.nonblocking => Poll::Ready(Err(NetworkError::WouldBlock)),
            Poll::Pending => Poll::Pending,
        }
    }

    fn try_recv_from(&mut self, buf: &mut [MaybeUninit<u8>]) -> Result<(usize, SocketAddr)> {
        let buf: &mut [u8] = unsafe { std::mem::transmute(buf) };
        self.socket
            .try_recv_from(buf)
            .map_err(io_err_into_net_error)
    }
}

impl VirtualSocket for LocalUdpSocket {
    fn set_ttl(&mut self, ttl: u32) -> Result<()> {
        self.socket.set_ttl(ttl).map_err(io_err_into_net_error)
    }

    fn ttl(&self) -> Result<u32> {
        self.socket.ttl().map_err(io_err_into_net_error)
    }

    fn addr_local(&self) -> Result<SocketAddr> {
        self.socket.local_addr().map_err(io_err_into_net_error)
    }

    fn status(&self) -> Result<SocketStatus> {
        Ok(SocketStatus::Opened)
    }

    fn poll_read_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<usize>> {
        self.socket
            .poll_recv_ready(cx)
            .map_ok(|()| 8192usize)
            .map_err(io_err_into_net_error)
    }

    fn poll_write_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<usize>> {
        self.socket
            .poll_send_ready(cx)
            .map_ok(|()| 8192usize)
            .map_err(io_err_into_net_error)
    }
}

struct LocalUdpSocketReadReady<'a> {
    socket: &'a mut tokio::net::UdpSocket,
}
impl<'a> Future for LocalUdpSocketReadReady<'a> {
    type Output = Result<usize>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        self.socket
            .poll_recv_ready(cx)
            .map_err(io_err_into_net_error)
            .map_ok(|_| 1usize)
    }
}

struct LocalUdpSocketWriteReady<'a> {
    socket: &'a mut tokio::net::UdpSocket,
}
impl<'a> Future for LocalUdpSocketWriteReady<'a> {
    type Output = Result<usize>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        self.socket
            .poll_send_ready(cx)
            .map_err(io_err_into_net_error)
            .map_ok(|_| 1usize)
    }
}

const NOOP_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);
unsafe fn noop_clone(_data: *const ()) -> RawWaker {
    RawWaker::new(ptr::null(), &NOOP_WAKER_VTABLE)
}
unsafe fn noop(_data: *const ()) {}

pub fn io_err_into_net_error(net_error: std::io::Error) -> NetworkError {
    use std::io::ErrorKind;
    match net_error.kind() {
        ErrorKind::BrokenPipe => NetworkError::BrokenPipe,
        ErrorKind::AlreadyExists => NetworkError::AlreadyExists,
        ErrorKind::AddrInUse => NetworkError::AddressInUse,
        ErrorKind::AddrNotAvailable => NetworkError::AddressNotAvailable,
        ErrorKind::ConnectionAborted => NetworkError::ConnectionAborted,
        ErrorKind::ConnectionRefused => NetworkError::ConnectionRefused,
        ErrorKind::ConnectionReset => NetworkError::ConnectionReset,
        ErrorKind::Interrupted => NetworkError::Interrupted,
        ErrorKind::InvalidData => NetworkError::InvalidData,
        ErrorKind::InvalidInput => NetworkError::InvalidInput,
        ErrorKind::NotConnected => NetworkError::NotConnected,
        ErrorKind::PermissionDenied => NetworkError::PermissionDenied,
        ErrorKind::TimedOut => NetworkError::TimedOut,
        ErrorKind::UnexpectedEof => NetworkError::UnexpectedEof,
        ErrorKind::WouldBlock => NetworkError::WouldBlock,
        ErrorKind::WriteZero => NetworkError::WriteZero,
        ErrorKind::Unsupported => NetworkError::Unsupported,

        #[cfg(target_family = "unix")]
        _ => {
            if let Some(code) = net_error.raw_os_error() {
                match code {
                    libc::EPERM => NetworkError::PermissionDenied,
                    libc::EBADF => NetworkError::InvalidFd,
                    libc::ECHILD => NetworkError::InvalidFd,
                    libc::EMFILE => NetworkError::TooManyOpenFiles,
                    libc::EINTR => NetworkError::Interrupted,
                    libc::EIO => NetworkError::IOError,
                    libc::ENXIO => NetworkError::IOError,
                    libc::EAGAIN => NetworkError::WouldBlock,
                    libc::ENOMEM => NetworkError::InsufficientMemory,
                    libc::EACCES => NetworkError::PermissionDenied,
                    libc::ENODEV => NetworkError::NoDevice,
                    libc::EINVAL => NetworkError::InvalidInput,
                    libc::EPIPE => NetworkError::BrokenPipe,
                    err => {
                        tracing::trace!("unknown os error {}", err);
                        NetworkError::UnknownError
                    }
                }
            } else {
                NetworkError::UnknownError
            }
        }
        #[cfg(not(target_family = "unix"))]
        _ => NetworkError::UnknownError,
    }
}
