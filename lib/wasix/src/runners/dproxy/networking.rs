use std::sync::LazyLock;
use std::{
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
    time::Duration,
};

use virtual_net::{
    host::LocalNetworking, loopback::LoopbackNetworking, IpCidr, IpRoute, NetworkError,
    StreamSecurity, VirtualIcmpSocket, VirtualNetworking, VirtualRawSocket, VirtualTcpListener,
    VirtualTcpSocket, VirtualUdpSocket,
};

#[derive(Debug, Default)]
struct LocalWithLoopbackNetworkingListening {
    addresses: Vec<SocketAddr>,
    wakers: Vec<Waker>,
}

#[derive(Debug, Clone)]
pub struct LocalWithLoopbackNetworking {
    inner_networking: Arc<dyn VirtualNetworking + Send + Sync + 'static>,
    local_listening: Arc<Mutex<LocalWithLoopbackNetworkingListening>>,
    loopback_networking: LoopbackNetworking,
}

impl LocalWithLoopbackNetworking {
    pub fn new() -> Self {
        static LOCAL_NETWORKING: LazyLock<Arc<LocalNetworking>> = LazyLock::new(Arc::default);
        Self {
            local_listening: Default::default(),
            inner_networking: LOCAL_NETWORKING.clone(),
            loopback_networking: LoopbackNetworking::new(),
        }
    }

    pub fn poll_listening(&self, cx: &mut Context<'_>) -> Poll<SocketAddr> {
        let mut listening = self.local_listening.lock().unwrap();

        if let Some(addr) = listening.addresses.first() {
            return Poll::Ready(*addr);
        }

        if !listening.wakers.iter().any(|w| w.will_wake(cx.waker())) {
            listening.wakers.push(cx.waker().clone());
        }

        Poll::Pending
    }

    pub fn register_listener(&self, addr: SocketAddr) {
        let mut listening = self.local_listening.lock().unwrap();
        listening.addresses.push(addr);
        listening.addresses.sort_by_key(|a| a.port());
        listening.wakers.drain(..).for_each(|w| w.wake());
    }

    pub fn loopback_networking(&self) -> LoopbackNetworking {
        self.loopback_networking.clone()
    }
}

#[allow(unused_variables)]
#[async_trait::async_trait]
impl VirtualNetworking for LocalWithLoopbackNetworking {
    /// Bridges this local network with a remote network, which is required in
    /// order to make lower level networking calls (such as UDP/TCP)
    async fn bridge(
        &self,
        network: &str,
        access_token: &str,
        security: StreamSecurity,
    ) -> Result<(), NetworkError> {
        self.inner_networking
            .bridge(network, access_token, security)
            .await
    }

    /// Disconnects from the remote network essentially unbridging it
    async fn unbridge(&self) -> Result<(), NetworkError> {
        self.inner_networking.unbridge().await
    }

    /// Acquires an IP address on the network and configures the routing tables
    async fn dhcp_acquire(&self) -> Result<Vec<IpAddr>, NetworkError> {
        self.inner_networking.dhcp_acquire().await
    }

    /// Adds a static IP address to the interface with a netmask prefix
    async fn ip_add(&self, ip: IpAddr, prefix: u8) -> Result<(), NetworkError> {
        self.inner_networking.ip_add(ip, prefix).await
    }

    /// Removes a static (or dynamic) IP address from the interface
    async fn ip_remove(&self, ip: IpAddr) -> Result<(), NetworkError> {
        self.inner_networking.ip_remove(ip).await
    }

    /// Clears all the assigned IP addresses for this interface
    async fn ip_clear(&self) -> Result<(), NetworkError> {
        self.inner_networking.ip_clear().await
    }

    /// Lists all the IP addresses currently assigned to this interface
    async fn ip_list(&self) -> Result<Vec<IpCidr>, NetworkError> {
        self.inner_networking.ip_list().await
    }

    /// Returns the hardware MAC address for this interface
    async fn mac(&self) -> Result<[u8; 6], NetworkError> {
        self.inner_networking.mac().await
    }

    /// Adds a default gateway to the routing table
    async fn gateway_set(&self, ip: IpAddr) -> Result<(), NetworkError> {
        self.inner_networking.gateway_set(ip).await
    }

    /// Adds a specific route to the routing table
    async fn route_add(
        &self,
        cidr: IpCidr,
        via_router: IpAddr,
        preferred_until: Option<Duration>,
        expires_at: Option<Duration>,
    ) -> Result<(), NetworkError> {
        self.inner_networking
            .route_add(cidr, via_router, preferred_until, expires_at)
            .await
    }

    /// Removes a routing rule from the routing table
    async fn route_remove(&self, cidr: IpAddr) -> Result<(), NetworkError> {
        self.inner_networking.route_remove(cidr).await
    }

    /// Clears the routing table for this interface
    async fn route_clear(&self) -> Result<(), NetworkError> {
        self.inner_networking.route_clear().await
    }

    /// Lists all the routes defined in the routing table for this interface
    async fn route_list(&self) -> Result<Vec<IpRoute>, NetworkError> {
        self.inner_networking.route_list().await
    }

    /// Creates a low level socket that can read and write Ethernet packets
    /// directly to the interface
    async fn bind_raw(&self) -> Result<Box<dyn VirtualRawSocket + Sync>, NetworkError> {
        self.inner_networking.bind_raw().await
    }

    /// Listens for TCP connections on a specific IP and Port combination
    /// Multiple servers (processes or threads) can bind to the same port if they each set
    /// the reuse-port and-or reuse-addr flags
    async fn listen_tcp(
        &self,
        addr: SocketAddr,
        only_v6: bool,
        reuse_port: bool,
        reuse_addr: bool,
    ) -> Result<Box<dyn VirtualTcpListener + Sync>, NetworkError> {
        let backlog = 1024;

        tracing::debug!("registering listener on loopback networking");

        let ret: Result<Box<dyn VirtualTcpListener + Sync>, NetworkError> = self
            .loopback_networking
            .listen_tcp(addr, only_v6, reuse_port, reuse_addr)
            .await;

        if ret.is_ok() {
            tracing::debug!("registering listener on loopback networking");
            self.register_listener(addr);
        }

        ret
    }

    /// Opens a UDP socket that listens on a specific IP and Port combination
    /// Multiple servers (processes or threads) can bind to the same port if they each set
    /// the reuse-port and-or reuse-addr flags
    async fn bind_udp(
        &self,
        addr: SocketAddr,
        reuse_port: bool,
        reuse_addr: bool,
    ) -> Result<Box<dyn VirtualUdpSocket + Sync>, NetworkError> {
        self.inner_networking
            .bind_udp(addr, reuse_port, reuse_addr)
            .await
    }

    /// Creates a socket that can be used to send and receive ICMP packets
    /// from a paritcular IP address
    async fn bind_icmp(
        &self,
        addr: IpAddr,
    ) -> Result<Box<dyn VirtualIcmpSocket + Sync>, NetworkError> {
        self.inner_networking.bind_icmp(addr).await
    }

    /// Opens a TCP connection to a particular destination IP address and port
    async fn connect_tcp(
        &self,
        addr: SocketAddr,
        peer: SocketAddr,
    ) -> Result<Box<dyn VirtualTcpSocket + Sync>, NetworkError> {
        self.inner_networking.connect_tcp(addr, peer).await
    }

    /// Performs DNS resolution for a specific hostname
    async fn resolve(
        &self,
        host: &str,
        port: Option<u16>,
        dns_server: Option<IpAddr>,
    ) -> Result<Vec<IpAddr>, NetworkError> {
        self.inner_networking.resolve(host, port, dns_server).await
    }
}
