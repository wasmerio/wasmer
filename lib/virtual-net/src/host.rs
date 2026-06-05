#![allow(unused_variables)]
use crate::ruleset::{Direction, Ruleset};
#[allow(unused_imports)]
use crate::{
    IpCidr, IpRoute, NetworkError, Result, SocketStatus, StreamSecurity, VirtualConnectedSocket,
    VirtualConnectionlessSocket, VirtualIcmpSocket, VirtualNetworking, VirtualRawSocket,
    VirtualSocket, VirtualTcpListener, VirtualTcpSocket, VirtualUdpSocket,
};
use crate::{VirtualIoSource, io_err_into_net_error};
use bytes::{Buf, BytesMut};
use std::collections::{HashMap, VecDeque};
use std::io::{self, Read, Write};
use std::mem::MaybeUninit;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr};
#[cfg(not(target_os = "windows"))]
use std::os::fd::AsRawFd;
#[cfg(not(target_os = "windows"))]
use std::os::fd::RawFd;
#[cfg(windows)]
use std::os::windows::io::AsRawSocket;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::task::Poll;
use std::task::Waker;
use std::time::Duration;
use tokio::runtime::Handle;
#[allow(unused_imports, dead_code)]
use tracing::{debug, error, info, trace, warn};
use virtual_mio::{
    HandlerGuardState, InterestGuard, InterestHandler, InterestType, Selector, state_as_waker_map,
};

#[derive(Debug)]
pub struct LocalNetworking {
    selector: Arc<Selector>,
    handle: Handle,
    ruleset: Option<Ruleset>,
    multicast: Arc<Mutex<MulticastCoordinator>>,
    next_udp_socket_id: Arc<AtomicU64>,
}

impl LocalNetworking {
    pub fn new() -> Self {
        Self {
            selector: Selector::new(),
            handle: Handle::current(),
            ruleset: None,
            multicast: Default::default(),
            next_udp_socket_id: Default::default(),
        }
    }

    pub fn with_ruleset(ruleset: Ruleset) -> Self {
        Self {
            selector: Selector::new(),
            handle: Handle::current(),
            ruleset: Some(ruleset),
            multicast: Default::default(),
            next_udp_socket_id: Default::default(),
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

const MULTICAST_RING_CAPACITY: usize = 1024;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
struct MulticastKey {
    addr: IpAddr,
    port: u16,
}

impl MulticastKey {
    fn from_socket_addr(addr: SocketAddr) -> Option<Self> {
        match addr.ip() {
            IpAddr::V4(ip) if ip.is_multicast() => Some(Self {
                addr: IpAddr::V4(ip),
                port: addr.port(),
            }),
            IpAddr::V6(ip) if ip.is_multicast() => Some(Self {
                addr: IpAddr::V6(ip),
                port: addr.port(),
            }),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
struct MulticastPacket {
    seq: u64,
    data: Arc<[u8]>,
    from: SocketAddr,
}

#[derive(Debug, Default)]
struct MulticastGroup {
    next_seq: u64,
    packets: VecDeque<MulticastPacket>,
    members: HashMap<u64, Weak<LocalUdpSocketShared>>,
}

#[derive(Debug, Default)]
struct MulticastCoordinator {
    groups: HashMap<MulticastKey, MulticastGroup>,
}

impl MulticastCoordinator {
    fn join(&mut self, socket: &Arc<LocalUdpSocketShared>, key: MulticastKey) {
        let group = self.groups.entry(key).or_default();
        group.members.insert(socket.id, Arc::downgrade(socket));
        socket
            .multicast_reads
            .lock()
            .unwrap()
            .insert(key, group.next_seq);
    }

    fn leave(&mut self, socket: &Arc<LocalUdpSocketShared>, key: MulticastKey) {
        if let Some(group) = self.groups.get_mut(&key) {
            group.members.remove(&socket.id);
            if group.members.is_empty() {
                self.groups.remove(&key);
            }
        }
        socket.multicast_reads.lock().unwrap().remove(&key);
    }

    fn send(
        &mut self,
        sender: &Arc<LocalUdpSocketShared>,
        data: &[u8],
        from: SocketAddr,
        to: SocketAddr,
    ) -> Vec<Arc<LocalUdpSocketShared>> {
        let Some(key) = MulticastKey::from_socket_addr(to) else {
            return Vec::new();
        };
        let Some(group) = self.groups.get_mut(&key) else {
            return Vec::new();
        };

        let packet = MulticastPacket {
            seq: group.next_seq,
            data: Arc::from(data),
            from,
        };
        group.next_seq = group.next_seq.wrapping_add(1);
        group.packets.push_back(packet);
        while group.packets.len() > MULTICAST_RING_CAPACITY {
            group.packets.pop_front();
        }

        let mut stale = Vec::new();
        let mut subscribers = Vec::new();
        for (&id, member) in &group.members {
            let Some(member) = member.upgrade() else {
                stale.push(id);
                continue;
            };
            if Arc::ptr_eq(sender, &member) && !member.multicast_loop_for(key.addr) {
                if let Some(cursor) = member.multicast_reads.lock().unwrap().get_mut(&key) {
                    *cursor = group.next_seq;
                }
                continue;
            }
            subscribers.push(member);
        }
        for id in stale {
            group.members.remove(&id);
        }
        subscribers
    }

    fn next_packet_len(&mut self, socket: &Arc<LocalUdpSocketShared>) -> Option<usize> {
        let mut reads = socket.multicast_reads.lock().unwrap();
        for (key, cursor) in reads.iter_mut() {
            let Some(group) = self.groups.get(key) else {
                continue;
            };
            if !group.members.contains_key(&socket.id) {
                continue;
            }
            let Some(front) = group.packets.front() else {
                continue;
            };
            if *cursor < front.seq {
                *cursor = front.seq;
            }
            if let Some(packet) = group.packets.iter().find(|packet| packet.seq >= *cursor) {
                return Some(packet.data.len());
            }
        }
        None
    }

    fn recv(
        &mut self,
        socket: &Arc<LocalUdpSocketShared>,
        buf: &mut [MaybeUninit<u8>],
        peek: bool,
    ) -> Result<(usize, SocketAddr)> {
        let mut reads = socket.multicast_reads.lock().unwrap();
        for (key, cursor) in reads.iter_mut() {
            let Some(group) = self.groups.get(key) else {
                continue;
            };
            if !group.members.contains_key(&socket.id) {
                continue;
            }
            let Some(front) = group.packets.front() else {
                continue;
            };
            if *cursor < front.seq {
                *cursor = front.seq;
            }
            let Some(packet) = group.packets.iter().find(|packet| packet.seq >= *cursor) else {
                continue;
            };

            let amt = buf.len().min(packet.data.len());
            let buf: &mut [u8] = unsafe { std::mem::transmute(buf) };
            buf[..amt].copy_from_slice(&packet.data[..amt]);
            if !peek {
                *cursor = packet.seq.wrapping_add(1);
            }
            return Ok((amt, packet.from));
        }
        Err(NetworkError::WouldBlock)
    }
}

struct LocalUdpSocketShared {
    id: u64,
    multicast_reads: Mutex<HashMap<MulticastKey, u64>>,
    multicast_loop_v4: Mutex<bool>,
    multicast_loop_v6: Mutex<bool>,
    read_wakers: Mutex<Vec<Waker>>,
    handler: Mutex<Option<Box<dyn InterestHandler + Send + Sync>>>,
}

impl std::fmt::Debug for LocalUdpSocketShared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalUdpSocketShared")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl LocalUdpSocketShared {
    fn new(id: u64) -> Self {
        Self {
            id,
            multicast_reads: Default::default(),
            multicast_loop_v4: Mutex::new(true),
            multicast_loop_v6: Mutex::new(true),
            read_wakers: Default::default(),
            handler: Default::default(),
        }
    }

    fn multicast_loop_for(&self, addr: IpAddr) -> bool {
        match addr {
            IpAddr::V4(_) => *self.multicast_loop_v4.lock().unwrap(),
            IpAddr::V6(_) => *self.multicast_loop_v6.lock().unwrap(),
        }
    }

    fn notify_readable(&self) {
        for waker in self.read_wakers.lock().unwrap().drain(..) {
            waker.wake();
        }
        if let Some(handler) = self.handler.lock().unwrap().as_mut() {
            handler.push_interest(InterestType::Readable);
        }
    }
}

#[derive(Debug)]
struct LocalUdpSocketInterestHandler {
    shared: Arc<LocalUdpSocketShared>,
}

impl InterestHandler for LocalUdpSocketInterestHandler {
    fn push_interest(&mut self, interest: InterestType) {
        if let Some(handler) = self.shared.handler.lock().unwrap().as_mut() {
            handler.push_interest(interest);
        }
    }

    fn pop_interest(&mut self, interest: InterestType) -> bool {
        self.shared
            .handler
            .lock()
            .unwrap()
            .as_mut()
            .map(|handler| handler.pop_interest(interest))
            .unwrap_or(false)
    }

    fn has_interest(&self, interest: InterestType) -> bool {
        self.shared
            .handler
            .lock()
            .unwrap()
            .as_ref()
            .map(|handler| handler.has_interest(interest))
            .unwrap_or(false)
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
        if let Some(ruleset) = self.ruleset.as_ref()
            && !ruleset.allows_socket(addr, Direction::Inbound)
        {
            tracing::warn!(%addr, "listen_tcp blocked by firewall rule");
            return Err(NetworkError::PermissionDenied);
        }

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
                    ruleset: self.ruleset.clone(),
                })
            })
            .map_err(io_err_into_net_error)?;
        Ok(listener)
    }

    async fn bind_udp(
        &self,
        addr: SocketAddr,
        reuse_port: bool,
        reuse_addr: bool,
    ) -> Result<Box<dyn VirtualUdpSocket + Sync>> {
        #[cfg(not(windows))]
        use socket2::{Domain, Socket, Type};

        if let Some(ruleset) = self.ruleset.as_ref()
            && !ruleset.allows_socket(addr, Direction::Inbound)
        {
            tracing::warn!(%addr, "bind_udp blocked by firewall rule");
            return Err(NetworkError::PermissionDenied);
        }

        #[cfg(not(windows))]
        let socket = {
            let domain = if addr.is_ipv4() {
                Domain::IPV4
            } else {
                Domain::IPV6
            };
            let std_sock = Socket::new(domain, Type::DGRAM, None).map_err(io_err_into_net_error)?;
            std_sock
                .set_nonblocking(true)
                .map_err(io_err_into_net_error)?;
            std_sock
                .set_reuse_address(reuse_addr)
                .map_err(io_err_into_net_error)?;
            std_sock
                .set_reuse_port(reuse_port)
                .map_err(io_err_into_net_error)?;
            std_sock.bind(&addr.into()).map_err(io_err_into_net_error)?;
            mio::net::UdpSocket::from_std(std_sock.into())
        };
        #[cfg(windows)]
        let socket = mio::net::UdpSocket::bind(addr).map_err(io_err_into_net_error)?;

        #[allow(unused_mut)]
        let mut ret = LocalUdpSocket {
            selector: self.selector.clone(),
            socket,
            addr,
            handler_guard: HandlerGuardState::None,
            backlog: Default::default(),
            ruleset: self.ruleset.clone(),
            multicast: self.multicast.clone(),
            shared: Arc::new(LocalUdpSocketShared::new(
                self.next_udp_socket_id.fetch_add(1, Ordering::Relaxed),
            )),
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
        if let Some(ruleset) = self.ruleset.as_ref()
            && !ruleset.allows_socket(peer, Direction::Outbound)
        {
            tracing::warn!(%peer, "connect_tcp blocked by firewall rule");
            return Err(NetworkError::PermissionDenied);
        }

        let stream = mio::net::TcpStream::connect(peer).map_err(io_err_into_net_error)?;

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
        if let Some(ruleset) = self.ruleset.as_ref()
            && !ruleset.allows_domain(host)
        {
            tracing::warn!(%host, "dns resolve blocked by firewall rule");
            return Err(NetworkError::PermissionDenied);
        }

        let host_to_lookup = if host.contains(':') {
            host.to_string()
        } else {
            format!("{}:{}", host, port.unwrap_or(0))
        };
        let addrs = self
            .handle
            .spawn(tokio::net::lookup_host(host_to_lookup))
            .await
            .map_err(|_| NetworkError::IOError)?
            .map(|a| a.map(|a| a.ip()).collect::<Vec<_>>())
            .map_err(io_err_into_net_error)?;

        if let Some(ruleset) = self.ruleset.as_ref() {
            if let Err(e) = ruleset.expand_domain(host, &addrs) {
                tracing::debug!(err=%e, "ruleset expansion failed");
            } else {
                tracing::debug!(addrs=?addrs, domain = host, "ruleset expansion")
            }
        }

        Ok(addrs)
    }
}

#[derive(Debug)]
pub struct LocalTcpListener {
    stream: mio::net::TcpListener,
    selector: Arc<Selector>,
    handler_guard: HandlerGuardState,
    no_delay: Option<bool>,
    keep_alive: Option<bool>,
    backlog: VecDeque<(Box<dyn VirtualTcpSocket + Sync>, SocketAddr)>,
    ruleset: Option<Ruleset>,
}

impl LocalTcpListener {
    fn try_accept_internal(&mut self) -> Result<(Box<dyn VirtualTcpSocket + Sync>, SocketAddr)> {
        match self.stream.accept().map_err(io_err_into_net_error) {
            Ok((stream, addr)) => {
                if let Some(ruleset) = self.ruleset.as_ref()
                    && !ruleset.allows_socket(addr, Direction::Outbound)
                {
                    tracing::warn!(%addr, "try_accept blocked by firewall rule");
                    return Err(NetworkError::PermissionDenied);
                }

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
enum ConnectState {
    Unknown,
    Opened,
    Failed(NetworkError),
}

#[derive(Debug)]
pub struct LocalTcpStream {
    stream: mio::net::TcpStream,
    addr: SocketAddr,
    shutdown: Option<Shutdown>,
    selector: Arc<Selector>,
    handler_guard: HandlerGuardState,
    buffer: BytesMut,
    connect_state: Mutex<ConnectState>,
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
            connect_state: Mutex::new(ConnectState::Unknown),
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

    fn with_sock_ref<F, R>(&self, f: F) -> R
    where
        for<'a> F: FnOnce(socket2::SockRef<'a>) -> R,
    {
        #[cfg(not(windows))]
        let r = socket2::SockRef::from(&self.stream);

        #[cfg(windows)]
        let b = unsafe {
            std::os::windows::io::BorrowedSocket::borrow_raw(self.stream.as_raw_socket())
        };
        #[cfg(windows)]
        let r = socket2::SockRef::from(&b);

        f(r)
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
        self.with_sock_ref(|s| s.set_keepalive(true))
            .map_err(io_err_into_net_error)?;
        Ok(())
    }

    fn keepalive(&self) -> Result<bool> {
        let ret = self
            .with_sock_ref(|s| s.keepalive())
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
        self.with_sock_ref(|s| s.set_linger(linger))
            .map_err(io_err_into_net_error)?;
        Ok(())
    }

    fn linger(&self) -> Result<Option<Duration>> {
        self.with_sock_ref(|s| s.linger())
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

    fn try_recv(&mut self, buf: &mut [MaybeUninit<u8>], peek: bool) -> Result<usize> {
        let buf: &mut [u8] = unsafe { std::mem::transmute(buf) };
        if !self.buffer.is_empty() {
            let amt = buf.len().min(self.buffer.len());
            buf[..amt].copy_from_slice(&self.buffer[..amt]);
            if !peek {
                self.buffer.advance(amt);
            }
            return Ok(amt);
        }

        if peek {
            self.stream.peek(buf)
        } else {
            self.stream.read(buf)
        }
        .map_err(io_err_into_net_error)
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
        // `take_error()` consumes the latched socket error, so once the
        // connect resolves we cache the terminal state to keep status() stable.
        let mut connect_state = self.connect_state.lock().unwrap();
        match *connect_state {
            ConnectState::Opened => return Ok(SocketStatus::Opened),
            ConnectState::Failed(_) => return Ok(SocketStatus::Failed),
            ConnectState::Unknown => {}
        }

        if let Some(err) = self
            .with_sock_ref(|sockref| sockref.take_error())
            .map_err(io_err_into_net_error)?
        {
            *connect_state = ConnectState::Failed(io_err_into_net_error(err));
            return Ok(SocketStatus::Failed); // connect error on the socket
        }
        match self.stream.peer_addr() {
            Ok(_) => {
                *connect_state = ConnectState::Opened;
                Ok(SocketStatus::Opened) // TCP handshake completed.
            }
            Err(err) => {
                if matches!(
                    err.kind(),
                    io::ErrorKind::NotConnected | io::ErrorKind::WouldBlock
                ) {
                    Ok(SocketStatus::Opening) // The connect is still in progress
                } else {
                    *connect_state = ConnectState::Failed(io_err_into_net_error(err));
                    Ok(SocketStatus::Failed) // Any other error means the socket is unusable
                }
            }
        }
    }

    fn last_error(&self) -> Result<Option<NetworkError>> {
        let connect_state = self.connect_state.lock().unwrap();
        Ok(match *connect_state {
            ConnectState::Failed(err) => Some(err),
            _ => None,
        })
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
    ruleset: Option<Ruleset>,
    multicast: Arc<Mutex<MulticastCoordinator>>,
    shared: Arc<LocalUdpSocketShared>,
}

impl Drop for LocalUdpSocket {
    fn drop(&mut self) {
        let keys: Vec<_> = self
            .shared
            .multicast_reads
            .lock()
            .unwrap()
            .keys()
            .copied()
            .collect();
        let mut multicast = self.multicast.lock().unwrap();
        for key in keys {
            multicast.leave(&self.shared, key);
        }
    }
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
        *self.shared.multicast_loop_v4.lock().unwrap() = val;
        self.socket
            .set_multicast_loop_v4(val)
            .map_err(io_err_into_net_error)
    }

    fn multicast_loop_v4(&self) -> Result<bool> {
        Ok(*self.shared.multicast_loop_v4.lock().unwrap())
    }

    fn set_multicast_loop_v6(&mut self, val: bool) -> Result<()> {
        *self.shared.multicast_loop_v6.lock().unwrap() = val;
        self.socket
            .set_multicast_loop_v6(val)
            .map_err(io_err_into_net_error)
    }

    fn multicast_loop_v6(&self) -> Result<bool> {
        Ok(*self.shared.multicast_loop_v6.lock().unwrap())
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
        let port = self.addr_local()?.port();
        self.multicast.lock().unwrap().join(
            &self.shared,
            MulticastKey {
                addr: IpAddr::V4(multiaddr),
                port,
            },
        );
        Ok(())
    }

    fn leave_multicast_v4(&mut self, multiaddr: Ipv4Addr, iface: Ipv4Addr) -> Result<()> {
        let port = self.addr_local()?.port();
        self.multicast.lock().unwrap().leave(
            &self.shared,
            MulticastKey {
                addr: IpAddr::V4(multiaddr),
                port,
            },
        );
        Ok(())
    }

    fn join_multicast_v6(&mut self, multiaddr: Ipv6Addr, iface: u32) -> Result<()> {
        let port = self.addr_local()?.port();
        self.multicast.lock().unwrap().join(
            &self.shared,
            MulticastKey {
                addr: IpAddr::V6(multiaddr),
                port,
            },
        );
        Ok(())
    }

    fn leave_multicast_v6(&mut self, multiaddr: Ipv6Addr, iface: u32) -> Result<()> {
        let port = self.addr_local()?.port();
        self.multicast.lock().unwrap().leave(
            &self.shared,
            MulticastKey {
                addr: IpAddr::V6(multiaddr),
                port,
            },
        );
        Ok(())
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
        if let Some(ruleset) = self.ruleset.as_ref()
            && !ruleset.allows_socket(addr, Direction::Outbound)
        {
            tracing::warn!(%addr, "try_send blocked by firewall rule");
            return Err(NetworkError::PermissionDenied);
        }

        let multicast_subscribers = match MulticastKey::from_socket_addr(addr) {
            Some(_) => {
                let from = self.addr_local().unwrap_or(self.addr);
                self.multicast
                    .lock()
                    .unwrap()
                    .send(&self.shared, data, from, addr)
            }
            None => Vec::new(),
        };
        for subscriber in &multicast_subscribers {
            subscriber.notify_readable();
        }

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
        if ret.is_err() && !multicast_subscribers.is_empty() {
            return Ok(data.len());
        }
        ret
    }

    fn try_recv_from(
        &mut self,
        buf: &mut [MaybeUninit<u8>],
        peek: bool,
    ) -> Result<(usize, SocketAddr)> {
        if let Some((packet, addr)) = self.backlog.front() {
            let buf: &mut [u8] = unsafe { std::mem::transmute(buf) };
            let amt = buf.len().min(packet.len());
            let addr = *addr;
            buf[..amt].copy_from_slice(&packet[..amt]);
            if !peek {
                self.backlog.pop_front();
            }
            return Ok((amt, addr));
        }

        match self.multicast.lock().unwrap().recv(&self.shared, buf, peek) {
            Ok(ret) => return Ok(ret),
            Err(NetworkError::WouldBlock) => {}
            Err(err) => return Err(err),
        }

        self.recv_into_backlog().map_err(io_err_into_net_error)?;
        self.try_recv_from(buf, peek)
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
        *self.shared.handler.lock().unwrap() = Some(handler);

        if let HandlerGuardState::ExternalHandler(_) = &mut self.handler_guard {
            return Ok(());
        }
        let handler = Box::new(LocalUdpSocketInterestHandler {
            shared: self.shared.clone(),
        });

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
    fn recv_into_backlog(&mut self) -> io::Result<()> {
        let mut buffer = BytesMut::default();
        buffer.reserve(10240);
        let uninit: &mut [MaybeUninit<u8>] = buffer.spare_capacity_mut();
        let uninit_unsafe: &mut [u8] = unsafe { std::mem::transmute(uninit) };

        let (amt, peer) = self.socket.recv_from(uninit_unsafe)?;
        unsafe {
            buffer.set_len(amt);
        }
        self.backlog.push_back((buffer, peer));
        Ok(())
    }

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
        *self.shared.handler.lock().unwrap() = None;
        self.shared.read_wakers.lock().unwrap().clear();
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
            let len = self.backlog.front().map(|a| a.0.len()).unwrap_or_default();
            return Poll::Ready(Ok(len));
        }
        if let Some(len) = self.multicast.lock().unwrap().next_packet_len(&self.shared) {
            return Poll::Ready(Ok(len));
        }

        let (state, selector, socket) = self.split_borrow();
        let map = state_as_waker_map(state, selector, socket).map_err(io_err_into_net_error)?;
        map.pop(InterestType::Readable);
        map.add(InterestType::Readable, cx.waker());
        {
            let mut wakers = self.shared.read_wakers.lock().unwrap();
            if !wakers.iter().any(|waker| waker.will_wake(cx.waker())) {
                wakers.push(cx.waker().clone());
            }
        }

        match self.recv_into_backlog() {
            Ok(()) => {
                let len = self.backlog.front().map(|a| a.0.len()).unwrap_or_default();
                Poll::Ready(Ok(len))
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
