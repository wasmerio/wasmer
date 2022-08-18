#![allow(unused_variables)]
use bytes::Bytes;
use tokio::io::{AsyncRead, AsyncWriteExt};
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr};
use std::pin::Pin;
use std::ptr;
use std::sync::Mutex;
use std::task::{RawWakerVTable, RawWaker, Waker, Context, Poll};
use std::time::Duration;
#[allow(unused_imports, dead_code)]
use tracing::{debug, error, info, trace, warn};
#[allow(unused_imports)]
use wasmer_vnet::{
    io_err_into_net_error, IpCidr, IpRoute, NetworkError, Result, SocketHttpRequest, SocketReceive,
    SocketReceiveFrom, SocketStatus, StreamSecurity, TimeType, VirtualConnectedSocket,
    VirtualConnectionlessSocket, VirtualIcmpSocket, VirtualNetworking, VirtualRawSocket,
    VirtualSocket, VirtualTcpListener, VirtualTcpSocket, VirtualUdpSocket, VirtualWebSocket,
};

#[derive(Debug, Default)]
pub struct LocalNetworking {}

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
                    timeout: None,
                    backlog: Mutex::new(Vec::new()),
                    nonblocking: false,
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
            socket: LocalUdpSocketMode::Async(socket),
            addr,
            nonblocking: false
        }))
    }

    async fn connect_tcp(
        &self,
        _addr: SocketAddr,
        peer: SocketAddr,
        timeout: Option<Duration>,
    ) -> Result<Box<dyn VirtualTcpSocket + Sync>> {
        let stream = if let Some(timeout) = timeout {
            match tokio::time::timeout(timeout, tokio::net::TcpStream::connect(&peer))
                .await
            {
                Ok(a) => a,
                Err(err) => {
                    Err(Into::<std::io::Error>::into(std::io::ErrorKind::TimedOut))
                }
            }
        } else {
            tokio::net::TcpStream::connect(peer).await
        }
        .map_err(io_err_into_net_error)?;
        let peer = stream.peer_addr().map_err(io_err_into_net_error)?;
        Ok(Box::new(LocalTcpStream {
            stream: stream,
            addr: peer,
            connect_timeout: None,
            read_timeout: None,
            write_timeout: None,
            linger_timeout: None,
            nonblocking: false,
            shutdown: None
        }))
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
    timeout: Option<Duration>,
    backlog: Mutex<Vec<(Box<LocalTcpStream>, SocketAddr)>>,
    nonblocking: bool,
}

#[async_trait::async_trait]
impl VirtualTcpListener for LocalTcpListener {
    async fn accept(&mut self) -> Result<(Box<dyn VirtualTcpSocket + Sync>, SocketAddr)> {
        {
            let mut backlog = self.backlog.lock().unwrap();
            if let Some((sock, addr)) = backlog.pop() {
                return Ok((sock, addr));
            }
        }

        let nonblocking = self.nonblocking;
        if nonblocking {
            let waker = unsafe { Waker::from_raw(RawWaker::new(ptr::null(), &NOOP_WAKER_VTABLE)) };
            let mut cx = Context::from_waker(&waker);
            return match self.stream
                .poll_accept(&mut cx)
                .map_err(io_err_into_net_error)
            {
                Poll::Ready(Ok((sock, addr))) => {
                    Ok(
                        (
                            Box::new(LocalTcpStream {
                                stream: sock,
                                addr,
                                connect_timeout: None,
                                read_timeout: None,
                                write_timeout: None,
                                linger_timeout: None,
                                nonblocking,
                                shutdown: None
                            }),
                            addr,
                        )
                    )
                },
                Poll::Ready(Err(err)) => Err(err),
                Poll::Pending => Err(NetworkError::WouldBlock)
            };
        }

        let timeout = self.timeout.clone();
        let work = async move {
            match timeout {
                Some(timeout) => {
                    tokio::time::timeout(timeout, self.stream.accept())
                        .await
                        .map_err(|_| Into::<std::io::Error>::into(std::io::ErrorKind::WouldBlock))?                        
                },
                None => self.stream.accept().await
            }
        };

        let (sock, addr) = work
            .await
            .map(|(sock, addr)| {
                (
                    Box::new(LocalTcpStream {
                        stream: sock,
                        addr,
                        connect_timeout: None,
                        read_timeout: None,
                        write_timeout: None,
                        linger_timeout: None,
                        nonblocking,
                        shutdown: None
                    }),
                    addr,
                )
            })
            .map_err(io_err_into_net_error)?;
        Ok((sock, addr))
    }

    fn peek(&mut self) -> Result<usize> {
        {
            let backlog = self.backlog.lock().unwrap();
            if backlog.is_empty() == false {
                return Ok(backlog.len());
            }
        }

        let waker = unsafe { Waker::from_raw(RawWaker::new(ptr::null(), &NOOP_WAKER_VTABLE)) };
        let mut cx = Context::from_waker(&waker);
        match self.stream.poll_accept(&mut cx) {
            Poll::Ready(Ok((sock, addr))) => {
                let mut backlog = self.backlog.lock().unwrap();
                backlog.push((
                    Box::new(LocalTcpStream {
                        stream: sock,
                        addr,
                        connect_timeout: None,
                        read_timeout: None,
                        write_timeout: None,
                        linger_timeout: None,
                        nonblocking: self.nonblocking,
                        shutdown: None
                    }),
                    addr,
                ));
                Ok(backlog.len())
            },
            Poll::Ready(Err(err)) => {
                Err(io_err_into_net_error(err))
            }
            Poll::Pending => {
                let backlog = self.backlog.lock().unwrap();
                Ok(backlog.len())
            }
        }
    }

    fn poll_accept_ready(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<usize>> {
        self.stream
            .poll_accept(cx)
            .map_err(io_err_into_net_error)
            .map_ok(|(sock, addr)| {
                let mut backlog = self.backlog.lock().unwrap();
                backlog.push((
                    Box::new(LocalTcpStream {
                        stream: sock,
                        addr,
                        connect_timeout: None,
                        read_timeout: None,
                        write_timeout: None,
                        linger_timeout: None,
                        nonblocking: self.nonblocking,
                        shutdown: None
                    }),
                    addr,
                ));
                backlog.len()  
            })
    }

    /// Sets the accept timeout
    fn set_timeout(&mut self, timeout: Option<Duration>) -> Result<()> {
        self.timeout = timeout;
        Ok(())
    }

    /// Gets the accept timeout
    fn timeout(&self) -> Result<Option<Duration>> {
        Ok(self.timeout)
    }

    fn addr_local(&self) -> Result<SocketAddr> {
        self.stream.local_addr().map_err(io_err_into_net_error)
    }

    fn set_ttl(&mut self, ttl: u8) -> Result<()> {
        self.stream.set_ttl(ttl as u32).map_err(io_err_into_net_error)
    }

    fn ttl(&self) -> Result<u8> {
        self.stream.ttl().map(|ttl| ttl as u8).map_err(io_err_into_net_error)
    }

    fn set_nonblocking(&mut self, nonblocking: bool) -> Result<()> {
        self.nonblocking = nonblocking;
        Ok(())
    }

    fn nonblocking(&self) -> Result<bool> {
        Ok(self.nonblocking)
    }
}

#[derive(Debug)]
pub struct LocalTcpStream {
    stream: tokio::net::TcpStream,
    addr: SocketAddr,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
    connect_timeout: Option<Duration>,
    linger_timeout: Option<Duration>,
    nonblocking: bool,
    shutdown: Option<Shutdown>,
}

#[async_trait::async_trait]
impl VirtualTcpSocket for LocalTcpStream {
    fn set_opt_time(&mut self, ty: TimeType, timeout: Option<Duration>) -> Result<()> {
        match ty {
            TimeType::ReadTimeout => {
                self.read_timeout = timeout.clone();
            },
            TimeType::WriteTimeout => {
                self.write_timeout = timeout.clone();
            },
            TimeType::ConnectTimeout => {
                self.connect_timeout = timeout;
            }
            #[cfg(feature = "wasix")]
            TimeType::Linger => {
                self.linger_timeout = timeout.clone();
            },
            _ => {
                return Err(NetworkError::InvalidInput)
            },
        }
        Ok(())
    }

    fn opt_time(&self, ty: TimeType) -> Result<Option<Duration>> {
        match ty {
            TimeType::ReadTimeout => Ok(self.read_timeout),
            TimeType::WriteTimeout => Ok(self.write_timeout),
            TimeType::ConnectTimeout => Ok(self.connect_timeout),
            TimeType::Linger => Ok(self.linger_timeout),
            _ => Err(NetworkError::InvalidInput),
        }
    }

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
        self.stream.set_nodelay(nodelay).map_err(io_err_into_net_error)
    }

    fn nodelay(&self) -> Result<bool> {
        self.stream.nodelay().map_err(io_err_into_net_error)
    }

    fn addr_peer(&self) -> Result<SocketAddr> {
        Ok(self.addr)
    }

    async fn flush(&mut self) -> Result<()> {
        Ok(())
    }

    async fn shutdown(&mut self, how: Shutdown) -> Result<()> {
        self.stream.flush().await.map_err(io_err_into_net_error)?;
        self.shutdown = Some(how);
        Ok(())
    }
}

#[async_trait::async_trait]
impl VirtualConnectedSocket for LocalTcpStream {
    fn set_linger(&mut self, linger: Option<Duration>) -> Result<()> {
        #[cfg(feature = "wasix")]
        self.stream
            .set_linger(linger)
            .map_err(io_err_into_net_error)?;
        Ok(())
    }

    #[cfg(feature = "wasix")]
    fn linger(&self) -> Result<Option<Duration>> {
        self.stream.linger().map_err(io_err_into_net_error)
    }

    #[cfg(not(feature = "wasix"))]
    fn linger(&self) -> Result<Option<Duration>> {
        Ok(None)
    }

    async fn send(&mut self, data: Bytes) -> Result<usize> {
        let nonblocking = self.nonblocking;
        if nonblocking {
            let waker = unsafe { Waker::from_raw(RawWaker::new(ptr::null(), &NOOP_WAKER_VTABLE)) };
            let mut cx = Context::from_waker(&waker);
            if self.stream.poll_write_ready(&mut cx).is_pending() {
                return Err(NetworkError::WouldBlock);
            }
        }

        use tokio::io::AsyncWriteExt;
        let timeout = self.write_timeout.clone();
        let work = async move {
            match timeout {
                Some(timeout) => {
                    tokio::time::timeout(timeout, self.stream.write_all(&data[..]))
                        .await
                        .map_err(|_| Into::<std::io::Error>::into(std::io::ErrorKind::WouldBlock))?                        
                },
                None => self.stream.write_all(&data[..]).await
            }
            .map(|_| data.len())
        };

        let amt = work
            .await
            .map_err(io_err_into_net_error)?;
        if amt == 0 {
            if nonblocking {
                return Err(NetworkError::WouldBlock);
            } else {
                return Err(NetworkError::BrokenPipe);
            }
        }
        Ok(amt)
    }

    async fn flush(&mut self) -> Result<()> {
        if self.nonblocking {
            let waker = unsafe { Waker::from_raw(RawWaker::new(ptr::null(), &NOOP_WAKER_VTABLE)) };
            let mut cx = Context::from_waker(&waker);
            if self.stream.poll_write_ready(&mut cx).is_pending() {
                return Err(NetworkError::WouldBlock);
            }
        }
        use tokio::io::AsyncWriteExt;
        let timeout = self.write_timeout.clone();
        let work = async move {
            match timeout {
                Some(timeout) => {
                    tokio::time::timeout(timeout, self.stream.flush())
                        .await
                        .map_err(|_| Into::<std::io::Error>::into(std::io::ErrorKind::WouldBlock))?                        
                },
                None => self.stream.flush().await
            }
        };

        work
            .await
            .map_err(io_err_into_net_error)
    }

    async fn recv(&mut self) -> Result<SocketReceive> {
        use tokio::io::AsyncReadExt;
        let max_buf_size = 8192;
        let mut buf = Vec::with_capacity(max_buf_size);
        unsafe { buf.set_len(max_buf_size); }

        let nonblocking = self.nonblocking;
        if nonblocking {
            let waker = unsafe { Waker::from_raw(RawWaker::new(ptr::null(), &NOOP_WAKER_VTABLE)) };
            let mut cx = Context::from_waker(&waker);
            let stream = Pin::new(&mut self.stream);
            let mut read_buf = tokio::io::ReadBuf::new(&mut buf);
            return match stream.poll_read(&mut cx, &mut read_buf) {
                Poll::Ready(Ok(read)) => {
                    let read = read_buf.remaining();
                    unsafe { buf.set_len(read); }
                    if read == 0 {
                        return Err(NetworkError::WouldBlock);
                    }
                    let buf = Bytes::from(buf);
                    Ok(SocketReceive {
                        data: buf,
                        truncated: read == max_buf_size,
                    })
                },
                Poll::Ready(Err(err)) => {
                    Err(io_err_into_net_error(err))
                },
                Poll::Pending => {
                    Err(NetworkError::WouldBlock)
                }
            };
        }

        let timeout = self.write_timeout.clone();
        let work = async move {
            match timeout {
                Some(timeout) => {
                    tokio::time::timeout(timeout, self.stream.read(&mut buf[..]))
                        .await
                        .map_err(|_| Into::<std::io::Error>::into(std::io::ErrorKind::WouldBlock))?                        
                },
                None => self.stream.read(&mut buf[..]).await
            }
            .map(|read| {
                unsafe { buf.set_len(read); }
                Bytes::from(buf)
            })
        };

        let buf = work
            .await
            .map_err(io_err_into_net_error)?;
        if buf.is_empty() {
            if nonblocking {
                return Err(NetworkError::WouldBlock);
            } else {
                return Err(NetworkError::BrokenPipe);
            }
        }
        Ok(SocketReceive {
            truncated: buf.len() == max_buf_size,
            data: buf,
        })
    }

    fn try_recv(&mut self) -> Result<Option<SocketReceive>> {
        let max_buf_size = 8192;
        let mut buf = Vec::with_capacity(max_buf_size);
        unsafe { buf.set_len(max_buf_size); }

        let waker = unsafe { Waker::from_raw(RawWaker::new(ptr::null(), &NOOP_WAKER_VTABLE)) };
        let mut cx = Context::from_waker(&waker);
        let stream = Pin::new(&mut self.stream);
        let mut read_buf = tokio::io::ReadBuf::new(&mut buf);
        match stream.poll_read(&mut cx, &mut read_buf) {
            Poll::Ready(Ok(read)) => {
                let read = read_buf.remaining();
                unsafe { buf.set_len(read); }
                if read == 0 {
                    return Err(NetworkError::WouldBlock);
                }
                let buf = Bytes::from(buf);
                Ok(Some(SocketReceive {
                    data: buf,
                    truncated: read == max_buf_size,
                }))
            },
            Poll::Ready(Err(err)) => {
                Err(io_err_into_net_error(err))
            },
            Poll::Pending => {
                Ok(None)
            }
        }
    }

    async fn peek(&mut self) -> Result<SocketReceive> {
        let max_buf_size = 8192;
        let mut buf = Vec::with_capacity(max_buf_size);
        unsafe { buf.set_len(max_buf_size); }

        if self.nonblocking {
            let waker = unsafe { Waker::from_raw(RawWaker::new(ptr::null(), &NOOP_WAKER_VTABLE)) };
            let mut cx = Context::from_waker(&waker);
            let stream = Pin::new(&mut self.stream);
            let mut read_buf = tokio::io::ReadBuf::new(&mut buf);
            return match stream.poll_peek(&mut cx, &mut read_buf) {
                Poll::Ready(Ok(read)) => {
                    unsafe { buf.set_len(read); }
                    if read == 0 {
                        return Err(NetworkError::WouldBlock);
                    }
                    let buf = Bytes::from(buf);
                    Ok(SocketReceive {
                        data: buf,
                        truncated: read == max_buf_size,
                    })
                },
                Poll::Ready(Err(err)) => {
                    Err(io_err_into_net_error(err))
                },
                Poll::Pending => {
                    Err(NetworkError::WouldBlock)
                }
            };
        }

        let timeout = self.write_timeout.clone();
        let work = async move {
            match timeout {
                Some(timeout) => {
                    tokio::time::timeout(timeout, self.stream.peek(&mut buf[..]))
                        .await
                        .map_err(|_| Into::<std::io::Error>::into(std::io::ErrorKind::WouldBlock))?                        
                },
                None => self.stream.peek(&mut buf[..]).await
            }
            .map(|read| {
                unsafe { buf.set_len(read); }
                Bytes::from(buf)
            })
        };

        let buf = work
            .await
            .map_err(io_err_into_net_error)?;
        if buf.len() == 0 {
            return Err(NetworkError::BrokenPipe);
        }
        Ok(SocketReceive {
            truncated: buf.len() == max_buf_size,
            data: buf,            
        })
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

    fn set_nonblocking(&mut self, nonblocking: bool) -> Result<()> {
        self.nonblocking = nonblocking;
        Ok(())
    }

    fn nonblocking(&self) -> Result<bool> {
        Ok(self.nonblocking)
    }

    fn addr_local(&self) -> Result<SocketAddr> {
        self.stream.local_addr().map_err(io_err_into_net_error)
    }

    fn status(&self) -> Result<SocketStatus> {
        Ok(SocketStatus::Opened)
    }

    fn poll_read_ready(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<usize>>
    {
        self.stream
            .poll_read_ready(cx)
            .map_ok(|a| 8192usize)
            .map_err(io_err_into_net_error)
    }

    fn poll_write_ready(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<usize>>
    {
        self.stream
            .poll_write_ready(cx)
            .map_ok(|a| 8192usize)
            .map_err(io_err_into_net_error)
    }
}

struct LocalTcpStreamReadReady<'a> {
    stream: &'a mut tokio::net::TcpStream,
}
impl<'a> Future
for LocalTcpStreamReadReady<'a>
{
    type Output = Result<usize>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        self.stream
            .poll_read_ready(cx)
            .map_err(io_err_into_net_error)
            .map_ok(|_| 1usize)
    }
}

struct LocalTcpStreamWriteReady<'a> {
    stream: &'a mut tokio::net::TcpStream,
}
impl<'a> Future
for LocalTcpStreamWriteReady<'a>
{
    type Output = Result<usize>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        self.stream
            .poll_write_ready(cx)
            .map_err(io_err_into_net_error)
            .map_ok(|_| 1usize)
    }
}

#[derive(Debug)]
pub struct LocalUdpSocket {
    socket: LocalUdpSocketMode,
    #[allow(dead_code)]
    addr: SocketAddr,
    nonblocking: bool
}

#[derive(Debug)]
enum LocalUdpSocketMode {
    Blocking(std::net::UdpSocket),
    Async(tokio::net::UdpSocket),
    Uninitialized
}

impl LocalUdpSocketMode
{
    fn as_blocking_mut(&mut self) -> std::io::Result<&mut std::net::UdpSocket> {
        match self {
            Self::Blocking(a) => Ok(a),
            Self::Async(_) => {
                let mut listener = Self::Uninitialized;
                std::mem::swap(self, &mut listener);
                listener = match listener {
                    Self::Async(a) => Self::Blocking(a.into_std()?),
                    a => unreachable!(),
                };
                std::mem::swap(self, &mut listener);
                match self {
                    Self::Blocking(a) => Ok(a),
                    _ => unreachable!()
                }
            },
            Self::Uninitialized => unreachable!()
        }
    }

    fn as_async_mut(&mut self) -> std::io::Result<&mut tokio::net::UdpSocket> {
        match self {
            Self::Async(a) => Ok(a),
            Self::Blocking(_) => {
                let mut listener = Self::Uninitialized;
                std::mem::swap(self, &mut listener);
                listener = match listener {
                    Self::Blocking(a) => Self::Async(tokio::net::UdpSocket::from_std(a)?),
                    a => unreachable!(),
                };
                std::mem::swap(self, &mut listener);
                match self {
                    Self::Async(a) => Ok(a),
                    _ => unreachable!()
                }
            },
            Self::Uninitialized => unreachable!()
        }
    }
}

#[async_trait::async_trait]
impl VirtualUdpSocket for LocalUdpSocket {
    async fn connect(&mut self, addr: SocketAddr) -> Result<()> {
        self.socket
            .as_async_mut()
            .map_err(io_err_into_net_error)?
            .connect(addr)
            .await
            .map_err(io_err_into_net_error)
    }

    fn set_broadcast(&mut self, broadcast: bool) -> Result<()> {
        match &mut self.socket {
            LocalUdpSocketMode::Blocking(a) => a.set_broadcast(broadcast).map_err(io_err_into_net_error),
            LocalUdpSocketMode::Async(a) => a.set_broadcast(broadcast).map_err(io_err_into_net_error),
            LocalUdpSocketMode::Uninitialized => unreachable!()
        }
    }

    fn broadcast(&self) -> Result<bool> {
        match &self.socket {
            LocalUdpSocketMode::Blocking(a) => a.broadcast().map_err(io_err_into_net_error),
            LocalUdpSocketMode::Async(a) => a.broadcast().map_err(io_err_into_net_error),
            LocalUdpSocketMode::Uninitialized => unreachable!()
        }
    }

    fn set_multicast_loop_v4(&mut self, val: bool) -> Result<()> {
        match &mut self.socket {
            LocalUdpSocketMode::Blocking(a) => a.set_multicast_loop_v4(val).map_err(io_err_into_net_error),
            LocalUdpSocketMode::Async(a) => a.set_multicast_loop_v4(val).map_err(io_err_into_net_error),
            LocalUdpSocketMode::Uninitialized => unreachable!()
        }
    }

    fn multicast_loop_v4(&self) -> Result<bool> {
        match &self.socket {
            LocalUdpSocketMode::Blocking(a) => a.multicast_loop_v4().map_err(io_err_into_net_error),
            LocalUdpSocketMode::Async(a) => a.multicast_loop_v4().map_err(io_err_into_net_error),
            LocalUdpSocketMode::Uninitialized => unreachable!()
        }
    }

    fn set_multicast_loop_v6(&mut self, val: bool) -> Result<()> {
        match &mut self.socket {
            LocalUdpSocketMode::Blocking(a) => a.set_multicast_loop_v6(val).map_err(io_err_into_net_error),
            LocalUdpSocketMode::Async(a) => a.set_multicast_loop_v6(val).map_err(io_err_into_net_error),
            LocalUdpSocketMode::Uninitialized => unreachable!()
        }
    }

    fn multicast_loop_v6(&self) -> Result<bool> {
        match &self.socket {
            LocalUdpSocketMode::Blocking(a) => a.multicast_loop_v6().map_err(io_err_into_net_error),
            LocalUdpSocketMode::Async(a) => a.multicast_loop_v6().map_err(io_err_into_net_error),
            LocalUdpSocketMode::Uninitialized => unreachable!()
        }
    }

    fn set_multicast_ttl_v4(&mut self, ttl: u32) -> Result<()> {
        match &mut self.socket {
            LocalUdpSocketMode::Blocking(a) => a.set_multicast_ttl_v4(ttl).map_err(io_err_into_net_error),
            LocalUdpSocketMode::Async(a) => a.set_multicast_ttl_v4(ttl).map_err(io_err_into_net_error),
            LocalUdpSocketMode::Uninitialized => unreachable!()
        }
    }

    fn multicast_ttl_v4(&self) -> Result<u32> {
        match &self.socket {
            LocalUdpSocketMode::Blocking(a) => a.multicast_ttl_v4().map_err(io_err_into_net_error),
            LocalUdpSocketMode::Async(a) => a.multicast_ttl_v4().map_err(io_err_into_net_error),
            LocalUdpSocketMode::Uninitialized => unreachable!()
        }
    }

    fn join_multicast_v4(&mut self, multiaddr: Ipv4Addr, iface: Ipv4Addr) -> Result<()> {
        match &mut self.socket {
            LocalUdpSocketMode::Blocking(a) => a.join_multicast_v4(&multiaddr, &iface).map_err(io_err_into_net_error),
            LocalUdpSocketMode::Async(a) => a.join_multicast_v4(multiaddr, iface).map_err(io_err_into_net_error),
            LocalUdpSocketMode::Uninitialized => unreachable!()
        }
    }

    fn leave_multicast_v4(&mut self, multiaddr: Ipv4Addr, iface: Ipv4Addr) -> Result<()> {
        match &mut self.socket {
            LocalUdpSocketMode::Blocking(a) => a.leave_multicast_v4(&multiaddr, &iface).map_err(io_err_into_net_error),
            LocalUdpSocketMode::Async(a) => a.leave_multicast_v4(multiaddr, iface).map_err(io_err_into_net_error),
            LocalUdpSocketMode::Uninitialized => unreachable!()
        }
    }

    fn join_multicast_v6(&mut self, multiaddr: Ipv6Addr, iface: u32) -> Result<()> {
        match &mut self.socket {
            LocalUdpSocketMode::Blocking(a) => a.join_multicast_v6(&multiaddr, iface).map_err(io_err_into_net_error),
            LocalUdpSocketMode::Async(a) => a.join_multicast_v6(&multiaddr, iface).map_err(io_err_into_net_error),
            LocalUdpSocketMode::Uninitialized => unreachable!()
        }
    }

    fn leave_multicast_v6(&mut self, multiaddr: Ipv6Addr, iface: u32) -> Result<()> {
        match &mut self.socket {
            LocalUdpSocketMode::Blocking(a) => a.leave_multicast_v6(&multiaddr, iface).map_err(io_err_into_net_error),
            LocalUdpSocketMode::Async(a) => a.leave_multicast_v6(&multiaddr, iface).map_err(io_err_into_net_error),
            LocalUdpSocketMode::Uninitialized => unreachable!()
        }
    }

    fn addr_peer(&self) -> Result<Option<SocketAddr>> {
        match &self.socket {
            LocalUdpSocketMode::Blocking(a) => a.peer_addr().map(Some).map_err(io_err_into_net_error),
            LocalUdpSocketMode::Async(a) => a.peer_addr().map(Some).map_err(io_err_into_net_error),
            LocalUdpSocketMode::Uninitialized => unreachable!()
        }
    }
}

#[async_trait::async_trait]
impl VirtualConnectedSocket for LocalUdpSocket {
    fn set_linger(&mut self, linger: Option<Duration>) -> Result<()> {
        Err(NetworkError::Unsupported)
    }

    fn linger(&self) -> Result<Option<Duration>> {
        Err(NetworkError::Unsupported)
    }

    async fn send(&mut self, data: Bytes) -> Result<usize> {
        let amt = self.socket
            .as_async_mut()
            .map_err(io_err_into_net_error)?
            .send(&data[..])
            .await
            .map_err(io_err_into_net_error)?;
        if amt == 0 {
            if self.nonblocking {
                return Err(NetworkError::WouldBlock);
            } else {
                return Err(NetworkError::BrokenPipe);
            }
        }
        Ok(amt)
    }

    async fn flush(&mut self) -> Result<()> {
        Ok(())
    }

    async fn recv(&mut self) -> Result<SocketReceive> {
        let buf_size = 8192;
        let mut buf = Vec::with_capacity(buf_size);
        unsafe { buf.set_len(buf_size); }

        let read = self.socket
            .as_async_mut()
            .map_err(io_err_into_net_error)?
            .recv(&mut buf[..])
            .await
            .map_err(io_err_into_net_error)?;
        unsafe { buf.set_len(read); }
        if read == 0 {
            if self.nonblocking {
                return Err(NetworkError::WouldBlock);
            } else {
                return Err(NetworkError::BrokenPipe);
            }
        }
        let buf = Bytes::from(buf);
        Ok(SocketReceive {
            data: buf,
            truncated: read == buf_size,
        })
    }

    fn try_recv(&mut self) -> Result<Option<SocketReceive>> {
        let buf_size = 8192;
        let mut buf = Vec::with_capacity(buf_size);
        unsafe { buf.set_len(buf_size); }

        let socket = self.socket.as_blocking_mut().map_err(io_err_into_net_error)?;
        socket.set_nonblocking(true).map_err(io_err_into_net_error)?;
        let read = socket.recv(&mut buf[..]);
        let _ = socket.set_nonblocking(self.nonblocking);

        let read = match read {
            Ok(0) => {
                return Ok(None);
            }
            Ok(a) => Ok(a),
            Err(err) if err.kind() == std::io::ErrorKind::TimedOut ||
                               err.kind() == std::io::ErrorKind::WouldBlock => {
                return Ok(None);
            },
            Err(err) => Err(io_err_into_net_error(err))
        }?;
        unsafe { buf.set_len(read); }

        let buf = Bytes::from(buf);
        Ok(Some(SocketReceive {
            data: buf,
            truncated: read == buf_size,
        }))
    }

    async fn peek(&mut self) -> Result<SocketReceive> {
        let buf_size = 8192;
        let mut buf = Vec::with_capacity(buf_size);
        unsafe { buf.set_len(buf_size); }

        let read = self.socket
            .as_blocking_mut()
            .map_err(io_err_into_net_error)?
            .peek(&mut buf[..])
            .map_err(io_err_into_net_error)?;
        unsafe { buf.set_len(read); }
        if read == 0 {
            if self.nonblocking {
                return Err(NetworkError::WouldBlock);
            } else {
                return Err(NetworkError::BrokenPipe);
            }
        }

        let buf = Bytes::from(buf);
        Ok(SocketReceive {
            data: buf,
            truncated: read == buf_size,
        })
    }
}

#[async_trait::async_trait]
impl VirtualConnectionlessSocket for LocalUdpSocket {
    async fn send_to(&mut self, data: Bytes, addr: SocketAddr) -> Result<usize> {
        let amt = self.socket
            .as_async_mut()
            .map_err(io_err_into_net_error)?
            .send_to(&data[..], addr)
            .await
            .map_err(io_err_into_net_error)?;
        if amt == 0 {
            if self.nonblocking {
                return Err(NetworkError::WouldBlock);
            } else {
                return Err(NetworkError::BrokenPipe);
            }
        }
        Ok(amt)
    }

    fn try_recv_from(&mut self) -> Result<Option<SocketReceiveFrom>> {
        let buf_size = 8192;
        let mut buf = Vec::with_capacity(buf_size);
        unsafe { buf.set_len(buf_size); }

        let socket = self.socket.as_blocking_mut().map_err(io_err_into_net_error)?;
        socket.set_nonblocking(true).map_err(io_err_into_net_error)?;
        let read = socket.recv_from(&mut buf[..]);
        let _ = socket.set_nonblocking(self.nonblocking);

        let (read, peer) = match read {
            Ok((0, _))=> {
                return Ok(None);
            }
            Ok((a, b)) => Ok((a, b)),
            Err(err) if err.kind() == std::io::ErrorKind::TimedOut ||
                               err.kind() == std::io::ErrorKind::WouldBlock => {
                return Ok(None);
            },
            Err(err) => Err(io_err_into_net_error(err))
        }?;
        unsafe { buf.set_len(read); }
        
        let buf = Bytes::from(buf);
        Ok(Some(SocketReceiveFrom {
            data: buf,
            truncated: read == buf_size,
            addr: peer,
        }))
    }

    async fn recv_from(&mut self) -> Result<SocketReceiveFrom> {
        let buf_size = 8192;
        let mut buf = Vec::with_capacity(buf_size);
        unsafe { buf.set_len(buf_size); }

        let (read, peer) = self
            .socket
            .as_async_mut()
            .map_err(io_err_into_net_error)?
            .recv_from(&mut buf[..])
            .await
            .map_err(io_err_into_net_error)?;
        unsafe { buf.set_len(read); }
        if read == 0 {
            if self.nonblocking {
                return Err(NetworkError::WouldBlock);
            } else {
                return Err(NetworkError::BrokenPipe);
            }
        }
        let buf = Bytes::from(buf);
        Ok(SocketReceiveFrom {
            data: buf,
            truncated: read == buf_size,
            addr: peer,
        })
    }

    fn peek_from(&mut self) -> Result<SocketReceiveFrom> {
        let buf_size = 8192;
        let mut buf = Vec::with_capacity(buf_size);
        unsafe { buf.set_len(buf_size); }

        let (read, peer) = self
            .socket
            .as_blocking_mut()
            .map_err(io_err_into_net_error)?
            .peek_from(&mut buf[..])
            .map_err(io_err_into_net_error)?;
        unsafe { buf.set_len(read); }
        if read == 0 {
            if self.nonblocking {
                return Err(NetworkError::WouldBlock);
            } else {
                return Err(NetworkError::BrokenPipe);
            }
        }
        let buf = Bytes::from(buf);
        Ok(SocketReceiveFrom {
            data: buf,
            truncated: read == buf_size,
            addr: peer,
        })
    }
}

#[async_trait::async_trait]
impl VirtualSocket for LocalUdpSocket {
    fn set_ttl(&mut self, ttl: u32) -> Result<()> {
        match &mut self.socket {
            LocalUdpSocketMode::Blocking(a) => a.set_ttl(ttl).map_err(io_err_into_net_error),
            LocalUdpSocketMode::Async(a) => a.set_ttl(ttl).map_err(io_err_into_net_error),
            LocalUdpSocketMode::Uninitialized => unreachable!()
        }
    }

    fn set_nonblocking(&mut self, nonblocking: bool) -> Result<()> {
        self.nonblocking = nonblocking;
        self.socket
            .as_blocking_mut()
            .map_err(io_err_into_net_error)?
            .set_nonblocking(nonblocking)
            .map_err(io_err_into_net_error)?;
        Ok(())
    }

    fn nonblocking(&self) -> Result<bool> {
        Ok(self.nonblocking)
    }

    fn ttl(&self) -> Result<u32> {
        match &self.socket {
            LocalUdpSocketMode::Blocking(a) => a.ttl().map_err(io_err_into_net_error),
            LocalUdpSocketMode::Async(a) => a.ttl().map_err(io_err_into_net_error),
            LocalUdpSocketMode::Uninitialized => unreachable!()
        }
    }

    fn addr_local(&self) -> Result<SocketAddr> {
        match &self.socket {
            LocalUdpSocketMode::Blocking(a) => a.local_addr().map_err(io_err_into_net_error),
            LocalUdpSocketMode::Async(a) => a.local_addr().map_err(io_err_into_net_error),
            LocalUdpSocketMode::Uninitialized => unreachable!()
        }
    }

    fn status(&self) -> Result<SocketStatus> {
        Ok(SocketStatus::Opened)
    }

    fn poll_read_ready(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<usize>>
    {
        let socket = self.socket
            .as_async_mut()
            .map_err(io_err_into_net_error)?;
        socket
            .poll_recv_ready(cx)
            .map_ok(|a| 8192usize)
            .map_err(io_err_into_net_error)
        
    }

    fn poll_write_ready(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<usize>>
    {
        let socket = self.socket
            .as_async_mut()
            .map_err(io_err_into_net_error)?;
        socket
            .poll_send_ready(cx)
            .map_ok(|a| 8192usize)
            .map_err(io_err_into_net_error)
    }
}

struct LocalUdpSocketReadReady<'a> {
    socket: &'a mut tokio::net::UdpSocket,
}
impl<'a> Future
for LocalUdpSocketReadReady<'a>
{
    type Output = Result<usize>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        self.socket
            .poll_recv_ready(cx)
            .map_err(io_err_into_net_error)
            .map_ok(|_| 1usize)
    }
}

struct LocalUdpSocketWriteReady<'a> {
    socket: &'a mut tokio::net::UdpSocket,
}
impl<'a> Future
for LocalUdpSocketWriteReady<'a>
{
    type Output = Result<usize>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
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
