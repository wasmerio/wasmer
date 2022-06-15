use super::types::net_error_into_wasi_err;
use crate::syscalls::types::*;
use crate::syscalls::{read_bytes, write_bytes};
use bytes::{Buf, Bytes};
use std::convert::TryInto;
use std::io::{self, Read};
use std::mem::transmute;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Mutex;
use std::time::Duration;
#[allow(unused_imports)]
use tracing::{debug, error, info, warn};
use wasmer::{Memory, MemorySize, WasmPtr, WasmSlice};
use wasmer_vnet::{net_error_into_io_err, TimeType};
use wasmer_vnet::{
    IpCidr, IpRoute, SocketHttpRequest, VirtualIcmpSocket, VirtualNetworking, VirtualRawSocket,
    VirtualTcpListener, VirtualTcpSocket, VirtualUdpSocket, VirtualWebSocket,
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
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub enum InodeSocketKind {
    PreSocket {
        family: __wasi_addressfamily_t,
        ty: __wasi_socktype_t,
        pt: __wasi_sockproto_t,
        addr: Option<SocketAddr>,
        only_v6: bool,
        reuse_port: bool,
        reuse_addr: bool,
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

impl From<__wasi_sockoption_t> for WasiSocketOption {
    fn from(opt: __wasi_sockoption_t) -> Self {
        use WasiSocketOption::*;
        match opt {
            __WASI_SOCK_OPTION_NOOP => Noop,
            __WASI_SOCK_OPTION_REUSE_PORT => ReusePort,
            __WASI_SOCK_OPTION_REUSE_ADDR => ReuseAddr,
            __WASI_SOCK_OPTION_NO_DELAY => NoDelay,
            __WASI_SOCK_OPTION_DONT_ROUTE => DontRoute,
            __WASI_SOCK_OPTION_ONLY_V6 => OnlyV6,
            __WASI_SOCK_OPTION_BROADCAST => Broadcast,
            __WASI_SOCK_OPTION_MULTICAST_LOOP_V4 => MulticastLoopV4,
            __WASI_SOCK_OPTION_MULTICAST_LOOP_V6 => MulticastLoopV6,
            __WASI_SOCK_OPTION_PROMISCUOUS => Promiscuous,
            __WASI_SOCK_OPTION_LISTENING => Listening,
            __WASI_SOCK_OPTION_LAST_ERROR => LastError,
            __WASI_SOCK_OPTION_KEEP_ALIVE => KeepAlive,
            __WASI_SOCK_OPTION_LINGER => Linger,
            __WASI_SOCK_OPTION_OOB_INLINE => OobInline,
            __WASI_SOCK_OPTION_RECV_BUF_SIZE => RecvBufSize,
            __WASI_SOCK_OPTION_SEND_BUF_SIZE => SendBufSize,
            __WASI_SOCK_OPTION_RECV_LOWAT => RecvLowat,
            __WASI_SOCK_OPTION_SEND_LOWAT => SendLowat,
            __WASI_SOCK_OPTION_RECV_TIMEOUT => RecvTimeout,
            __WASI_SOCK_OPTION_SEND_TIMEOUT => SendTimeout,
            __WASI_SOCK_OPTION_CONNECT_TIMEOUT => ConnectTimeout,
            __WASI_SOCK_OPTION_ACCEPT_TIMEOUT => AcceptTimeout,
            __WASI_SOCK_OPTION_TTL => Ttl,
            __WASI_SOCK_OPTION_MULTICAST_TTL_V4 => MulticastTtlV4,
            __WASI_SOCK_OPTION_TYPE => Type,
            __WASI_SOCK_OPTION_PROTO => Proto,
            _ => Noop,
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
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct InodeSocket {
    kind: InodeSocketKind,
    read_buffer: Option<Bytes>,
    read_addr: Option<SocketAddr>,
}

impl InodeSocket {
    pub fn new(kind: InodeSocketKind) -> InodeSocket {
        InodeSocket {
            kind,
            read_buffer: None,
            read_addr: None,
        }
    }

    pub fn bind(
        &mut self,
        net: &(dyn VirtualNetworking),
        set_addr: SocketAddr,
    ) -> Result<Option<InodeSocket>, __wasi_errno_t> {
        match &mut self.kind {
            InodeSocketKind::PreSocket {
                family,
                ty,
                addr,
                reuse_port,
                reuse_addr,
                ..
            } => {
                match *family {
                    __WASI_ADDRESS_FAMILY_INET4 => {
                        if !set_addr.is_ipv4() {
                            return Err(__WASI_EINVAL);
                        }
                    }
                    __WASI_ADDRESS_FAMILY_INET6 => {
                        if !set_addr.is_ipv6() {
                            return Err(__WASI_EINVAL);
                        }
                    }
                    _ => {
                        return Err(__WASI_ENOTSUP);
                    }
                }

                addr.replace(set_addr);
                let addr = (*addr).unwrap();

                Ok(match *ty {
                    __WASI_SOCK_TYPE_STREAM => {
                        // we already set the socket address - next we need a bind or connect so nothing
                        // more to do at this time
                        None
                    }
                    __WASI_SOCK_TYPE_DGRAM => {
                        let socket = net
                            .bind_udp(addr, *reuse_port, *reuse_addr)
                            .map_err(net_error_into_wasi_err)?;
                        Some(InodeSocket::new(InodeSocketKind::UdpSocket(socket)))
                    }
                    _ => return Err(__WASI_EINVAL),
                })
            }
            _ => Err(__WASI_ENOTSUP),
        }
    }

    pub fn listen(
        &mut self,
        net: &(dyn VirtualNetworking),
        _backlog: usize,
    ) -> Result<Option<InodeSocket>, __wasi_errno_t> {
        match &self.kind {
            InodeSocketKind::PreSocket {
                ty,
                addr,
                only_v6,
                reuse_port,
                reuse_addr,
                accept_timeout,
                ..
            } => Ok(match *ty {
                __WASI_SOCK_TYPE_STREAM => {
                    if addr.is_none() {
                        return Err(__WASI_EINVAL);
                    }
                    let addr = *addr.as_ref().unwrap();
                    let mut socket = net
                        .listen_tcp(addr, *only_v6, *reuse_port, *reuse_addr)
                        .map_err(net_error_into_wasi_err)?;
                    if let Some(accept_timeout) = accept_timeout {
                        socket
                            .set_timeout(Some(*accept_timeout))
                            .map_err(net_error_into_wasi_err)?;
                    }
                    Some(InodeSocket::new(InodeSocketKind::TcpListener(socket)))
                }
                _ => return Err(__WASI_ENOTSUP),
            }),
            InodeSocketKind::Closed => Err(__WASI_EIO),
            _ => Err(__WASI_ENOTSUP),
        }
    }

    pub fn accept(
        &self,
        _fd_flags: __wasi_fdflags_t,
    ) -> Result<(Box<dyn VirtualTcpSocket + Sync>, SocketAddr), __wasi_errno_t> {
        let (sock, addr) = match &self.kind {
            InodeSocketKind::TcpListener(sock) => sock.accept().map_err(net_error_into_wasi_err),
            InodeSocketKind::PreSocket { .. } => Err(__WASI_ENOTCONN),
            InodeSocketKind::Closed => Err(__WASI_EIO),
            _ => Err(__WASI_ENOTSUP),
        }?;
        Ok((sock, addr))
    }

    pub fn accept_timeout(
        &self,
        _fd_flags: __wasi_fdflags_t,
        timeout: Duration,
    ) -> Result<(Box<dyn VirtualTcpSocket + Sync>, SocketAddr), __wasi_errno_t> {
        let (sock, addr) = match &self.kind {
            InodeSocketKind::TcpListener(sock) => sock
                .accept_timeout(timeout)
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::PreSocket { .. } => Err(__WASI_ENOTCONN),
            InodeSocketKind::Closed => Err(__WASI_EIO),
            _ => Err(__WASI_ENOTSUP),
        }?;
        Ok((sock, addr))
    }

    pub fn connect(
        &mut self,
        net: &(dyn VirtualNetworking),
        peer: SocketAddr,
    ) -> Result<Option<InodeSocket>, __wasi_errno_t> {
        match &mut self.kind {
            InodeSocketKind::PreSocket {
                ty,
                addr,
                send_timeout,
                recv_timeout,
                connect_timeout,
                ..
            } => Ok(match *ty {
                __WASI_SOCK_TYPE_STREAM => {
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
                __WASI_SOCK_TYPE_DGRAM => return Err(__WASI_EINVAL),
                _ => return Err(__WASI_ENOTSUP),
            }),
            InodeSocketKind::UdpSocket(sock) => {
                sock.connect(peer).map_err(net_error_into_wasi_err)?;
                Ok(None)
            }
            InodeSocketKind::Closed => Err(__WASI_EIO),
            _ => Err(__WASI_ENOTSUP),
        }
    }

    pub fn status(&self) -> Result<WasiSocketStatus, __wasi_errno_t> {
        Ok(match &self.kind {
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

    pub fn http_status(&self) -> Result<WasiHttpStatus, __wasi_errno_t> {
        Ok(match &self.kind {
            InodeSocketKind::HttpRequest(http, ..) => {
                let http = http.lock().unwrap();
                let guard = http.status.lock().unwrap();
                let status = guard
                    .recv()
                    .map_err(|_| __WASI_EIO)?
                    .map_err(net_error_into_wasi_err)?;
                WasiHttpStatus {
                    ok: true,
                    redirected: status.redirected,
                    status: status.status,
                    size: status.size as u64,
                }
            }
            InodeSocketKind::Closed => return Err(__WASI_EIO),
            _ => return Err(__WASI_ENOTSUP),
        })
    }

    pub fn addr_local(&self) -> Result<SocketAddr, __wasi_errno_t> {
        Ok(match &self.kind {
            InodeSocketKind::PreSocket { family, addr, .. } => {
                if let Some(addr) = addr {
                    *addr
                } else {
                    SocketAddr::new(
                        match *family {
                            __WASI_ADDRESS_FAMILY_INET4 => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                            __WASI_ADDRESS_FAMILY_INET6 => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
                            _ => return Err(__WASI_EINVAL),
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
            InodeSocketKind::Closed => return Err(__WASI_EIO),
            _ => return Err(__WASI_ENOTSUP),
        })
    }

    pub fn addr_peer(&self) -> Result<SocketAddr, __wasi_errno_t> {
        Ok(match &self.kind {
            InodeSocketKind::PreSocket { family, .. } => SocketAddr::new(
                match *family {
                    __WASI_ADDRESS_FAMILY_INET4 => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                    __WASI_ADDRESS_FAMILY_INET6 => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
                    _ => return Err(__WASI_EINVAL),
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
            InodeSocketKind::Closed => return Err(__WASI_EIO),
            _ => return Err(__WASI_ENOTSUP),
        })
    }

    pub fn set_opt_flag(
        &mut self,
        option: WasiSocketOption,
        val: bool,
    ) -> Result<(), __wasi_errno_t> {
        match &mut self.kind {
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
                    _ => return Err(__WASI_EINVAL),
                };
            }
            InodeSocketKind::Raw(sock) => match option {
                WasiSocketOption::Promiscuous => {
                    sock.set_promiscuous(val).map_err(net_error_into_wasi_err)?
                }
                _ => return Err(__WASI_EINVAL),
            },
            InodeSocketKind::TcpStream(sock) => match option {
                WasiSocketOption::NoDelay => {
                    sock.set_nodelay(val).map_err(net_error_into_wasi_err)?
                }
                _ => return Err(__WASI_EINVAL),
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
                _ => return Err(__WASI_EINVAL),
            },
            InodeSocketKind::Closed => return Err(__WASI_EIO),
            _ => return Err(__WASI_ENOTSUP),
        }
        Ok(())
    }

    pub fn get_opt_flag(&self, option: WasiSocketOption) -> Result<bool, __wasi_errno_t> {
        Ok(match &self.kind {
            InodeSocketKind::PreSocket {
                only_v6,
                reuse_port,
                reuse_addr,
                ..
            } => match option {
                WasiSocketOption::OnlyV6 => *only_v6,
                WasiSocketOption::ReusePort => *reuse_port,
                WasiSocketOption::ReuseAddr => *reuse_addr,
                _ => return Err(__WASI_EINVAL),
            },
            InodeSocketKind::Raw(sock) => match option {
                WasiSocketOption::Promiscuous => {
                    sock.promiscuous().map_err(net_error_into_wasi_err)?
                }
                _ => return Err(__WASI_EINVAL),
            },
            InodeSocketKind::TcpStream(sock) => match option {
                WasiSocketOption::NoDelay => sock.nodelay().map_err(net_error_into_wasi_err)?,
                _ => return Err(__WASI_EINVAL),
            },
            InodeSocketKind::UdpSocket(sock) => match option {
                WasiSocketOption::Broadcast => sock.broadcast().map_err(net_error_into_wasi_err)?,
                WasiSocketOption::MulticastLoopV4 => {
                    sock.multicast_loop_v4().map_err(net_error_into_wasi_err)?
                }
                WasiSocketOption::MulticastLoopV6 => {
                    sock.multicast_loop_v6().map_err(net_error_into_wasi_err)?
                }
                _ => return Err(__WASI_EINVAL),
            },
            InodeSocketKind::Closed => return Err(__WASI_EIO),
            _ => return Err(__WASI_ENOTSUP),
        })
    }

    pub fn set_send_buf_size(&mut self, size: usize) -> Result<(), __wasi_errno_t> {
        match &mut self.kind {
            InodeSocketKind::PreSocket { send_buf_size, .. } => {
                *send_buf_size = Some(size);
            }
            InodeSocketKind::TcpStream(sock) => {
                sock.set_send_buf_size(size)
                    .map_err(net_error_into_wasi_err)?;
            }
            InodeSocketKind::Closed => return Err(__WASI_EIO),
            _ => return Err(__WASI_ENOTSUP),
        }
        Ok(())
    }

    pub fn send_buf_size(&self) -> Result<usize, __wasi_errno_t> {
        match &self.kind {
            InodeSocketKind::PreSocket { send_buf_size, .. } => {
                Ok((*send_buf_size).unwrap_or_default())
            }
            InodeSocketKind::TcpStream(sock) => {
                sock.send_buf_size().map_err(net_error_into_wasi_err)
            }
            InodeSocketKind::Closed => Err(__WASI_EIO),
            _ => Err(__WASI_ENOTSUP),
        }
    }

    pub fn set_recv_buf_size(&mut self, size: usize) -> Result<(), __wasi_errno_t> {
        match &mut self.kind {
            InodeSocketKind::PreSocket { recv_buf_size, .. } => {
                *recv_buf_size = Some(size);
            }
            InodeSocketKind::TcpStream(sock) => {
                sock.set_recv_buf_size(size)
                    .map_err(net_error_into_wasi_err)?;
            }
            InodeSocketKind::Closed => return Err(__WASI_EIO),
            _ => return Err(__WASI_ENOTSUP),
        }
        Ok(())
    }

    pub fn recv_buf_size(&self) -> Result<usize, __wasi_errno_t> {
        match &self.kind {
            InodeSocketKind::PreSocket { recv_buf_size, .. } => {
                Ok((*recv_buf_size).unwrap_or_default())
            }
            InodeSocketKind::TcpStream(sock) => {
                sock.recv_buf_size().map_err(net_error_into_wasi_err)
            }
            InodeSocketKind::Closed => Err(__WASI_EIO),
            _ => Err(__WASI_ENOTSUP),
        }
    }

    pub fn set_linger(
        &mut self,
        linger: Option<std::time::Duration>,
    ) -> Result<(), __wasi_errno_t> {
        match &mut self.kind {
            InodeSocketKind::TcpStream(sock) => {
                sock.set_linger(linger).map_err(net_error_into_wasi_err)
            }
            InodeSocketKind::PreSocket { .. } => Err(__WASI_EIO),
            InodeSocketKind::Closed => Err(__WASI_EIO),
            _ => Err(__WASI_ENOTSUP),
        }
    }

    pub fn linger(&self) -> Result<Option<std::time::Duration>, __wasi_errno_t> {
        match &self.kind {
            InodeSocketKind::TcpStream(sock) => sock.linger().map_err(net_error_into_wasi_err),
            InodeSocketKind::PreSocket { .. } => Err(__WASI_EIO),
            InodeSocketKind::Closed => Err(__WASI_EIO),
            _ => Err(__WASI_ENOTSUP),
        }
    }

    pub fn set_opt_time(
        &mut self,
        ty: TimeType,
        timeout: Option<std::time::Duration>,
    ) -> Result<(), __wasi_errno_t> {
        match &mut self.kind {
            InodeSocketKind::TcpStream(sock) => sock
                .set_opt_time(ty, timeout)
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::TcpListener(sock) => match ty {
                TimeType::AcceptTimeout => {
                    sock.set_timeout(timeout).map_err(net_error_into_wasi_err)
                }
                _ => Err(__WASI_EINVAL),
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
                _ => Err(__WASI_EIO),
            },
            InodeSocketKind::Closed => Err(__WASI_EIO),
            _ => Err(__WASI_ENOTSUP),
        }
    }

    pub fn opt_time(&self, ty: TimeType) -> Result<Option<std::time::Duration>, __wasi_errno_t> {
        match &self.kind {
            InodeSocketKind::TcpStream(sock) => sock.opt_time(ty).map_err(net_error_into_wasi_err),
            InodeSocketKind::TcpListener(sock) => match ty {
                TimeType::AcceptTimeout => sock.timeout().map_err(net_error_into_wasi_err),
                _ => Err(__WASI_EINVAL),
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
                _ => Err(__WASI_EINVAL),
            },
            InodeSocketKind::Closed => Err(__WASI_EIO),
            _ => Err(__WASI_ENOTSUP),
        }
    }

    pub fn set_ttl(&mut self, ttl: u32) -> Result<(), __wasi_errno_t> {
        match &mut self.kind {
            InodeSocketKind::TcpStream(sock) => sock.set_ttl(ttl).map_err(net_error_into_wasi_err),
            InodeSocketKind::UdpSocket(sock) => sock.set_ttl(ttl).map_err(net_error_into_wasi_err),
            InodeSocketKind::PreSocket { .. } => Err(__WASI_EIO),
            InodeSocketKind::Closed => Err(__WASI_EIO),
            _ => Err(__WASI_ENOTSUP),
        }
    }

    pub fn ttl(&self) -> Result<u32, __wasi_errno_t> {
        match &self.kind {
            InodeSocketKind::TcpStream(sock) => sock.ttl().map_err(net_error_into_wasi_err),
            InodeSocketKind::UdpSocket(sock) => sock.ttl().map_err(net_error_into_wasi_err),
            InodeSocketKind::PreSocket { .. } => Err(__WASI_EIO),
            InodeSocketKind::Closed => Err(__WASI_EIO),
            _ => Err(__WASI_ENOTSUP),
        }
    }

    pub fn set_multicast_ttl_v4(&mut self, ttl: u32) -> Result<(), __wasi_errno_t> {
        match &mut self.kind {
            InodeSocketKind::UdpSocket(sock) => sock
                .set_multicast_ttl_v4(ttl)
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::PreSocket { .. } => Err(__WASI_EIO),
            InodeSocketKind::Closed => Err(__WASI_EIO),
            _ => Err(__WASI_ENOTSUP),
        }
    }

    pub fn multicast_ttl_v4(&self) -> Result<u32, __wasi_errno_t> {
        match &self.kind {
            InodeSocketKind::UdpSocket(sock) => {
                sock.multicast_ttl_v4().map_err(net_error_into_wasi_err)
            }
            InodeSocketKind::PreSocket { .. } => Err(__WASI_EIO),
            InodeSocketKind::Closed => Err(__WASI_EIO),
            _ => Err(__WASI_ENOTSUP),
        }
    }

    pub fn join_multicast_v4(
        &mut self,
        multiaddr: Ipv4Addr,
        iface: Ipv4Addr,
    ) -> Result<(), __wasi_errno_t> {
        match &mut self.kind {
            InodeSocketKind::UdpSocket(sock) => sock
                .join_multicast_v4(multiaddr, iface)
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::PreSocket { .. } => Err(__WASI_EIO),
            InodeSocketKind::Closed => Err(__WASI_EIO),
            _ => Err(__WASI_ENOTSUP),
        }
    }

    pub fn leave_multicast_v4(
        &mut self,
        multiaddr: Ipv4Addr,
        iface: Ipv4Addr,
    ) -> Result<(), __wasi_errno_t> {
        match &mut self.kind {
            InodeSocketKind::UdpSocket(sock) => sock
                .leave_multicast_v4(multiaddr, iface)
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::PreSocket { .. } => Err(__WASI_EIO),
            InodeSocketKind::Closed => Err(__WASI_EIO),
            _ => Err(__WASI_ENOTSUP),
        }
    }

    pub fn join_multicast_v6(
        &mut self,
        multiaddr: Ipv6Addr,
        iface: u32,
    ) -> Result<(), __wasi_errno_t> {
        match &mut self.kind {
            InodeSocketKind::UdpSocket(sock) => sock
                .join_multicast_v6(multiaddr, iface)
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::PreSocket { .. } => Err(__WASI_EIO),
            InodeSocketKind::Closed => Err(__WASI_EIO),
            _ => Err(__WASI_ENOTSUP),
        }
    }

    pub fn leave_multicast_v6(
        &mut self,
        multiaddr: Ipv6Addr,
        iface: u32,
    ) -> Result<(), __wasi_errno_t> {
        match &mut self.kind {
            InodeSocketKind::UdpSocket(sock) => sock
                .leave_multicast_v6(multiaddr, iface)
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::PreSocket { .. } => Err(__WASI_EIO),
            InodeSocketKind::Closed => Err(__WASI_EIO),
            _ => Err(__WASI_ENOTSUP),
        }
    }

    pub fn send<M: MemorySize>(
        &mut self,
        memory: &Memory,
        iov: WasmSlice<__wasi_ciovec_t<M>>,
    ) -> Result<usize, __wasi_errno_t> {
        let buf_len: M::Offset = iov
            .iter()
            .filter_map(|a| a.read().ok())
            .map(|a| a.buf_len)
            .sum();
        let buf_len: usize = buf_len.try_into().map_err(|_| __WASI_EINVAL)?;
        let mut buf = Vec::with_capacity(buf_len);
        write_bytes(&mut buf, memory, iov)?;
        match &mut self.kind {
            InodeSocketKind::HttpRequest(sock, ty) => {
                let sock = sock.get_mut().unwrap();
                match ty {
                    InodeHttpSocketType::Request => {
                        if sock.request.is_none() {
                            return Err(__WASI_EIO);
                        }
                        let request = sock.request.as_ref().unwrap();
                        request.send(buf).map(|_| buf_len).map_err(|_| __WASI_EIO)
                    }
                    _ => {
                        return Err(__WASI_EIO);
                    }
                }
            }
            InodeSocketKind::WebSocket(sock) => sock
                .send(Bytes::from(buf))
                .map(|_| buf_len)
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::Raw(sock) => {
                sock.send(Bytes::from(buf)).map_err(net_error_into_wasi_err)
            }
            InodeSocketKind::TcpStream(sock) => {
                sock.send(Bytes::from(buf)).map_err(net_error_into_wasi_err)
            }
            InodeSocketKind::UdpSocket(sock) => {
                sock.send(Bytes::from(buf)).map_err(net_error_into_wasi_err)
            }
            InodeSocketKind::PreSocket { .. } => Err(__WASI_ENOTCONN),
            InodeSocketKind::Closed => Err(__WASI_EIO),
            _ => Err(__WASI_ENOTSUP),
        }
        .map(|_| buf_len)
    }

    pub fn send_bytes<M: MemorySize>(&mut self, buf: Bytes) -> Result<usize, __wasi_errno_t> {
        let buf_len = buf.len();
        match &mut self.kind {
            InodeSocketKind::HttpRequest(sock, ty) => {
                let sock = sock.get_mut().unwrap();
                match ty {
                    InodeHttpSocketType::Request => {
                        if sock.request.is_none() {
                            return Err(__WASI_EIO);
                        }
                        let request = sock.request.as_ref().unwrap();
                        request
                            .send(buf.to_vec())
                            .map(|_| buf_len)
                            .map_err(|_| __WASI_EIO)
                    }
                    _ => {
                        return Err(__WASI_EIO);
                    }
                }
            }
            InodeSocketKind::WebSocket(sock) => sock
                .send(buf)
                .map(|_| buf_len)
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::Raw(sock) => sock.send(buf).map_err(net_error_into_wasi_err),
            InodeSocketKind::TcpStream(sock) => sock.send(buf).map_err(net_error_into_wasi_err),
            InodeSocketKind::UdpSocket(sock) => sock.send(buf).map_err(net_error_into_wasi_err),
            InodeSocketKind::PreSocket { .. } => Err(__WASI_ENOTCONN),
            InodeSocketKind::Closed => Err(__WASI_EIO),
            _ => Err(__WASI_ENOTSUP),
        }
        .map(|_| buf_len)
    }

    pub fn send_to<M: MemorySize>(
        &mut self,
        memory: &Memory,
        iov: WasmSlice<__wasi_ciovec_t<M>>,
        addr: WasmPtr<__wasi_addr_port_t, M>,
    ) -> Result<usize, __wasi_errno_t> {
        let (addr_ip, addr_port) = read_ip_port(memory, addr)?;
        let addr = SocketAddr::new(addr_ip, addr_port);
        let buf_len: M::Offset = iov
            .iter()
            .filter_map(|a| a.read().ok())
            .map(|a| a.buf_len)
            .sum();
        let buf_len: usize = buf_len.try_into().map_err(|_| __WASI_EINVAL)?;
        let mut buf = Vec::with_capacity(buf_len);
        write_bytes(&mut buf, memory, iov)?;
        match &mut self.kind {
            InodeSocketKind::Icmp(sock) => sock
                .send_to(Bytes::from(buf), addr)
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::UdpSocket(sock) => sock
                .send_to(Bytes::from(buf), addr)
                .map_err(net_error_into_wasi_err),
            InodeSocketKind::PreSocket { .. } => Err(__WASI_ENOTCONN),
            InodeSocketKind::Closed => Err(__WASI_EIO),
            _ => Err(__WASI_ENOTSUP),
        }
        .map(|_| buf_len)
    }

    pub fn recv<M: MemorySize>(
        &mut self,
        memory: &Memory,
        iov: WasmSlice<__wasi_iovec_t<M>>,
    ) -> Result<usize, __wasi_errno_t> {
        loop {
            if let Some(buf) = self.read_buffer.as_mut() {
                let buf_len = buf.len();
                if buf_len > 0 {
                    let reader = buf.as_ref();
                    let read = read_bytes(reader, memory, iov).map(|_| buf_len)?;
                    if let InodeSocketKind::TcpStream(..) = &self.kind {
                        buf.advance(read);
                    } else {
                        buf.clear();
                    }
                    return Ok(read);
                }
            }
            let data = match &mut self.kind {
                InodeSocketKind::HttpRequest(sock, ty) => {
                    let sock = sock.get_mut().unwrap();
                    match ty {
                        InodeHttpSocketType::Response => {
                            if sock.response.is_none() {
                                return Err(__WASI_EIO);
                            }
                            let response = sock.response.as_ref().unwrap();
                            Bytes::from(response.recv().map_err(|_| __WASI_EIO)?)
                        }
                        InodeHttpSocketType::Headers => {
                            if sock.headers.is_none() {
                                return Err(__WASI_EIO);
                            }
                            let headers = sock.headers.as_ref().unwrap();
                            let headers = headers.recv().map_err(|_| __WASI_EIO)?;
                            let headers = format!("{}: {}", headers.0, headers.1);
                            Bytes::from(headers.as_bytes().to_vec())
                        }
                        _ => {
                            return Err(__WASI_EIO);
                        }
                    }
                }
                InodeSocketKind::WebSocket(sock) => {
                    let read = sock.recv().map_err(net_error_into_wasi_err)?;
                    read.data
                }
                InodeSocketKind::Raw(sock) => {
                    let read = sock.recv().map_err(net_error_into_wasi_err)?;
                    read.data
                }
                InodeSocketKind::TcpStream(sock) => {
                    let read = sock.recv().map_err(net_error_into_wasi_err)?;
                    read.data
                }
                InodeSocketKind::UdpSocket(sock) => {
                    let read = sock.recv().map_err(net_error_into_wasi_err)?;
                    read.data
                }
                InodeSocketKind::PreSocket { .. } => return Err(__WASI_ENOTCONN),
                InodeSocketKind::Closed => return Err(__WASI_EIO),
                _ => return Err(__WASI_ENOTSUP),
            };
            self.read_buffer.replace(data);
            self.read_addr.take();
        }
    }

    pub fn recv_from<M: MemorySize>(
        &mut self,
        memory: &Memory,
        iov: WasmSlice<__wasi_iovec_t<M>>,
        addr: WasmPtr<__wasi_addr_port_t, M>,
    ) -> Result<usize, __wasi_errno_t> {
        loop {
            if let Some(buf) = self.read_buffer.as_mut() {
                if !buf.is_empty() {
                    let reader = buf.as_ref();
                    let ret = read_bytes(reader, memory, iov)?;
                    let peer = self
                        .read_addr
                        .unwrap_or_else(|| SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0));
                    write_ip_port(memory, addr, peer.ip(), peer.port())?;
                    return Ok(ret);
                }
            }
            let rcv = match &mut self.kind {
                InodeSocketKind::Icmp(sock) => sock.recv_from().map_err(net_error_into_wasi_err)?,
                InodeSocketKind::UdpSocket(sock) => {
                    sock.recv_from().map_err(net_error_into_wasi_err)?
                }
                InodeSocketKind::PreSocket { .. } => return Err(__WASI_ENOTCONN),
                InodeSocketKind::Closed => return Err(__WASI_EIO),
                _ => return Err(__WASI_ENOTSUP),
            };
            self.read_buffer.replace(rcv.data);
            self.read_addr.replace(rcv.addr);
        }
    }

    pub fn shutdown(&mut self, how: std::net::Shutdown) -> Result<(), __wasi_errno_t> {
        use std::net::Shutdown;
        match &mut self.kind {
            InodeSocketKind::TcpStream(sock) => {
                sock.shutdown(how).map_err(net_error_into_wasi_err)?;
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
            InodeSocketKind::PreSocket { .. } => return Err(__WASI_ENOTCONN),
            InodeSocketKind::Closed => return Err(__WASI_EIO),
            _ => return Err(__WASI_ENOTSUP),
        }
        Ok(())
    }
}

impl Read for InodeSocket {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            if let Some(read_buf) = self.read_buffer.as_mut() {
                let buf_len = read_buf.len();
                if buf_len > 0 {
                    let mut reader = read_buf.as_ref();
                    let read = reader.read(buf)?;
                    read_buf.advance(read);
                    return Ok(read);
                }
            }
            let data = match &mut self.kind {
                InodeSocketKind::HttpRequest(sock, ty) => {
                    let sock = sock.get_mut().unwrap();
                    match ty {
                        InodeHttpSocketType::Response => {
                            if sock.response.is_none() {
                                return Err(io::Error::new(
                                    io::ErrorKind::BrokenPipe,
                                    "the socket is not connected".to_string(),
                                ));
                            }
                            let response = sock.response.as_ref().unwrap();
                            Bytes::from(response.recv().map_err(|_| {
                                io::Error::new(
                                    io::ErrorKind::BrokenPipe,
                                    "the wasi pipe is not connected".to_string(),
                                )
                            })?)
                        }
                        InodeHttpSocketType::Headers => {
                            if sock.headers.is_none() {
                                return Err(io::Error::new(
                                    io::ErrorKind::BrokenPipe,
                                    "the socket is not connected".to_string(),
                                ));
                            }
                            let headers = sock.headers.as_ref().unwrap();
                            let headers = headers.recv().map_err(|_| {
                                io::Error::new(
                                    io::ErrorKind::BrokenPipe,
                                    "the wasi pipe is not connected".to_string(),
                                )
                            })?;
                            let headers = format!("{}: {}", headers.0, headers.1);
                            Bytes::from(headers.as_bytes().to_vec())
                        }
                        _ => {
                            return Err(io::Error::new(
                                io::ErrorKind::Unsupported,
                                "the socket is of an unsupported type".to_string(),
                            ));
                        }
                    }
                }
                InodeSocketKind::WebSocket(sock) => {
                    let read = sock.recv().map_err(net_error_into_io_err)?;
                    read.data
                }
                InodeSocketKind::Raw(sock) => {
                    let read = sock.recv().map_err(net_error_into_io_err)?;
                    read.data
                }
                InodeSocketKind::TcpStream(sock) => {
                    let read = sock.recv().map_err(net_error_into_io_err)?;
                    read.data
                }
                InodeSocketKind::UdpSocket(sock) => {
                    let read = sock.recv().map_err(net_error_into_io_err)?;
                    read.data
                }
                InodeSocketKind::PreSocket { .. } => {
                    return Err(io::Error::new(
                        io::ErrorKind::NotConnected,
                        "the socket is not connected".to_string(),
                    ))
                }
                InodeSocketKind::Closed => {
                    return Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "the socket has been closed".to_string(),
                    ))
                }
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "the socket type is not supported".to_string(),
                    ))
                }
            };
            self.read_buffer.replace(data);
            self.read_addr.take();
        }
    }
}

impl Drop for InodeSocket {
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
    memory: &Memory,
    ptr: WasmPtr<__wasi_addr_t, M>,
) -> Result<IpAddr, __wasi_errno_t> {
    let addr_ptr = ptr.deref(memory);
    let addr = addr_ptr.read().map_err(crate::mem_error_to_wasi)?;

    let o = addr.u.octs;
    Ok(match addr.tag {
        __WASI_ADDRESS_FAMILY_INET4 => IpAddr::V4(Ipv4Addr::new(o[0], o[1], o[2], o[3])),
        __WASI_ADDRESS_FAMILY_INET6 => {
            let [a, b, c, d, e, f, g, h] = unsafe { transmute::<_, [u16; 8]>(o) };
            IpAddr::V6(Ipv6Addr::new(a, b, c, d, e, f, g, h))
        }
        _ => return Err(__WASI_EINVAL),
    })
}

pub(crate) fn read_ip_v4<M: MemorySize>(
    memory: &Memory,
    ptr: WasmPtr<__wasi_addr_ip4_t, M>,
) -> Result<Ipv4Addr, __wasi_errno_t> {
    let addr_ptr = ptr.deref(memory);
    let addr = addr_ptr.read().map_err(crate::mem_error_to_wasi)?;

    let o = addr.octs;
    Ok(Ipv4Addr::new(o[0], o[1], o[2], o[3]))
}

pub(crate) fn read_ip_v6<M: MemorySize>(
    memory: &Memory,
    ptr: WasmPtr<__wasi_addr_ip6_t, M>,
) -> Result<Ipv6Addr, __wasi_errno_t> {
    let addr_ptr = ptr.deref(memory);
    let addr = addr_ptr.read().map_err(crate::mem_error_to_wasi)?;

    let [a, b, c, d, e, f, g, h] = unsafe { transmute::<_, [u16; 8]>(addr.segs) };
    Ok(Ipv6Addr::new(a, b, c, d, e, f, g, h))
}

pub(crate) fn write_ip<M: MemorySize>(
    memory: &Memory,
    ptr: WasmPtr<__wasi_addr_t, M>,
    ip: IpAddr,
) -> Result<(), __wasi_errno_t> {
    let ip = match ip {
        IpAddr::V4(ip) => {
            let o = ip.octets();
            __wasi_addr_t {
                tag: __WASI_ADDRESS_FAMILY_INET4,
                u: __wasi_addr_u {
                    octs: [o[0], o[1], o[2], o[3], 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                },
            }
        }
        IpAddr::V6(ip) => {
            let o = ip.octets();
            __wasi_addr_t {
                tag: __WASI_ADDRESS_FAMILY_INET6,
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
    memory: &Memory,
    ptr: WasmPtr<__wasi_cidr_t, M>,
) -> Result<IpCidr, __wasi_errno_t> {
    let addr_ptr = ptr.deref(memory);
    let addr = addr_ptr.read().map_err(crate::mem_error_to_wasi)?;

    let o = addr.u.octs;
    Ok(match addr.tag {
        __WASI_ADDRESS_FAMILY_INET4 => IpCidr {
            ip: IpAddr::V4(Ipv4Addr::new(o[0], o[1], o[2], o[3])),
            prefix: o[4],
        },
        __WASI_ADDRESS_FAMILY_INET6 => {
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
        _ => return Err(__WASI_EINVAL),
    })
}

#[allow(dead_code)]
pub(crate) fn write_cidr<M: MemorySize>(
    memory: &Memory,
    ptr: WasmPtr<__wasi_cidr_t, M>,
    cidr: IpCidr,
) -> Result<(), __wasi_errno_t> {
    let p = cidr.prefix;
    let cidr = match cidr.ip {
        IpAddr::V4(ip) => {
            let o = ip.octets();
            __wasi_cidr_t {
                tag: __WASI_ADDRESS_FAMILY_INET4,
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
                tag: __WASI_ADDRESS_FAMILY_INET6,
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
    memory: &Memory,
    ptr: WasmPtr<__wasi_addr_port_t, M>,
) -> Result<(IpAddr, u16), __wasi_errno_t> {
    let addr_ptr = ptr.deref(memory);
    let addr = addr_ptr.read().map_err(crate::mem_error_to_wasi)?;

    let o = addr.u.octs;
    Ok(match addr.tag {
        __WASI_ADDRESS_FAMILY_INET4 => {
            let port = u16::from_ne_bytes([o[0], o[1]]);
            (IpAddr::V4(Ipv4Addr::new(o[2], o[3], o[4], o[5])), port)
        }
        __WASI_ADDRESS_FAMILY_INET6 => {
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
        _ => return Err(__WASI_EINVAL),
    })
}

#[allow(dead_code)]
pub(crate) fn write_ip_port<M: MemorySize>(
    memory: &Memory,
    ptr: WasmPtr<__wasi_addr_port_t, M>,
    ip: IpAddr,
    port: u16,
) -> Result<(), __wasi_errno_t> {
    let p = port.to_be_bytes();
    let ipport = match ip {
        IpAddr::V4(ip) => {
            let o = ip.octets();
            __wasi_addr_port_t {
                tag: __WASI_ADDRESS_FAMILY_INET4,
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
                tag: __WASI_ADDRESS_FAMILY_INET6,
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
    memory: &Memory,
    ptr: WasmPtr<__wasi_route_t, M>,
) -> Result<IpRoute, __wasi_errno_t> {
    let route_ptr = ptr.deref(memory);
    let route = route_ptr.read().map_err(crate::mem_error_to_wasi)?;

    Ok(IpRoute {
        cidr: {
            let o = route.cidr.u.octs;
            match route.cidr.tag {
                __WASI_ADDRESS_FAMILY_INET4 => IpCidr {
                    ip: IpAddr::V4(Ipv4Addr::new(o[0], o[1], o[2], o[3])),
                    prefix: o[4],
                },
                __WASI_ADDRESS_FAMILY_INET6 => {
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
                _ => return Err(__WASI_EINVAL),
            }
        },
        via_router: {
            let o = route.via_router.u.octs;
            match route.via_router.tag {
                __WASI_ADDRESS_FAMILY_INET4 => IpAddr::V4(Ipv4Addr::new(o[0], o[1], o[2], o[3])),
                __WASI_ADDRESS_FAMILY_INET6 => {
                    let [a, b, c, d, e, f, g, h] = unsafe { transmute::<_, [u16; 8]>(o) };
                    IpAddr::V6(Ipv6Addr::new(a, b, c, d, e, f, g, h))
                }
                _ => return Err(__WASI_EINVAL),
            }
        },
        preferred_until: match route.preferred_until.tag {
            __WASI_OPTION_NONE => None,
            __WASI_OPTION_SOME => Some(Duration::from_nanos(route.preferred_until.u)),
            _ => return Err(__WASI_EINVAL),
        },
        expires_at: match route.expires_at.tag {
            __WASI_OPTION_NONE => None,
            __WASI_OPTION_SOME => Some(Duration::from_nanos(route.expires_at.u)),
            _ => return Err(__WASI_EINVAL),
        },
    })
}

pub(crate) fn write_route<M: MemorySize>(
    memory: &Memory,
    ptr: WasmPtr<__wasi_route_t, M>,
    route: IpRoute,
) -> Result<(), __wasi_errno_t> {
    let cidr = {
        let p = route.cidr.prefix;
        match route.cidr.ip {
            IpAddr::V4(ip) => {
                let o = ip.octets();
                __wasi_cidr_t {
                    tag: __WASI_ADDRESS_FAMILY_INET4,
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
                    tag: __WASI_ADDRESS_FAMILY_INET6,
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
                tag: __WASI_ADDRESS_FAMILY_INET4,
                u: __wasi_addr_u {
                    octs: [o[0], o[1], o[2], o[3], 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                },
            }
        }
        IpAddr::V6(ip) => {
            let o = ip.octets();
            __wasi_addr_t {
                tag: __WASI_ADDRESS_FAMILY_INET6,
                u: __wasi_addr_u { octs: o },
            }
        }
    };
    let preferred_until = match route.preferred_until {
        None => __wasi_option_timestamp_t {
            tag: __WASI_OPTION_NONE,
            u: 0,
        },
        Some(u) => __wasi_option_timestamp_t {
            tag: __WASI_OPTION_SOME,
            u: u.as_nanos() as u64,
        },
    };
    let expires_at = match route.expires_at {
        None => __wasi_option_timestamp_t {
            tag: __WASI_OPTION_NONE,
            u: 0,
        },
        Some(u) => __wasi_option_timestamp_t {
            tag: __WASI_OPTION_SOME,
            u: u.as_nanos() as u64,
        },
    };

    let route = __wasi_route_t {
        cidr,
        via_router,
        preferred_until,
        expires_at,
    };

    let route_ptr = ptr.deref(memory);
    route_ptr.write(route).map_err(crate::mem_error_to_wasi)?;
    Ok(())
}

pub(crate) fn all_socket_rights() -> __wasi_rights_t {
    __WASI_RIGHT_FD_FDSTAT_SET_FLAGS
        | __WASI_RIGHT_FD_FILESTAT_GET
        | __WASI_RIGHT_FD_READ
        | __WASI_RIGHT_FD_WRITE
        | __WASI_RIGHT_POLL_FD_READWRITE
        | __WASI_RIGHT_SOCK_SHUTDOWN
        | __WASI_RIGHT_SOCK_CONNECT
        | __WASI_RIGHT_SOCK_LISTEN
        | __WASI_RIGHT_SOCK_BIND
        | __WASI_RIGHT_SOCK_ACCEPT
        | __WASI_RIGHT_SOCK_RECV
        | __WASI_RIGHT_SOCK_SEND
        | __WASI_RIGHT_SOCK_ADDR_LOCAL
        | __WASI_RIGHT_SOCK_ADDR_REMOTE
        | __WASI_RIGHT_SOCK_RECV_FROM
        | __WASI_RIGHT_SOCK_SEND_TO
}
