use super::types::net_error_into_wasi_err;
use crate::syscalls::types::*;
use bytes::{Buf, Bytes};
use std::future::Future;
use std::mem::transmute;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
#[allow(unused_imports)]
use tracing::{debug, error, info, warn};
use wasmer::{MemorySize, MemoryView, WasmPtr};
use wasmer_vnet::TimeType;
use wasmer_vnet::{
    IpCidr, IpRoute, SocketHttpRequest, VirtualIcmpSocket, VirtualNetworking, VirtualRawSocket,
    VirtualTcpListener, VirtualTcpSocket, VirtualUdpSocket, VirtualWebSocket,
};
use wasmer_wasi_types::wasi::{
    Addressfamily, Errno, Fdflags, OptionTag, Rights, Sockoption, Socktype,
};

#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};

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
    HttpRequest(Mutex<SocketHttpRequest>, InodeHttpSocketType),
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
pub struct WasiHttpStatus {
    pub ok: bool,
    pub redirected: bool,
    pub size: u64,
    pub status: u16,
}

#[derive(Debug)]
//#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub(crate) struct InodeSocketInner {
    pub kind: InodeSocketKind,
    pub read_buffer: Option<Bytes>,
    pub read_addr: Option<SocketAddr>,
    pub silence_write_ready: bool,
}

#[derive(Debug, Clone)]
//#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct InodeSocket {
    pub(crate) inner: Arc<RwLock<InodeSocketInner>>,
}

impl InodeSocket {
    pub fn new(kind: InodeSocketKind) -> Self {
        Self {
            inner: Arc::new(RwLock::new(InodeSocketInner {
                kind,
                read_buffer: None,
                read_addr: None,
                silence_write_ready: false,
            })),
        }
    }

    pub async fn bind(
        &self,
        net: Arc<dyn VirtualNetworking + Send + Sync + 'static>,
        set_addr: SocketAddr,
    ) -> Result<Option<InodeSocket>, Errno> {
        let mut inner = self.inner.write().unwrap();
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
        net: Arc<dyn VirtualNetworking + Send + Sync + 'static>,
        _backlog: usize,
    ) -> Result<Option<InodeSocket>, Errno> {
        let inner = self.inner.read().unwrap();
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
        _fd_flags: Fdflags,
    ) -> Result<(Box<dyn VirtualTcpSocket + Sync>, SocketAddr), Errno> {
        let mut inner = self.inner.write().unwrap();
        let (sock, addr) = match &mut inner.kind {
            InodeSocketKind::TcpListener(sock) => {
                let (child, addr) = sock.accept().await.map_err(net_error_into_wasi_err)?;
                Ok((child, addr))
            }
            InodeSocketKind::PreSocket { .. } => Err(Errno::Notconn),
            InodeSocketKind::Closed => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }?;
        Ok((sock, addr))
    }

    pub async fn connect(
        &mut self,
        net: Arc<dyn VirtualNetworking + Send + Sync + 'static>,
        peer: SocketAddr,
    ) -> Result<Option<InodeSocket>, Errno> {
        let mut inner = self.inner.write().unwrap();
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

    pub fn status(&self) -> Result<WasiSocketStatus, Errno> {
        let inner = self.inner.read().unwrap();
        Ok(match &inner.kind {
            InodeSocketKind::PreSocket { .. } => WasiSocketStatus::Opening,
            InodeSocketKind::WebSocket(_) => WasiSocketStatus::Opened,
            InodeSocketKind::HttpRequest(..) => WasiSocketStatus::Opened,
            InodeSocketKind::TcpListener(_) => WasiSocketStatus::Opened,
            InodeSocketKind::TcpStream(_) => WasiSocketStatus::Opened,
            InodeSocketKind::UdpSocket(_) => WasiSocketStatus::Opened,
            InodeSocketKind::Closed => WasiSocketStatus::Closed,
            _ => WasiSocketStatus::Failed,
        })
    }

    pub fn http_status(&self) -> Result<WasiHttpStatus, Errno> {
        let inner = self.inner.read().unwrap();
        Ok(match &inner.kind {
            InodeSocketKind::HttpRequest(http, ..) => {
                let http = http.lock().unwrap();
                let guard = http.status.lock().unwrap();
                let status = guard
                    .recv()
                    .map_err(|_| Errno::Io)?
                    .map_err(net_error_into_wasi_err)?;
                WasiHttpStatus {
                    ok: true,
                    redirected: status.redirected,
                    status: status.status,
                    size: status.size as u64,
                }
            }
            InodeSocketKind::Closed => return Err(Errno::Io),
            _ => return Err(Errno::Notsup),
        })
    }

    pub fn addr_local(&self) -> Result<SocketAddr, Errno> {
        let inner = self.inner.read().unwrap();
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

    pub fn addr_peer(&self) -> Result<SocketAddr, Errno> {
        let inner = self.inner.read().unwrap();
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

    pub fn set_opt_flag(&mut self, option: WasiSocketOption, val: bool) -> Result<(), Errno> {
        let mut inner = self.inner.write().unwrap();
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
                WasiSocketOption::Promiscuous => {
                    sock.set_promiscuous(val).map_err(net_error_into_wasi_err)?
                }
                _ => return Err(Errno::Inval),
            },
            InodeSocketKind::TcpStream(sock) => match option {
                WasiSocketOption::NoDelay => {
                    sock.set_nodelay(val).map_err(net_error_into_wasi_err)?
                }
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

    pub fn get_opt_flag(&self, option: WasiSocketOption) -> Result<bool, Errno> {
        let mut inner = self.inner.write().unwrap();
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

    pub fn set_send_buf_size(&mut self, size: usize) -> Result<(), Errno> {
        let mut inner = self.inner.write().unwrap();
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

    pub fn send_buf_size(&self) -> Result<usize, Errno> {
        let inner = self.inner.read().unwrap();
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

    pub fn set_recv_buf_size(&mut self, size: usize) -> Result<(), Errno> {
        let mut inner = self.inner.write().unwrap();
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

    pub fn recv_buf_size(&self) -> Result<usize, Errno> {
        let inner = self.inner.read().unwrap();
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

    pub fn set_linger(&mut self, linger: Option<std::time::Duration>) -> Result<(), Errno> {
        let mut inner = self.inner.write().unwrap();
        match &mut inner.kind {
            InodeSocketKind::TcpStream(sock) => {
                sock.set_linger(linger).map_err(net_error_into_wasi_err)
            }
            InodeSocketKind::PreSocket { .. } => Err(Errno::Io),
            InodeSocketKind::Closed => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub fn nonblocking(&self) -> Result<bool, Errno> {
        let inner = self.inner.read().unwrap();
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

    pub fn set_nonblocking(&self, val: bool) -> Result<(), Errno> {
        let mut inner = self.inner.write().unwrap();
        Ok(match &mut inner.kind {
            InodeSocketKind::TcpStream(sock) => {
                sock.set_nonblocking(val).map_err(net_error_into_wasi_err)?
            }
            InodeSocketKind::TcpListener(sock, ..) => {
                sock.set_nonblocking(val).map_err(net_error_into_wasi_err)?
            }
            InodeSocketKind::UdpSocket(sock, ..) => {
                sock.set_nonblocking(val).map_err(net_error_into_wasi_err)?
            }
            InodeSocketKind::Raw(sock, ..) => {
                sock.set_nonblocking(val).map_err(net_error_into_wasi_err)?
            }
            InodeSocketKind::Icmp(sock, ..) => {
                sock.set_nonblocking(val).map_err(net_error_into_wasi_err)?
            }
            InodeSocketKind::PreSocket { nonblocking, .. } => {
                (*nonblocking) = val;
            }
            _ => {
                return Err(Errno::Notsup);
            }
        })
    }

    pub fn linger(&self) -> Result<Option<std::time::Duration>, Errno> {
        let inner = self.inner.read().unwrap();
        match &inner.kind {
            InodeSocketKind::TcpStream(sock) => sock.linger().map_err(net_error_into_wasi_err),
            InodeSocketKind::PreSocket { .. } => Err(Errno::Io),
            InodeSocketKind::Closed => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub fn set_opt_time(
        &self,
        ty: TimeType,
        timeout: Option<std::time::Duration>,
    ) -> Result<(), Errno> {
        let mut inner = self.inner.write().unwrap();
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

    pub fn opt_time(&self, ty: TimeType) -> Result<Option<std::time::Duration>, Errno> {
        let inner = self.inner.read().unwrap();
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

    pub fn set_ttl(&self, ttl: u32) -> Result<(), Errno> {
        let mut inner = self.inner.write().unwrap();
        match &mut inner.kind {
            InodeSocketKind::TcpStream(sock) => sock.set_ttl(ttl).map_err(net_error_into_wasi_err),
            InodeSocketKind::UdpSocket(sock) => sock.set_ttl(ttl).map_err(net_error_into_wasi_err),
            InodeSocketKind::PreSocket { .. } => Err(Errno::Io),
            InodeSocketKind::Closed => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub fn ttl(&self) -> Result<u32, Errno> {
        let inner = self.inner.read().unwrap();
        match &inner.kind {
            InodeSocketKind::TcpStream(sock) => sock.ttl().map_err(net_error_into_wasi_err),
            InodeSocketKind::UdpSocket(sock) => sock.ttl().map_err(net_error_into_wasi_err),
            InodeSocketKind::PreSocket { .. } => Err(Errno::Io),
            InodeSocketKind::Closed => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub fn set_multicast_ttl_v4(&self, ttl: u32) -> Result<(), Errno> {
        let mut inner = self.inner.write().unwrap();
        match &mut inner.kind {
            InodeSocketKind::UdpSocket(sock) => sock
                .set_multicast_ttl_v4(ttl)
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::PreSocket { .. } => Err(Errno::Io),
            InodeSocketKind::Closed => Err(Errno::Io),
            _ => Err(Errno::Notsup),
        }
    }

    pub fn multicast_ttl_v4(&self) -> Result<u32, Errno> {
        let inner = self.inner.read().unwrap();
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
        let mut inner = self.inner.write().unwrap();
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
        let mut inner = self.inner.write().unwrap();
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
        let mut inner = self.inner.write().unwrap();
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
        let mut inner = self.inner.write().unwrap();
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
        let mut inner = self.inner.write().unwrap();

        let ret = match &mut inner.kind {
            InodeSocketKind::HttpRequest(sock, ty) => {
                let sock = sock.get_mut().unwrap();
                match ty {
                    InodeHttpSocketType::Request => {
                        if sock.request.is_none() {
                            return Err(Errno::Io);
                        }
                        let request = sock.request.as_ref().unwrap();
                        request.send(buf).map(|_| buf_len).map_err(|_| Errno::Io)
                    }
                    _ => {
                        return Err(Errno::Io);
                    }
                }
            }
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

        if ret > 0 {
            inner.silence_write_ready = false;
        }
        Ok(ret)
    }

    pub async fn send_to<M: MemorySize>(
        &self,
        buf: Vec<u8>,
        addr: SocketAddr,
    ) -> Result<usize, Errno> {
        let buf_len = buf.len();
        let mut inner = self.inner.write().unwrap();

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

        if ret > 0 {
            inner.silence_write_ready = false;
        }
        Ok(ret)
    }

    pub fn peek(&self) -> Result<usize, Errno> {
        let mut inner = self.inner.write().unwrap();
        if let Some(buf) = inner.read_buffer.as_ref() {
            if buf.len() > 0 {
                return Ok(buf.len());
            }
        }
        let data = match &mut inner.kind {
            InodeSocketKind::HttpRequest(sock, ty) => {
                let sock = sock.get_mut().unwrap();
                match ty {
                    InodeHttpSocketType::Response => {
                        if sock.response.is_none() {
                            return Err(Errno::Io);
                        }
                        let response = sock.response.as_ref().unwrap();

                        use std::sync::mpsc::TryRecvError;
                        match response.try_recv() {
                            Ok(a) => Bytes::from(a),
                            Err(TryRecvError::Disconnected) => {
                                return Err(Errno::Io);
                            }
                            Err(TryRecvError::Empty) => {
                                return Ok(0);
                            }
                        }
                    }
                    InodeHttpSocketType::Headers => {
                        if sock.headers.is_none() {
                            return Err(Errno::Io);
                        }
                        let headers = sock.headers.as_ref().unwrap();

                        use std::sync::mpsc::TryRecvError;
                        let headers = match headers.try_recv() {
                            Ok(a) => a,
                            Err(TryRecvError::Disconnected) => {
                                return Err(Errno::Io);
                            }
                            Err(TryRecvError::Empty) => {
                                return Ok(0);
                            }
                        };

                        let headers = format!("{}: {}", headers.0, headers.1);
                        Bytes::from(headers.as_bytes().to_vec())
                    }
                    _ => {
                        return Err(Errno::Io);
                    }
                }
            }
            InodeSocketKind::WebSocket(sock) => {
                let read = match sock.try_recv().map_err(net_error_into_wasi_err)? {
                    Some(a) => a,
                    None => {
                        return Ok(0);
                    }
                };
                read.data
            }
            InodeSocketKind::Raw(sock) => {
                let read = match sock.try_recv().map_err(net_error_into_wasi_err)? {
                    Some(a) => a,
                    None => {
                        return Ok(0);
                    }
                };
                read.data
            }
            InodeSocketKind::TcpStream(sock) => {
                let read = match sock.try_recv().map_err(net_error_into_wasi_err)? {
                    Some(a) => a,
                    None => {
                        return Ok(0);
                    }
                };
                read.data
            }
            InodeSocketKind::UdpSocket(sock) => {
                let read = match sock.try_recv().map_err(net_error_into_wasi_err)? {
                    Some(a) => a,
                    None => {
                        return Ok(0);
                    }
                };
                read.data
            }
            InodeSocketKind::TcpListener(sock) => {
                return sock.peek().map_err(net_error_into_wasi_err);
            }
            InodeSocketKind::PreSocket { .. } => return Err(Errno::Notconn),
            InodeSocketKind::Closed => return Err(Errno::Io),
            _ => return Err(Errno::Notsup),
        };
        inner.read_buffer.replace(data);
        inner.read_addr.take();
        if let Some(buf) = inner.read_buffer.as_ref() {
            Ok(buf.len())
        } else {
            Ok(0)
        }
    }

    pub async fn recv(&self, max_size: usize) -> Result<Bytes, Errno> {
        let mut inner = self.inner.write().unwrap();
        loop {
            let is_tcp = if let InodeSocketKind::TcpStream(..) = &inner.kind {
                true
            } else {
                false
            };
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
                InodeSocketKind::HttpRequest(sock, ty) => {
                    let sock = sock.get_mut().unwrap();
                    match ty {
                        InodeHttpSocketType::Response => {
                            if sock.response.is_none() {
                                return Err(Errno::Io);
                            }
                            let response = sock.response.as_ref().unwrap();
                            Bytes::from(response.recv().map_err(|_| Errno::Io)?)
                        }
                        InodeHttpSocketType::Headers => {
                            if sock.headers.is_none() {
                                return Err(Errno::Io);
                            }
                            let headers = sock.headers.as_ref().unwrap();
                            let headers = headers.recv().map_err(|_| Errno::Io)?;
                            let headers = format!("{}: {}", headers.0, headers.1);
                            Bytes::from(headers.as_bytes().to_vec())
                        }
                        _ => {
                            return Err(Errno::Io);
                        }
                    }
                }
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
                InodeSocketKind::Closed => return Err(Errno::Io),
                _ => return Err(Errno::Notsup),
            };
            if data.len() == 0 {
                return Err(Errno::Io);
            }
            inner.read_buffer.replace(data);
            inner.read_addr.take();
        }
    }

    pub async fn peek_from(&self) -> Result<usize, Errno> {
        let mut inner = self.inner.write().unwrap();
        if let Some(buf) = inner.read_buffer.as_ref() {
            if buf.len() > 0 {
                return Ok(buf.len());
            }
        }
        let rcv = match &mut inner.kind {
            InodeSocketKind::Icmp(sock) => {
                match sock.try_recv_from().map_err(net_error_into_wasi_err)? {
                    Some(a) => a,
                    None => {
                        return Ok(0);
                    }
                }
            }
            InodeSocketKind::UdpSocket(sock) => {
                match sock.try_recv_from().map_err(net_error_into_wasi_err)? {
                    Some(a) => a,
                    None => {
                        return Ok(0);
                    }
                }
            }
            InodeSocketKind::PreSocket { .. } => return Err(Errno::Notconn),
            InodeSocketKind::Closed => return Err(Errno::Io),
            _ => return Err(Errno::Notsup),
        };
        inner.read_buffer.replace(rcv.data);
        inner.read_addr.replace(rcv.addr);
        if let Some(buf) = inner.read_buffer.as_ref() {
            Ok(buf.len())
        } else {
            Ok(0)
        }
    }

    pub async fn recv_from(&self, max_size: usize) -> Result<(Bytes, SocketAddr), Errno> {
        let mut inner = self.inner.write().unwrap();
        loop {
            let is_tcp = if let InodeSocketKind::TcpStream(..) = &inner.kind {
                true
            } else {
                false
            };
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
        use std::net::Shutdown;
        let mut inner = self.inner.write().unwrap();
        inner.silence_write_ready = false;
        match &mut inner.kind {
            InodeSocketKind::TcpStream(sock) => {
                sock.shutdown(how).await.map_err(net_error_into_wasi_err)?;
            }
            InodeSocketKind::HttpRequest(http, ..) => {
                let http = http.get_mut().unwrap();
                match how {
                    Shutdown::Read => {
                        http.response.take();
                        http.headers.take();
                    }
                    Shutdown::Write => {
                        http.request.take();
                    }
                    Shutdown::Both => {
                        http.request.take();
                        http.response.take();
                        http.headers.take();
                    }
                };
            }
            InodeSocketKind::PreSocket { .. } => return Err(Errno::Notconn),
            InodeSocketKind::Closed => return Err(Errno::Io),
            _ => return Err(Errno::Notsup),
        }
        Ok(())
    }

    pub fn can_write(&self) -> bool {
        if let Ok(mut guard) = self.inner.try_write() {
            match &mut guard.kind {
                InodeSocketKind::TcpListener(socket) => {
                    socket.peek().ok().map(|a| a > 0).unwrap_or_default()
                }
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

    pub fn poll_read_ready(
        &self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<wasmer_vnet::Result<usize>> {
        let mut inner = self.inner.write().unwrap();
        match &mut inner.kind {
            InodeSocketKind::TcpListener(socket) => socket.poll_accept_ready(cx),
            InodeSocketKind::TcpStream(socket) => socket.poll_read_ready(cx),
            InodeSocketKind::UdpSocket(socket) => socket.poll_read_ready(cx),
            InodeSocketKind::Raw(socket) => socket.poll_read_ready(cx),
            InodeSocketKind::WebSocket(socket) => socket.poll_read_ready(cx),
            InodeSocketKind::Icmp(socket) => socket.poll_read_ready(cx),
            InodeSocketKind::PreSocket { .. } => {
                std::task::Poll::Ready(Err(wasmer_vnet::NetworkError::IOError))
            }
            InodeSocketKind::HttpRequest(..) => std::task::Poll::Pending,
            InodeSocketKind::Closed => {
                std::task::Poll::Ready(Err(wasmer_vnet::NetworkError::ConnectionAborted))
            }
        }
    }

    pub fn poll_write_ready(
        &self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<wasmer_vnet::Result<usize>> {
        let mut inner = self.inner.write().unwrap();
        if inner.silence_write_ready {
            return std::task::Poll::Pending;
        }
        let ret = match &mut inner.kind {
            InodeSocketKind::TcpListener(_) => std::task::Poll::Pending,
            InodeSocketKind::TcpStream(socket) => socket.poll_write_ready(cx),
            InodeSocketKind::UdpSocket(socket) => socket.poll_write_ready(cx),
            InodeSocketKind::Raw(socket) => socket.poll_write_ready(cx),
            InodeSocketKind::WebSocket(socket) => socket.poll_write_ready(cx),
            InodeSocketKind::Icmp(socket) => socket.poll_write_ready(cx),
            InodeSocketKind::PreSocket { .. } => {
                std::task::Poll::Ready(Err(wasmer_vnet::NetworkError::IOError))
            }
            InodeSocketKind::HttpRequest(..) => std::task::Poll::Pending,
            InodeSocketKind::Closed => {
                std::task::Poll::Ready(Err(wasmer_vnet::NetworkError::ConnectionAborted))
            }
        };
        if ret.is_ready() {
            // TODO - This will suppress the write ready notifications
            inner.silence_write_ready = true;
        }
        ret
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

impl Drop for InodeSocketInner {
    fn drop(&mut self) {
        if let InodeSocketKind::HttpRequest(http, ty) = &self.kind {
            let mut guard = http.lock().unwrap();
            match ty {
                InodeHttpSocketType::Request => {
                    guard.request.take();
                }
                InodeHttpSocketType::Response => {
                    guard.response.take();
                }
                InodeHttpSocketType::Headers => {
                    guard.headers.take();
                }
            }
        }
    }
}

#[allow(dead_code)]
pub(crate) fn read_ip<M: MemorySize>(
    memory: &MemoryView,
    ptr: WasmPtr<__wasi_addr_t, M>,
) -> Result<IpAddr, Errno> {
    let addr_ptr = ptr.deref(memory);
    let addr = addr_ptr.read().map_err(crate::mem_error_to_wasi)?;

    let o = addr.u.octs;
    Ok(match addr.tag {
        Addressfamily::Inet4 => IpAddr::V4(Ipv4Addr::new(o[0], o[1], o[2], o[3])),
        Addressfamily::Inet6 => {
            let [a, b, c, d, e, f, g, h] = unsafe { transmute::<_, [u16; 8]>(o) };
            IpAddr::V6(Ipv6Addr::new(a, b, c, d, e, f, g, h))
        }
        _ => return Err(Errno::Inval),
    })
}

pub(crate) fn read_ip_v4<M: MemorySize>(
    memory: &MemoryView,
    ptr: WasmPtr<__wasi_addr_ip4_t, M>,
) -> Result<Ipv4Addr, Errno> {
    let addr_ptr = ptr.deref(memory);
    let addr = addr_ptr.read().map_err(crate::mem_error_to_wasi)?;

    let o = addr.octs;
    Ok(Ipv4Addr::new(o[0], o[1], o[2], o[3]))
}

pub(crate) fn read_ip_v6<M: MemorySize>(
    memory: &MemoryView,
    ptr: WasmPtr<__wasi_addr_ip6_t, M>,
) -> Result<Ipv6Addr, Errno> {
    let addr_ptr = ptr.deref(memory);
    let addr = addr_ptr.read().map_err(crate::mem_error_to_wasi)?;

    let [a, b, c, d, e, f, g, h] = unsafe { transmute::<_, [u16; 8]>(addr.segs) };
    Ok(Ipv6Addr::new(a, b, c, d, e, f, g, h))
}

pub(crate) fn write_ip<M: MemorySize>(
    memory: &MemoryView,
    ptr: WasmPtr<__wasi_addr_t, M>,
    ip: IpAddr,
) -> Result<(), Errno> {
    let ip = match ip {
        IpAddr::V4(ip) => {
            let o = ip.octets();
            __wasi_addr_t {
                tag: Addressfamily::Inet4,
                u: __wasi_addr_u {
                    octs: [o[0], o[1], o[2], o[3], 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                },
            }
        }
        IpAddr::V6(ip) => {
            let o = ip.octets();
            __wasi_addr_t {
                tag: Addressfamily::Inet6,
                u: __wasi_addr_u { octs: o },
            }
        }
    };

    let addr_ptr = ptr.deref(memory);
    addr_ptr.write(ip).map_err(crate::mem_error_to_wasi)?;
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn read_cidr<M: MemorySize>(
    memory: &MemoryView,
    ptr: WasmPtr<__wasi_cidr_t, M>,
) -> Result<IpCidr, Errno> {
    let addr_ptr = ptr.deref(memory);
    let addr = addr_ptr.read().map_err(crate::mem_error_to_wasi)?;

    let o = addr.u.octs;
    Ok(match addr.tag {
        Addressfamily::Inet4 => IpCidr {
            ip: IpAddr::V4(Ipv4Addr::new(o[0], o[1], o[2], o[3])),
            prefix: o[4],
        },
        Addressfamily::Inet6 => {
            let [a, b, c, d, e, f, g, h] = {
                let o = [
                    o[0], o[1], o[2], o[3], o[4], o[5], o[6], o[7], o[8], o[9], o[10], o[11],
                    o[12], o[13], o[14], o[15],
                ];
                unsafe { transmute::<_, [u16; 8]>(o) }
            };
            IpCidr {
                ip: IpAddr::V6(Ipv6Addr::new(a, b, c, d, e, f, g, h)),
                prefix: o[16],
            }
        }
        _ => return Err(Errno::Inval),
    })
}

#[allow(dead_code)]
pub(crate) fn write_cidr<M: MemorySize>(
    memory: &MemoryView,
    ptr: WasmPtr<__wasi_cidr_t, M>,
    cidr: IpCidr,
) -> Result<(), Errno> {
    let p = cidr.prefix;
    let cidr = match cidr.ip {
        IpAddr::V4(ip) => {
            let o = ip.octets();
            __wasi_cidr_t {
                tag: Addressfamily::Inet4,
                u: __wasi_cidr_u {
                    octs: [
                        o[0], o[1], o[2], o[3], p, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    ],
                },
            }
        }
        IpAddr::V6(ip) => {
            let o = ip.octets();
            __wasi_cidr_t {
                tag: Addressfamily::Inet6,
                u: __wasi_cidr_u {
                    octs: [
                        o[0], o[1], o[2], o[3], o[4], o[5], o[6], o[7], o[8], o[9], o[10], o[11],
                        o[12], o[13], o[14], o[15], p,
                    ],
                },
            }
        }
    };

    let addr_ptr = ptr.deref(memory);
    addr_ptr.write(cidr).map_err(crate::mem_error_to_wasi)?;
    Ok(())
}

pub(crate) fn read_ip_port<M: MemorySize>(
    memory: &MemoryView,
    ptr: WasmPtr<__wasi_addr_port_t, M>,
) -> Result<(IpAddr, u16), Errno> {
    let addr_ptr = ptr.deref(memory);
    let addr = addr_ptr.read().map_err(crate::mem_error_to_wasi)?;

    let o = addr.u.octs;
    Ok(match addr.tag {
        Addressfamily::Inet4 => {
            let port = u16::from_ne_bytes([o[0], o[1]]);
            (IpAddr::V4(Ipv4Addr::new(o[2], o[3], o[4], o[5])), port)
        }
        Addressfamily::Inet6 => {
            let [a, b, c, d, e, f, g, h] = {
                let o = [
                    o[2], o[3], o[4], o[5], o[6], o[7], o[8], o[9], o[10], o[11], o[12], o[13],
                    o[14], o[15], o[16], o[17],
                ];
                unsafe { transmute::<_, [u16; 8]>(o) }
            };
            (
                IpAddr::V6(Ipv6Addr::new(a, b, c, d, e, f, g, h)),
                u16::from_ne_bytes([o[0], o[1]]),
            )
        }
        _ => return Err(Errno::Inval),
    })
}

#[allow(dead_code)]
pub(crate) fn write_ip_port<M: MemorySize>(
    memory: &MemoryView,
    ptr: WasmPtr<__wasi_addr_port_t, M>,
    ip: IpAddr,
    port: u16,
) -> Result<(), Errno> {
    let p = port.to_be_bytes();
    let ipport = match ip {
        IpAddr::V4(ip) => {
            let o = ip.octets();
            __wasi_addr_port_t {
                tag: Addressfamily::Inet4,
                u: __wasi_addr_port_u {
                    octs: [
                        p[0], p[1], o[0], o[1], o[2], o[3], 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    ],
                },
            }
        }
        IpAddr::V6(ip) => {
            let o = ip.octets();
            __wasi_addr_port_t {
                tag: Addressfamily::Inet6,
                u: __wasi_addr_port_u {
                    octs: [
                        p[0], p[1], o[0], o[1], o[2], o[3], o[4], o[5], o[6], o[7], o[8], o[9],
                        o[10], o[11], o[12], o[13], o[14], o[15],
                    ],
                },
            }
        }
    };

    let addr_ptr = ptr.deref(memory);
    addr_ptr.write(ipport).map_err(crate::mem_error_to_wasi)?;
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn read_route<M: MemorySize>(
    memory: &MemoryView,
    ptr: WasmPtr<Route, M>,
) -> Result<IpRoute, Errno> {
    let route_ptr = ptr.deref(memory);
    let route = route_ptr.read().map_err(crate::mem_error_to_wasi)?;

    Ok(IpRoute {
        cidr: {
            let o = route.cidr.u.octs;
            match route.cidr.tag {
                Addressfamily::Inet4 => IpCidr {
                    ip: IpAddr::V4(Ipv4Addr::new(o[0], o[1], o[2], o[3])),
                    prefix: o[4],
                },
                Addressfamily::Inet6 => {
                    let [a, b, c, d, e, f, g, h] = {
                        let o = [
                            o[0], o[1], o[2], o[3], o[4], o[5], o[6], o[7], o[8], o[9], o[10],
                            o[11], o[12], o[13], o[14], o[15],
                        ];
                        unsafe { transmute::<_, [u16; 8]>(o) }
                    };
                    IpCidr {
                        ip: IpAddr::V6(Ipv6Addr::new(a, b, c, d, e, f, g, h)),
                        prefix: o[16],
                    }
                }
                _ => return Err(Errno::Inval),
            }
        },
        via_router: {
            let o = route.via_router.u.octs;
            match route.via_router.tag {
                Addressfamily::Inet4 => IpAddr::V4(Ipv4Addr::new(o[0], o[1], o[2], o[3])),
                Addressfamily::Inet6 => {
                    let [a, b, c, d, e, f, g, h] = unsafe { transmute::<_, [u16; 8]>(o) };
                    IpAddr::V6(Ipv6Addr::new(a, b, c, d, e, f, g, h))
                }
                _ => return Err(Errno::Inval),
            }
        },
        preferred_until: match route.preferred_until.tag {
            OptionTag::None => None,
            OptionTag::Some => Some(Duration::from_nanos(route.preferred_until.u)),
        },
        expires_at: match route.expires_at.tag {
            OptionTag::None => None,
            OptionTag::Some => Some(Duration::from_nanos(route.expires_at.u)),
        },
    })
}

pub(crate) fn write_route<M: MemorySize>(
    memory: &MemoryView,
    ptr: WasmPtr<Route, M>,
    route: IpRoute,
) -> Result<(), Errno> {
    let cidr = {
        let p = route.cidr.prefix;
        match route.cidr.ip {
            IpAddr::V4(ip) => {
                let o = ip.octets();
                __wasi_cidr_t {
                    tag: Addressfamily::Inet4,
                    u: __wasi_cidr_u {
                        octs: [
                            o[0], o[1], o[2], o[3], p, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                        ],
                    },
                }
            }
            IpAddr::V6(ip) => {
                let o = ip.octets();
                __wasi_cidr_t {
                    tag: Addressfamily::Inet6,
                    u: __wasi_cidr_u {
                        octs: [
                            o[0], o[1], o[2], o[3], o[4], o[5], o[6], o[7], o[8], o[9], o[10],
                            o[11], o[12], o[13], o[14], o[15], p,
                        ],
                    },
                }
            }
        }
    };
    let via_router = match route.via_router {
        IpAddr::V4(ip) => {
            let o = ip.octets();
            __wasi_addr_t {
                tag: Addressfamily::Inet4,
                u: __wasi_addr_u {
                    octs: [o[0], o[1], o[2], o[3], 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                },
            }
        }
        IpAddr::V6(ip) => {
            let o = ip.octets();
            __wasi_addr_t {
                tag: Addressfamily::Inet6,
                u: __wasi_addr_u { octs: o },
            }
        }
    };
    let preferred_until = match route.preferred_until {
        None => OptionTimestamp {
            tag: OptionTag::None,
            u: 0,
        },
        Some(u) => OptionTimestamp {
            tag: OptionTag::Some,
            u: u.as_nanos() as u64,
        },
    };
    let expires_at = match route.expires_at {
        None => OptionTimestamp {
            tag: OptionTag::None,
            u: 0,
        },
        Some(u) => OptionTimestamp {
            tag: OptionTag::Some,
            u: u.as_nanos() as u64,
        },
    };

    let route = Route {
        cidr,
        via_router,
        preferred_until,
        expires_at,
    };

    let route_ptr = ptr.deref(memory);
    route_ptr.write(route).map_err(crate::mem_error_to_wasi)?;
    Ok(())
}

#[cfg_attr(not(feature = "os"), allow(dead_code))]
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
