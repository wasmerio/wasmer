#![allow(unused_variables)]
use crate::{io_err_into_net_error, VirtualIoSource};
#[allow(unused_imports)]
use crate::{
    IpCidr, IpRoute, NetworkError, Result, SocketStatus, StreamSecurity, VirtualConnectedSocket,
    VirtualConnectionlessSocket, VirtualIcmpSocket, VirtualNetworking, VirtualRawSocket,
    VirtualSocket, VirtualTcpListener, VirtualTcpSocket, VirtualUdpSocket,
};
use bytes::{Buf, BytesMut};
use derivative::Derivative;
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::mem::MaybeUninit;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr};
#[cfg(not(target_os = "windows"))]
use std::os::fd::AsRawFd;
#[cfg(not(target_os = "windows"))]
use std::os::fd::RawFd;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;
use tokio::runtime::Handle;
#[allow(unused_imports, dead_code)]
use tracing::{debug, error, info, trace, warn};
use virtual_mio::{
    state_as_waker_map, HandlerGuardState, InterestGuard, InterestHandler, InterestType, Selector,
};

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
                    handler_guard: HandlerGuardState::None,
                    no_delay: None,
                    keep_alive: None,
                    backlog: Default::default(),
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

        #[allow(unused_mut)]
        let mut ret = LocalUdpSocket {
            selector: self.selector.clone(),
            socket,
            addr,
            handler_guard: HandlerGuardState::None,
            backlog: Default::default(),
        };

        // In windows we can not poll the socket as it is not supported and hence
        // what we do is immediately set the writable flag and relay on `mio` to
        // refresh that flag when the state changes. In Linux what we do is actually
        // make a non-blocking `poll` call to determine this state
        #[cfg(target_os = "windows")]
        {
            let (state, selector, socket) = ret.split_borrow();
            let map = state_as_waker_map(state, selector, socket).map_err(io_err_into_net_error)?;
            map.push(InterestType::Writable);
        }

        Ok(Box::new(ret))
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
        let socket = Box::new(LocalTcpStream::new(self.selector.clone(), stream, peer));
        Ok(socket)
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
    handler_guard: HandlerGuardState,
    no_delay: Option<bool>,
    keep_alive: Option<bool>,
    backlog: VecDeque<(Box<dyn VirtualTcpSocket + Sync>, SocketAddr)>,
}

impl LocalTcpListener {
    fn try_accept_internal(&mut self) -> Result<(Box<dyn VirtualTcpSocket + Sync>, SocketAddr)> {
        match self.stream.accept().map_err(io_err_into_net_error) {
            Ok((stream, addr)) => {
                socket2::SockRef::from(&self.stream)
                    .set_nonblocking(true)
                    .ok();
                let mut socket = LocalTcpStream::new(self.selector.clone(), stream, addr);
                if let Some(no_delay) = self.no_delay {
                    socket.set_nodelay(no_delay).ok();
                }
                if let Some(keep_alive) = self.keep_alive {
                    socket.set_keepalive(keep_alive).ok();
                }
                Ok((Box::new(socket), addr))
            }
            Err(NetworkError::WouldBlock) => {
                if let HandlerGuardState::WakerMap(_, map) = &mut self.handler_guard {
                    map.pop(InterestType::Readable);
                    map.pop(InterestType::Writable);
                }
                Err(NetworkError::WouldBlock)
            }
            Err(err) => Err(err),
        }
    }
}

impl VirtualTcpListener for LocalTcpListener {
    fn try_accept(&mut self) -> Result<(Box<dyn VirtualTcpSocket + Sync>, SocketAddr)> {
        if let Some(child) = self.backlog.pop_front() {
            return Ok(child);
        }
        self.try_accept_internal()
    }

    fn set_handler(&mut self, mut handler: Box<dyn InterestHandler + Send + Sync>) -> Result<()> {
        if let HandlerGuardState::ExternalHandler(guard) = &mut self.handler_guard {
            match guard.replace_handler(handler) {
                Ok(()) => return Ok(()),
                Err(h) => handler = h,
            }

            // the handler could not be replaced so we need to build a new handler instead
            if let Err(err) = guard.unregister(&mut self.stream) {
                tracing::debug!("failed to unregister previous token - {}", err);
            }
        }

        let guard = InterestGuard::new(
            &self.selector,
            handler,
            &mut self.stream,
            mio::Interest::READABLE.add(mio::Interest::WRITABLE),
        )
        .map_err(io_err_into_net_error)?;

        self.handler_guard = HandlerGuardState::ExternalHandler(guard);

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

impl LocalTcpListener {
    fn split_borrow(
        &mut self,
    ) -> (
        &mut HandlerGuardState,
        &Arc<Selector>,
        &mut mio::net::TcpListener,
    ) {
        (&mut self.handler_guard, &self.selector, &mut self.stream)
    }
}

impl VirtualIoSource for LocalTcpListener {
    fn remove_handler(&mut self) {
        let mut guard = HandlerGuardState::None;
        std::mem::swap(&mut guard, &mut self.handler_guard);
        match guard {
            HandlerGuardState::ExternalHandler(mut guard) => {
                guard.unregister(&mut self.stream).ok();
            }
            HandlerGuardState::WakerMap(mut guard, _) => {
                guard.unregister(&mut self.stream).ok();
            }
            HandlerGuardState::None => {}
        }
    }

    fn poll_read_ready(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<usize>> {
        if !self.backlog.is_empty() {
            return Poll::Ready(Ok(self.backlog.len()));
        }

        let (state, selector, source) = self.split_borrow();
        let map = state_as_waker_map(state, selector, source).map_err(io_err_into_net_error)?;
        map.add(InterestType::Readable, cx.waker());

        if let Ok(child) = self.try_accept_internal() {
            self.backlog.push_back(child);
            return Poll::Ready(Ok(1));
        }
        Poll::Pending
    }

    fn poll_write_ready(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<usize>> {
        if !self.backlog.is_empty() {
            return Poll::Ready(Ok(self.backlog.len()));
        }

        let (state, selector, source) = self.split_borrow();
        let map = state_as_waker_map(state, selector, source).map_err(io_err_into_net_error)?;
        map.add(InterestType::Writable, cx.waker());

        if let Ok(child) = self.try_accept_internal() {
            self.backlog.push_back(child);
            return Poll::Ready(Ok(1));
        }
        Poll::Pending
    }
}

#[derive(Debug)]
pub struct LocalTcpStream {
    stream: mio::net::TcpStream,
    addr: SocketAddr,
    shutdown: Option<Shutdown>,
    selector: Arc<Selector>,
    handler_guard: HandlerGuardState,
    buffer: BytesMut,
}

impl LocalTcpStream {
    fn new(selector: Arc<Selector>, stream: mio::net::TcpStream, addr: SocketAddr) -> Self {
        #[allow(unused_mut)]
        let mut ret = Self {
            stream,
            addr,
            shutdown: None,
            selector,
            handler_guard: HandlerGuardState::None,
            buffer: BytesMut::new(),
        };

        // In windows we can not poll the socket as it is not supported and hence
        // what we do is immediately set the writable flag and relay on `mio` to
        // refresh that flag when the state changes. In Linux what we do is actually
        // make a non-blocking `poll` call to determine this state
        #[cfg(target_os = "windows")]
        {
            let (state, selector, socket, _) = ret.split_borrow();
            if let Ok(map) = state_as_waker_map(state, selector, socket) {
                map.push(InterestType::Writable);
            }
        }

        ret
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

    fn set_keepalive(&mut self, keepalive: bool) -> Result<()> {
        socket2::SockRef::from(&self.stream)
            .set_keepalive(true)
            .map_err(io_err_into_net_error)?;
        Ok(())
    }

    fn keepalive(&self) -> Result<bool> {
        let ret = socket2::SockRef::from(&self.stream)
            .keepalive()
            .map_err(io_err_into_net_error)?;
        Ok(ret)
    }

    #[cfg(not(target_os = "windows"))]
    fn set_dontroute(&mut self, val: bool) -> Result<()> {
        // TODO:
        // Don't route is being set by WASIX which breaks networking
        // Why this is being set is unknown but we need to disable
        // the functionality for now as it breaks everything

        let val = val as libc::c_int;
        let payload = &val as *const libc::c_int as *const libc::c_void;
        let err = unsafe {
            libc::setsockopt(
                self.stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_DONTROUTE,
                payload,
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        if err == -1 {
            return Err(io_err_into_net_error(std::io::Error::last_os_error()));
        }
        Ok(())
    }
    #[cfg(target_os = "windows")]
    fn set_dontroute(&mut self, val: bool) -> Result<()> {
        Err(NetworkError::Unsupported)
    }

    #[cfg(not(target_os = "windows"))]
    fn dontroute(&self) -> Result<bool> {
        let mut payload: MaybeUninit<libc::c_int> = MaybeUninit::uninit();
        let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
        let err = unsafe {
            libc::getsockopt(
                self.stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_DONTROUTE,
                payload.as_mut_ptr().cast(),
                &mut len,
            )
        };
        if err == -1 {
            return Err(io_err_into_net_error(std::io::Error::last_os_error()));
        }
        Ok(unsafe { payload.assume_init() != 0 })
    }
    #[cfg(target_os = "windows")]
    fn dontroute(&self) -> Result<bool> {
        Err(NetworkError::Unsupported)
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
        let ret = self.stream.write(data).map_err(io_err_into_net_error);
        match &ret {
            Ok(0) | Err(NetworkError::WouldBlock) => {
                if let HandlerGuardState::WakerMap(_, map) = &mut self.handler_guard {
                    map.pop(InterestType::Writable);
                }
            }
            _ => {}
        }
        ret
    }

    fn try_flush(&mut self) -> Result<()> {
        self.stream.flush().map_err(io_err_into_net_error)
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn try_recv(&mut self, buf: &mut [MaybeUninit<u8>]) -> Result<usize> {
        let buf: &mut [u8] = unsafe { std::mem::transmute(buf) };
        if !self.buffer.is_empty() {
            let amt = buf.len().min(self.buffer.len());
            buf[..amt].copy_from_slice(&self.buffer[..amt]);
            self.buffer.advance(amt);
            return Ok(amt);
        }

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
        if let HandlerGuardState::ExternalHandler(guard) = &mut self.handler_guard {
            match guard.replace_handler(handler) {
                Ok(()) => return Ok(()),
                Err(h) => handler = h,
            }

            // the handler could not be replaced so we need to build a new handler instead
            if let Err(err) = guard.unregister(&mut self.stream) {
                tracing::debug!("failed to unregister previous token - {}", err);
            }
        }

        let guard = InterestGuard::new(
            &self.selector,
            handler,
            &mut self.stream,
            mio::Interest::READABLE.add(mio::Interest::WRITABLE),
        )
        .map_err(io_err_into_net_error)?;

        self.handler_guard = HandlerGuardState::ExternalHandler(guard);

        Ok(())
    }
}

impl LocalTcpStream {
    fn split_borrow(
        &mut self,
    ) -> (
        &mut HandlerGuardState,
        &Arc<Selector>,
        &mut mio::net::TcpStream,
        &mut BytesMut,
    ) {
        (
            &mut self.handler_guard,
            &self.selector,
            &mut self.stream,
            &mut self.buffer,
        )
    }
}

impl VirtualIoSource for LocalTcpStream {
    fn remove_handler(&mut self) {
        let mut guard = HandlerGuardState::None;
        std::mem::swap(&mut guard, &mut self.handler_guard);
        match guard {
            HandlerGuardState::ExternalHandler(mut guard) => {
                guard.unregister(&mut self.stream).ok();
            }
            HandlerGuardState::WakerMap(mut guard, _) => {
                guard.unregister(&mut self.stream).ok();
            }
            HandlerGuardState::None => {}
        }
    }

    fn poll_read_ready(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<usize>> {
        if !self.buffer.is_empty() {
            return Poll::Ready(Ok(self.buffer.len()));
        }

        let (state, selector, stream, buffer) = self.split_borrow();
        let map = state_as_waker_map(state, selector, stream).map_err(io_err_into_net_error)?;
        map.pop(InterestType::Readable);
        map.add(InterestType::Readable, cx.waker());

        buffer.reserve(buffer.len() + 10240);
        let uninit: &mut [MaybeUninit<u8>] = buffer.spare_capacity_mut();
        let uninit_unsafe: &mut [u8] = unsafe { std::mem::transmute(uninit) };

        match stream.read(uninit_unsafe) {
            Ok(0) => Poll::Ready(Ok(0)),
            Ok(amt) => {
                unsafe {
                    buffer.set_len(buffer.len() + amt);
                }
                Poll::Ready(Ok(amt))
            }
            Err(err) if err.kind() == io::ErrorKind::ConnectionAborted => Poll::Ready(Ok(0)),
            Err(err) if err.kind() == io::ErrorKind::ConnectionReset => Poll::Ready(Ok(0)),
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => Poll::Pending,
            Err(err) => Poll::Ready(Err(io_err_into_net_error(err))),
        }
    }

    fn poll_write_ready(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<usize>> {
        let (state, selector, stream, _) = self.split_borrow();
        let map = state_as_waker_map(state, selector, stream).map_err(io_err_into_net_error)?;
        #[cfg(not(target_os = "windows"))]
        map.pop(InterestType::Writable);
        map.add(InterestType::Writable, cx.waker());
        map.add(InterestType::Closed, cx.waker());
        if map.has_interest(InterestType::Closed) {
            return Poll::Ready(Ok(0));
        }

        #[cfg(not(target_os = "windows"))]
        match libc_poll(stream.as_raw_fd(), libc::POLLOUT | libc::POLLHUP) {
            Some(val) if (val & libc::POLLHUP) != 0 => {
                return Poll::Ready(Ok(0));
            }
            Some(val) if (val & libc::POLLOUT) != 0 => return Poll::Ready(Ok(10240)),
            _ => {}
        }

        // In windows we can not poll the socket as it is not supported and hence
        // what we do is immediately set the writable flag and relay on `mio` to
        // refresh that flag when the state changes. In Linux what we do is actually
        // make a non-blocking `poll` call to determine this state
        #[cfg(target_os = "windows")]
        if map.has_interest(InterestType::Writable) {
            return Poll::Ready(Ok(10240));
        }

        Poll::Pending
    }
}

#[cfg(not(target_os = "windows"))]
fn libc_poll(fd: RawFd, events: libc::c_short) -> Option<libc::c_short> {
    let mut fds: [libc::pollfd; 1] = [libc::pollfd {
        fd,
        events,
        revents: 0,
    }];
    let fds_mut = &mut fds[..];
    let ret = unsafe { libc::poll(fds_mut.as_mut_ptr(), 1, 0) };
    match ret == 1 {
        true => Some(fds[0].revents),
        false => None,
    }
}

#[derive(Debug)]
pub struct LocalUdpSocket {
    socket: mio::net::UdpSocket,
    #[allow(dead_code)]
    addr: SocketAddr,
    selector: Arc<Selector>,
    handler_guard: HandlerGuardState,
    backlog: VecDeque<(BytesMut, SocketAddr)>,
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
        let ret = self
            .socket
            .send_to(data, addr)
            .map_err(io_err_into_net_error);
        match &ret {
            Ok(0) | Err(NetworkError::WouldBlock) => {
                if let HandlerGuardState::WakerMap(_, map) = &mut self.handler_guard {
                    map.pop(InterestType::Writable);
                }
            }
            _ => {}
        }
        ret
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

    fn set_handler(&mut self, mut handler: Box<dyn InterestHandler + Send + Sync>) -> Result<()> {
        if let HandlerGuardState::ExternalHandler(guard) = &mut self.handler_guard {
            match guard.replace_handler(handler) {
                Ok(()) => {
                    return Ok(());
                }
                Err(h) => handler = h,
            }

            // the handler could not be replaced so we need to build a new handler instead
            if let Err(err) = guard.unregister(&mut self.socket) {
                tracing::debug!("failed to unregister previous token - {}", err);
            }
        }

        let guard = InterestGuard::new(
            &self.selector,
            handler,
            &mut self.socket,
            mio::Interest::READABLE.add(mio::Interest::WRITABLE),
        )
        .map_err(io_err_into_net_error)?;

        self.handler_guard = HandlerGuardState::ExternalHandler(guard);

        Ok(())
    }
}

impl LocalUdpSocket {
    fn split_borrow(
        &mut self,
    ) -> (
        &mut HandlerGuardState,
        &Arc<Selector>,
        &mut mio::net::UdpSocket,
    ) {
        (&mut self.handler_guard, &self.selector, &mut self.socket)
    }
}

impl VirtualIoSource for LocalUdpSocket {
    fn remove_handler(&mut self) {
        let mut guard = HandlerGuardState::None;
        std::mem::swap(&mut guard, &mut self.handler_guard);
        match guard {
            HandlerGuardState::ExternalHandler(mut guard) => {
                guard.unregister(&mut self.socket).ok();
            }
            HandlerGuardState::WakerMap(mut guard, _) => {
                guard.unregister(&mut self.socket).ok();
            }
            HandlerGuardState::None => {}
        }
    }

    fn poll_read_ready(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<usize>> {
        if !self.backlog.is_empty() {
            let total = self.backlog.iter().map(|a| a.0.len()).sum();
            return Poll::Ready(Ok(total));
        }

        let (state, selector, socket) = self.split_borrow();
        let map = state_as_waker_map(state, selector, socket).map_err(io_err_into_net_error)?;
        map.pop(InterestType::Readable);
        map.add(InterestType::Readable, cx.waker());

        let mut buffer = BytesMut::default();
        buffer.reserve(10240);
        let uninit: &mut [MaybeUninit<u8>] = buffer.spare_capacity_mut();
        let uninit_unsafe: &mut [u8] = unsafe { std::mem::transmute(uninit) };

        match self.socket.recv_from(uninit_unsafe) {
            Ok((0, _)) => Poll::Ready(Ok(0)),
            Ok((amt, peer)) => {
                unsafe {
                    buffer.set_len(amt);
                }
                self.backlog.push_back((buffer, peer));
                Poll::Ready(Ok(amt))
            }
            Err(err) if err.kind() == io::ErrorKind::ConnectionAborted => Poll::Ready(Ok(0)),
            Err(err) if err.kind() == io::ErrorKind::ConnectionReset => Poll::Ready(Ok(0)),
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => Poll::Pending,
            Err(err) => Poll::Ready(Err(io_err_into_net_error(err))),
        }
    }

    fn poll_write_ready(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<usize>> {
        let (state, selector, socket) = self.split_borrow();
        let map = state_as_waker_map(state, selector, socket).map_err(io_err_into_net_error)?;
        #[cfg(not(target_os = "windows"))]
        map.pop(InterestType::Writable);
        map.add(InterestType::Writable, cx.waker());

        #[cfg(not(target_os = "windows"))]
        match libc_poll(socket.as_raw_fd(), libc::POLLOUT | libc::POLLHUP) {
            Some(val) if (val & libc::POLLHUP) != 0 => {
                return Poll::Ready(Ok(0));
            }
            Some(val) if (val & libc::POLLOUT) != 0 => return Poll::Ready(Ok(10240)),
            _ => {}
        }

        // In windows we can not poll the socket as it is not supported and hence
        // what we do is immediately set the writable flag and relay on `mio` to
        // refresh that flag when the state changes. In Linux what we do is actually
        // make a non-blocking `poll` call to determine this state
        #[cfg(target_os = "windows")]
        if map.has_interest(InterestType::Writable) {
            return Poll::Ready(Ok(10240));
        }

        Poll::Pending
    }
}
