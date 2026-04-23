use std::collections::{HashSet, VecDeque};
use std::net::SocketAddr;
use std::sync::Mutex;
use std::task::{Context, Poll, Waker};
use std::{collections::HashMap, sync::Arc};

use crate::tcp_pair::TcpSocketHalf;
use crate::{
    InterestHandler, IpAddr, IpCidr, Ipv4Addr, Ipv6Addr, NetworkError, VirtualIoSource,
    VirtualNetworking, VirtualSocket, VirtualTcpBoundSocket, VirtualTcpListener, VirtualTcpSocket,
};
use virtual_mio::InterestType;

const DEFAULT_MAX_BUFFER_SIZE: usize = 1_048_576;
const LOOPBACK_EPHEMERAL_PORT_START: u16 = 49152;

#[derive(Debug)]
struct LoopbackNetworkingState {
    tcp_listeners: HashMap<SocketAddr, LoopbackTcpListener>,
    tcp_bound: HashSet<SocketAddr>,
    ip_addresses: Vec<IpCidr>,
    next_ephemeral_port: u16,
}

impl Default for LoopbackNetworkingState {
    fn default() -> Self {
        Self {
            tcp_listeners: HashMap::new(),
            tcp_bound: HashSet::new(),
            ip_addresses: Vec::new(),
            next_ephemeral_port: LOOPBACK_EPHEMERAL_PORT_START,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoopbackNetworking {
    state: Arc<Mutex<LoopbackNetworkingState>>,
}

impl LoopbackNetworking {
    pub fn new() -> Self {
        LoopbackNetworking {
            state: Arc::new(Mutex::new(Default::default())),
        }
    }

    pub fn loopback_connect_to(
        &self,
        mut local_addr: SocketAddr,
        peer_addr: SocketAddr,
    ) -> Option<TcpSocketHalf> {
        let mut port = local_addr.port();
        if port == 0 {
            port = peer_addr.port();
        }

        local_addr = match local_addr.ip() {
            IpAddr::V4(Ipv4Addr::UNSPECIFIED) => {
                SocketAddr::new(Ipv4Addr::new(127, 0, 0, 100).into(), port)
            }
            IpAddr::V6(Ipv6Addr::UNSPECIFIED) => {
                SocketAddr::new(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 100).into(), port)
            }
            ip => SocketAddr::new(ip, port),
        };

        let state = self.state.lock().unwrap();
        if let Some(listener) = state.tcp_listeners.get(&peer_addr) {
            Some(listener.connect_to(local_addr))
        } else {
            state
                .tcp_listeners
                .iter()
                .next()
                .map(|listener| listener.1.connect_to(local_addr))
        }
    }

    fn allocate_tcp_bind_addr(
        state: &mut LoopbackNetworkingState,
        mut addr: SocketAddr,
    ) -> crate::Result<SocketAddr> {
        let is_available = |candidate: SocketAddr, state: &LoopbackNetworkingState| {
            let key = Self::normalize_listener_addr(candidate);
            !state.tcp_listeners.contains_key(&key) && !state.tcp_bound.contains(&key)
        };

        if addr.port() == 0 {
            let start = state.next_ephemeral_port;
            let mut candidate = start;
            loop {
                let candidate_addr = SocketAddr::new(addr.ip(), candidate);
                if is_available(candidate_addr, state) {
                    addr.set_port(candidate);
                    state.tcp_bound.insert(Self::normalize_listener_addr(addr));
                    state.next_ephemeral_port = if candidate == u16::MAX {
                        LOOPBACK_EPHEMERAL_PORT_START
                    } else {
                        candidate + 1
                    };
                    return Ok(addr);
                }

                candidate = if candidate == u16::MAX {
                    LOOPBACK_EPHEMERAL_PORT_START
                } else {
                    candidate + 1
                };
                if candidate == start {
                    return Err(NetworkError::AddressInUse);
                }
            }
        }

        let reservation_key = Self::normalize_listener_addr(addr);
        if state.tcp_listeners.contains_key(&reservation_key)
            || state.tcp_bound.contains(&reservation_key)
        {
            return Err(NetworkError::AddressInUse);
        }
        state.tcp_bound.insert(reservation_key);
        Ok(addr)
    }

    fn normalize_listener_addr(mut addr: SocketAddr) -> SocketAddr {
        if addr.ip() == IpAddr::V4(Ipv4Addr::UNSPECIFIED) {
            addr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), addr.port());
        } else if addr.ip() == IpAddr::V6(Ipv6Addr::UNSPECIFIED) {
            addr = SocketAddr::new(Ipv6Addr::LOCALHOST.into(), addr.port());
        }
        addr
    }
}

impl Default for LoopbackNetworking {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(unused_variables)]
#[async_trait::async_trait]
impl VirtualNetworking for LoopbackNetworking {
    async fn dhcp_acquire(&self) -> crate::Result<Vec<IpAddr>> {
        let mut state: std::sync::MutexGuard<'_, LoopbackNetworkingState> =
            self.state.lock().unwrap();
        state.ip_addresses.clear();
        state.ip_addresses.push(IpCidr {
            ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            prefix: 32,
        });
        state.ip_addresses.push(IpCidr {
            ip: IpAddr::V6(Ipv6Addr::LOCALHOST),
            prefix: 128,
        });
        Ok(state.ip_addresses.iter().map(|cidr| cidr.ip).collect())
    }

    async fn ip_add(&self, ip: IpAddr, prefix: u8) -> crate::Result<()> {
        let mut state = self.state.lock().unwrap();
        state.ip_addresses.push(IpCidr { ip, prefix });
        Ok(())
    }

    async fn ip_remove(&self, ip: IpAddr) -> crate::Result<()> {
        let mut state: std::sync::MutexGuard<'_, LoopbackNetworkingState> =
            self.state.lock().unwrap();
        state.ip_addresses.retain(|cidr| cidr.ip != ip);
        Ok(())
    }

    async fn ip_clear(&self) -> crate::Result<()> {
        let mut state: std::sync::MutexGuard<'_, LoopbackNetworkingState> =
            self.state.lock().unwrap();
        state.ip_addresses.clear();
        Ok(())
    }

    async fn ip_list(&self) -> crate::Result<Vec<IpCidr>> {
        let state: std::sync::MutexGuard<'_, LoopbackNetworkingState> = self.state.lock().unwrap();
        Ok(state.ip_addresses.clone())
    }

    async fn listen_tcp(
        &self,
        addr: SocketAddr,
        only_v6: bool,
        reuse_port: bool,
        reuse_addr: bool,
    ) -> crate::Result<Box<dyn VirtualTcpListener + Sync>> {
        self.bind_tcp(addr, only_v6, reuse_port, reuse_addr)
            .await?
            .listen()
    }

    async fn bind_tcp(
        &self,
        addr: SocketAddr,
        _only_v6: bool,
        _reuse_port: bool,
        _reuse_addr: bool,
    ) -> crate::Result<Box<dyn VirtualTcpBoundSocket + Sync>> {
        let mut state = self.state.lock().unwrap();
        let addr = Self::allocate_tcp_bind_addr(&mut state, addr)?;
        Ok(Box::new(LoopbackTcpBoundSocket {
            networking: self.clone(),
            local_addr: addr,
            reservation_key: Some(Self::normalize_listener_addr(addr)),
            ttl: 64,
        }))
    }
}

#[derive(Debug)]
struct LoopbackTcpListenerState {
    handler: Option<Box<dyn InterestHandler + Send + Sync>>,
    addr_local: SocketAddr,
    ttl: u8,
    backlog: VecDeque<TcpSocketHalf>,
    wakers: Vec<Waker>,
}

#[derive(Debug, Clone)]
pub struct LoopbackTcpListener {
    state: Arc<Mutex<LoopbackTcpListenerState>>,
}

impl LoopbackTcpListener {
    pub fn new(addr_local: SocketAddr, ttl: u8) -> Self {
        Self {
            state: Arc::new(Mutex::new(LoopbackTcpListenerState {
                handler: None,
                addr_local,
                ttl,
                backlog: Default::default(),
                wakers: Default::default(),
            })),
        }
    }

    pub fn connect_to(&self, addr_local: SocketAddr) -> TcpSocketHalf {
        let mut state = self.state.lock().unwrap();
        let (mut half1, half2) =
            TcpSocketHalf::channel(DEFAULT_MAX_BUFFER_SIZE, state.addr_local, addr_local);
        half1.set_ttl(u32::from(state.ttl)).ok();

        state.backlog.push_back(half1);
        if let Some(handler) = state.handler.as_mut() {
            handler.push_interest(InterestType::Readable);
        }
        state.wakers.drain(..).for_each(|w| w.wake());

        half2
    }
}

impl VirtualIoSource for LoopbackTcpListener {
    fn remove_handler(&mut self) {
        let mut state = self.state.lock().unwrap();
        state.handler.take();
    }

    fn poll_read_ready(&mut self, cx: &mut Context<'_>) -> Poll<crate::Result<usize>> {
        let mut state = self.state.lock().unwrap();
        if !state.backlog.is_empty() {
            return Poll::Ready(Ok(state.backlog.len()));
        }
        if !state.wakers.iter().any(|w| w.will_wake(cx.waker())) {
            state.wakers.push(cx.waker().clone());
        }
        Poll::Pending
    }

    fn poll_write_ready(&mut self, _cx: &mut Context<'_>) -> Poll<crate::Result<usize>> {
        Poll::Pending
    }
}

impl VirtualTcpListener for LoopbackTcpListener {
    fn try_accept(
        &mut self,
    ) -> crate::Result<(Box<dyn crate::VirtualTcpSocket + Sync>, SocketAddr)> {
        let mut state = self.state.lock().unwrap();
        let next = state.backlog.pop_front();
        if let Some(next) = next {
            let peer = next.addr_peer()?;
            return Ok((Box::new(next), peer));
        }
        Err(NetworkError::WouldBlock)
    }

    fn set_handler(
        &mut self,
        mut handler: Box<dyn InterestHandler + Send + Sync>,
    ) -> crate::Result<()> {
        let mut state = self.state.lock().unwrap();
        if !state.backlog.is_empty() {
            handler.push_interest(InterestType::Readable);
        }
        state.handler.replace(handler);
        Ok(())
    }

    fn addr_local(&self) -> crate::Result<SocketAddr> {
        let state = self.state.lock().unwrap();
        Ok(state.addr_local)
    }

    fn set_ttl(&mut self, ttl: u8) -> crate::Result<()> {
        let mut state = self.state.lock().unwrap();
        state.ttl = ttl;
        Ok(())
    }

    fn ttl(&self) -> crate::Result<u8> {
        let state = self.state.lock().unwrap();
        Ok(state.ttl)
    }
}

#[derive(Debug)]
pub struct LoopbackTcpBoundSocket {
    networking: LoopbackNetworking,
    local_addr: SocketAddr,
    reservation_key: Option<SocketAddr>,
    ttl: u32,
}

impl LoopbackTcpBoundSocket {
    fn release_reservation(&mut self) -> crate::Result<()> {
        let reservation_key = self.reservation_key.take().ok_or(NetworkError::InvalidFd)?;
        let mut state = self.networking.state.lock().unwrap();
        if !state.tcp_bound.remove(&reservation_key) {
            return Err(NetworkError::InvalidFd);
        }
        Ok(())
    }
}

impl Drop for LoopbackTcpBoundSocket {
    fn drop(&mut self) {
        if let Some(reservation_key) = self.reservation_key.take() {
            let mut state = self.networking.state.lock().unwrap();
            state.tcp_bound.remove(&reservation_key);
        }
    }
}

impl VirtualTcpBoundSocket for LoopbackTcpBoundSocket {
    fn addr_local(&self) -> crate::Result<SocketAddr> {
        Ok(self.local_addr)
    }

    fn listen(&mut self) -> crate::Result<Box<dyn VirtualTcpListener + Sync>> {
        let listener =
            LoopbackTcpListener::new(self.local_addr, u8::try_from(self.ttl).unwrap_or(u8::MAX));
        let mut state = self.networking.state.lock().unwrap();
        let reservation_key = self.reservation_key.ok_or(NetworkError::InvalidFd)?;
        if !state.tcp_bound.remove(&reservation_key) {
            return Err(NetworkError::InvalidFd);
        }
        if state.tcp_listeners.contains_key(&reservation_key) {
            state.tcp_bound.insert(reservation_key);
            return Err(NetworkError::AddressInUse);
        }
        state
            .tcp_listeners
            .insert(reservation_key, listener.clone());
        self.reservation_key = None;
        Ok(Box::new(listener))
    }

    fn connect(&mut self, peer: SocketAddr) -> crate::Result<Box<dyn VirtualTcpSocket + Sync>> {
        let mut socket = self
            .networking
            .loopback_connect_to(self.local_addr, peer)
            .ok_or(NetworkError::ConnectionRefused)?;
        self.release_reservation()?;
        socket.set_ttl(self.ttl)?;
        Ok(Box::new(socket))
    }

    fn set_ttl(&mut self, ttl: u32) -> crate::Result<()> {
        self.ttl = ttl;
        Ok(())
    }

    fn ttl(&self) -> crate::Result<u32> {
        Ok(self.ttl)
    }
}
