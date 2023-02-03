use std::{
    future::Future,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    ops::DerefMut,
    pin::Pin,
    sync::Arc,
    task::Poll,
    time::Duration,
};

use bytes::{Buf, Bytes};
#[cfg(feature = "enable-serde")]
use serde_derive::{Deserialize, Serialize};
use tokio::sync::OwnedRwLockWriteGuard;
use wasmer_types::MemorySize;
use wasmer_vnet::{
    DynVirtualNetworking, TimeType, VirtualConnectedSocket, VirtualIcmpSocket, VirtualRawSocket,
    VirtualTcpListener, VirtualTcpSocket, VirtualUdpSocket, VirtualWebSocket,
};
use wasmer_wasi_types::wasi::{
    Addressfamily, Errno, Fdflags, Rights, SockProto, Sockoption, Socktype,
};

use crate::net::net_error_into_wasi_err;

#[derive(Debug)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub enum InodeHttpSocketType {
    /// Used to feed the bytes into the request itself
    Request,
    /// Used to receive the bytes from the HTTP server
    Response,
    /// Used to read the headers from the HTTP server
    Headers,
}

#[derive(Debug)]
//#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub enum InodeSocketKind {
    PreSocket {
        family: Addressfamily,
        ty: Socktype,
        pt: SockProto,
        addr: Option<SocketAddr>,
        only_v6: bool,
        reuse_port: bool,
        reuse_addr: bool,
        nonblocking: bool,
        send_buf_size: Option<usize>,
        recv_buf_size: Option<usize>,
        send_timeout: Option<Duration>,
        recv_timeout: Option<Duration>,
        connect_timeout: Option<Duration>,
        accept_timeout: Option<Duration>,
    },
    WebSocket(Box<dyn VirtualWebSocket + Sync>),
    Icmp(Box<dyn VirtualIcmpSocket + Sync>),
    Raw(Box<dyn VirtualRawSocket + Sync>),
    TcpListener(Box<dyn VirtualTcpListener + Sync>),
    TcpStream(Box<dyn VirtualTcpSocket + Sync>),
    UdpSocket(Box<dyn VirtualUdpSocket + Sync>),
    Closed,
}

pub enum WasiSocketOption {
    Noop,
    ReusePort,
    ReuseAddr,
    NoDelay,
    DontRoute,
    OnlyV6,
    Broadcast,
    MulticastLoopV4,
    MulticastLoopV6,
    Promiscuous,
    Listening,
    LastError,
    KeepAlive,
    Linger,
    OobInline,
    RecvBufSize,
    SendBufSize,
    RecvLowat,
    SendLowat,
    RecvTimeout,
    SendTimeout,
    ConnectTimeout,
    AcceptTimeout,
    Ttl,
    MulticastTtlV4,
    Type,
    Proto,
}

impl From<Sockoption> for WasiSocketOption {
    fn from(opt: Sockoption) -> Self {
        use WasiSocketOption::*;
        match opt {
            Sockoption::Noop => Noop,
            Sockoption::ReusePort => ReusePort,
            Sockoption::ReuseAddr => ReuseAddr,
            Sockoption::NoDelay => NoDelay,
            Sockoption::DontRoute => DontRoute,
            Sockoption::OnlyV6 => OnlyV6,
            Sockoption::Broadcast => Broadcast,
            Sockoption::MulticastLoopV4 => MulticastLoopV4,
            Sockoption::MulticastLoopV6 => MulticastLoopV6,
            Sockoption::Promiscuous => Promiscuous,
            Sockoption::Listening => Listening,
            Sockoption::LastError => LastError,
            Sockoption::KeepAlive => KeepAlive,
            Sockoption::Linger => Linger,
            Sockoption::OobInline => OobInline,
            Sockoption::RecvBufSize => RecvBufSize,
            Sockoption::SendBufSize => SendBufSize,
            Sockoption::RecvLowat => RecvLowat,
            Sockoption::SendLowat => SendLowat,
            Sockoption::RecvTimeout => RecvTimeout,
            Sockoption::SendTimeout => SendTimeout,
            Sockoption::ConnectTimeout => ConnectTimeout,
            Sockoption::AcceptTimeout => AcceptTimeout,
            Sockoption::Ttl => Ttl,
            Sockoption::MulticastTtlV4 => MulticastTtlV4,
            Sockoption::Type => Type,
            Sockoption::Proto => Proto,
        }
    }
}

#[derive(Debug)]
pub enum WasiSocketStatus {
    Opening,
    Opened,
    Closed,
    Failed,
}

#[derive(Debug)]
//#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub(crate) struct InodeSocketInner {
    pub kind: InodeSocketKind,
    pub read_buffer: Option<Bytes>,
    pub read_addr: Option<SocketAddr>,
}

#[derive(Debug, Clone)]
//#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct InodeSocket {
    pub(crate) inner: Arc<tokio::sync::RwLock<InodeSocketInner>>,
}

impl InodeSocket {
    pub fn new(kind: InodeSocketKind) -> Self {
        Self {
            inner: Arc::new(tokio::sync::RwLock::new(InodeSocketInner {
                kind,
                read_buffer: None,
                read_addr: None,
            })),
        }
    }

    pub async fn bind(
        &self,
        net: DynVirtualNetworking,
        set_addr: SocketAddr,
    ) -> Result<Option<InodeSocket>, Errno> {
        let mut inner = self.inner.write().await;
        match &mut inner.kind {
            InodeSocketKind::PreSocket {
                family,
                ty,
                addr,
                reuse_port,
                reuse_addr,
                nonblocking,
                ..
            } => {
                match *family {
                    Addressfamily::Inet4 => {
                        if !set_addr.is_ipv4() {
                            return Err(Errno::Inval);
                        }
                    }
                    Addressfamily::Inet6 => {
                        if !set_addr.is_ipv6() {
                            return Err(Errno::Inval);
                        }
                    }
                    _ => {
                        return Err(Errno::Notsup);
                    }
                }

                addr.replace(set_addr);
                let addr = (*addr).unwrap();

                Ok(match *ty {
                    Socktype::Stream => {
                        // we already set the socket address - next we need a bind or connect so nothing
                        // more to do at this time
                        None
                    }
                    Socktype::Dgram => {
                        let mut socket = net
                            .bind_udp(addr, *reuse_port, *reuse_addr)
                            .await
                            .map_err(net_error_into_wasi_err)?;
                        socket
                            .set_nonblocking(*nonblocking)
                            .map_err(net_error_into_wasi_err)?;
                        Some(InodeSocket::new(InodeSocketKind::UdpSocket(socket)))
                    }
                    _ => return Err(Errno::Inval),
                })
            }
            _ => Err(Errno::Notsup),
        }
    }

    pub async fn listen(
        &self,
        net: DynVirtualNetworking,
        _backlog: usize,
    ) -> Result<Option<InodeSocket>, Errno> {
        let inner = self.inner.read().await;
        match &inner.kind {
            InodeSocketKind::PreSocket {
                ty,
                addr,
                only_v6,
                reuse_port,
                reuse_addr,
                accept_timeout,
                nonblocking,
                ..
            } => Ok(match *ty {
                Socktype::Stream => {
                    if addr.is_none() {
                        tracing::warn!("wasi[?]::sock_listen - failed - address not set");
                        return Err(Errno::Inval);
                    }
                    let addr = *addr.as_ref().unwrap();
                    let mut socket = net
                        .listen_tcp(addr, *only_v6, *reuse_port, *reuse_addr)
                        .await
                        .map_err(|err| {
                            tracing::warn!("wasi[?]::sock_listen - failed - {}", err);
                            net_error_into_wasi_err(err)
                        })?;
                    socket
                        .set_nonblocking(*nonblocking)
                        .map_err(net_error_into_wasi_err)?;
                    if let Some(accept_timeout) = accept_timeout {
                        socket
                            .set_timeout(Some(*accept_timeout))
                            .map_err(net_error_into_wasi_err)?;
                    }
                    Some(InodeSocket::new(InodeSocketKind::TcpListener(socket)))
                }
                _ => {
                    tracing::warn!("wasi[?]::sock_listen - failed - not supported(1)");
                    return Err(Errno::Notsup);
                }
            }),
            InodeSocketKind::Closed => {
                tracing::warn!("wasi[?]::sock_listen - failed - socket closed");
                Err(Errno::Io)
            }
            _ => {
                tracing::warn!("wasi[?]::sock_listen - failed - not supported(2)");
                Err(Errno::Notsup)
            }
        }
    }

    pub async fn accept(
        &self,
        fd_flags: Fdflags,
    ) -> Result<(Box<dyn VirtualTcpSocket + Sync>, SocketAddr), Errno> {
        let nonblocking = fd_flags.contains(Fdflags::NONBLOCK);
        let timeout = self.opt_time(TimeType::AcceptTimeout).await?;
        struct SocketAccepter<'a> {
            sock: &'a InodeSocket,
            nonblocking: bool,
            next_lock:
                Option<Pin<Box<dyn Future<Output = OwnedRwLockWriteGuard<InodeSocketInner>>>>>,
        }
        impl<'a> Future for SocketAccepter<'a> {
            type Output = Result<(Box<dyn VirtualTcpSocket + Sync>, SocketAddr), Errno>;
            fn poll(
                mut self: Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Self::Output> {
                if self.next_lock.is_none() {
                    let inner = self.sock.inner.clone();
                    self.next_lock.replace(Box::pin(inner.write_owned()));
                }
                let next_lock = self.next_lock.as_mut().unwrap().as_mut();
                match next_lock.poll(cx) {
                    Poll::Ready(mut inner) => {
                        self.next_lock.take();
                        match &mut inner.kind {
                            InodeSocketKind::TcpListener(sock) => {
                                if self.nonblocking {
                                    match sock.try_accept() {
                                        Some(Ok((mut child, addr))) => {
                                            if let Err(err) = child.set_nonblocking(true) {
                                                child.close().ok();
                                                return Poll::Ready(Err(net_error_into_wasi_err(
                                                    err,
                                                )));
                                            }
                                            Poll::Ready(Ok((child, addr)))
                                        }
                                        Some(Err(err)) => {
                                            Poll::Ready(Err(net_error_into_wasi_err(err)))
                                        }
                                        None => Poll::Ready(Err(Errno::Again)),
                                    }
                                } else {
                                    sock.poll_accept(cx).map_err(net_error_into_wasi_err)
                                }
                            }
                            InodeSocketKind::PreSocket { .. } => Poll::Ready(Err(Errno::Notconn)),
                            InodeSocketKind::Closed => Poll::Ready(Err(Errno::Io)),
                            _ => Poll::Ready(Err(Errno::Notsup)),
                        }
                    }
                    Poll::Pending => Poll::Pending,
                }
            }
        }
        if let Some(timeout) = timeout {
            #[cfg(not(feature = "js"))]
            tokio::select! {
                res = SocketAccepter { sock: self, nonblocking, next_lock: None } => res,
                _ = tokio::time::sleep(timeout) => Err(Errno::Timedout)
            }

            // FIXME: enable timeouts for JS! (sleep not available in js tokio)
            #[cfg(feature = "js")]
            {
                let _ = timeout;

                tokio::select! {
                    res = SocketAccepter { sock: self, next_lock: None, nonblocking } => res,
                }
            }
        } else {
            tokio::select! {
                res = SocketAccepter { sock: self, nonblocking, next_lock: None } => res,
            }
        }
    }

    pub async fn close(&self) -> Result<(), Errno> {
        let mut inner = self.inner.write().await;
        match &mut inner.kind {
            InodeSocketKind::TcpListener(_) => {}
            InodeSocketKind::TcpStream(sock) => {
                sock.close().map_err(net_error_into_wasi_err)?;
            }
            InodeSocketKind::Icmp(_) => {}
            InodeSocketKind::UdpSocket(sock) => {
                sock.close().map_err(net_error_into_wasi_err)?;
            }
            InodeSocketKind::WebSocket(_) => {}
            InodeSocketKind::Raw(_) => {}
            InodeSocketKind::PreSocket { .. } => return Err(Errno::Notconn),
            InodeSocketKind::Closed => return Err(Errno::Notconn),
        };
        Ok(())
    }

    pub async fn flush(&self) -> Result<(), Errno> {
        let mut inner = self.inner.write().await;
        match &mut inner.kind {
            InodeSocketKind::TcpListener(_) => {}
            InodeSocketKind::TcpStream(sock) => {
                VirtualConnectedSocket::flush(sock.deref_mut())
                    .await
                    .map_err(net_error_into_wasi_err)?;
            }
            InodeSocketKind::Icmp(_) => {}
            InodeSocketKind::UdpSocket(sock) => {
                VirtualConnectedSocket::flush(sock.as_mut())
                    .await
                    .map_err(net_error_into_wasi_err)?;
            }
            InodeSocketKind::WebSocket(_) => {}
            InodeSocketKind::Raw(sock) => {
                VirtualRawSocket::flush(sock.deref_mut())
                    .await
                    .map_err(net_error_into_wasi_err)?;
            }
            InodeSocketKind::PreSocket { .. } => return Err(Errno::Notconn),
            InodeSocketKind::Closed => return Err(Errno::Notconn),
        };
        Ok(())
    }

    pub async fn connect(
        &mut self,
        net: DynVirtualNetworking,
        peer: SocketAddr,
    ) -> Result<Option<InodeSocket>, Errno> {
        let mut inner = self.inner.write().await;
        match &mut inner.kind {
            InodeSocketKind::PreSocket {
                ty,
                addr,
                send_timeout,
                recv_timeout,
                connect_timeout,
                ..
            } => Ok(match *ty {
                Socktype::Stream => {
                    let addr = match addr {
                        Some(a) => *a,
                        None => {
                            let ip = match peer.is_ipv4() {
                                true => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                                false => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
                            };
                            SocketAddr::new(ip, 0)
                        }
                    };
                    let mut socket = net
                        .connect_tcp(addr, peer, *connect_timeout)
                        .await
                        .map_err(net_error_into_wasi_err)?;
                    if let Some(timeout) = send_timeout {
                        socket
                            .set_opt_time(TimeType::WriteTimeout, Some(*timeout))
                            .map_err(net_error_into_wasi_err)?;
                    }
                    if let Some(timeout) = recv_timeout {
                        socket
                            .set_opt_time(TimeType::ReadTimeout, Some(*timeout))
                            .map_err(net_error_into_wasi_err)?;
                    }
                    Some(InodeSocket::new(InodeSocketKind::TcpStream(socket)))
                }
                Socktype::Dgram => return Err(Errno::Inval),
                _ => return Err(Errno::Notsup),
            }),
            InodeSocketKind::UdpSocket(sock) => {
                sock.connect(peer).await.map_err(net_error_into_wasi_err)?;
                Ok(None)
            }
            InodeSocketKind::Closed => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub async fn status(&self) -> Result<WasiSocketStatus, Errno> {
        let inner = self.inner.read().await;
        Ok(match &inner.kind {
            InodeSocketKind::PreSocket { .. } => WasiSocketStatus::Opening,
            InodeSocketKind::WebSocket(_) => WasiSocketStatus::Opened,
            InodeSocketKind::TcpListener(_) => WasiSocketStatus::Opened,
            InodeSocketKind::TcpStream(_) => WasiSocketStatus::Opened,
            InodeSocketKind::UdpSocket(_) => WasiSocketStatus::Opened,
            InodeSocketKind::Closed => WasiSocketStatus::Closed,
            _ => WasiSocketStatus::Failed,
        })
    }

    pub async fn addr_local(&self) -> Result<SocketAddr, Errno> {
        let inner = self.inner.read().await;
        Ok(match &inner.kind {
            InodeSocketKind::PreSocket { family, addr, .. } => {
                if let Some(addr) = addr {
                    *addr
                } else {
                    SocketAddr::new(
                        match *family {
                            Addressfamily::Inet4 => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                            Addressfamily::Inet6 => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
                            _ => return Err(Errno::Inval),
                        },
                        0,
                    )
                }
            }
            InodeSocketKind::Icmp(sock) => sock.addr_local().map_err(net_error_into_wasi_err)?,
            InodeSocketKind::TcpListener(sock) => {
                sock.addr_local().map_err(net_error_into_wasi_err)?
            }
            InodeSocketKind::TcpStream(sock) => {
                sock.addr_local().map_err(net_error_into_wasi_err)?
            }
            InodeSocketKind::UdpSocket(sock) => {
                sock.addr_local().map_err(net_error_into_wasi_err)?
            }
            InodeSocketKind::Closed => return Err(Errno::Io),
            _ => return Err(Errno::Notsup),
        })
    }

    pub async fn addr_peer(&self) -> Result<SocketAddr, Errno> {
        let inner = self.inner.read().await;
        Ok(match &inner.kind {
            InodeSocketKind::PreSocket { family, .. } => SocketAddr::new(
                match *family {
                    Addressfamily::Inet4 => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                    Addressfamily::Inet6 => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
                    _ => return Err(Errno::Inval),
                },
                0,
            ),
            InodeSocketKind::TcpStream(sock) => {
                sock.addr_peer().map_err(net_error_into_wasi_err)?
            }
            InodeSocketKind::UdpSocket(sock) => sock
                .addr_peer()
                .map_err(net_error_into_wasi_err)?
                .map(Ok)
                .unwrap_or_else(|| {
                    sock.addr_local()
                        .map_err(net_error_into_wasi_err)
                        .map(|addr| {
                            SocketAddr::new(
                                match addr {
                                    SocketAddr::V4(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                                    SocketAddr::V6(_) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
                                },
                                0,
                            )
                        })
                })?,
            InodeSocketKind::Closed => return Err(Errno::Io),
            _ => return Err(Errno::Notsup),
        })
    }

    pub async fn set_opt_flag(&mut self, option: WasiSocketOption, val: bool) -> Result<(), Errno> {
        let mut inner = self.inner.write().await;
        match &mut inner.kind {
            InodeSocketKind::PreSocket {
                only_v6,
                reuse_port,
                reuse_addr,
                ..
            } => {
                match option {
                    WasiSocketOption::OnlyV6 => *only_v6 = val,
                    WasiSocketOption::ReusePort => *reuse_port = val,
                    WasiSocketOption::ReuseAddr => *reuse_addr = val,
                    _ => return Err(Errno::Inval),
                };
            }
            InodeSocketKind::Raw(sock) => match option {
                WasiSocketOption::Promiscuous => sock
                    .set_promiscuous(val)
                    .await
                    .map_err(net_error_into_wasi_err)?,
                _ => return Err(Errno::Inval),
            },
            InodeSocketKind::TcpStream(sock) => match option {
                WasiSocketOption::NoDelay => sock
                    .set_nodelay(val)
                    .await
                    .map_err(net_error_into_wasi_err)?,
                _ => return Err(Errno::Inval),
            },
            InodeSocketKind::UdpSocket(sock) => match option {
                WasiSocketOption::Broadcast => {
                    sock.set_broadcast(val).map_err(net_error_into_wasi_err)?
                }
                WasiSocketOption::MulticastLoopV4 => sock
                    .set_multicast_loop_v4(val)
                    .map_err(net_error_into_wasi_err)?,
                WasiSocketOption::MulticastLoopV6 => sock
                    .set_multicast_loop_v6(val)
                    .map_err(net_error_into_wasi_err)?,
                _ => return Err(Errno::Inval),
            },
            InodeSocketKind::Closed => return Err(Errno::Io),
            _ => return Err(Errno::Notsup),
        }
        Ok(())
    }

    pub async fn get_opt_flag(&self, option: WasiSocketOption) -> Result<bool, Errno> {
        let mut inner = self.inner.write().await;
        Ok(match &mut inner.kind {
            InodeSocketKind::PreSocket {
                only_v6,
                reuse_port,
                reuse_addr,
                ..
            } => match option {
                WasiSocketOption::OnlyV6 => *only_v6,
                WasiSocketOption::ReusePort => *reuse_port,
                WasiSocketOption::ReuseAddr => *reuse_addr,
                _ => return Err(Errno::Inval),
            },
            InodeSocketKind::Raw(sock) => match option {
                WasiSocketOption::Promiscuous => {
                    sock.promiscuous().map_err(net_error_into_wasi_err)?
                }
                _ => return Err(Errno::Inval),
            },
            InodeSocketKind::TcpStream(sock) => match option {
                WasiSocketOption::NoDelay => sock.nodelay().map_err(net_error_into_wasi_err)?,
                _ => return Err(Errno::Inval),
            },
            InodeSocketKind::UdpSocket(sock) => match option {
                WasiSocketOption::Broadcast => sock.broadcast().map_err(net_error_into_wasi_err)?,
                WasiSocketOption::MulticastLoopV4 => {
                    sock.multicast_loop_v4().map_err(net_error_into_wasi_err)?
                }
                WasiSocketOption::MulticastLoopV6 => {
                    sock.multicast_loop_v6().map_err(net_error_into_wasi_err)?
                }
                _ => return Err(Errno::Inval),
            },
            InodeSocketKind::Closed => return Err(Errno::Io),
            _ => return Err(Errno::Notsup),
        })
    }

    pub async fn set_send_buf_size(&mut self, size: usize) -> Result<(), Errno> {
        let mut inner = self.inner.write().await;
        match &mut inner.kind {
            InodeSocketKind::PreSocket { send_buf_size, .. } => {
                *send_buf_size = Some(size);
            }
            InodeSocketKind::TcpStream(sock) => {
                sock.set_send_buf_size(size)
                    .map_err(net_error_into_wasi_err)?;
            }
            InodeSocketKind::Closed => return Err(Errno::Io),
            _ => return Err(Errno::Notsup),
        }
        Ok(())
    }

    pub async fn send_buf_size(&self) -> Result<usize, Errno> {
        let inner = self.inner.read().await;
        match &inner.kind {
            InodeSocketKind::PreSocket { send_buf_size, .. } => {
                Ok((*send_buf_size).unwrap_or_default())
            }
            InodeSocketKind::TcpStream(sock) => {
                sock.send_buf_size().map_err(net_error_into_wasi_err)
            }
            InodeSocketKind::Closed => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub async fn set_recv_buf_size(&mut self, size: usize) -> Result<(), Errno> {
        let mut inner = self.inner.write().await;
        match &mut inner.kind {
            InodeSocketKind::PreSocket { recv_buf_size, .. } => {
                *recv_buf_size = Some(size);
            }
            InodeSocketKind::TcpStream(sock) => {
                sock.set_recv_buf_size(size)
                    .map_err(net_error_into_wasi_err)?;
            }
            InodeSocketKind::Closed => return Err(Errno::Io),
            _ => return Err(Errno::Notsup),
        }
        Ok(())
    }

    pub async fn recv_buf_size(&self) -> Result<usize, Errno> {
        let inner = self.inner.read().await;
        match &inner.kind {
            InodeSocketKind::PreSocket { recv_buf_size, .. } => {
                Ok((*recv_buf_size).unwrap_or_default())
            }
            InodeSocketKind::TcpStream(sock) => {
                sock.recv_buf_size().map_err(net_error_into_wasi_err)
            }
            InodeSocketKind::Closed => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub async fn set_linger(&mut self, linger: Option<std::time::Duration>) -> Result<(), Errno> {
        let mut inner = self.inner.write().await;
        match &mut inner.kind {
            InodeSocketKind::TcpStream(sock) => {
                sock.set_linger(linger).map_err(net_error_into_wasi_err)
            }
            InodeSocketKind::PreSocket { .. } => Err(Errno::Io),
            InodeSocketKind::Closed => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub async fn nonblocking(&self) -> Result<bool, Errno> {
        let inner = self.inner.read().await;
        Ok(match &inner.kind {
            InodeSocketKind::TcpStream(sock) => {
                sock.nonblocking().map_err(net_error_into_wasi_err)?
            }
            InodeSocketKind::TcpListener(sock, ..) => {
                sock.nonblocking().map_err(net_error_into_wasi_err)?
            }
            InodeSocketKind::UdpSocket(sock, ..) => {
                sock.nonblocking().map_err(net_error_into_wasi_err)?
            }
            InodeSocketKind::Raw(sock, ..) => {
                sock.nonblocking().map_err(net_error_into_wasi_err)?
            }
            InodeSocketKind::Icmp(sock, ..) => {
                sock.nonblocking().map_err(net_error_into_wasi_err)?
            }
            InodeSocketKind::PreSocket { nonblocking, .. } => *nonblocking,
            _ => {
                return Err(Errno::Notsup);
            }
        })
    }

    pub async fn set_nonblocking(&self, val: bool) -> Result<(), Errno> {
        let mut inner = self.inner.write().await;
        match &mut inner.kind {
            InodeSocketKind::TcpStream(sock) => {
                sock.set_nonblocking(val).map_err(net_error_into_wasi_err)
            }
            InodeSocketKind::TcpListener(sock, ..) => {
                sock.set_nonblocking(val).map_err(net_error_into_wasi_err)
            }
            InodeSocketKind::UdpSocket(sock, ..) => {
                sock.set_nonblocking(val).map_err(net_error_into_wasi_err)
            }
            InodeSocketKind::Raw(sock, ..) => {
                sock.set_nonblocking(val).map_err(net_error_into_wasi_err)
            }
            InodeSocketKind::Icmp(sock, ..) => {
                sock.set_nonblocking(val).map_err(net_error_into_wasi_err)
            }
            InodeSocketKind::PreSocket { nonblocking, .. } => {
                (*nonblocking) = val;
                Ok(())
            }
            _ => Err(Errno::Notsup),
        }
    }

    pub async fn linger(&self) -> Result<Option<std::time::Duration>, Errno> {
        let inner = self.inner.read().await;
        match &inner.kind {
            InodeSocketKind::TcpStream(sock) => sock.linger().map_err(net_error_into_wasi_err),
            InodeSocketKind::PreSocket { .. } => Err(Errno::Io),
            InodeSocketKind::Closed => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub async fn set_opt_time(
        &self,
        ty: TimeType,
        timeout: Option<std::time::Duration>,
    ) -> Result<(), Errno> {
        let mut inner = self.inner.write().await;
        match &mut inner.kind {
            InodeSocketKind::TcpStream(sock) => sock
                .set_opt_time(ty, timeout)
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::TcpListener(sock) => match ty {
                TimeType::AcceptTimeout => {
                    sock.set_timeout(timeout).map_err(net_error_into_wasi_err)
                }
                _ => Err(Errno::Inval),
            },
            InodeSocketKind::PreSocket {
                recv_timeout,
                send_timeout,
                connect_timeout,
                accept_timeout,
                ..
            } => match ty {
                TimeType::ConnectTimeout => {
                    *connect_timeout = timeout;
                    Ok(())
                }
                TimeType::AcceptTimeout => {
                    *accept_timeout = timeout;
                    Ok(())
                }
                TimeType::ReadTimeout => {
                    *recv_timeout = timeout;
                    Ok(())
                }
                TimeType::WriteTimeout => {
                    *send_timeout = timeout;
                    Ok(())
                }
                _ => Err(Errno::Io),
            },
            InodeSocketKind::Closed => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub async fn opt_time(&self, ty: TimeType) -> Result<Option<std::time::Duration>, Errno> {
        let inner = self.inner.read().await;
        match &inner.kind {
            InodeSocketKind::TcpStream(sock) => sock.opt_time(ty).map_err(net_error_into_wasi_err),
            InodeSocketKind::TcpListener(sock) => match ty {
                TimeType::AcceptTimeout => sock.timeout().map_err(net_error_into_wasi_err),
                _ => Err(Errno::Inval),
            },
            InodeSocketKind::PreSocket {
                recv_timeout,
                send_timeout,
                connect_timeout,
                accept_timeout,
                ..
            } => match ty {
                TimeType::ConnectTimeout => Ok(*connect_timeout),
                TimeType::AcceptTimeout => Ok(*accept_timeout),
                TimeType::ReadTimeout => Ok(*recv_timeout),
                TimeType::WriteTimeout => Ok(*send_timeout),
                _ => Err(Errno::Inval),
            },
            InodeSocketKind::Closed => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub async fn set_ttl(&self, ttl: u32) -> Result<(), Errno> {
        let mut inner = self.inner.write().await;
        match &mut inner.kind {
            InodeSocketKind::TcpStream(sock) => {
                sock.set_ttl(ttl).await.map_err(net_error_into_wasi_err)
            }
            InodeSocketKind::UdpSocket(sock) => {
                sock.set_ttl(ttl).await.map_err(net_error_into_wasi_err)
            }
            InodeSocketKind::PreSocket { .. } => Err(Errno::Io),
            InodeSocketKind::Closed => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub async fn ttl(&self) -> Result<u32, Errno> {
        let inner = self.inner.read().await;
        match &inner.kind {
            InodeSocketKind::TcpStream(sock) => sock.ttl().map_err(net_error_into_wasi_err),
            InodeSocketKind::UdpSocket(sock) => sock.ttl().map_err(net_error_into_wasi_err),
            InodeSocketKind::PreSocket { .. } => Err(Errno::Io),
            InodeSocketKind::Closed => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub async fn set_multicast_ttl_v4(&self, ttl: u32) -> Result<(), Errno> {
        let mut inner = self.inner.write().await;
        match &mut inner.kind {
            InodeSocketKind::UdpSocket(sock) => sock
                .set_multicast_ttl_v4(ttl)
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::PreSocket { .. } => Err(Errno::Io),
            InodeSocketKind::Closed => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub async fn multicast_ttl_v4(&self) -> Result<u32, Errno> {
        let inner = self.inner.read().await;
        match &inner.kind {
            InodeSocketKind::UdpSocket(sock) => {
                sock.multicast_ttl_v4().map_err(net_error_into_wasi_err)
            }
            InodeSocketKind::PreSocket { .. } => Err(Errno::Io),
            InodeSocketKind::Closed => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub async fn join_multicast_v4(
        &self,
        multiaddr: Ipv4Addr,
        iface: Ipv4Addr,
    ) -> Result<(), Errno> {
        let mut inner = self.inner.write().await;
        match &mut inner.kind {
            InodeSocketKind::UdpSocket(sock) => sock
                .join_multicast_v4(multiaddr, iface)
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::PreSocket { .. } => Err(Errno::Io),
            InodeSocketKind::Closed => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub async fn leave_multicast_v4(
        &self,
        multiaddr: Ipv4Addr,
        iface: Ipv4Addr,
    ) -> Result<(), Errno> {
        let mut inner = self.inner.write().await;
        match &mut inner.kind {
            InodeSocketKind::UdpSocket(sock) => sock
                .leave_multicast_v4(multiaddr, iface)
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::PreSocket { .. } => Err(Errno::Io),
            InodeSocketKind::Closed => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub async fn join_multicast_v6(&self, multiaddr: Ipv6Addr, iface: u32) -> Result<(), Errno> {
        let mut inner = self.inner.write().await;
        match &mut inner.kind {
            InodeSocketKind::UdpSocket(sock) => sock
                .join_multicast_v6(multiaddr, iface)
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::PreSocket { .. } => Err(Errno::Io),
            InodeSocketKind::Closed => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub async fn leave_multicast_v6(
        &mut self,
        multiaddr: Ipv6Addr,
        iface: u32,
    ) -> Result<(), Errno> {
        let mut inner = self.inner.write().await;
        match &mut inner.kind {
            InodeSocketKind::UdpSocket(sock) => sock
                .leave_multicast_v6(multiaddr, iface)
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::PreSocket { .. } => Err(Errno::Io),
            InodeSocketKind::Closed => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub async fn send(&self, buf: Vec<u8>) -> Result<usize, Errno> {
        let buf_len = buf.len();
        let mut inner = self.inner.write().await;

        let ret = match &mut inner.kind {
            InodeSocketKind::WebSocket(sock) => sock
                .send(Bytes::from(buf))
                .await
                .map(|_| buf_len)
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::Raw(sock) => sock
                .send(Bytes::from(buf))
                .await
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::TcpStream(sock) => sock
                .send(Bytes::from(buf))
                .await
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::UdpSocket(sock) => sock
                .send(Bytes::from(buf))
                .await
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::PreSocket { .. } => Err(Errno::Notconn),
            InodeSocketKind::Closed => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
        .map(|_| buf_len)?;
        Ok(ret)
    }

    pub async fn send_to<M: MemorySize>(
        &self,
        buf: Vec<u8>,
        addr: SocketAddr,
    ) -> Result<usize, Errno> {
        let buf_len = buf.len();
        let mut inner = self.inner.write().await;
        let ret = match &mut inner.kind {
            InodeSocketKind::Icmp(sock) => sock
                .send_to(Bytes::from(buf), addr)
                .await
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::UdpSocket(sock) => sock
                .send_to(Bytes::from(buf), addr)
                .await
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::PreSocket { .. } => Err(Errno::Notconn),
            InodeSocketKind::Closed => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
        .map(|_| buf_len)?;
        Ok(ret)
    }

    pub async fn recv(&self, max_size: usize) -> Result<Bytes, Errno> {
        let mut inner = self.inner.write().await;
        loop {
            let is_tcp = matches!(&inner.kind, InodeSocketKind::TcpStream(..));
            if let Some(buf) = inner.read_buffer.as_mut() {
                let buf_len = buf.len();
                if buf_len > 0 {
                    let read = buf_len.min(max_size);
                    let ret = buf.slice(..read);
                    if is_tcp {
                        buf.advance(read);
                    } else {
                        buf.clear();
                    }
                    return Ok(ret);
                }
            }
            let data = match &mut inner.kind {
                InodeSocketKind::WebSocket(sock) => {
                    let read = sock.recv().await.map_err(net_error_into_wasi_err)?;
                    read.data
                }
                InodeSocketKind::Raw(sock) => {
                    let read = sock.recv().await.map_err(net_error_into_wasi_err)?;
                    read.data
                }
                InodeSocketKind::TcpStream(sock) => {
                    let read = sock.recv().await.map_err(net_error_into_wasi_err)?;
                    read.data
                }
                InodeSocketKind::UdpSocket(sock) => {
                    let read = sock.recv().await.map_err(net_error_into_wasi_err)?;
                    read.data
                }
                InodeSocketKind::PreSocket { .. } => return Err(Errno::Notconn),
                InodeSocketKind::Closed => return Ok(Bytes::new()),
                _ => return Err(Errno::Notsup),
            };
            if data.is_empty() {
                return Ok(Bytes::new());
            }
            inner.read_buffer.replace(data);
            inner.read_addr.take();
        }
    }

    pub async fn recv_from(&self, max_size: usize) -> Result<(Bytes, SocketAddr), Errno> {
        let mut inner = self.inner.write().await;
        loop {
            let is_tcp = matches!(&inner.kind, InodeSocketKind::TcpStream(..));
            if let Some(buf) = inner.read_buffer.as_mut() {
                if !buf.is_empty() {
                    let buf_len = buf.len();
                    let read = buf_len.min(max_size);
                    let ret = buf.slice(..read);
                    if is_tcp {
                        buf.advance(read);
                    } else {
                        buf.clear();
                    }
                    let peer = inner
                        .read_addr
                        .unwrap_or_else(|| SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0));
                    return Ok((ret, peer));
                }
            }
            let rcv = match &mut inner.kind {
                InodeSocketKind::Icmp(sock) => {
                    sock.recv_from().await.map_err(net_error_into_wasi_err)?
                }
                InodeSocketKind::UdpSocket(sock) => {
                    sock.recv_from().await.map_err(net_error_into_wasi_err)?
                }
                InodeSocketKind::PreSocket { .. } => return Err(Errno::Notconn),
                InodeSocketKind::Closed => return Err(Errno::Io),
                _ => return Err(Errno::Notsup),
            };
            inner.read_buffer.replace(rcv.data);
            inner.read_addr.replace(rcv.addr);
        }
    }

    pub async fn shutdown(&mut self, how: std::net::Shutdown) -> Result<(), Errno> {
        let mut inner = self.inner.write().await;
        match &mut inner.kind {
            InodeSocketKind::TcpStream(sock) => {
                sock.shutdown(how).await.map_err(net_error_into_wasi_err)?;
            }
            InodeSocketKind::PreSocket { .. } => return Err(Errno::Notconn),
            InodeSocketKind::Closed => return Err(Errno::Io),
            _ => return Err(Errno::Notsup),
        }
        Ok(())
    }

    pub async fn can_write(&self) -> bool {
        if let Ok(mut guard) = self.inner.try_write() {
            #[allow(clippy::match_like_matches_macro)]
            match &mut guard.kind {
                InodeSocketKind::TcpStream(..)
                | InodeSocketKind::UdpSocket(..)
                | InodeSocketKind::Raw(..)
                | InodeSocketKind::WebSocket(..) => true,
                _ => false,
            }
        } else {
            false
        }
    }
}

impl InodeSocketInner {
    pub fn poll_read_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<wasmer_vnet::Result<usize>> {
        match &mut self.kind {
            InodeSocketKind::TcpListener(socket) => socket.poll_accept_ready(cx),
            InodeSocketKind::TcpStream(socket) => socket.poll_read_ready(cx),
            InodeSocketKind::UdpSocket(socket) => socket.poll_read_ready(cx),
            InodeSocketKind::Raw(socket) => socket.poll_read_ready(cx),
            InodeSocketKind::WebSocket(socket) => socket.poll_read_ready(cx),
            InodeSocketKind::Icmp(socket) => socket.poll_read_ready(cx),
            InodeSocketKind::PreSocket { .. } => {
                std::task::Poll::Ready(Err(wasmer_vnet::NetworkError::IOError))
            }
            InodeSocketKind::Closed => {
                std::task::Poll::Ready(Err(wasmer_vnet::NetworkError::ConnectionAborted))
            }
        }
    }

    pub fn poll_write_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<wasmer_vnet::Result<usize>> {
        match &mut self.kind {
            InodeSocketKind::TcpListener(_) => std::task::Poll::Pending,
            InodeSocketKind::TcpStream(socket) => socket.poll_write_ready(cx),
            InodeSocketKind::UdpSocket(socket) => socket.poll_write_ready(cx),
            InodeSocketKind::Raw(socket) => socket.poll_write_ready(cx),
            InodeSocketKind::WebSocket(socket) => socket.poll_write_ready(cx),
            InodeSocketKind::Icmp(socket) => socket.poll_write_ready(cx),
            InodeSocketKind::PreSocket { .. } => {
                std::task::Poll::Ready(Err(wasmer_vnet::NetworkError::IOError))
            }
            InodeSocketKind::Closed => {
                std::task::Poll::Ready(Err(wasmer_vnet::NetworkError::ConnectionAborted))
            }
        }
    }
}

#[derive(Default)]
struct IndefinitePoll {}

impl Future for IndefinitePoll {
    type Output = ();
    fn poll(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        std::task::Poll::Pending
    }
}

// TODO: review allow...
#[allow(dead_code)]
pub(crate) fn all_socket_rights() -> Rights {
    Rights::FD_FDSTAT_SET_FLAGS
        .union(Rights::FD_FILESTAT_GET)
        .union(Rights::FD_READ)
        .union(Rights::FD_WRITE)
        .union(Rights::POLL_FD_READWRITE)
        .union(Rights::SOCK_SHUTDOWN)
        .union(Rights::SOCK_CONNECT)
        .union(Rights::SOCK_LISTEN)
        .union(Rights::SOCK_BIND)
        .union(Rights::SOCK_ACCEPT)
        .union(Rights::SOCK_RECV)
        .union(Rights::SOCK_SEND)
        .union(Rights::SOCK_ADDR_LOCAL)
        .union(Rights::SOCK_ADDR_REMOTE)
        .union(Rights::SOCK_RECV_FROM)
        .union(Rights::SOCK_SEND_TO)
}
