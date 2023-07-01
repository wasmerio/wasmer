use std::collections::HashMap;
use std::net::IpAddr;
use std::net::SocketAddr;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use derivative::Derivative;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use virtual_io::InlineWaker;

use crate::meta::RequestType;
use crate::meta::ResponseType;
use crate::meta::SocketId;
use crate::meta::{InterfaceId, MessageRequest, MessageResponse};
use crate::IpCidr;
use crate::IpRoute;
use crate::NetworkError;
use crate::StreamSecurity;
use crate::VirtualConnectedSocket;
use crate::VirtualConnectionlessSocket;
use crate::VirtualIcmpSocket;
use crate::VirtualIoSource;
use crate::VirtualNetworking;
use crate::VirtualRawSocket;
use crate::VirtualSocket;
use crate::VirtualTcpListener;
use crate::VirtualTcpSocket;
use crate::VirtualUdpSocket;

use crate::Result;

#[derive(Debug)]
enum RemoteTx {
    Mpsc(mpsc::Sender<MessageRequest>),
}
impl RemoteTx {
    async fn send(&self, req: MessageRequest) -> Result<()> {
        match self {
            RemoteTx::Mpsc(tx) => tx
                .send(req)
                .await
                .map_err(|_| NetworkError::ConnectionAborted),
        }
    }
}

#[derive(Debug)]
enum RemoteRx {
    Mpsc(mpsc::Receiver<MessageResponse>),
}
impl RemoteRx {
    async fn recv(&mut self) -> Option<MessageResponse> {
        match self {
            RemoteRx::Mpsc(rx) => rx.recv().await,
        }
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
struct RemoteCommon {
    interface_id: InterfaceId,
    tx: RemoteTx,
    rx: Mutex<RemoteRx>,
    request_seed: AtomicU64,
    requests: Mutex<HashMap<u64, mpsc::UnboundedSender<ResponseType>>>,
    socket_seed: AtomicU64,
    handler_seed: AtomicU64,
    #[derivative(Debug = "ignore")]
    handlers: Mutex<HashMap<u64, Box<dyn virtual_io::InterestHandler + Send + Sync>>>,
}

impl RemoteCommon {
    async fn io(&self, req_id: u64) -> ResponseType {
        let req_rx = {
            let (tx, rx) = mpsc::unbounded_channel();
            let mut guard = self.requests.lock().await;
            guard.insert(req_id, tx);
            rx
        };

        loop {
            let mut rx_guard = tokio::select! {
                rx = self.rx.lock() => rx,
                res = req_rx => return res,
            };
            tokio::select! {
                msg = rx_guard.recv() => {
                    drop(rx_guard);

                    let msg = match msg {
                        Some(msg) => msg,
                        None => return ResponseType::Err(NetworkError::ConnectionAborted)
                    };
                    let mut requests = self.requests.lock().await;
                    if let Some(request) = requests.remove(&msg.req_id) {
                        request.send(msg.res);
                    }
                },
                res = req_rx => return res,
            }
        }
    }

    async fn io_iface(&self, req: RequestType) -> ResponseType {
        let req_id = self.request_seed.fetch_add(1, Ordering::SeqCst);
        if let Err(err) = self
            .tx
            .send(MessageRequest::Interface {
                iface: self.interface_id,
                req_id,
                req,
            })
            .await
        {
            return ResponseType::Err(err);
        };
        self.io(req_id).await
    }

    fn blocking_io_iface(&self, req: RequestType) -> ResponseType {
        InlineWaker::block_on(self.io_iface(req))
    }
}

#[derive(Debug)]
pub struct RemoteNetworking {
    common: Arc<RemoteCommon>,
}

impl RemoteNetworking {
    /// Creates a new interface on the remote location using
    /// a unique interface ID and a pair of channels
    pub fn new_from_mpsc(
        id: InterfaceId,
        tx: mpsc::Sender<MessageRequest>,
        rx: mpsc::Receiver<MessageResponse>,
    ) -> Self {
        let common = RemoteCommon {
            interface_id: id,
            tx: RemoteTx::Mpsc(tx),
            rx: Mutex::new(RemoteRx::Mpsc(rx)),
            request_seed: AtomicU64::new(1),
            requests: Default::default(),
            socket_seed: AtomicU64::new(1),
            handler_seed: AtomicU64::new(1),
            handlers: Default::default(),
        };
        Self {
            common: Arc::new(common),
        }
    }
}

#[async_trait::async_trait]
impl VirtualNetworking for RemoteNetworking {
    async fn bridge(
        &self,
        network: &str,
        access_token: &str,
        security: StreamSecurity,
    ) -> Result<()> {
        match self
            .common
            .io_iface(RequestType::Bridge {
                network: network.to_string(),
                access_token: access_token.to_string(),
                security,
            })
            .await
        {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    async fn unbridge(&self) -> Result<()> {
        match self.common.io_iface(RequestType::Unbridge).await {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    async fn dhcp_acquire(&self) -> Result<Vec<IpAddr>> {
        match self.common.io_iface(RequestType::DhcpAcquire).await {
            ResponseType::Err(err) => Err(err),
            ResponseType::IpAddressList(ips) => Ok(ips),
            _ => Err(NetworkError::IOError),
        }
    }

    fn ip_add(&self, ip: IpAddr, prefix: u8) -> Result<()> {
        match self
            .common
            .blocking_io_iface(RequestType::IpAdd { ip, prefix })
        {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn ip_remove(&self, ip: IpAddr) -> Result<()> {
        match self.common.blocking_io_iface(RequestType::IpRemove(ip)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn ip_clear(&self) -> Result<()> {
        match self.common.blocking_io_iface(RequestType::IpClear) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn ip_list(&self) -> Result<Vec<IpCidr>> {
        match self.common.blocking_io_iface(RequestType::Unbridge) {
            ResponseType::Err(err) => Err(err),
            ResponseType::CidrList(routes) => Ok(routes),
            _ => Err(NetworkError::IOError),
        }
    }

    fn mac(&self) -> Result<[u8; 6]> {
        match self.common.blocking_io_iface(RequestType::GetMac) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Mac(mac) => Ok(mac),
            _ => Err(NetworkError::IOError),
        }
    }

    fn gateway_set(&self, ip: IpAddr) -> Result<()> {
        match self.common.blocking_io_iface(RequestType::GatewaySet(ip)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn route_add(
        &self,
        cidr: IpCidr,
        via_router: IpAddr,
        preferred_until: Option<Duration>,
        expires_at: Option<Duration>,
    ) -> Result<()> {
        match self.common.blocking_io_iface(RequestType::RouteAdd {
            cidr,
            via_router,
            preferred_until,
            expires_at,
        }) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn route_remove(&self, cidr: IpAddr) -> Result<()> {
        match self
            .common
            .blocking_io_iface(RequestType::RouteRemove(cidr))
        {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn route_clear(&self) -> Result<()> {
        match self.common.blocking_io_iface(RequestType::RouteClear) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn route_list(&self) -> Result<Vec<IpRoute>> {
        match self.common.blocking_io_iface(RequestType::GetRouteList) {
            ResponseType::Err(err) => Err(err),
            ResponseType::RouteList(routes) => Ok(routes),
            _ => Err(NetworkError::IOError),
        }
    }

    async fn bind_raw(&self) -> Result<Box<dyn VirtualRawSocket + Sync>> {
        let socket_id: SocketId = self
            .common
            .socket_seed
            .fetch_add(1, Ordering::SeqCst)
            .into();
        match self.common.io_iface(RequestType::BindRaw(socket_id)).await {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(Box::new(RemoteSocket {
                socket_id,
                common: self.common.clone(),
            })),
            _ => Err(NetworkError::IOError),
        }
    }

    async fn listen_tcp(
        &self,
        addr: SocketAddr,
        only_v6: bool,
        reuse_port: bool,
        reuse_addr: bool,
    ) -> Result<Box<dyn VirtualTcpListener + Sync>> {
        let socket_id: SocketId = self
            .common
            .socket_seed
            .fetch_add(1, Ordering::SeqCst)
            .into();
        match self
            .common
            .io_iface(RequestType::ListenTcp {
                socket_id,
                addr,
                only_v6,
                reuse_port,
                reuse_addr,
            })
            .await
        {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(Box::new(RemoteSocket {
                socket_id,
                common: self.common.clone(),
            })),
            _ => Err(NetworkError::IOError),
        }
    }

    async fn bind_udp(
        &self,
        addr: SocketAddr,
        reuse_port: bool,
        reuse_addr: bool,
    ) -> Result<Box<dyn VirtualUdpSocket + Sync>> {
        let socket_id: SocketId = self
            .common
            .socket_seed
            .fetch_add(1, Ordering::SeqCst)
            .into();
        match self
            .common
            .io_iface(RequestType::BindUdp {
                socket_id,
                addr,
                reuse_port,
                reuse_addr,
            })
            .await
        {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(Box::new(RemoteSocket {
                socket_id,
                common: self.common.clone(),
            })),
            _ => Err(NetworkError::IOError),
        }
    }

    async fn bind_icmp(&self, addr: IpAddr) -> Result<Box<dyn VirtualIcmpSocket + Sync>> {
        let socket_id: SocketId = self
            .common
            .socket_seed
            .fetch_add(1, Ordering::SeqCst)
            .into();
        match self
            .common
            .io_iface(RequestType::BindIcmp { socket_id, addr })
            .await
        {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(Box::new(RemoteSocket {
                socket_id,
                common: self.common.clone(),
            })),
            _ => Err(NetworkError::IOError),
        }
    }

    async fn connect_tcp(
        &self,
        addr: SocketAddr,
        peer: SocketAddr,
    ) -> Result<Box<dyn VirtualTcpSocket + Sync>> {
        let socket_id: SocketId = self
            .common
            .socket_seed
            .fetch_add(1, Ordering::SeqCst)
            .into();
        match self
            .common
            .io_iface(RequestType::ConnectTcp {
                socket_id,
                addr,
                peer,
            })
            .await
        {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(Box::new(RemoteSocket {
                socket_id,
                common: self.common.clone(),
            })),
            _ => Err(NetworkError::IOError),
        }
    }

    async fn resolve(
        &self,
        host: &str,
        port: Option<u16>,
        dns_server: Option<IpAddr>,
    ) -> Result<Vec<IpAddr>> {
        match self.common.io_iface(RequestType::RouteClear).await {
            ResponseType::Err(err) => Err(err),
            ResponseType::IpAddressList(ips) => Ok(ips),
            _ => Err(NetworkError::IOError),
        }
    }
}

#[derive(Debug)]
struct RemoteSocket {
    socket_id: SocketId,
    common: Arc<RemoteCommon>,
}

impl RemoteSocket {
    async fn io_socket(&self, req: RequestType) -> ResponseType {
        let req_id = self.common.request_seed.fetch_add(1, Ordering::SeqCst);
        if let Err(_) = self
            .common
            .tx
            .send(MessageRequest::Socket {
                socket: self.socket_id,
                req_id,
                req,
            })
            .await
        {
            return ResponseType::Err(NetworkError::ConnectionAborted);
        };
        self.common.io(req_id).await
    }

    fn blocking_io_socket(&self, req: RequestType) -> ResponseType {
        InlineWaker::block_on(self.io_socket(req))
    }
}

impl VirtualIoSource for RemoteSocket {
    fn remove_handler(&mut self) {
        self.blocking_io_socket(RequestType::RemoveHandler);
    }
}

impl VirtualSocket for RemoteSocket {
    fn set_ttl(&mut self, ttl: u32) -> Result<()> {
        match self.blocking_io_socket(RequestType::SetTtl(ttl)) {
            ResponseType::Err(err) => Err(err),
            ResponseType::None => Ok(()),
            _ => Err(NetworkError::IOError),
        }
    }

    fn ttl(&self) -> Result<u32> {
        match self.blocking_io_socket(RequestType::GetTtl) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Ttl(ttl) => Ok(ttl),
            _ => Err(NetworkError::IOError),
        }
    }

    fn addr_local(&self) -> Result<SocketAddr> {
        match self.blocking_io_socket(RequestType::GetAddrLocal) {
            ResponseType::Err(err) => Err(err),
            ResponseType::SocketAddr(addr) => Ok(addr),
            _ => Err(NetworkError::IOError),
        }
    }

    fn status(&self) -> Result<crate::SocketStatus> {
        match self.blocking_io_socket(RequestType::GetStatus) {
            ResponseType::Err(err) => Err(err),
            ResponseType::Status(status) => Ok(status),
            _ => Err(NetworkError::IOError),
        }
    }

    fn set_handler(
        &mut self,
        handler: Box<dyn virtual_io::InterestHandler + Send + Sync>,
    ) -> Result<()> {
        todo!()
    }
}

impl VirtualTcpListener for RemoteSocket {
    fn try_accept(&mut self) -> Result<(Box<dyn VirtualTcpSocket + Sync>, SocketAddr)> {
        todo!()
    }

    fn set_handler(
        &mut self,
        handler: Box<dyn virtual_io::InterestHandler + Send + Sync>,
    ) -> Result<()> {
        todo!()
    }

    fn addr_local(&self) -> Result<SocketAddr> {
        todo!()
    }

    fn set_ttl(&mut self, ttl: u8) -> Result<()> {
        todo!()
    }

    fn ttl(&self) -> Result<u8> {
        todo!()
    }
}

impl VirtualRawSocket for RemoteSocket {
    fn try_send(&mut self, data: &[u8]) -> Result<usize> {
        todo!()
    }

    fn try_flush(&mut self) -> Result<()> {
        todo!()
    }

    fn try_recv(&mut self, buf: &mut [std::mem::MaybeUninit<u8>]) -> Result<usize> {
        todo!()
    }

    fn set_promiscuous(&mut self, promiscuous: bool) -> Result<()> {
        todo!()
    }

    fn promiscuous(&self) -> Result<bool> {
        todo!()
    }
}

impl VirtualConnectionlessSocket for RemoteSocket {
    fn try_send_to(&mut self, data: &[u8], addr: SocketAddr) -> Result<usize> {
        todo!()
    }

    fn try_recv_from(
        &mut self,
        buf: &mut [std::mem::MaybeUninit<u8>],
    ) -> Result<(usize, SocketAddr)> {
        todo!()
    }
}

impl VirtualUdpSocket for RemoteSocket {
    fn set_broadcast(&mut self, broadcast: bool) -> Result<()> {
        todo!()
    }

    fn broadcast(&self) -> Result<bool> {
        todo!()
    }

    fn set_multicast_loop_v4(&mut self, val: bool) -> Result<()> {
        todo!()
    }

    fn multicast_loop_v4(&self) -> Result<bool> {
        todo!()
    }

    fn set_multicast_loop_v6(&mut self, val: bool) -> Result<()> {
        todo!()
    }

    fn multicast_loop_v6(&self) -> Result<bool> {
        todo!()
    }

    fn set_multicast_ttl_v4(&mut self, ttl: u32) -> Result<()> {
        todo!()
    }

    fn multicast_ttl_v4(&self) -> Result<u32> {
        todo!()
    }

    fn join_multicast_v4(
        &mut self,
        multiaddr: std::net::Ipv4Addr,
        iface: std::net::Ipv4Addr,
    ) -> Result<()> {
        todo!()
    }

    fn leave_multicast_v4(
        &mut self,
        multiaddr: std::net::Ipv4Addr,
        iface: std::net::Ipv4Addr,
    ) -> Result<()> {
        todo!()
    }

    fn join_multicast_v6(&mut self, multiaddr: std::net::Ipv6Addr, iface: u32) -> Result<()> {
        todo!()
    }

    fn leave_multicast_v6(&mut self, multiaddr: std::net::Ipv6Addr, iface: u32) -> Result<()> {
        todo!()
    }

    fn addr_peer(&self) -> Result<Option<SocketAddr>> {
        todo!()
    }
}

impl VirtualIcmpSocket for RemoteSocket {}

impl VirtualConnectedSocket for RemoteSocket {
    fn set_linger(&mut self, linger: Option<Duration>) -> Result<()> {
        todo!()
    }

    fn linger(&self) -> Result<Option<Duration>> {
        todo!()
    }

    fn try_send(&mut self, data: &[u8]) -> Result<usize> {
        todo!()
    }

    fn try_flush(&mut self) -> Result<()> {
        todo!()
    }

    fn close(&mut self) -> Result<()> {
        todo!()
    }

    fn try_recv(&mut self, buf: &mut [std::mem::MaybeUninit<u8>]) -> Result<usize> {
        todo!()
    }
}

impl VirtualTcpSocket for RemoteSocket {
    fn set_recv_buf_size(&mut self, size: usize) -> Result<()> {
        todo!()
    }

    fn recv_buf_size(&self) -> Result<usize> {
        todo!()
    }

    fn set_send_buf_size(&mut self, size: usize) -> Result<()> {
        todo!()
    }

    fn send_buf_size(&self) -> Result<usize> {
        todo!()
    }

    fn set_nodelay(&mut self, reuse: bool) -> Result<()> {
        todo!()
    }

    fn nodelay(&self) -> Result<bool> {
        todo!()
    }

    fn addr_peer(&self) -> Result<SocketAddr> {
        todo!()
    }

    fn shutdown(&mut self, how: std::net::Shutdown) -> Result<()> {
        todo!()
    }

    fn is_closed(&self) -> bool {
        todo!()
    }
}
