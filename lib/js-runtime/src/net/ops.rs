use std::borrow::Cow;
use std::cell::RefCell;
use std::net::{IpAddr as StdIpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::rc::Rc;
use std::str::FromStr;

use deno_core::AsyncRefCell;
use deno_core::CancelHandle;
use deno_core::CancelTryFuture;
use deno_core::JsBuffer;
use deno_core::OpState;
use deno_core::RcRef;
use deno_core::Resource;
use deno_core::ResourceId;
use deno_core::op2;
use deno_runtime::deno_permissions::PermissionsContainer;
use serde::{Deserialize, Serialize};
use virtual_net::{
    VirtualConnectionlessSocketExt, VirtualNetworking, VirtualTcpListenerExt, VirtualUdpSocket,
    net_error_into_io_err,
};

use super::io::{MapError, TcpStreamResource};
use super::net_from_state;
use super::stream::SharedTcpStream;

pub type Fd = u32;

#[derive(Debug, Deserialize, Serialize)]
pub struct IpAddr {
    pub hostname: String,
    pub port: u16,
}

impl From<SocketAddr> for IpAddr {
    fn from(addr: SocketAddr) -> Self {
        Self {
            hostname: addr.ip().to_string(),
            port: addr.port(),
        }
    }
}

#[derive(Debug, thiserror::Error, deno_error::JsError)]
pub enum NetError {
    #[class("BadResource")]
    #[error("Listener has been closed")]
    ListenerClosed,
    #[class("Busy")]
    #[error("Listener already in use")]
    ListenerBusy,
    #[class("BadResource")]
    #[error("Socket has been closed")]
    SocketClosed,
    #[class("NotConnected")]
    #[error("Socket has been closed")]
    SocketClosedNotConnected,
    #[class("Busy")]
    #[error("Socket already in use")]
    SocketBusy,
    #[class(inherit)]
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[class("Busy")]
    #[error("Another accept task is ongoing")]
    AcceptTaskOngoing,
    #[class(inherit)]
    #[error(transparent)]
    Permission(#[from] deno_runtime::deno_permissions::PermissionCheckError),
    #[class(inherit)]
    #[error("{0}")]
    Resource(#[from] deno_core::error::ResourceError),
    #[class(inherit)]
    #[error("{0}")]
    Canceled(#[from] deno_core::Canceled),
    #[class(generic)]
    #[error("No resolved address found")]
    NoResolvedAddress,
    #[class(generic)]
    #[error("{0}")]
    AddrParse(#[from] std::net::AddrParseError),
    #[class(generic)]
    #[error("Invalid hostname: {0}")]
    InvalidHostname(String),
    #[class(generic)]
    #[error("unexpected key type")]
    UnexpectedKeyType,
    #[class("Busy")]
    #[error("TCP stream is currently in use")]
    TcpStreamBusy,
    #[class(inherit)]
    #[error("{0}")]
    Rustls(#[from] deno_runtime::deno_tls::rustls::Error),
    #[class(inherit)]
    #[error("{0}")]
    Tls(#[from] deno_runtime::deno_tls::TlsError),
    #[class("InvalidData")]
    #[error("Error creating TLS certificate: Deno.listenTls requires a key")]
    ListenTlsRequiresKey,
    #[class(inherit)]
    #[error("{0}")]
    RootCertStore(deno_error::JsErrorBox),
    #[class(generic)]
    #[error("VSOCK is not supported on this platform")]
    VsockUnsupported,
    #[class(generic)]
    #[error("Tunnel is not open")]
    TunnelMissing,
    #[class(generic)]
    #[error("{0}")]
    Map(#[from] MapError),
}

pub(crate) fn accept_err(e: std::io::Error) -> NetError {
    if let std::io::ErrorKind::Interrupted = e.kind() {
        NetError::ListenerClosed
    } else {
        NetError::Io(e)
    }
}

#[derive(Default)]
pub struct NetPermToken {
    pub hostname: String,
    pub resolved_ips: Vec<String>,
}

// SAFETY: we're sure `NetPermToken` can be GCed
unsafe impl deno_core::GarbageCollected for NetPermToken {
    fn trace(&self, _tracer: &deno_core::garbage_collector::GcVisitor) {}

    fn name(&self) -> &'static std::ffi::CStr {
        c"NetPermToken"
    }
}

impl NetPermToken {
    pub fn includes(&self, addr: &str) -> bool {
        self.resolved_ips.iter().any(|ip| ip == addr)
    }
}

#[op2]
#[serde]
pub fn op_net_get_ips_from_perm_token(#[cppgc] token: &NetPermToken) -> Vec<String> {
    token.resolved_ips.clone()
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TlsHandshakeInfo {
    pub alpn_protocol: Option<deno_core::ByteString>,
    #[serde(skip_serializing)]
    pub peer_certificates: Option<Vec<deno_tls::rustls::pki_types::CertificateDer<'static>>>,
}

#[derive(Debug)]
pub struct TcpListenerResource {
    listener: AsyncRefCell<Box<dyn virtual_net::VirtualTcpListener + Sync>>,
    cancel: CancelHandle,
}

impl Resource for TcpListenerResource {
    fn name(&self) -> Cow<'_, str> {
        "tcpListener".into()
    }

    fn close(self: Rc<Self>) {
        self.cancel.cancel();
    }
}

impl TcpListenerResource {
    pub fn new(listener: Box<dyn virtual_net::VirtualTcpListener + Sync>) -> Self {
        Self {
            listener: AsyncRefCell::new(listener),
            cancel: Default::default(),
        }
    }
}

#[derive(Debug)]
pub struct UdpSocketResource {
    socket: AsyncRefCell<Box<dyn VirtualUdpSocket + Sync>>,
    cancel: CancelHandle,
}

impl Resource for UdpSocketResource {
    fn name(&self) -> Cow<'_, str> {
        "udpSocket".into()
    }

    fn close(self: Rc<Self>) {
        self.cancel.cancel()
    }
}

async fn resolve_addr(
    net: &dyn VirtualNetworking,
    host: &str,
    port: u16,
) -> Result<SocketAddr, NetError> {
    let addrs = net
        .resolve(host, Some(port), None)
        .await
        .map_err(net_error_into_io_err)?;
    let ip = addrs
        .into_iter()
        .next()
        .ok_or(NetError::NoResolvedAddress)?;
    Ok(SocketAddr::new(ip, port))
}

fn resolve_addr_sync(
    net: &dyn VirtualNetworking,
    host: &str,
    port: u16,
) -> Result<SocketAddr, NetError> {
    futures_util::executor::block_on(resolve_addr(net, host, port))
}

#[op2(async, stack_trace)]
#[serde]
pub async fn op_net_connect_tcp(
    state: Rc<RefCell<OpState>>,
    #[serde] addr: IpAddr,
    #[cppgc] net_perm_token: Option<&NetPermToken>,
    #[smi] resource_abort_id: Option<ResourceId>,
) -> Result<(ResourceId, IpAddr, IpAddr, Option<Fd>), NetError> {
    let net = {
        let mut state_ = state.borrow_mut();
        let hostname_to_check = match net_perm_token {
            Some(token) if token.includes(&addr.hostname) => token.hostname.clone(),
            _ => addr.hostname.clone(),
        };
        state_
            .borrow_mut::<PermissionsContainer>()
            .check_net(&(&hostname_to_check, Some(addr.port)), "Deno.connect()")?;
        net_from_state(&state_)
    };

    let addr = resolve_addr(net.as_ref(), &addr.hostname, addr.port).await?;

    let cancel_handle = resource_abort_id.and_then(|rid| {
        state
            .borrow_mut()
            .resource_table
            .get::<CancelHandle>(rid)
            .ok()
    });

    let connect_fut = net.connect_tcp(SocketAddr::new(StdIpAddr::from([0, 0, 0, 0]), 0), addr);
    let socket_result = if let Some(cancel) = cancel_handle.as_ref() {
        connect_fut.or_cancel(cancel).await?
    } else {
        connect_fut.await
    };

    if let Some(cancel_rid) = resource_abort_id
        && let Ok(res) = state.borrow_mut().resource_table.take_any(cancel_rid)
    {
        res.close();
    }

    let mut socket = socket_result.map_err(net_error_into_io_err)?;
    let local_addr = socket.addr_local().map_err(net_error_into_io_err)?;
    let remote_addr = socket.addr_peer().map_err(net_error_into_io_err)?;

    let (shared, socket_ref) = SharedTcpStream::new(socket);
    let rid = state
        .borrow_mut()
        .resource_table
        .add(TcpStreamResource::new(shared, socket_ref));
    Ok((
        rid,
        IpAddr::from(local_addr),
        IpAddr::from(remote_addr),
        None,
    ))
}

#[op2(stack_trace)]
#[serde]
pub fn op_net_listen_tcp(
    state: &mut OpState,
    #[serde] addr: IpAddr,
    reuse_port: bool,
    _load_balanced: bool,
    _tcp_backlog: i32,
) -> Result<(ResourceId, IpAddr), NetError> {
    if reuse_port {
        super::check_unstable(state, "Deno.listen({ reusePort: true })");
    }
    state
        .borrow_mut::<PermissionsContainer>()
        .check_net(&(&addr.hostname, Some(addr.port)), "Deno.listen()")?;
    let net = net_from_state(state);
    let addr = resolve_addr_sync(net.as_ref(), &addr.hostname, addr.port)?;
    let listener = futures_util::executor::block_on(net.listen_tcp(addr, false, reuse_port, true))
        .map_err(net_error_into_io_err)?;
    let local_addr = listener.addr_local().map_err(net_error_into_io_err)?;
    let rid = state.resource_table.add(TcpListenerResource::new(listener));
    Ok((rid, IpAddr::from(local_addr)))
}

#[op2(async)]
#[serde]
pub async fn op_net_accept_tcp(
    state: Rc<RefCell<OpState>>,
    #[smi] rid: ResourceId,
) -> Result<(ResourceId, IpAddr, IpAddr, Option<Fd>), NetError> {
    let resource = state
        .borrow()
        .resource_table
        .get::<TcpListenerResource>(rid)
        .map_err(|_| NetError::ListenerClosed)?;
    let listener = RcRef::map(&resource, |r| &r.listener)
        .try_borrow_mut()
        .ok_or(NetError::AcceptTaskOngoing)?;
    let cancel = RcRef::map(resource, |r| &r.cancel);
    let (socket, _addr) = listener
        .accept()
        .try_or_cancel(cancel)
        .await
        .map_err(accept_err)?;
    let local_addr = socket.addr_local().map_err(net_error_into_io_err)?;
    let remote_addr = socket.addr_peer().map_err(net_error_into_io_err)?;
    let (shared, socket_ref) = SharedTcpStream::new(socket);
    let rid = state
        .borrow_mut()
        .resource_table
        .add(TcpStreamResource::new(shared, socket_ref));
    Ok((
        rid,
        IpAddr::from(local_addr),
        IpAddr::from(remote_addr),
        None,
    ))
}

fn net_listen_udp(
    state: &mut OpState,
    addr: IpAddr,
    reuse_address: bool,
    _loopback: bool,
) -> Result<(ResourceId, IpAddr), NetError> {
    state
        .borrow_mut::<PermissionsContainer>()
        .check_net(&(&addr.hostname, Some(addr.port)), "Deno.listenDatagram()")?;
    let net = net_from_state(state);
    let addr = resolve_addr_sync(net.as_ref(), &addr.hostname, addr.port)?;
    let socket = futures_util::executor::block_on(net.bind_udp(addr, reuse_address, reuse_address))
        .map_err(net_error_into_io_err)?;
    let local_addr = socket.addr_local().map_err(net_error_into_io_err)?;
    let rid = state.resource_table.add(UdpSocketResource {
        socket: AsyncRefCell::new(socket),
        cancel: Default::default(),
    });
    Ok((rid, IpAddr::from(local_addr)))
}

#[op2(stack_trace)]
#[serde]
pub fn op_net_listen_udp(
    state: &mut OpState,
    #[serde] addr: IpAddr,
    reuse_address: bool,
    loopback: bool,
) -> Result<(ResourceId, IpAddr), NetError> {
    net_listen_udp(state, addr, reuse_address, loopback)
}

#[op2(stack_trace)]
#[serde]
pub fn op_node_unstable_net_listen_udp(
    state: &mut OpState,
    #[serde] addr: IpAddr,
    reuse_address: bool,
    loopback: bool,
) -> Result<(ResourceId, IpAddr), NetError> {
    net_listen_udp(state, addr, reuse_address, loopback)
}

#[op2(async)]
#[serde]
pub async fn op_net_recv_udp(
    state: Rc<RefCell<OpState>>,
    #[smi] rid: ResourceId,
    #[buffer] mut buf: JsBuffer,
) -> Result<(usize, IpAddr), NetError> {
    let resource = state
        .borrow_mut()
        .resource_table
        .get::<UdpSocketResource>(rid)
        .map_err(|_| NetError::SocketClosed)?;
    let mut socket = RcRef::map(&resource, |r| &r.socket).borrow_mut().await;
    let mut buf_uninit = unsafe {
        std::slice::from_raw_parts_mut(
            buf.as_mut_ptr() as *mut std::mem::MaybeUninit<u8>,
            buf.len(),
        )
    };
    let (nread, addr) = socket
        .recv_from(&mut buf_uninit, false)
        .await
        .map_err(net_error_into_io_err)?;
    Ok((nread, IpAddr::from(addr)))
}

#[op2(async, stack_trace)]
#[number]
pub async fn op_net_send_udp(
    state: Rc<RefCell<OpState>>,
    #[smi] rid: ResourceId,
    #[serde] addr: IpAddr,
    #[buffer] zero_copy: JsBuffer,
) -> Result<usize, NetError> {
    state
        .borrow_mut()
        .borrow_mut::<PermissionsContainer>()
        .check_net(
            &(&addr.hostname, Some(addr.port)),
            "Deno.DatagramConn.send()",
        )?;
    let net = net_from_state(&state.borrow());
    let addr = resolve_addr(net.as_ref(), &addr.hostname, addr.port).await?;
    let resource = state
        .borrow_mut()
        .resource_table
        .get::<UdpSocketResource>(rid)
        .map_err(|_| NetError::SocketClosed)?;
    let mut socket = RcRef::map(&resource, |r| &r.socket).borrow_mut().await;
    let nwritten = socket
        .send_to(&zero_copy, addr)
        .await
        .map_err(net_error_into_io_err)?;
    Ok(nwritten)
}

#[op2(fast)]
pub fn op_net_validate_multicast(
    #[string] address: String,
    #[string] multi_interface: String,
) -> Result<(), NetError> {
    let addr = Ipv4Addr::from_str(address.as_str())?;
    let interface_addr = Ipv4Addr::from_str(multi_interface.as_str())?;

    if !addr.is_multicast() {
        return Err(NetError::InvalidHostname(address));
    }

    if !interface_addr.is_multicast() {
        return Err(NetError::InvalidHostname(multi_interface));
    }

    Ok(())
}

#[op2(async)]
pub async fn op_net_join_multi_v4_udp(
    state: Rc<RefCell<OpState>>,
    #[smi] rid: ResourceId,
    #[string] address: String,
    #[string] multi_interface: String,
) -> Result<(), NetError> {
    let resource = state
        .borrow_mut()
        .resource_table
        .get::<UdpSocketResource>(rid)
        .map_err(|_| NetError::SocketClosed)?;
    let mut socket = RcRef::map(&resource, |r| &r.socket).borrow_mut().await;
    let addr = Ipv4Addr::from_str(address.as_str())?;
    let interface_addr = Ipv4Addr::from_str(multi_interface.as_str())?;
    socket
        .join_multicast_v4(addr, interface_addr)
        .map_err(net_error_into_io_err)?;
    Ok(())
}

#[op2(async)]
pub async fn op_net_join_multi_v6_udp(
    state: Rc<RefCell<OpState>>,
    #[smi] rid: ResourceId,
    #[string] address: String,
    #[number] multi_interface: u32,
) -> Result<(), NetError> {
    let resource = state
        .borrow_mut()
        .resource_table
        .get::<UdpSocketResource>(rid)
        .map_err(|_| NetError::SocketClosed)?;
    let mut socket = RcRef::map(&resource, |r| &r.socket).borrow_mut().await;
    let addr = Ipv6Addr::from_str(address.as_str())?;
    socket
        .join_multicast_v6(addr, multi_interface)
        .map_err(net_error_into_io_err)?;
    Ok(())
}

#[op2(async)]
pub async fn op_net_leave_multi_v4_udp(
    state: Rc<RefCell<OpState>>,
    #[smi] rid: ResourceId,
    #[string] address: String,
    #[string] multi_interface: String,
) -> Result<(), NetError> {
    let resource = state
        .borrow_mut()
        .resource_table
        .get::<UdpSocketResource>(rid)
        .map_err(|_| NetError::SocketClosed)?;
    let mut socket = RcRef::map(&resource, |r| &r.socket).borrow_mut().await;
    let addr = Ipv4Addr::from_str(address.as_str())?;
    let interface_addr = Ipv4Addr::from_str(multi_interface.as_str())?;
    socket
        .leave_multicast_v4(addr, interface_addr)
        .map_err(net_error_into_io_err)?;
    Ok(())
}

#[op2(async)]
pub async fn op_net_leave_multi_v6_udp(
    state: Rc<RefCell<OpState>>,
    #[smi] rid: ResourceId,
    #[string] address: String,
    #[number] multi_interface: u32,
) -> Result<(), NetError> {
    let resource = state
        .borrow_mut()
        .resource_table
        .get::<UdpSocketResource>(rid)
        .map_err(|_| NetError::SocketClosed)?;
    let mut socket = RcRef::map(&resource, |r| &r.socket).borrow_mut().await;
    let addr = Ipv6Addr::from_str(address.as_str())?;
    socket
        .leave_multicast_v6(addr, multi_interface)
        .map_err(net_error_into_io_err)?;
    Ok(())
}

#[op2(async)]
pub async fn op_net_set_multi_loopback_udp(
    state: Rc<RefCell<OpState>>,
    #[smi] rid: ResourceId,
    enable: bool,
) -> Result<(), NetError> {
    let resource = state
        .borrow_mut()
        .resource_table
        .get::<UdpSocketResource>(rid)
        .map_err(|_| NetError::SocketClosed)?;
    let mut socket = RcRef::map(&resource, |r| &r.socket).borrow_mut().await;
    socket
        .set_multicast_loop_v4(enable)
        .map_err(net_error_into_io_err)?;
    socket
        .set_multicast_loop_v6(enable)
        .map_err(net_error_into_io_err)?;
    Ok(())
}

#[op2(async)]
pub async fn op_net_set_multi_ttl_udp(
    state: Rc<RefCell<OpState>>,
    #[smi] rid: ResourceId,
    ttl: u32,
) -> Result<(), NetError> {
    let resource = state
        .borrow_mut()
        .resource_table
        .get::<UdpSocketResource>(rid)
        .map_err(|_| NetError::SocketClosed)?;
    let mut socket = RcRef::map(&resource, |r| &r.socket).borrow_mut().await;
    socket
        .set_multicast_ttl_v4(ttl)
        .map_err(net_error_into_io_err)?;
    Ok(())
}

#[op2(async)]
pub async fn op_net_set_broadcast_udp(
    state: Rc<RefCell<OpState>>,
    #[smi] rid: ResourceId,
    enable: bool,
) -> Result<(), NetError> {
    let resource = state
        .borrow_mut()
        .resource_table
        .get::<UdpSocketResource>(rid)
        .map_err(|_| NetError::SocketClosed)?;
    let mut socket = RcRef::map(&resource, |r| &r.socket).borrow_mut().await;
    socket
        .set_broadcast(enable)
        .map_err(net_error_into_io_err)?;
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsResolveOptions {
    pub name_server: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsResolveArgs {
    pub cancel_rid: Option<ResourceId>,
    pub query: String,
    pub record_type: RecordType,
    pub options: Option<DnsResolveOptions>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsRecordWithTtl {
    pub ttl: u32,
    pub data: DnsRecordData,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type", content = "data")]
pub enum DnsRecordData {
    A { address: String },
    Aaaa { address: String },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum RecordType {
    A,
    AAAA,
    ANY,
}

#[op2(async)]
#[serde]
pub async fn op_dns_resolve(
    state: Rc<RefCell<OpState>>,
    #[serde] args: DnsResolveArgs,
) -> Result<Vec<DnsRecordWithTtl>, NetError> {
    let cancel_handle = args.cancel_rid.and_then(|rid| {
        state
            .borrow_mut()
            .resource_table
            .get::<CancelHandle>(rid)
            .ok()
    });
    let net = net_from_state(&state.borrow());
    let resolve_fut = net.resolve(&args.query, None, None);
    let addrs = if let Some(cancel) = cancel_handle.as_ref() {
        resolve_fut.or_cancel(cancel).await?
    } else {
        resolve_fut.await
    }
    .map_err(net_error_into_io_err)?;
    let mut out = Vec::new();
    for addr in addrs {
        match (addr, &args.record_type) {
            (StdIpAddr::V4(v4), RecordType::A | RecordType::ANY) => out.push(DnsRecordWithTtl {
                ttl: 0,
                data: DnsRecordData::A {
                    address: v4.to_string(),
                },
            }),
            (StdIpAddr::V6(v6), RecordType::AAAA | RecordType::ANY) => out.push(DnsRecordWithTtl {
                ttl: 0,
                data: DnsRecordData::Aaaa {
                    address: v6.to_string(),
                },
            }),
            _ => {}
        }
    }
    Ok(out)
}

#[op2(fast)]
pub fn op_set_nodelay(
    state: &mut OpState,
    #[smi] rid: ResourceId,
    nodelay: bool,
) -> Result<(), NetError> {
    let resource = state
        .resource_table
        .get::<TcpStreamResource>(rid)
        .map_err(|_| NetError::SocketClosed)?;
    resource.set_nodelay(nodelay)?;
    Ok(())
}

#[op2(fast)]
pub fn op_set_keepalive(
    state: &mut OpState,
    #[smi] rid: ResourceId,
    keepalive: bool,
) -> Result<(), NetError> {
    let resource = state
        .resource_table
        .get::<TcpStreamResource>(rid)
        .map_err(|_| NetError::SocketClosed)?;
    resource.set_keepalive(keepalive)?;
    Ok(())
}

#[op2(fast)]
pub fn op_net_listen_vsock() -> Result<(), NetError> {
    Err(NetError::VsockUnsupported)
}

#[op2(fast)]
pub fn op_net_accept_vsock() -> Result<(), NetError> {
    Err(NetError::VsockUnsupported)
}

#[op2(fast)]
pub fn op_net_connect_vsock() -> Result<(), NetError> {
    Err(NetError::VsockUnsupported)
}

#[op2(fast)]
pub fn op_net_listen_tunnel() -> Result<(), NetError> {
    Err(NetError::TunnelMissing)
}

#[op2(fast)]
pub fn op_net_accept_tunnel() -> Result<(), NetError> {
    Err(NetError::TunnelMissing)
}

#[op2(fast)]
pub fn op_net_accept_unix() -> Result<(), NetError> {
    Err(NetError::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Unix sockets are not supported in js-runtime net",
    )))
}

#[op2(fast)]
pub fn op_net_connect_unix() -> Result<(), NetError> {
    Err(NetError::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Unix sockets are not supported in js-runtime net",
    )))
}

#[op2(fast)]
pub fn op_net_listen_unix() -> Result<(), NetError> {
    Err(NetError::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Unix sockets are not supported in js-runtime net",
    )))
}

#[op2(fast)]
pub fn op_net_listen_unixpacket() -> Result<(), NetError> {
    Err(NetError::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Unix packet sockets are not supported in js-runtime net",
    )))
}

#[op2(fast)]
pub fn op_node_unstable_net_listen_unixpacket() -> Result<(), NetError> {
    Err(NetError::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Unix packet sockets are not supported in js-runtime net",
    )))
}

#[op2(fast)]
pub fn op_net_recv_unixpacket() -> Result<(), NetError> {
    Err(NetError::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Unix packet sockets are not supported in js-runtime net",
    )))
}

#[op2(fast)]
pub fn op_net_send_unixpacket() -> Result<(), NetError> {
    Err(NetError::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Unix packet sockets are not supported in js-runtime net",
    )))
}

#[op2(fast)]
pub fn op_net_unix_stream_from_fd() -> Result<(), NetError> {
    Err(NetError::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Unix sockets are not supported in js-runtime net",
    )))
}
