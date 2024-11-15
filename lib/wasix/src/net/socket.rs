use std::{
    future::Future,
    io,
    mem::MaybeUninit,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    pin::Pin,
    sync::{Arc, RwLock},
    task::{Context, Poll},
    time::Duration,
};

#[cfg(feature = "enable-serde")]
use serde_derive::{Deserialize, Serialize};
use virtual_mio::InterestHandler;
use virtual_net::{
    net_error_into_io_err, NetworkError, VirtualIcmpSocket, VirtualNetworking, VirtualRawSocket,
    VirtualTcpListener, VirtualTcpSocket, VirtualUdpSocket,
};
use wasmer_types::MemorySize;
use wasmer_wasix_types::wasi::{Addressfamily, Errno, Rights, SockProto, Sockoption, Socktype};

use crate::{net::net_error_into_wasi_err, VirtualTaskManager};

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
pub struct SocketProperties {
    pub family: Addressfamily,
    pub ty: Socktype,
    pub pt: SockProto,
    pub only_v6: bool,
    pub reuse_port: bool,
    pub reuse_addr: bool,
    pub no_delay: Option<bool>,
    pub keep_alive: Option<bool>,
    pub dont_route: Option<bool>,
    pub send_buf_size: Option<usize>,
    pub recv_buf_size: Option<usize>,
    pub write_timeout: Option<Duration>,
    pub read_timeout: Option<Duration>,
    pub accept_timeout: Option<Duration>,
    pub connect_timeout: Option<Duration>,
    pub handler: Option<Box<dyn InterestHandler + Send + Sync>>,
}

#[derive(Debug)]
//#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub enum InodeSocketKind {
    PreSocket {
        props: SocketProperties,
        addr: Option<SocketAddr>,
    },
    Icmp(Box<dyn VirtualIcmpSocket + Sync>),
    Raw(Box<dyn VirtualRawSocket + Sync>),
    TcpListener {
        socket: Box<dyn VirtualTcpListener + Sync>,
        accept_timeout: Option<Duration>,
    },
    TcpStream {
        socket: Box<dyn VirtualTcpSocket + Sync>,
        write_timeout: Option<Duration>,
        read_timeout: Option<Duration>,
    },
    UdpSocket {
        socket: Box<dyn VirtualUdpSocket + Sync>,
        peer: Option<SocketAddr>,
    },
    RemoteSocket {
        props: SocketProperties,
        local_addr: SocketAddr,
        peer_addr: SocketAddr,
        ttl: u32,
        multicast_ttl: u32,
        is_dead: bool,
    },
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

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub enum TimeType {
    ReadTimeout,
    WriteTimeout,
    AcceptTimeout,
    ConnectTimeout,
    BindTimeout,
    Linger,
}

impl From<TimeType> for wasmer_journal::SocketOptTimeType {
    fn from(value: TimeType) -> Self {
        match value {
            TimeType::ReadTimeout => Self::ReadTimeout,
            TimeType::WriteTimeout => Self::WriteTimeout,
            TimeType::AcceptTimeout => Self::AcceptTimeout,
            TimeType::ConnectTimeout => Self::ConnectTimeout,
            TimeType::BindTimeout => Self::BindTimeout,
            TimeType::Linger => Self::Linger,
        }
    }
}

impl From<wasmer_journal::SocketOptTimeType> for TimeType {
    fn from(value: wasmer_journal::SocketOptTimeType) -> Self {
        use wasmer_journal::SocketOptTimeType;
        match value {
            SocketOptTimeType::ReadTimeout => TimeType::ReadTimeout,
            SocketOptTimeType::WriteTimeout => TimeType::WriteTimeout,
            SocketOptTimeType::AcceptTimeout => TimeType::AcceptTimeout,
            SocketOptTimeType::ConnectTimeout => TimeType::ConnectTimeout,
            SocketOptTimeType::BindTimeout => TimeType::BindTimeout,
            SocketOptTimeType::Linger => TimeType::Linger,
        }
    }
}

#[derive(Debug)]
//#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub(crate) struct InodeSocketProtected {
    pub kind: InodeSocketKind,
}

#[derive(Debug)]
//#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub(crate) struct InodeSocketInner {
    pub protected: RwLock<InodeSocketProtected>,
}

#[derive(Debug, Clone)]
//#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct InodeSocket {
    pub(crate) inner: Arc<InodeSocketInner>,
}

impl InodeSocket {
    pub fn new(kind: InodeSocketKind) -> Self {
        let protected = InodeSocketProtected { kind };
        Self {
            inner: Arc::new(InodeSocketInner {
                protected: RwLock::new(protected),
            }),
        }
    }

    pub fn poll_read_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        let mut inner = self.inner.protected.write().unwrap();
        inner.poll_read_ready(cx)
    }

    pub fn poll_write_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        let mut inner = self.inner.protected.write().unwrap();
        inner.poll_read_ready(cx)
    }

    pub async fn bind(
        &self,
        tasks: &dyn VirtualTaskManager,
        net: &dyn VirtualNetworking,
        set_addr: SocketAddr,
    ) -> Result<Option<InodeSocket>, Errno> {
        let timeout = self
            .opt_time(TimeType::BindTimeout)
            .ok()
            .flatten()
            .unwrap_or(Duration::from_secs(30));

        let socket = {
            let mut inner = self.inner.protected.write().unwrap();
            match &mut inner.kind {
                InodeSocketKind::PreSocket { props, addr, .. } => {
                    match props.family {
                        Addressfamily::Inet4 => {
                            if !set_addr.is_ipv4() {
                                tracing::debug!(
                                    "IP address is the wrong type IPv4 ({set_addr}) vs IPv6 family"
                                );
                                return Err(Errno::Inval);
                            }
                        }
                        Addressfamily::Inet6 => {
                            if !set_addr.is_ipv6() {
                                tracing::debug!(
                                    "IP address is the wrong type IPv6 ({set_addr}) vs IPv4 family"
                                );
                                return Err(Errno::Inval);
                            }
                        }
                        _ => {
                            return Err(Errno::Notsup);
                        }
                    }

                    addr.replace(set_addr);
                    let addr = (*addr).unwrap();

                    match props.ty {
                        Socktype::Stream => {
                            // we already set the socket address - next we need a listen or connect so nothing
                            // more to do at this time
                            return Ok(None);
                        }
                        Socktype::Dgram => {
                            let reuse_port = props.reuse_port;
                            let reuse_addr = props.reuse_addr;
                            drop(inner);

                            net.bind_udp(addr, reuse_port, reuse_addr)
                        }
                        _ => return Err(Errno::Inval),
                    }
                }
                InodeSocketKind::RemoteSocket {
                    props,
                    local_addr: addr,
                    ..
                } => {
                    match props.family {
                        Addressfamily::Inet4 => {
                            if !set_addr.is_ipv4() {
                                tracing::debug!(
                                    "IP address is the wrong type IPv4 ({set_addr}) vs IPv6 family"
                                );
                                return Err(Errno::Inval);
                            }
                        }
                        Addressfamily::Inet6 => {
                            if !set_addr.is_ipv6() {
                                tracing::debug!(
                                    "IP address is the wrong type IPv6 ({set_addr}) vs IPv4 family"
                                );
                                return Err(Errno::Inval);
                            }
                        }
                        _ => {
                            return Err(Errno::Notsup);
                        }
                    }

                    *addr = set_addr;
                    let addr = *addr;

                    match props.ty {
                        Socktype::Stream => {
                            // we already set the socket address - next we need a listen or connect so nothing
                            // more to do at this time
                            return Ok(None);
                        }
                        Socktype::Dgram => {
                            let reuse_port = props.reuse_port;
                            let reuse_addr = props.reuse_addr;
                            drop(inner);

                            net.bind_udp(addr, reuse_port, reuse_addr)
                        }
                        _ => return Err(Errno::Inval),
                    }
                }
                _ => return Err(Errno::Notsup),
            }
        };

        tokio::select! {
            socket = socket => {
                let socket = socket.map_err(net_error_into_wasi_err)?;
                Ok(Some(InodeSocket::new(InodeSocketKind::UdpSocket { socket, peer: None })))
            },
            _ = tasks.sleep_now(timeout) => Err(Errno::Timedout)
        }
    }

    pub async fn listen(
        &self,
        tasks: &dyn VirtualTaskManager,
        net: &dyn VirtualNetworking,
        _backlog: usize,
    ) -> Result<Option<InodeSocket>, Errno> {
        let timeout = self
            .opt_time(TimeType::AcceptTimeout)
            .ok()
            .flatten()
            .unwrap_or(Duration::from_secs(30));

        let socket = {
            let inner = self.inner.protected.read().unwrap();
            match &inner.kind {
                InodeSocketKind::PreSocket { props, addr, .. } => match props.ty {
                    Socktype::Stream => {
                        if addr.is_none() {
                            tracing::warn!("wasi[?]::sock_listen - failed - address not set");
                            return Err(Errno::Inval);
                        }
                        let addr = *addr.as_ref().unwrap();
                        let only_v6 = props.only_v6;
                        let reuse_port = props.reuse_port;
                        let reuse_addr = props.reuse_addr;
                        drop(inner);

                        net.listen_tcp(addr, only_v6, reuse_port, reuse_addr)
                    }
                    ty => {
                        tracing::warn!(
                            "wasi[?]::sock_listen - failed - not supported(pre-socket:{:?})",
                            ty
                        );
                        return Err(Errno::Notsup);
                    }
                },
                InodeSocketKind::RemoteSocket {
                    props,
                    local_addr: addr,
                    ..
                } => match props.ty {
                    Socktype::Stream => {
                        let addr = *addr;
                        let only_v6 = props.only_v6;
                        let reuse_port = props.reuse_port;
                        let reuse_addr = props.reuse_addr;
                        drop(inner);

                        net.listen_tcp(addr, only_v6, reuse_port, reuse_addr)
                    }
                    ty => {
                        tracing::warn!(
                            "wasi[?]::sock_listen - failed - not supported(remote-socket:{:?})",
                            ty
                        );
                        return Err(Errno::Notsup);
                    }
                },
                InodeSocketKind::Icmp(_) => {
                    tracing::warn!("wasi[?]::sock_listen - failed - not supported(icmp)");
                    return Err(Errno::Notsup);
                }
                InodeSocketKind::Raw(_) => {
                    tracing::warn!("wasi[?]::sock_listen - failed - not supported(raw)");
                    return Err(Errno::Notsup);
                }
                InodeSocketKind::TcpListener { .. } => {
                    tracing::warn!(
                        "wasi[?]::sock_listen - failed - already listening (tcp-listener)"
                    );
                    return Err(Errno::Notsup);
                }
                InodeSocketKind::TcpStream { .. } => {
                    tracing::warn!("wasi[?]::sock_listen - failed - not supported(tcp-stream)");
                    return Err(Errno::Notsup);
                }
                InodeSocketKind::UdpSocket { .. } => {
                    tracing::warn!("wasi[?]::sock_listen - failed - not supported(udp-socket)");
                    return Err(Errno::Notsup);
                }
            }
        };

        tokio::select! {
            socket = socket => {
                let socket = socket.map_err(net_error_into_wasi_err)?;
                Ok(Some(InodeSocket::new(InodeSocketKind::TcpListener {
                    socket,
                    accept_timeout: Some(timeout),
                })))
            },
            _ = tasks.sleep_now(timeout) => Err(Errno::Timedout)
        }
    }

    pub async fn accept(
        &self,
        tasks: &dyn VirtualTaskManager,
        nonblocking: bool,
        timeout: Option<Duration>,
    ) -> Result<(Box<dyn VirtualTcpSocket + Sync>, SocketAddr), Errno> {
        struct SocketAccepter<'a> {
            sock: &'a InodeSocket,
            nonblocking: bool,
            handler_registered: bool,
        }
        impl<'a> Drop for SocketAccepter<'a> {
            fn drop(&mut self) {
                if self.handler_registered {
                    let mut inner = self.sock.inner.protected.write().unwrap();
                    inner.remove_handler();
                }
            }
        }
        impl<'a> Future for SocketAccepter<'a> {
            type Output = Result<(Box<dyn VirtualTcpSocket + Sync>, SocketAddr), Errno>;
            fn poll(
                mut self: Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Self::Output> {
                loop {
                    let mut inner = self.sock.inner.protected.write().unwrap();
                    return match &mut inner.kind {
                        InodeSocketKind::TcpListener { socket, .. } => match socket.try_accept() {
                            Ok((child, addr)) => Poll::Ready(Ok((child, addr))),
                            Err(NetworkError::WouldBlock) if self.nonblocking => {
                                Poll::Ready(Err(Errno::Again))
                            }
                            Err(NetworkError::WouldBlock) if !self.handler_registered => {
                                let res = socket.set_handler(cx.waker().into());
                                if let Err(err) = res {
                                    return Poll::Ready(Err(net_error_into_wasi_err(err)));
                                }
                                drop(inner);
                                self.handler_registered = true;
                                continue;
                            }
                            Err(NetworkError::WouldBlock) => Poll::Pending,
                            Err(err) => Poll::Ready(Err(net_error_into_wasi_err(err))),
                        },
                        InodeSocketKind::PreSocket { .. } => Poll::Ready(Err(Errno::Notconn)),
                        _ => Poll::Ready(Err(Errno::Notsup)),
                    };
                }
            }
        }

        let acceptor = SocketAccepter {
            sock: self,
            nonblocking,
            handler_registered: false,
        };
        if let Some(timeout) = timeout {
            tokio::select! {
                res = acceptor => res,
                _ = tasks.sleep_now(timeout) => Err(Errno::Timedout)
            }
        } else {
            acceptor.await
        }
    }

    pub fn close(&self) -> Result<(), Errno> {
        let mut inner = self.inner.protected.write().unwrap();
        match &mut inner.kind {
            InodeSocketKind::TcpListener { .. } => {}
            InodeSocketKind::TcpStream { socket, .. } => {
                socket.close().map_err(net_error_into_wasi_err)?;
            }
            InodeSocketKind::Icmp(_) => {}
            InodeSocketKind::UdpSocket { .. } => {}
            InodeSocketKind::Raw(_) => {}
            InodeSocketKind::PreSocket { .. } => return Err(Errno::Notconn),
            InodeSocketKind::RemoteSocket { .. } => {}
        };
        Ok(())
    }

    pub async fn connect(
        &mut self,
        tasks: &dyn VirtualTaskManager,
        net: &dyn VirtualNetworking,
        peer: SocketAddr,
        timeout: Option<std::time::Duration>,
        nonblocking: bool,
    ) -> Result<Option<InodeSocket>, Errno> {
        let new_write_timeout;
        let new_read_timeout;

        let timeout = timeout.unwrap_or(Duration::from_secs(30));

        let handler;
        let connect = {
            let mut inner = self.inner.protected.write().unwrap();
            match &mut inner.kind {
                InodeSocketKind::PreSocket { props, addr, .. } => {
                    handler = props.handler.take();
                    new_write_timeout = props.write_timeout;
                    new_read_timeout = props.read_timeout;
                    match props.ty {
                        Socktype::Stream => {
                            let no_delay = props.no_delay;
                            let keep_alive = props.keep_alive;
                            let dont_route = props.dont_route;
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
                            Box::pin(async move {
                                let mut ret = net.connect_tcp(addr, peer).await?;
                                if let Some(no_delay) = no_delay {
                                    ret.set_nodelay(no_delay).ok();
                                }
                                if let Some(keep_alive) = keep_alive {
                                    ret.set_keepalive(keep_alive).ok();
                                }
                                if let Some(dont_route) = dont_route {
                                    ret.set_dontroute(dont_route).ok();
                                }
                                if !nonblocking {
                                    futures::future::poll_fn(|cx| ret.poll_write_ready(cx)).await?;
                                }
                                Ok(ret)
                            })
                        }
                        Socktype::Dgram => return Err(Errno::Inval),
                        _ => return Err(Errno::Notsup),
                    }
                }
                InodeSocketKind::UdpSocket {
                    peer: target_peer, ..
                } => {
                    target_peer.replace(peer);
                    return Ok(None);
                }
                InodeSocketKind::RemoteSocket { peer_addr, .. } => {
                    *peer_addr = peer;
                    return Ok(None);
                }
                _ => return Err(Errno::Notsup),
            }
        };

        let mut socket = tokio::select! {
            res = connect => res.map_err(net_error_into_wasi_err)?,
            _ = tasks.sleep_now(timeout) => return Err(Errno::Timedout)
        };

        if let Some(handler) = handler {
            socket
                .set_handler(handler)
                .map_err(net_error_into_wasi_err)?;
        }

        let socket = InodeSocket::new(InodeSocketKind::TcpStream {
            socket,
            write_timeout: new_write_timeout,
            read_timeout: new_read_timeout,
        });

        Ok(Some(socket))
    }

    pub fn status(&self) -> Result<WasiSocketStatus, Errno> {
        let inner = self.inner.protected.read().unwrap();
        Ok(match &inner.kind {
            InodeSocketKind::PreSocket { .. } => WasiSocketStatus::Opening,
            InodeSocketKind::TcpListener { .. } => WasiSocketStatus::Opened,
            InodeSocketKind::TcpStream { .. } => WasiSocketStatus::Opened,
            InodeSocketKind::UdpSocket { .. } => WasiSocketStatus::Opened,
            InodeSocketKind::RemoteSocket { is_dead, .. } => match is_dead {
                true => WasiSocketStatus::Closed,
                false => WasiSocketStatus::Opened,
            },
            _ => WasiSocketStatus::Failed,
        })
    }

    pub fn addr_local(&self) -> Result<SocketAddr, Errno> {
        let inner = self.inner.protected.read().unwrap();
        Ok(match &inner.kind {
            InodeSocketKind::PreSocket { props, addr, .. } => {
                if let Some(addr) = addr {
                    *addr
                } else {
                    SocketAddr::new(
                        match props.family {
                            Addressfamily::Inet4 => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                            Addressfamily::Inet6 => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
                            _ => return Err(Errno::Inval),
                        },
                        0,
                    )
                }
            }
            InodeSocketKind::Icmp(sock) => sock.addr_local().map_err(net_error_into_wasi_err)?,
            InodeSocketKind::TcpListener { socket, .. } => {
                socket.addr_local().map_err(net_error_into_wasi_err)?
            }
            InodeSocketKind::TcpStream { socket, .. } => {
                socket.addr_local().map_err(net_error_into_wasi_err)?
            }
            InodeSocketKind::UdpSocket { socket, .. } => {
                socket.addr_local().map_err(net_error_into_wasi_err)?
            }
            InodeSocketKind::RemoteSocket {
                local_addr: addr, ..
            } => *addr,
            _ => return Err(Errno::Notsup),
        })
    }

    pub fn addr_peer(&self) -> Result<SocketAddr, Errno> {
        let inner = self.inner.protected.read().unwrap();
        Ok(match &inner.kind {
            InodeSocketKind::PreSocket { props, .. } => SocketAddr::new(
                match props.family {
                    Addressfamily::Inet4 => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                    Addressfamily::Inet6 => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
                    _ => return Err(Errno::Inval),
                },
                0,
            ),
            InodeSocketKind::TcpStream { socket, .. } => {
                socket.addr_peer().map_err(net_error_into_wasi_err)?
            }
            InodeSocketKind::UdpSocket { socket, .. } => socket
                .addr_peer()
                .map_err(net_error_into_wasi_err)?
                .map(Ok)
                .unwrap_or_else(|| {
                    socket
                        .addr_local()
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
            InodeSocketKind::RemoteSocket { peer_addr, .. } => *peer_addr,
            _ => return Err(Errno::Notsup),
        })
    }

    pub fn set_opt_flag(&mut self, option: WasiSocketOption, val: bool) -> Result<(), Errno> {
        let mut inner = self.inner.protected.write().unwrap();
        match &mut inner.kind {
            InodeSocketKind::PreSocket { props, .. }
            | InodeSocketKind::RemoteSocket { props, .. } => {
                match option {
                    WasiSocketOption::OnlyV6 => props.only_v6 = val,
                    WasiSocketOption::ReusePort => props.reuse_port = val,
                    WasiSocketOption::ReuseAddr => props.reuse_addr = val,
                    WasiSocketOption::NoDelay => props.no_delay = Some(val),
                    WasiSocketOption::KeepAlive => props.keep_alive = Some(val),
                    WasiSocketOption::DontRoute => props.dont_route = Some(val),
                    _ => return Err(Errno::Inval),
                };
            }
            InodeSocketKind::Raw(sock) => match option {
                WasiSocketOption::Promiscuous => {
                    sock.set_promiscuous(val).map_err(net_error_into_wasi_err)?
                }
                _ => return Err(Errno::Inval),
            },
            InodeSocketKind::TcpStream { socket, .. } => match option {
                WasiSocketOption::NoDelay => {
                    socket.set_nodelay(val).map_err(net_error_into_wasi_err)?
                }
                WasiSocketOption::KeepAlive => {
                    socket.set_keepalive(val).map_err(net_error_into_wasi_err)?
                }
                WasiSocketOption::DontRoute => {
                    socket.set_dontroute(val).map_err(net_error_into_wasi_err)?
                }
                _ => return Err(Errno::Inval),
            },
            InodeSocketKind::TcpListener { .. } => return Err(Errno::Inval),
            InodeSocketKind::UdpSocket { socket, .. } => match option {
                WasiSocketOption::Broadcast => {
                    socket.set_broadcast(val).map_err(net_error_into_wasi_err)?
                }
                WasiSocketOption::MulticastLoopV4 => socket
                    .set_multicast_loop_v4(val)
                    .map_err(net_error_into_wasi_err)?,
                WasiSocketOption::MulticastLoopV6 => socket
                    .set_multicast_loop_v6(val)
                    .map_err(net_error_into_wasi_err)?,
                _ => return Err(Errno::Inval),
            },
            _ => return Err(Errno::Notsup),
        }
        Ok(())
    }

    pub fn get_opt_flag(&self, option: WasiSocketOption) -> Result<bool, Errno> {
        let mut inner = self.inner.protected.write().unwrap();
        Ok(match &mut inner.kind {
            InodeSocketKind::PreSocket { props, .. }
            | InodeSocketKind::RemoteSocket { props, .. } => match option {
                WasiSocketOption::OnlyV6 => props.only_v6,
                WasiSocketOption::ReusePort => props.reuse_port,
                WasiSocketOption::ReuseAddr => props.reuse_addr,
                WasiSocketOption::NoDelay => props.no_delay.unwrap_or_default(),
                WasiSocketOption::KeepAlive => props.keep_alive.unwrap_or_default(),
                _ => return Err(Errno::Inval),
            },
            InodeSocketKind::Raw(sock) => match option {
                WasiSocketOption::Promiscuous => {
                    sock.promiscuous().map_err(net_error_into_wasi_err)?
                }
                _ => return Err(Errno::Inval),
            },
            InodeSocketKind::TcpStream { socket, .. } => match option {
                WasiSocketOption::NoDelay => socket.nodelay().map_err(net_error_into_wasi_err)?,
                WasiSocketOption::KeepAlive => {
                    socket.keepalive().map_err(net_error_into_wasi_err)?
                }
                WasiSocketOption::DontRoute => {
                    socket.dontroute().map_err(net_error_into_wasi_err)?
                }
                _ => return Err(Errno::Inval),
            },
            InodeSocketKind::UdpSocket { socket, .. } => match option {
                WasiSocketOption::Broadcast => {
                    socket.broadcast().map_err(net_error_into_wasi_err)?
                }
                WasiSocketOption::MulticastLoopV4 => socket
                    .multicast_loop_v4()
                    .map_err(net_error_into_wasi_err)?,
                WasiSocketOption::MulticastLoopV6 => socket
                    .multicast_loop_v6()
                    .map_err(net_error_into_wasi_err)?,
                _ => return Err(Errno::Inval),
            },
            _ => return Err(Errno::Notsup),
        })
    }

    pub fn set_send_buf_size(&mut self, size: usize) -> Result<(), Errno> {
        let mut inner = self.inner.protected.write().unwrap();
        match &mut inner.kind {
            InodeSocketKind::PreSocket { props, .. }
            | InodeSocketKind::RemoteSocket { props, .. } => {
                props.send_buf_size = Some(size);
            }
            InodeSocketKind::TcpStream { socket, .. } => {
                socket
                    .set_send_buf_size(size)
                    .map_err(net_error_into_wasi_err)?;
            }
            _ => return Err(Errno::Notsup),
        }
        Ok(())
    }

    pub fn send_buf_size(&self) -> Result<usize, Errno> {
        let inner = self.inner.protected.read().unwrap();
        match &inner.kind {
            InodeSocketKind::PreSocket { props, .. }
            | InodeSocketKind::RemoteSocket { props, .. } => {
                Ok(props.send_buf_size.unwrap_or_default())
            }
            InodeSocketKind::TcpStream { socket, .. } => {
                socket.send_buf_size().map_err(net_error_into_wasi_err)
            }
            _ => Err(Errno::Notsup),
        }
    }

    pub fn set_recv_buf_size(&mut self, size: usize) -> Result<(), Errno> {
        let mut inner = self.inner.protected.write().unwrap();
        match &mut inner.kind {
            InodeSocketKind::PreSocket { props, .. }
            | InodeSocketKind::RemoteSocket { props, .. } => {
                props.recv_buf_size = Some(size);
            }
            InodeSocketKind::TcpStream { socket, .. } => {
                socket
                    .set_recv_buf_size(size)
                    .map_err(net_error_into_wasi_err)?;
            }
            _ => return Err(Errno::Notsup),
        }
        Ok(())
    }

    pub fn recv_buf_size(&self) -> Result<usize, Errno> {
        let inner = self.inner.protected.read().unwrap();
        match &inner.kind {
            InodeSocketKind::PreSocket { props, .. }
            | InodeSocketKind::RemoteSocket { props, .. } => {
                Ok(props.recv_buf_size.unwrap_or_default())
            }
            InodeSocketKind::TcpStream { socket, .. } => {
                socket.recv_buf_size().map_err(net_error_into_wasi_err)
            }
            _ => Err(Errno::Notsup),
        }
    }

    pub fn set_linger(&mut self, linger: Option<std::time::Duration>) -> Result<(), Errno> {
        let mut inner = self.inner.protected.write().unwrap();
        match &mut inner.kind {
            InodeSocketKind::TcpStream { socket, .. } => {
                socket.set_linger(linger).map_err(net_error_into_wasi_err)
            }
            InodeSocketKind::RemoteSocket { .. } => Ok(()),
            InodeSocketKind::PreSocket { .. } => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub fn linger(&self) -> Result<Option<std::time::Duration>, Errno> {
        let inner = self.inner.protected.read().unwrap();
        match &inner.kind {
            InodeSocketKind::TcpStream { socket, .. } => {
                socket.linger().map_err(net_error_into_wasi_err)
            }
            InodeSocketKind::PreSocket { .. } => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub fn set_opt_time(
        &self,
        ty: TimeType,
        timeout: Option<std::time::Duration>,
    ) -> Result<(), Errno> {
        let mut inner = self.inner.protected.write().unwrap();
        match &mut inner.kind {
            InodeSocketKind::TcpStream {
                write_timeout,
                read_timeout,
                ..
            } => {
                match ty {
                    TimeType::WriteTimeout => *write_timeout = timeout,
                    TimeType::ReadTimeout => *read_timeout = timeout,
                    _ => return Err(Errno::Inval),
                }
                Ok(())
            }
            InodeSocketKind::TcpListener { accept_timeout, .. } => {
                match ty {
                    TimeType::AcceptTimeout => *accept_timeout = timeout,
                    _ => return Err(Errno::Inval),
                }
                Ok(())
            }
            InodeSocketKind::PreSocket { props, .. }
            | InodeSocketKind::RemoteSocket { props, .. } => {
                match ty {
                    TimeType::ConnectTimeout => props.connect_timeout = timeout,
                    TimeType::AcceptTimeout => props.accept_timeout = timeout,
                    TimeType::ReadTimeout => props.read_timeout = timeout,
                    TimeType::WriteTimeout => props.write_timeout = timeout,
                    _ => return Err(Errno::Io),
                }
                Ok(())
            }
            _ => Err(Errno::Notsup),
        }
    }

    pub fn opt_time(&self, ty: TimeType) -> Result<Option<std::time::Duration>, Errno> {
        let inner = self.inner.protected.read().unwrap();
        match &inner.kind {
            InodeSocketKind::TcpStream {
                read_timeout,
                write_timeout,
                ..
            } => Ok(match ty {
                TimeType::ReadTimeout => *read_timeout,
                TimeType::WriteTimeout => *write_timeout,
                _ => return Err(Errno::Inval),
            }),
            InodeSocketKind::TcpListener { accept_timeout, .. } => Ok(match ty {
                TimeType::AcceptTimeout => *accept_timeout,
                _ => return Err(Errno::Inval),
            }),
            InodeSocketKind::PreSocket { props, .. }
            | InodeSocketKind::RemoteSocket { props, .. } => match ty {
                TimeType::ConnectTimeout => Ok(props.connect_timeout),
                TimeType::AcceptTimeout => Ok(props.accept_timeout),
                TimeType::ReadTimeout => Ok(props.read_timeout),
                TimeType::WriteTimeout => Ok(props.write_timeout),
                _ => Err(Errno::Inval),
            },
            _ => Err(Errno::Notsup),
        }
    }

    pub fn set_ttl(&self, ttl: u32) -> Result<(), Errno> {
        let mut inner = self.inner.protected.write().unwrap();
        match &mut inner.kind {
            InodeSocketKind::TcpStream { socket, .. } => {
                socket.set_ttl(ttl).map_err(net_error_into_wasi_err)
            }
            InodeSocketKind::UdpSocket { socket, .. } => {
                socket.set_ttl(ttl).map_err(net_error_into_wasi_err)
            }
            InodeSocketKind::RemoteSocket { ttl: set_ttl, .. } => {
                *set_ttl = ttl;
                Ok(())
            }
            InodeSocketKind::PreSocket { .. } => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub fn ttl(&self) -> Result<u32, Errno> {
        let inner = self.inner.protected.read().unwrap();
        match &inner.kind {
            InodeSocketKind::TcpStream { socket, .. } => {
                socket.ttl().map_err(net_error_into_wasi_err)
            }
            InodeSocketKind::UdpSocket { socket, .. } => {
                socket.ttl().map_err(net_error_into_wasi_err)
            }
            InodeSocketKind::RemoteSocket { ttl, .. } => Ok(*ttl),
            InodeSocketKind::PreSocket { .. } => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub fn set_multicast_ttl_v4(&self, ttl: u32) -> Result<(), Errno> {
        let mut inner = self.inner.protected.write().unwrap();
        match &mut inner.kind {
            InodeSocketKind::UdpSocket { socket, .. } => socket
                .set_multicast_ttl_v4(ttl)
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::RemoteSocket {
                multicast_ttl: set_ttl,
                ..
            } => {
                *set_ttl = ttl;
                Ok(())
            }
            InodeSocketKind::PreSocket { .. } => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub fn multicast_ttl_v4(&self) -> Result<u32, Errno> {
        let inner = self.inner.protected.read().unwrap();
        match &inner.kind {
            InodeSocketKind::UdpSocket { socket, .. } => {
                socket.multicast_ttl_v4().map_err(net_error_into_wasi_err)
            }
            InodeSocketKind::RemoteSocket { multicast_ttl, .. } => Ok(*multicast_ttl),
            InodeSocketKind::PreSocket { .. } => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub fn join_multicast_v4(&self, multiaddr: Ipv4Addr, iface: Ipv4Addr) -> Result<(), Errno> {
        let mut inner = self.inner.protected.write().unwrap();
        match &mut inner.kind {
            InodeSocketKind::UdpSocket { socket, .. } => socket
                .join_multicast_v4(multiaddr, iface)
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::RemoteSocket { .. } => Ok(()),
            InodeSocketKind::PreSocket { .. } => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub fn leave_multicast_v4(&self, multiaddr: Ipv4Addr, iface: Ipv4Addr) -> Result<(), Errno> {
        let mut inner = self.inner.protected.write().unwrap();
        match &mut inner.kind {
            InodeSocketKind::UdpSocket { socket, .. } => socket
                .leave_multicast_v4(multiaddr, iface)
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::RemoteSocket { .. } => Ok(()),
            InodeSocketKind::PreSocket { .. } => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub fn join_multicast_v6(&self, multiaddr: Ipv6Addr, iface: u32) -> Result<(), Errno> {
        let mut inner = self.inner.protected.write().unwrap();
        match &mut inner.kind {
            InodeSocketKind::UdpSocket { socket, .. } => socket
                .join_multicast_v6(multiaddr, iface)
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::RemoteSocket { .. } => Ok(()),
            InodeSocketKind::PreSocket { .. } => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub fn leave_multicast_v6(&mut self, multiaddr: Ipv6Addr, iface: u32) -> Result<(), Errno> {
        let mut inner = self.inner.protected.write().unwrap();
        match &mut inner.kind {
            InodeSocketKind::UdpSocket { socket, .. } => socket
                .leave_multicast_v6(multiaddr, iface)
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::RemoteSocket { .. } => Ok(()),
            InodeSocketKind::PreSocket { .. } => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub async fn send(
        &self,
        tasks: &dyn VirtualTaskManager,
        buf: &[u8],
        timeout: Option<Duration>,
        nonblocking: bool,
    ) -> Result<usize, Errno> {
        struct SocketSender<'a, 'b> {
            inner: &'a InodeSocketInner,
            data: &'b [u8],
            nonblocking: bool,
            handler_registered: bool,
        }
        impl<'a, 'b> Drop for SocketSender<'a, 'b> {
            fn drop(&mut self) {
                if self.handler_registered {
                    let mut inner = self.inner.protected.write().unwrap();
                    inner.remove_handler();
                }
            }
        }
        impl<'a, 'b> Future for SocketSender<'a, 'b> {
            type Output = Result<usize, Errno>;
            fn poll(
                mut self: Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
            ) -> Poll<Self::Output> {
                loop {
                    let mut inner = self.inner.protected.write().unwrap();
                    let res = match &mut inner.kind {
                        InodeSocketKind::Raw(socket) => socket.try_send(self.data),
                        InodeSocketKind::TcpStream { socket, .. } => socket.try_send(self.data),
                        InodeSocketKind::UdpSocket { socket, peer } => {
                            if let Some(peer) = peer {
                                socket.try_send_to(self.data, *peer)
                            } else {
                                Err(NetworkError::NotConnected)
                            }
                        }
                        InodeSocketKind::PreSocket { .. } => {
                            return Poll::Ready(Err(Errno::Notconn))
                        }
                        InodeSocketKind::RemoteSocket { is_dead, .. } => {
                            return match is_dead {
                                true => Poll::Ready(Err(Errno::Connreset)),
                                false => Poll::Ready(Ok(self.data.len())),
                            }
                        }
                        _ => return Poll::Ready(Err(Errno::Notsup)),
                    };
                    return match res {
                        Ok(amt) => Poll::Ready(Ok(amt)),
                        Err(NetworkError::WouldBlock) if self.nonblocking => {
                            Poll::Ready(Err(Errno::Again))
                        }
                        Err(NetworkError::WouldBlock) if !self.handler_registered => {
                            inner
                                .set_handler(cx.waker().into())
                                .map_err(net_error_into_wasi_err)?;
                            drop(inner);
                            self.handler_registered = true;
                            continue;
                        }
                        Err(NetworkError::WouldBlock) => Poll::Pending,
                        Err(err) => Poll::Ready(Err(net_error_into_wasi_err(err))),
                    };
                }
            }
        }

        let poller = SocketSender {
            inner: &self.inner,
            data: buf,
            nonblocking,
            handler_registered: false,
        };
        if let Some(timeout) = timeout {
            tokio::select! {
                res = poller => res,
                _ = tasks.sleep_now(timeout) => Err(Errno::Timedout)
            }
        } else {
            poller.await
        }
    }

    pub async fn send_to<M: MemorySize>(
        &self,
        tasks: &dyn VirtualTaskManager,
        buf: &[u8],
        addr: SocketAddr,
        timeout: Option<Duration>,
        nonblocking: bool,
    ) -> Result<usize, Errno> {
        struct SocketSender<'a, 'b> {
            inner: &'a InodeSocketInner,
            data: &'b [u8],
            addr: SocketAddr,
            nonblocking: bool,
            handler_registered: bool,
        }
        impl<'a, 'b> Drop for SocketSender<'a, 'b> {
            fn drop(&mut self) {
                if self.handler_registered {
                    let mut inner = self.inner.protected.write().unwrap();
                    inner.remove_handler();
                }
            }
        }
        impl<'a, 'b> Future for SocketSender<'a, 'b> {
            type Output = Result<usize, Errno>;
            fn poll(
                mut self: Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
            ) -> Poll<Self::Output> {
                loop {
                    let mut inner = self.inner.protected.write().unwrap();
                    let res = match &mut inner.kind {
                        InodeSocketKind::Icmp(socket) => socket.try_send_to(self.data, self.addr),
                        InodeSocketKind::UdpSocket { socket, .. } => {
                            socket.try_send_to(self.data, self.addr)
                        }
                        InodeSocketKind::PreSocket { .. } => {
                            return Poll::Ready(Err(Errno::Notconn))
                        }
                        InodeSocketKind::RemoteSocket { is_dead, .. } => {
                            return match is_dead {
                                true => Poll::Ready(Err(Errno::Connreset)),
                                false => Poll::Ready(Ok(self.data.len())),
                            };
                        }
                        _ => return Poll::Ready(Err(Errno::Notsup)),
                    };
                    return match res {
                        Ok(amt) => Poll::Ready(Ok(amt)),
                        Err(NetworkError::WouldBlock) if self.nonblocking => {
                            Poll::Ready(Err(Errno::Again))
                        }
                        Err(NetworkError::WouldBlock) if !self.handler_registered => {
                            inner
                                .set_handler(cx.waker().into())
                                .map_err(net_error_into_wasi_err)?;
                            self.handler_registered = true;
                            drop(inner);
                            continue;
                        }
                        Err(NetworkError::WouldBlock) => Poll::Pending,
                        Err(err) => Poll::Ready(Err(net_error_into_wasi_err(err))),
                    };
                }
            }
        }

        let poller = SocketSender {
            inner: &self.inner,
            data: buf,
            addr,
            nonblocking,
            handler_registered: false,
        };
        if let Some(timeout) = timeout {
            tokio::select! {
                res = poller => res,
                _ = tasks.sleep_now(timeout) => Err(Errno::Timedout)
            }
        } else {
            poller.await
        }
    }

    pub async fn recv(
        &self,
        tasks: &dyn VirtualTaskManager,
        buf: &mut [MaybeUninit<u8>],
        timeout: Option<Duration>,
        nonblocking: bool,
    ) -> Result<usize, Errno> {
        struct SocketReceiver<'a, 'b> {
            inner: &'a InodeSocketInner,
            data: &'b mut [MaybeUninit<u8>],
            nonblocking: bool,
            handler_registered: bool,
        }
        impl<'a, 'b> Drop for SocketReceiver<'a, 'b> {
            fn drop(&mut self) {
                if self.handler_registered {
                    let mut inner = self.inner.protected.write().unwrap();
                    inner.remove_handler();
                }
            }
        }
        impl<'a, 'b> Future for SocketReceiver<'a, 'b> {
            type Output = Result<usize, Errno>;
            fn poll(
                mut self: Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
            ) -> Poll<Self::Output> {
                loop {
                    let mut inner = self.inner.protected.write().unwrap();
                    let res = match &mut inner.kind {
                        InodeSocketKind::Raw(socket) => socket.try_recv(self.data),
                        InodeSocketKind::TcpStream { socket, .. } => socket.try_recv(self.data),
                        InodeSocketKind::UdpSocket { socket, peer } => {
                            if let Some(peer) = peer {
                                match socket.try_recv_from(self.data) {
                                    Ok((amt, addr)) if addr == *peer => Ok(amt),
                                    Ok(_) => Err(NetworkError::WouldBlock),
                                    Err(err) => Err(err),
                                }
                            } else {
                                match socket.try_recv_from(self.data) {
                                    Ok((amt, _)) => Ok(amt),
                                    Err(err) => Err(err),
                                }
                            }
                        }
                        InodeSocketKind::RemoteSocket { is_dead, .. } => {
                            return match is_dead {
                                true => Poll::Ready(Ok(0)),
                                false => Poll::Pending,
                            };
                        }
                        InodeSocketKind::PreSocket { .. } => {
                            return Poll::Ready(Err(Errno::Notconn))
                        }
                        _ => return Poll::Ready(Err(Errno::Notsup)),
                    };
                    return match res {
                        Ok(amt) => Poll::Ready(Ok(amt)),
                        Err(NetworkError::WouldBlock) if self.nonblocking => {
                            Poll::Ready(Err(Errno::Again))
                        }
                        Err(NetworkError::WouldBlock) if !self.handler_registered => {
                            inner
                                .set_handler(cx.waker().into())
                                .map_err(net_error_into_wasi_err)?;
                            self.handler_registered = true;
                            drop(inner);
                            continue;
                        }

                        Err(NetworkError::WouldBlock) => Poll::Pending,
                        Err(err) => Poll::Ready(Err(net_error_into_wasi_err(err))),
                    };
                }
            }
        }

        let poller = SocketReceiver {
            inner: &self.inner,
            data: buf,
            nonblocking,
            handler_registered: false,
        };
        if let Some(timeout) = timeout {
            tokio::select! {
                res = poller => res,
                _ = tasks.sleep_now(timeout) => Err(Errno::Timedout)
            }
        } else {
            poller.await
        }
    }

    pub async fn recv_from(
        &self,
        tasks: &dyn VirtualTaskManager,
        buf: &mut [MaybeUninit<u8>],
        timeout: Option<Duration>,
        nonblocking: bool,
    ) -> Result<(usize, SocketAddr), Errno> {
        struct SocketReceiver<'a, 'b> {
            inner: &'a InodeSocketInner,
            data: &'b mut [MaybeUninit<u8>],
            nonblocking: bool,
            handler_registered: bool,
        }
        impl<'a, 'b> Drop for SocketReceiver<'a, 'b> {
            fn drop(&mut self) {
                if self.handler_registered {
                    let mut inner = self.inner.protected.write().unwrap();
                    inner.remove_handler();
                }
            }
        }
        impl<'a, 'b> Future for SocketReceiver<'a, 'b> {
            type Output = Result<(usize, SocketAddr), Errno>;
            fn poll(
                mut self: Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
            ) -> Poll<Self::Output> {
                let mut inner = self.inner.protected.write().unwrap();
                loop {
                    let res = match &mut inner.kind {
                        InodeSocketKind::Icmp(socket) => socket.try_recv_from(self.data),
                        InodeSocketKind::UdpSocket { socket, .. } => {
                            socket.try_recv_from(self.data)
                        }
                        InodeSocketKind::RemoteSocket {
                            is_dead, peer_addr, ..
                        } => {
                            return match is_dead {
                                true => Poll::Ready(Ok((0, *peer_addr))),
                                false => Poll::Pending,
                            };
                        }
                        InodeSocketKind::PreSocket { .. } => {
                            return Poll::Ready(Err(Errno::Notconn))
                        }
                        _ => return Poll::Ready(Err(Errno::Notsup)),
                    };
                    return match res {
                        Ok((amt, addr)) => Poll::Ready(Ok((amt, addr))),
                        Err(NetworkError::WouldBlock) if self.nonblocking => {
                            Poll::Ready(Err(Errno::Again))
                        }
                        Err(NetworkError::WouldBlock) if !self.handler_registered => {
                            inner
                                .set_handler(cx.waker().into())
                                .map_err(net_error_into_wasi_err)?;
                            self.handler_registered = true;
                            continue;
                        }
                        Err(NetworkError::WouldBlock) => Poll::Pending,
                        Err(err) => Poll::Ready(Err(net_error_into_wasi_err(err))),
                    };
                }
            }
        }

        let poller = SocketReceiver {
            inner: &self.inner,
            data: buf,
            nonblocking,
            handler_registered: false,
        };
        if let Some(timeout) = timeout {
            tokio::select! {
                res = poller => res,
                _ = tasks.sleep_now(timeout) => Err(Errno::Timedout)
            }
        } else {
            poller.await
        }
    }

    pub fn shutdown(&mut self, how: std::net::Shutdown) -> Result<(), Errno> {
        let mut inner = self.inner.protected.write().unwrap();
        match &mut inner.kind {
            InodeSocketKind::TcpStream { socket, .. } => {
                socket.shutdown(how).map_err(net_error_into_wasi_err)?;
            }
            InodeSocketKind::RemoteSocket { .. } => return Ok(()),
            InodeSocketKind::PreSocket { .. } => return Err(Errno::Notconn),
            _ => return Err(Errno::Notsup),
        }
        Ok(())
    }

    pub async fn can_write(&self) -> bool {
        if let Ok(mut guard) = self.inner.protected.try_write() {
            #[allow(clippy::match_like_matches_macro)]
            match &mut guard.kind {
                InodeSocketKind::TcpStream { .. }
                | InodeSocketKind::UdpSocket { .. }
                | InodeSocketKind::Raw(..) => true,
                InodeSocketKind::RemoteSocket { is_dead, .. } => !(*is_dead),
                _ => false,
            }
        } else {
            false
        }
    }
}

impl InodeSocketProtected {
    pub fn remove_handler(&mut self) {
        match &mut self.kind {
            InodeSocketKind::TcpListener { socket, .. } => socket.remove_handler(),
            InodeSocketKind::TcpStream { socket, .. } => socket.remove_handler(),
            InodeSocketKind::UdpSocket { socket, .. } => socket.remove_handler(),
            InodeSocketKind::Raw(socket) => socket.remove_handler(),
            InodeSocketKind::Icmp(socket) => socket.remove_handler(),
            InodeSocketKind::PreSocket { props, .. } => {
                props.handler.take();
            }
            InodeSocketKind::RemoteSocket { props, .. } => {
                props.handler.take();
            }
        }
    }

    pub fn poll_read_ready(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        match &mut self.kind {
            InodeSocketKind::TcpListener { socket, .. } => socket.poll_read_ready(cx),
            InodeSocketKind::TcpStream { socket, .. } => socket.poll_read_ready(cx),
            InodeSocketKind::UdpSocket { socket, .. } => socket.poll_read_ready(cx),
            InodeSocketKind::Raw(socket) => socket.poll_read_ready(cx),
            InodeSocketKind::Icmp(socket) => socket.poll_read_ready(cx),
            InodeSocketKind::PreSocket { .. } => Poll::Pending,
            InodeSocketKind::RemoteSocket { is_dead, .. } => match is_dead {
                true => Poll::Ready(Ok(0)),
                false => Poll::Pending,
            },
        }
        .map_err(net_error_into_io_err)
    }

    pub fn poll_write_ready(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        match &mut self.kind {
            InodeSocketKind::TcpListener { socket, .. } => socket.poll_write_ready(cx),
            InodeSocketKind::TcpStream { socket, .. } => socket.poll_write_ready(cx),
            InodeSocketKind::UdpSocket { socket, .. } => socket.poll_write_ready(cx),
            InodeSocketKind::Raw(socket) => socket.poll_write_ready(cx),
            InodeSocketKind::Icmp(socket) => socket.poll_write_ready(cx),
            InodeSocketKind::PreSocket { .. } => Poll::Pending,
            InodeSocketKind::RemoteSocket { is_dead, .. } => match is_dead {
                true => Poll::Ready(Ok(0)),
                false => Poll::Pending,
            },
        }
        .map_err(net_error_into_io_err)
    }

    pub fn set_handler(
        &mut self,
        handler: Box<dyn InterestHandler + Send + Sync>,
    ) -> virtual_net::Result<()> {
        match &mut self.kind {
            InodeSocketKind::TcpListener { socket, .. } => socket.set_handler(handler),
            InodeSocketKind::TcpStream { socket, .. } => socket.set_handler(handler),
            InodeSocketKind::UdpSocket { socket, .. } => socket.set_handler(handler),
            InodeSocketKind::Raw(socket) => socket.set_handler(handler),
            InodeSocketKind::Icmp(socket) => socket.set_handler(handler),
            InodeSocketKind::PreSocket { props, .. }
            | InodeSocketKind::RemoteSocket { props, .. } => {
                props.handler.replace(handler);
                Ok(())
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
