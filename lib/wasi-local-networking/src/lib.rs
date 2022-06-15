#![allow(unused_variables)]
use bytes::{Bytes, BytesMut};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr};
use std::time::Duration;
#[allow(unused_imports, dead_code)]
use tracing::{debug, error, info, trace, warn};
use wasmer_vnet::{
    io_err_into_net_error, IpCidr, IpRoute, NetworkError, Result, SocketHttpRequest, SocketReceive,
    SocketReceiveFrom, SocketStatus, StreamSecurity, TimeType, VirtualConnectedSocket,
    VirtualConnectionlessSocket, VirtualIcmpSocket, VirtualNetworking, VirtualRawSocket,
    VirtualSocket, VirtualTcpListener, VirtualTcpSocket, VirtualUdpSocket, VirtualWebSocket,
};

#[derive(Debug, Default)]
pub struct LocalNetworking {}

#[allow(unused_variables)]
impl VirtualNetworking for LocalNetworking {
    fn ws_connect(&self, url: &str) -> Result<Box<dyn VirtualWebSocket + Sync>> {
        Err(NetworkError::Unsupported)
    }

    fn http_request(
        &self,
        url: &str,
        method: &str,
        headers: &str,
        gzip: bool,
    ) -> Result<SocketHttpRequest> {
        Err(NetworkError::Unsupported)
    }

    fn bridge(&self, network: &str, access_token: &str, security: StreamSecurity) -> Result<()> {
        Err(NetworkError::Unsupported)
    }

    fn unbridge(&self) -> Result<()> {
        Err(NetworkError::Unsupported)
    }

    fn dhcp_acquire(&self) -> Result<Vec<IpAddr>> {
        Err(NetworkError::Unsupported)
    }

    fn ip_add(&self, ip: IpAddr, prefix: u8) -> Result<()> {
        Err(NetworkError::Unsupported)
    }

    fn ip_remove(&self, ip: IpAddr) -> Result<()> {
        Err(NetworkError::Unsupported)
    }

    fn ip_clear(&self) -> Result<()> {
        Err(NetworkError::Unsupported)
    }

    fn ip_list(&self) -> Result<Vec<IpCidr>> {
        Err(NetworkError::Unsupported)
    }

    fn mac(&self) -> Result<[u8; 6]> {
        Err(NetworkError::Unsupported)
    }

    fn gateway_set(&self, ip: IpAddr) -> Result<()> {
        Err(NetworkError::Unsupported)
    }

    fn route_add(
        &self,
        cidr: IpCidr,
        via_router: IpAddr,
        preferred_until: Option<Duration>,
        expires_at: Option<Duration>,
    ) -> Result<()> {
        Err(NetworkError::Unsupported)
    }

    fn route_remove(&self, cidr: IpAddr) -> Result<()> {
        Err(NetworkError::Unsupported)
    }

    fn route_clear(&self) -> Result<()> {
        Err(NetworkError::Unsupported)
    }

    fn route_list(&self) -> Result<Vec<IpRoute>> {
        Err(NetworkError::Unsupported)
    }

    fn bind_raw(&self) -> Result<Box<dyn VirtualRawSocket + Sync>> {
        Err(NetworkError::Unsupported)
    }

    fn listen_tcp(
        &self,
        addr: SocketAddr,
        only_v6: bool,
        reuse_port: bool,
        reuse_addr: bool,
    ) -> Result<Box<dyn VirtualTcpListener + Sync>> {
        let listener = std::net::TcpListener::bind(addr)
            .map(|sock| {
                Box::new(LocalTcpListener {
                    stream: sock,
                    timeout: None,
                })
            })
            .map_err(io_err_into_net_error)?;
        Ok(listener)
    }

    fn bind_udp(
        &self,
        addr: SocketAddr,
        _reuse_port: bool,
        _reuse_addr: bool,
    ) -> Result<Box<dyn VirtualUdpSocket + Sync>> {
        let socket = std::net::UdpSocket::bind(addr).map_err(io_err_into_net_error)?;
        Ok(Box::new(LocalUdpSocket(socket, addr)))
    }

    fn bind_icmp(&self, addr: IpAddr) -> Result<Box<dyn VirtualIcmpSocket + Sync>> {
        Err(NetworkError::Unsupported)
    }

    fn connect_tcp(
        &self,
        _addr: SocketAddr,
        peer: SocketAddr,
        timeout: Option<Duration>,
    ) -> Result<Box<dyn VirtualTcpSocket + Sync>> {
        let stream = if let Some(timeout) = timeout {
            std::net::TcpStream::connect_timeout(&peer, timeout)
        } else {
            std::net::TcpStream::connect(peer)
        }
        .map_err(io_err_into_net_error)?;
        let peer = stream.peer_addr().map_err(io_err_into_net_error)?;
        Ok(Box::new(LocalTcpStream {
            stream,
            addr: peer,
            connect_timeout: None,
        }))
    }

    fn resolve(
        &self,
        host: &str,
        port: Option<u16>,
        dns_server: Option<IpAddr>,
    ) -> Result<Vec<IpAddr>> {
        use std::net::ToSocketAddrs;
        Ok(if let Some(port) = port {
            let host = format!("{}:{}", host, port);
            host.to_socket_addrs()
                .map(|a| a.map(|a| a.ip()).collect::<Vec<_>>())
                .map_err(io_err_into_net_error)?
        } else {
            host.to_socket_addrs()
                .map(|a| a.map(|a| a.ip()).collect::<Vec<_>>())
                .map_err(io_err_into_net_error)?
        })
    }
}

#[derive(Debug)]
pub struct LocalTcpListener {
    stream: std::net::TcpListener,
    timeout: Option<Duration>,
}

impl VirtualTcpListener for LocalTcpListener {
    fn accept(&self) -> Result<(Box<dyn VirtualTcpSocket + Sync>, SocketAddr)> {
        if let Some(timeout) = &self.timeout {
            return self.accept_timeout(*timeout);
        }
        let (sock, addr) = self
            .stream
            .accept()
            .map(|(sock, addr)| {
                (
                    Box::new(LocalTcpStream {
                        stream: sock,
                        addr,
                        connect_timeout: None,
                    }),
                    addr,
                )
            })
            .map_err(io_err_into_net_error)?;
        Ok((sock, addr))
    }

    #[cfg(feature = "wasix")]
    fn accept_timeout(
        &self,
        timeout: Duration,
    ) -> Result<(Box<dyn VirtualTcpSocket + Sync>, SocketAddr)> {
        let (sock, addr) = self
            .stream
            .accept_timeout(timeout)
            .map(|(sock, addr)| {
                (
                    Box::new(LocalTcpStream {
                        stream: sock,
                        addr: addr.clone(),
                        connect_timeout: None,
                    }),
                    addr,
                )
            })
            .map_err(io_err_into_net_error)?;
        Ok((sock, addr))
    }

    #[cfg(not(feature = "wasix"))]
    fn accept_timeout(
        &self,
        _timeout: Duration,
    ) -> Result<(Box<dyn VirtualTcpSocket + Sync>, SocketAddr)> {
        self.accept()
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
    stream: std::net::TcpStream,
    addr: SocketAddr,
    connect_timeout: Option<Duration>,
}

impl VirtualTcpSocket for LocalTcpStream {
    fn set_opt_time(&mut self, ty: TimeType, timeout: Option<Duration>) -> Result<()> {
        match ty {
            TimeType::ReadTimeout => self
                .stream
                .set_read_timeout(timeout)
                .map_err(io_err_into_net_error),
            TimeType::WriteTimeout => self
                .stream
                .set_write_timeout(timeout)
                .map_err(io_err_into_net_error),
            TimeType::ConnectTimeout => {
                self.connect_timeout = timeout;
                Ok(())
            }
            #[cfg(feature = "wasix")]
            TimeType::Linger => self
                .stream
                .set_linger(timeout)
                .map_err(io_err_into_net_error),
            _ => Err(NetworkError::InvalidInput),
        }
    }

    fn opt_time(&self, ty: TimeType) -> Result<Option<Duration>> {
        match ty {
            TimeType::ReadTimeout => self.stream.read_timeout().map_err(io_err_into_net_error),
            TimeType::WriteTimeout => self.stream.write_timeout().map_err(io_err_into_net_error),
            TimeType::ConnectTimeout => Ok(self.connect_timeout),
            #[cfg(feature = "wasix")]
            TimeType::Linger => self.stream.linger().map_err(io_err_into_net_error),
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

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }

    fn shutdown(&mut self, how: Shutdown) -> Result<()> {
        self.stream.shutdown(how).map_err(io_err_into_net_error)
    }
}

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

    fn send(&mut self, data: Bytes) -> Result<usize> {
        self.stream
            .write_all(&data[..])
            .map(|_| data.len())
            .map_err(io_err_into_net_error)
    }

    fn flush(&mut self) -> Result<()> {
        self.stream.flush().map_err(io_err_into_net_error)
    }

    fn recv(&mut self) -> Result<SocketReceive> {
        let buf_size = 8192;
        let mut buf = BytesMut::with_capacity(buf_size);
        let read = self
            .stream
            .read(&mut buf[..])
            .map_err(io_err_into_net_error)?;
        let buf = Bytes::from(buf).slice(..read);
        Ok(SocketReceive {
            data: buf,
            truncated: read == buf_size,
        })
    }

    fn peek(&mut self) -> Result<SocketReceive> {
        let buf_size = 8192;
        let mut buf = BytesMut::with_capacity(buf_size);
        let read = self
            .stream
            .peek(&mut buf[..])
            .map_err(io_err_into_net_error)?;
        let buf = Bytes::from(buf).slice(..read);
        Ok(SocketReceive {
            data: buf,
            truncated: read == buf_size,
        })
    }
}

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
}

#[derive(Debug)]
pub struct LocalUdpSocket(std::net::UdpSocket, SocketAddr);

impl VirtualUdpSocket for LocalUdpSocket {
    fn connect(&mut self, addr: SocketAddr) -> Result<()> {
        self.0.connect(addr).map_err(io_err_into_net_error)
    }

    fn set_broadcast(&mut self, broadcast: bool) -> Result<()> {
        self.0
            .set_broadcast(broadcast)
            .map_err(io_err_into_net_error)
    }

    fn broadcast(&self) -> Result<bool> {
        self.0.broadcast().map_err(io_err_into_net_error)
    }

    fn set_multicast_loop_v4(&mut self, val: bool) -> Result<()> {
        self.0
            .set_multicast_loop_v4(val)
            .map_err(io_err_into_net_error)
    }

    fn multicast_loop_v4(&self) -> Result<bool> {
        self.0.multicast_loop_v4().map_err(io_err_into_net_error)
    }

    fn set_multicast_loop_v6(&mut self, val: bool) -> Result<()> {
        self.0
            .set_multicast_loop_v6(val)
            .map_err(io_err_into_net_error)
    }

    fn multicast_loop_v6(&self) -> Result<bool> {
        self.0.multicast_loop_v6().map_err(io_err_into_net_error)
    }

    fn set_multicast_ttl_v4(&mut self, ttl: u32) -> Result<()> {
        self.0
            .set_multicast_ttl_v4(ttl)
            .map_err(io_err_into_net_error)
    }

    fn multicast_ttl_v4(&self) -> Result<u32> {
        self.0.multicast_ttl_v4().map_err(io_err_into_net_error)
    }

    fn join_multicast_v4(&mut self, multiaddr: Ipv4Addr, iface: Ipv4Addr) -> Result<()> {
        self.0
            .join_multicast_v4(&multiaddr, &iface)
            .map_err(io_err_into_net_error)
    }

    fn leave_multicast_v4(&mut self, multiaddr: Ipv4Addr, iface: Ipv4Addr) -> Result<()> {
        self.0
            .leave_multicast_v4(&multiaddr, &iface)
            .map_err(io_err_into_net_error)
    }

    fn join_multicast_v6(&mut self, multiaddr: Ipv6Addr, iface: u32) -> Result<()> {
        self.0
            .join_multicast_v6(&multiaddr, iface)
            .map_err(io_err_into_net_error)
    }

    fn leave_multicast_v6(&mut self, multiaddr: Ipv6Addr, iface: u32) -> Result<()> {
        self.0
            .leave_multicast_v6(&multiaddr, iface)
            .map_err(io_err_into_net_error)
    }

    fn addr_peer(&self) -> Result<Option<SocketAddr>> {
        self.0.peer_addr().map(Some).map_err(io_err_into_net_error)
    }
}

impl VirtualConnectedSocket for LocalUdpSocket {
    fn set_linger(&mut self, linger: Option<Duration>) -> Result<()> {
        Err(NetworkError::Unsupported)
    }

    fn linger(&self) -> Result<Option<Duration>> {
        Err(NetworkError::Unsupported)
    }

    fn send(&mut self, data: Bytes) -> Result<usize> {
        self.0.send(&data[..]).map_err(io_err_into_net_error)
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }

    fn recv(&mut self) -> Result<SocketReceive> {
        let buf_size = 8192;
        let mut buf = BytesMut::with_capacity(buf_size);
        let read = self.0.recv(&mut buf[..]).map_err(io_err_into_net_error)?;
        let buf = Bytes::from(buf).slice(..read);
        Ok(SocketReceive {
            data: buf,
            truncated: read == buf_size,
        })
    }

    fn peek(&mut self) -> Result<SocketReceive> {
        let buf_size = 8192;
        let mut buf = BytesMut::with_capacity(buf_size);
        let read = self.0.peek(&mut buf[..]).map_err(io_err_into_net_error)?;
        let buf = Bytes::from(buf).slice(..read);
        Ok(SocketReceive {
            data: buf,
            truncated: read == buf_size,
        })
    }
}

impl VirtualConnectionlessSocket for LocalUdpSocket {
    fn send_to(&mut self, data: Bytes, addr: SocketAddr) -> Result<usize> {
        self.0
            .send_to(&data[..], addr)
            .map_err(io_err_into_net_error)
    }

    fn recv_from(&mut self) -> Result<SocketReceiveFrom> {
        let buf_size = 8192;
        let mut buf = BytesMut::with_capacity(buf_size);
        let (read, peer) = self
            .0
            .recv_from(&mut buf[..])
            .map_err(io_err_into_net_error)?;
        let buf = Bytes::from(buf).slice(..read);
        Ok(SocketReceiveFrom {
            data: buf,
            truncated: read == buf_size,
            addr: peer,
        })
    }

    fn peek_from(&mut self) -> Result<SocketReceiveFrom> {
        let buf_size = 8192;
        let mut buf = BytesMut::with_capacity(buf_size);
        let (read, peer) = self
            .0
            .peek_from(&mut buf[..])
            .map_err(io_err_into_net_error)?;
        let buf = Bytes::from(buf).slice(..read);
        Ok(SocketReceiveFrom {
            data: buf,
            truncated: read == buf_size,
            addr: peer,
        })
    }
}

impl VirtualSocket for LocalUdpSocket {
    fn set_ttl(&mut self, ttl: u32) -> Result<()> {
        self.0.set_ttl(ttl).map_err(io_err_into_net_error)
    }

    fn ttl(&self) -> Result<u32> {
        self.0.ttl().map_err(io_err_into_net_error)
    }

    fn addr_local(&self) -> Result<SocketAddr> {
        self.0.local_addr().map_err(io_err_into_net_error)
    }

    fn status(&self) -> Result<SocketStatus> {
        Ok(SocketStatus::Opened)
    }
}
