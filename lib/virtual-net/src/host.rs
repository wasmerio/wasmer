#![allow(unused_variables)]
use crate::{io_err_into_net_error, VirtualIoSource};
#[allow(unused_imports)]
use crate::{
    IpCidr, IpRoute, NetworkError, Result, SocketStatus, StreamSecurity, VirtualConnectedSocket,
    VirtualConnectionlessSocket, VirtualIcmpSocket, VirtualNetworking, VirtualRawSocket,
    VirtualSocket, VirtualTcpListener, VirtualTcpSocket, VirtualUdpSocket,
};
use derivative::Derivative;
use std::io::{Read, Write};
use std::mem::MaybeUninit;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Handle;
#[allow(unused_imports, dead_code)]
use tracing::{debug, error, info, trace, warn};
use virtual_mio::{InterestGuard, InterestHandler, Selector};

#[derive(Derivative)]
#[derivative(Debug)]
pub struct LocalNetworking {
    selector: Arc<Selector>,
    handle: Handle,
}

impl LocalNetworking {
    pub fn new() -> Self {
        Self {
            selector: Selector::new(),
            handle: Handle::current(),
        }
    }
}

impl Drop for LocalNetworking {
    fn drop(&mut self) {
        self.selector.shutdown();
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
        let listener = std::net::TcpListener::bind(addr)
            .map(|sock| {
                sock.set_nonblocking(true).ok();
                Box::new(LocalTcpListener {
                    stream: mio::net::TcpListener::from_std(sock),
                    selector: self.selector.clone(),
                    handler_guard: None,
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
        let socket = mio::net::UdpSocket::bind(addr).map_err(io_err_into_net_error)?;
        socket2::SockRef::from(&socket).set_nonblocking(true).ok();
        Ok(Box::new(LocalUdpSocket {
            selector: self.selector.clone(),
            socket,
            addr,
            handler_guard: None,
        }))
    }

    async fn connect_tcp(
        &self,
        _addr: SocketAddr,
        mut peer: SocketAddr,
    ) -> Result<Box<dyn VirtualTcpSocket + Sync>> {
        let stream = mio::net::TcpStream::connect(peer).map_err(io_err_into_net_error)?;
        socket2::SockRef::from(&stream).set_nonblocking(true).ok();
        if let Ok(p) = stream.peer_addr() {
            peer = p;
        }
        Ok(Box::new(LocalTcpStream::new(
            self.selector.clone(),
            stream,
            peer,
        )))
    }

    async fn resolve(
        &self,
        host: &str,
        port: Option<u16>,
        dns_server: Option<IpAddr>,
    ) -> Result<Vec<IpAddr>> {
        let host_to_lookup = if host.contains(':') {
            host.to_string()
        } else {
            format!("{}:{}", host, port.unwrap_or(0))
        };
        self.handle
            .spawn(tokio::net::lookup_host(host_to_lookup))
            .await
            .map_err(|_| NetworkError::IOError)?
            .map(|a| a.map(|a| a.ip()).collect::<Vec<_>>())
            .map_err(io_err_into_net_error)
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct LocalTcpListener {
    stream: mio::net::TcpListener,
    selector: Arc<Selector>,
    handler_guard: Option<InterestGuard>,
}

impl VirtualTcpListener for LocalTcpListener {
    fn try_accept(&mut self) -> Result<(Box<dyn VirtualTcpSocket + Sync>, SocketAddr)> {
        match self.stream.accept().map_err(io_err_into_net_error) {
            Ok((stream, addr)) => {
                socket2::SockRef::from(&self.stream)
                    .set_nonblocking(true)
                    .ok();
                let mut socket = LocalTcpStream::new(self.selector.clone(), stream, addr);
                socket.set_first_handler_writeable();
                Ok((Box::new(socket), addr))
            }
            Err(NetworkError::WouldBlock) => Err(NetworkError::WouldBlock),
            Err(err) => Err(err),
        }
    }

    fn set_handler(&mut self, handler: Box<dyn InterestHandler + Send + Sync>) -> Result<()> {
        if let Some(guard) = self.handler_guard.take() {
            InterestGuard::unregister(guard, &self.selector, &mut self.stream);
        }

        let guard = InterestGuard::new(
            &self.selector,
            handler,
            &mut self.stream,
            mio::Interest::READABLE,
        )
        .map_err(io_err_into_net_error)?;

        self.handler_guard.replace(guard);

        Ok(())
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

impl VirtualIoSource for LocalTcpListener {
    fn remove_handler(&mut self) {
        if let Some(guard) = self.handler_guard.take() {
            InterestGuard::unregister(guard, &self.selector, &mut self.stream);
        }
    }
}

#[derive(Debug)]
pub struct LocalTcpStream {
    stream: mio::net::TcpStream,
    addr: SocketAddr,
    shutdown: Option<Shutdown>,
    selector: Arc<Selector>,
    handler_guard: Option<InterestGuard>,
    first_handler_writeable: bool,
}

impl LocalTcpStream {
    fn new(selector: Arc<Selector>, stream: mio::net::TcpStream, addr: SocketAddr) -> Self {
        Self {
            stream,
            addr,
            shutdown: None,
            selector,
            handler_guard: None,
            first_handler_writeable: false,
        }
    }
    fn set_first_handler_writeable(&mut self) {
        self.first_handler_writeable = true;
    }
}

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
        self.stream.shutdown(how).map_err(io_err_into_net_error)?;
        self.shutdown = Some(how);
        Ok(())
    }

    fn is_closed(&self) -> bool {
        false
    }
}

impl VirtualConnectedSocket for LocalTcpStream {
    fn set_linger(&mut self, linger: Option<Duration>) -> Result<()> {
        socket2::SockRef::from(&self.stream)
            .set_linger(linger)
            .map_err(io_err_into_net_error)?;
        Ok(())
    }

    fn linger(&self) -> Result<Option<Duration>> {
        socket2::SockRef::from(&self.stream)
            .linger()
            .map_err(io_err_into_net_error)
    }

    fn try_send(&mut self, data: &[u8]) -> Result<usize> {
        self.stream.write(data).map_err(io_err_into_net_error)
    }

    fn try_flush(&mut self) -> Result<()> {
        self.stream.flush().map_err(io_err_into_net_error)
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn try_recv(&mut self, buf: &mut [MaybeUninit<u8>]) -> Result<usize> {
        let buf: &mut [u8] = unsafe { std::mem::transmute(buf) };
        self.stream.read(buf).map_err(io_err_into_net_error)
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

    fn set_handler(&mut self, mut handler: Box<dyn InterestHandler + Send + Sync>) -> Result<()> {
        if let Some(guard) = self.handler_guard.take() {
            InterestGuard::unregister(guard, &self.selector, &mut self.stream);
        }

        if self.first_handler_writeable {
            self.first_handler_writeable = false;
            handler.interest(virtual_mio::InterestType::Writable);
        }

        let guard = InterestGuard::new(
            &self.selector,
            handler,
            &mut self.stream,
            mio::Interest::READABLE.add(mio::Interest::WRITABLE),
        )
        .map_err(io_err_into_net_error)?;

        self.handler_guard.replace(guard);

        Ok(())
    }
}

impl VirtualIoSource for LocalTcpStream {
    fn remove_handler(&mut self) {
        if let Some(guard) = self.handler_guard.take() {
            InterestGuard::unregister(guard, &self.selector, &mut self.stream);
        }
    }
}

#[derive(Debug)]
pub struct LocalUdpSocket {
    socket: mio::net::UdpSocket,
    #[allow(dead_code)]
    addr: SocketAddr,
    selector: Arc<Selector>,
    handler_guard: Option<InterestGuard>,
}

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
        socket2::SockRef::from(&self.socket)
            .join_multicast_v4(&multiaddr, &iface)
            .map_err(io_err_into_net_error)
    }

    fn leave_multicast_v4(&mut self, multiaddr: Ipv4Addr, iface: Ipv4Addr) -> Result<()> {
        socket2::SockRef::from(&self.socket)
            .leave_multicast_v4(&multiaddr, &iface)
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
    fn try_send_to(&mut self, data: &[u8], addr: SocketAddr) -> Result<usize> {
        self.socket
            .send_to(data, addr)
            .map_err(io_err_into_net_error)
    }

    fn try_recv_from(&mut self, buf: &mut [MaybeUninit<u8>]) -> Result<(usize, SocketAddr)> {
        let buf: &mut [u8] = unsafe { std::mem::transmute(buf) };
        self.socket.recv_from(buf).map_err(io_err_into_net_error)
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

    fn set_handler(&mut self, handler: Box<dyn InterestHandler + Send + Sync>) -> Result<()> {
        if let Some(guard) = self.handler_guard.take() {
            InterestGuard::unregister(guard, &self.selector, &mut self.socket);
        }

        let guard = InterestGuard::new(
            &self.selector,
            handler,
            &mut self.socket,
            mio::Interest::READABLE.add(mio::Interest::WRITABLE),
        )
        .map_err(io_err_into_net_error)?;

        self.handler_guard.replace(guard);

        Ok(())
    }
}

impl VirtualIoSource for LocalUdpSocket {
    fn remove_handler(&mut self) {
        if let Some(guard) = self.handler_guard.take() {
            InterestGuard::unregister(guard, &self.selector, &mut self.socket);
        }
    }
}
