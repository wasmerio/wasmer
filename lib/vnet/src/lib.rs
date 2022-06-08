use std::fmt;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::Ipv6Addr;
use std::net::Shutdown;
use std::net::SocketAddr;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use thiserror::Error;

pub use bytes::Bytes;
pub use bytes::BytesMut;

pub type Result<T> = std::result::Result<T, NetworkError>;

/// Socket descriptors are also file descriptors and so
/// all file operations can also be used on sockets
pub type SocketDescriptor = wasmer_vfs::FileDescriptor;

/// Represents an IP address and its netmask
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct IpCidr {
    pub ip: IpAddr,
    pub prefix: u8,
}

/// Represents a routing entry in the routing table of the interface
#[derive(Clone, Debug)]
pub struct IpRoute {
    pub cidr: IpCidr,
    pub via_router: IpAddr,
    pub preferred_until: Option<Duration>,
    pub expires_at: Option<Duration>,
}

/// An implementation of virtual networking
pub trait VirtualNetworking: fmt::Debug + Send + Sync + 'static {
    /// Establishes a web socket connection
    /// (note: this does not use the virtual sockets and is standalone
    ///        functionality that works without the network being connected)
    fn ws_connect(&self, url: &str) -> Result<Box<dyn VirtualWebSocket + Sync>>;

    /// Makes a HTTP request to a remote web resource
    /// The headers are separated by line breaks
    /// (note: this does not use the virtual sockets and is standalone
    ///        functionality that works without the network being connected)
    fn http_request(
        &self,
        url: &str,
        method: &str,
        headers: &str,
        gzip: bool,
    ) -> Result<SocketHttpRequest>;

    /// Bridges this local network with a remote network, which is required in
    /// order to make lower level networking calls (such as UDP/TCP)
    fn bridge(&self, network: &str, access_token: &str, security: StreamSecurity) -> Result<()>;

    /// Disconnects from the remote network essentially unbridging it
    fn unbridge(&self) -> Result<()>;

    /// Acquires an IP address on the network and configures the routing tables
    fn dhcp_acquire(&self) -> Result<Vec<IpAddr>>;

    /// Adds a static IP address to the interface with a netmask prefix
    fn ip_add(&self, ip: IpAddr, prefix: u8) -> Result<()>;

    /// Removes a static (or dynamic) IP address from the interface
    fn ip_remove(&self, ip: IpAddr) -> Result<()>;

    /// Clears all the assigned IP addresses for this interface
    fn ip_clear(&self) -> Result<()>;

    /// Lists all the IP addresses currently assigned to this interface
    fn ip_list(&self) -> Result<Vec<IpCidr>>;

    /// Returns the hardware MAC address for this interface
    fn mac(&self) -> Result<[u8; 6]>;

    /// Adds a default gateway to the routing table
    fn gateway_set(&self, ip: IpAddr) -> Result<()>;

    /// Adds a specific route to the routing table
    fn route_add(
        &self,
        cidr: IpCidr,
        via_router: IpAddr,
        preferred_until: Option<Duration>,
        expires_at: Option<Duration>,
    ) -> Result<()>;

    /// Removes a routing rule from the routing table
    fn route_remove(&self, cidr: IpAddr) -> Result<()>;

    /// Clears the routing table for this interface
    fn route_clear(&self) -> Result<()>;

    /// Lists all the routes defined in the routing table for this interface
    fn route_list(&self) -> Result<Vec<IpRoute>>;

    /// Creates a low level socket that can read and write Ethernet packets
    /// directly to the interface
    fn bind_raw(&self) -> Result<Box<dyn VirtualRawSocket + Sync>>;

    /// Lists for TCP connections on a specific IP and Port combination
    /// Multiple servers (processes or threads) can bind to the same port if they each set
    /// the reuse-port and-or reuse-addr flags
    fn listen_tcp(
        &self,
        addr: SocketAddr,
        only_v6: bool,
        reuse_port: bool,
        reuse_addr: bool,
    ) -> Result<Box<dyn VirtualTcpListener + Sync>>;

    /// Opens a UDP socket that listens on a specific IP and Port combination
    /// Multiple servers (processes or threads) can bind to the same port if they each set
    /// the reuse-port and-or reuse-addr flags
    fn bind_udp(
        &self,
        addr: SocketAddr,
        reuse_port: bool,
        reuse_addr: bool,
    ) -> Result<Box<dyn VirtualUdpSocket + Sync>>;

    /// Creates a socket that can be used to send and receive ICMP packets
    /// from a paritcular IP address
    fn bind_icmp(&self, addr: IpAddr) -> Result<Box<dyn VirtualIcmpSocket + Sync>>;

    /// Opens a TCP connection to a particular destination IP address and port
    fn connect_tcp(
        &self,
        addr: SocketAddr,
        peer: SocketAddr,
        timeout: Option<Duration>,
    ) -> Result<Box<dyn VirtualTcpSocket + Sync>>;

    /// Performs DNS resolution for a specific hostname
    fn resolve(
        &self,
        host: &str,
        port: Option<u16>,
        dns_server: Option<IpAddr>,
    ) -> Result<Vec<IpAddr>>;
}

/// Holds the interface used to work with a pending HTTP request
#[derive(Debug)]
pub struct SocketHttpRequest {
    /// Used to send the request bytes to the HTTP server
    /// (once all bytes are send the sender should be closed)
    pub request: Option<mpsc::Sender<Vec<u8>>>,
    /// Used to receive the response bytes from the HTTP server
    /// (once all the bytes have been received the receiver will be closed)
    pub response: Option<mpsc::Receiver<Vec<u8>>>,
    /// Used to receive all the headers from the HTTP server
    /// (once all the headers have been received the receiver will be closed)
    pub headers: Option<mpsc::Receiver<(String, String)>>,
    /// Used to watch for the status
    pub status: Arc<Mutex<mpsc::Receiver<Result<HttpStatus>>>>,
}

/// Represents the final result of a HTTP request
#[derive(Debug)]
pub struct HttpStatus {
    /// Indicates if the HTTP request was redirected to another URL / server
    pub redirected: bool,
    /// Size of the data held in the response receiver
    pub size: usize,
    /// Status code returned by the server
    pub status: u16,
    /// Status text returned by the server
    pub status_text: String,
}

#[derive(Debug)]
pub struct SocketReceive {
    /// Data that was received
    pub data: Bytes,
    /// Indicates if the data was truncated (e.g. UDP packet)
    pub truncated: bool,
}

#[derive(Debug)]
pub struct SocketReceiveFrom {
    /// Data that was received
    pub data: Bytes,
    /// Indicates if the data was truncated (e.g. UDP packet)
    pub truncated: bool,
    /// Peer sender address of the data
    pub addr: SocketAddr,
}

pub trait VirtualTcpListener: fmt::Debug + Send + Sync + 'static {
    /// Accepts an connection attempt that was made to this listener
    fn accept(&self) -> Result<(Box<dyn VirtualTcpSocket + Sync>, SocketAddr)>;

    /// Accepts an connection attempt that was made to this listener (or times out)
    fn accept_timeout(
        &self,
        timeout: Duration,
    ) -> Result<(Box<dyn VirtualTcpSocket + Sync>, SocketAddr)>;

    /// Sets the accept timeout
    fn set_timeout(&mut self, timeout: Option<Duration>) -> Result<()>;

    /// Gets the accept timeout
    fn timeout(&self) -> Result<Option<Duration>>;

    /// Returns the local address of this TCP listener
    fn addr_local(&self) -> Result<SocketAddr>;

    /// Sets how many network hops the packets are permitted for new connections
    fn set_ttl(&mut self, ttl: u8) -> Result<()>;

    /// Returns the maximum number of network hops before packets are dropped
    fn ttl(&self) -> Result<u8>;
}

pub trait VirtualSocket: fmt::Debug + Send + Sync + 'static {
    /// Sets how many network hops the packets are permitted for new connections
    fn set_ttl(&mut self, ttl: u32) -> Result<()>;

    /// Returns the maximum number of network hops before packets are dropped
    fn ttl(&self) -> Result<u32>;

    /// Returns the local address for this socket
    fn addr_local(&self) -> Result<SocketAddr>;

    /// Returns the status/state of the socket
    fn status(&self) -> Result<SocketStatus>;
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SocketStatus {
    Opening,
    Opened,
    Closed,
    Failed,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum StreamSecurity {
    Unencrypted,
    AnyEncyption,
    ClassicEncryption,
    DoubleEncryption,
}

/// Interface used for sending and receiving data from a web socket
pub trait VirtualWebSocket: fmt::Debug + Send + Sync + 'static {
    /// Sends out a datagram or stream of bytes on this socket
    fn send(&mut self, data: Bytes) -> Result<usize>;

    /// FLushes all the datagrams
    fn flush(&mut self) -> Result<()>;

    /// Recv a packet from the socket
    fn recv(&mut self) -> Result<SocketReceive>;
}

/// Connected sockets have a persistent connection to a remote peer
pub trait VirtualConnectedSocket: VirtualSocket + fmt::Debug + Send + Sync + 'static {
    /// Determines how long the socket will remain in a TIME_WAIT
    /// after it disconnects (only the one that initiates the close will
    /// be in a TIME_WAIT state thus the clients should always do this rather
    /// than the server)
    fn set_linger(&mut self, linger: Option<Duration>) -> Result<()>;

    /// Returns how long the socket will remain in a TIME_WAIT
    /// after it disconnects
    fn linger(&self) -> Result<Option<Duration>>;

    /// Sends out a datagram or stream of bytes on this socket
    fn send(&mut self, data: Bytes) -> Result<usize>;

    /// FLushes all the datagrams
    fn flush(&mut self) -> Result<()>;

    /// Recv a packet from the socket
    fn recv(&mut self) -> Result<SocketReceive>;

    /// Peeks for a packet from the socket
    fn peek(&mut self) -> Result<SocketReceive>;
}

/// Connectionless sockets are able to send and receive datagrams and stream
/// bytes to multiple addresses at the same time (peer-to-peer)
pub trait VirtualConnectionlessSocket: VirtualSocket + fmt::Debug + Send + Sync + 'static {
    /// Sends out a datagram or stream of bytes on this socket
    /// to a specific address
    fn send_to(&mut self, data: Bytes, addr: SocketAddr) -> Result<usize>;

    /// Recv a packet from the socket
    fn recv_from(&mut self) -> Result<SocketReceiveFrom>;

    /// Peeks for a packet from the socket
    fn peek_from(&mut self) -> Result<SocketReceiveFrom>;
}

/// ICMP sockets are low level devices bound to a specific address
/// that can send and receive ICMP packets
pub trait VirtualIcmpSocket:
    VirtualConnectionlessSocket + fmt::Debug + Send + Sync + 'static
{
}

pub trait VirtualRawSocket: VirtualSocket + fmt::Debug + Send + Sync + 'static {
    /// Sends out a raw packet on this socket
    fn send(&mut self, data: Bytes) -> Result<usize>;

    /// FLushes all the datagrams
    fn flush(&mut self) -> Result<()>;

    /// Recv a packet from the socket
    fn recv(&mut self) -> Result<SocketReceive>;

    /// Tells the raw socket and its backing switch that all packets
    /// should be received by this socket even if they are not
    /// destined for this device
    fn set_promiscuous(&mut self, promiscuous: bool) -> Result<()>;

    /// Returns if the socket is running in promiscuous mode whereby it
    /// will receive all packets even if they are not destined for the
    /// local interface
    fn promiscuous(&self) -> Result<bool>;
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TimeType {
    ReadTimeout,
    WriteTimeout,
    AcceptTimeout,
    ConnectTimeout,
    Linger,
}

pub trait VirtualTcpSocket: VirtualConnectedSocket + fmt::Debug + Send + Sync + 'static {
    /// Sets the timeout for a specific action on the socket
    fn set_opt_time(&mut self, ty: TimeType, timeout: Option<Duration>) -> Result<()>;

    /// Returns one of the previous set timeouts
    fn opt_time(&self, ty: TimeType) -> Result<Option<Duration>>;

    /// Sets the receive buffer size which acts as a trottle for how
    /// much data is buffered on this side of the pipe
    fn set_recv_buf_size(&mut self, size: usize) -> Result<()>;

    /// Size of the receive buffer that holds all data that has not
    /// yet been read
    fn recv_buf_size(&self) -> Result<usize>;

    /// Sets the size of the send buffer which will hold the bytes of
    /// data while they are being sent over to the peer
    fn set_send_buf_size(&mut self, size: usize) -> Result<()>;

    /// Size of the send buffer that holds all data that is currently
    /// being transmitted.
    fn send_buf_size(&self) -> Result<usize>;

    /// When NO_DELAY is set the data that needs to be transmitted to
    /// the peer is sent immediately rather than waiting for a bigger
    /// batch of data, this reduces latency but increases encapsulation
    /// overhead.
    fn set_nodelay(&mut self, reuse: bool) -> Result<()>;

    /// Indicates if the NO_DELAY flag is set which means that data
    /// is immediately sent to the peer without waiting. This reduces
    /// latency but increases encapsulation overhead.
    fn nodelay(&self) -> Result<bool>;

    /// Returns the address (IP and Port) of the peer socket that this
    /// is conencted to
    fn addr_peer(&self) -> Result<SocketAddr>;

    /// Causes all the data held in the send buffer to be immediately
    /// flushed to the destination peer
    fn flush(&mut self) -> Result<()>;

    /// Shuts down either the READER or WRITER sides of the socket
    /// connection.
    fn shutdown(&mut self, how: Shutdown) -> Result<()>;
}

pub trait VirtualUdpSocket:
    VirtualConnectedSocket + VirtualConnectionlessSocket + fmt::Debug + Send + Sync + 'static
{
    /// Connects to a destination peer so that the normal
    /// send/recv operations can be used.
    fn connect(&mut self, addr: SocketAddr) -> Result<()>;

    /// Sets a flag that means that the UDP socket is able
    /// to receive and process broadcast packets.
    fn set_broadcast(&mut self, broadcast: bool) -> Result<()>;

    /// Indicates if the SO_BROADCAST flag is set which means
    /// that the UDP socket will receive and process broadcast
    /// packets
    fn broadcast(&self) -> Result<bool>;

    /// Sets a flag that indicates if multicast packets that
    /// this socket is a member of will be looped back to
    /// the sending socket. This applies to IPv4 addresses
    fn set_multicast_loop_v4(&mut self, val: bool) -> Result<()>;

    /// Gets a flag that indicates if multicast packets that
    /// this socket is a member of will be looped back to
    /// the sending socket. This applies to IPv4 addresses
    fn multicast_loop_v4(&self) -> Result<bool>;

    /// Sets a flag that indicates if multicast packets that
    /// this socket is a member of will be looped back to
    /// the sending socket. This applies to IPv6 addresses
    fn set_multicast_loop_v6(&mut self, val: bool) -> Result<()>;

    /// Gets a flag that indicates if multicast packets that
    /// this socket is a member of will be looped back to
    /// the sending socket. This applies to IPv6 addresses
    fn multicast_loop_v6(&self) -> Result<bool>;

    /// Sets the TTL for IPv4 multicast packets which is the
    /// number of network hops before the packet is dropped
    fn set_multicast_ttl_v4(&mut self, ttl: u32) -> Result<()>;

    /// Gets the TTL for IPv4 multicast packets which is the
    /// number of network hops before the packet is dropped
    fn multicast_ttl_v4(&self) -> Result<u32>;

    /// Tells this interface that it will subscribe to a
    /// particular multicast address. This applies to IPv4 addresses
    fn join_multicast_v4(&mut self, multiaddr: Ipv4Addr, iface: Ipv4Addr) -> Result<()>;

    /// Tells this interface that it will unsubscribe to a
    /// particular multicast address. This applies to IPv4 addresses
    fn leave_multicast_v4(&mut self, multiaddr: Ipv4Addr, iface: Ipv4Addr) -> Result<()>;

    /// Tells this interface that it will subscribe to a
    /// particular multicast address. This applies to IPv6 addresses
    fn join_multicast_v6(&mut self, multiaddr: Ipv6Addr, iface: u32) -> Result<()>;

    /// Tells this interface that it will unsubscribe to a
    /// particular multicast address. This applies to IPv6 addresses
    fn leave_multicast_v6(&mut self, multiaddr: Ipv6Addr, iface: u32) -> Result<()>;

    /// Returns the remote address of this UDP socket if it has been
    /// connected to a specific target destination address
    fn addr_peer(&self) -> Result<Option<SocketAddr>>;
}

#[derive(Debug, Default)]
pub struct UnsupportedVirtualNetworking {}

impl VirtualNetworking for UnsupportedVirtualNetworking {
    fn ws_connect(&self, _url: &str) -> Result<Box<dyn VirtualWebSocket + Sync>> {
        Err(NetworkError::Unsupported)
    }

    fn http_request(
        &self,
        _url: &str,
        _method: &str,
        _headers: &str,
        _gzip: bool,
    ) -> Result<SocketHttpRequest> {
        Err(NetworkError::Unsupported)
    }

    fn bridge(&self, _network: &str, _access_token: &str, _security: StreamSecurity) -> Result<()> {
        Err(NetworkError::Unsupported)
    }

    fn unbridge(&self) -> Result<()> {
        Err(NetworkError::Unsupported)
    }

    fn dhcp_acquire(&self) -> Result<Vec<IpAddr>> {
        Err(NetworkError::Unsupported)
    }

    fn ip_add(&self, _ip: IpAddr, _prefix: u8) -> Result<()> {
        Err(NetworkError::Unsupported)
    }

    fn ip_remove(&self, _ip: IpAddr) -> Result<()> {
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

    fn gateway_set(&self, _ip: IpAddr) -> Result<()> {
        Err(NetworkError::Unsupported)
    }

    fn route_add(
        &self,
        _cidr: IpCidr,
        _via_router: IpAddr,
        _preferred_until: Option<Duration>,
        _expires_at: Option<Duration>,
    ) -> Result<()> {
        Err(NetworkError::Unsupported)
    }

    fn route_remove(&self, _cidr: IpAddr) -> Result<()> {
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

    fn bind_icmp(&self, _addr: IpAddr) -> Result<Box<dyn VirtualIcmpSocket + Sync>> {
        Err(NetworkError::Unsupported)
    }

    fn listen_tcp(
        &self,
        _addr: SocketAddr,
        _only_v6: bool,
        _reuse_port: bool,
        _reuse_addr: bool,
    ) -> Result<Box<dyn VirtualTcpListener + Sync>> {
        Err(NetworkError::Unsupported)
    }

    fn connect_tcp(
        &self,
        _addr: SocketAddr,
        _peer: SocketAddr,
        _timeout: Option<Duration>,
    ) -> Result<Box<dyn VirtualTcpSocket + Sync>> {
        Err(NetworkError::Unsupported)
    }

    fn bind_udp(
        &self,
        _addr: SocketAddr,
        _reuse_port: bool,
        _reuse_addr: bool,
    ) -> Result<Box<dyn VirtualUdpSocket + Sync>> {
        Err(NetworkError::Unsupported)
    }

    fn resolve(
        &self,
        _host: &str,
        _port: Option<u16>,
        _dns_server: Option<IpAddr>,
    ) -> Result<Vec<IpAddr>> {
        Err(NetworkError::Unsupported)
    }
}

#[derive(Error, Copy, Clone, Debug, PartialEq, Eq)]
pub enum NetworkError {
    /// The handle given was not usable
    #[error("invalid fd")]
    InvalidFd,
    /// File exists
    #[error("file exists")]
    AlreadyExists,
    /// The filesystem has failed to lock a resource.
    #[error("lock error")]
    Lock,
    /// Something failed when doing IO. These errors can generally not be handled.
    /// It may work if tried again.
    #[error("io error")]
    IOError,
    /// The address was in use
    #[error("address is in use")]
    AddressInUse,
    /// The address could not be found
    #[error("address could not be found")]
    AddressNotAvailable,
    /// A pipe was closed
    #[error("broken pipe (was closed)")]
    BrokenPipe,
    /// The connection was aborted
    #[error("connection aborted")]
    ConnectionAborted,
    /// The connection request was refused
    #[error("connection refused")]
    ConnectionRefused,
    /// The connection was reset
    #[error("connection reset")]
    ConnectionReset,
    /// The operation was interrupted before it could finish
    #[error("operation interrupted")]
    Interrupted,
    /// Invalid internal data, if the argument data is invalid, use `InvalidInput`
    #[error("invalid internal data")]
    InvalidData,
    /// The provided data is invalid
    #[error("invalid input")]
    InvalidInput,
    /// Could not perform the operation because there was not an open connection
    #[error("connection is not open")]
    NotConnected,
    /// The requested device couldn't be accessed
    #[error("can't access device")]
    NoDevice,
    /// Caller was not allowed to perform this operation
    #[error("permission denied")]
    PermissionDenied,
    /// The operation did not complete within the given amount of time
    #[error("time out")]
    TimedOut,
    /// Found EOF when EOF was not expected
    #[error("unexpected eof")]
    UnexpectedEof,
    /// Operation would block, this error lets the caller know that they can try again
    #[error("blocking operation. try again")]
    WouldBlock,
    /// A call to write returned 0
    #[error("write returned 0")]
    WriteZero,
    /// The operation is not supported.
    #[error("unsupported")]
    Unsupported,
    /// Some other unhandled error. If you see this, it's probably a bug.
    #[error("unknown error found")]
    UnknownError,
}

pub fn net_error_into_io_err(net_error: NetworkError) -> std::io::Error {
    use std::io::ErrorKind;
    match net_error {
        NetworkError::InvalidFd => ErrorKind::BrokenPipe.into(),
        NetworkError::AlreadyExists => ErrorKind::AlreadyExists.into(),
        NetworkError::Lock => ErrorKind::BrokenPipe.into(),
        NetworkError::IOError => ErrorKind::BrokenPipe.into(),
        NetworkError::AddressInUse => ErrorKind::AddrInUse.into(),
        NetworkError::AddressNotAvailable => ErrorKind::AddrNotAvailable.into(),
        NetworkError::BrokenPipe => ErrorKind::BrokenPipe.into(),
        NetworkError::ConnectionAborted => ErrorKind::ConnectionAborted.into(),
        NetworkError::ConnectionRefused => ErrorKind::ConnectionRefused.into(),
        NetworkError::ConnectionReset => ErrorKind::ConnectionReset.into(),
        NetworkError::Interrupted => ErrorKind::Interrupted.into(),
        NetworkError::InvalidData => ErrorKind::InvalidData.into(),
        NetworkError::InvalidInput => ErrorKind::InvalidInput.into(),
        NetworkError::NotConnected => ErrorKind::NotConnected.into(),
        NetworkError::NoDevice => ErrorKind::BrokenPipe.into(),
        NetworkError::PermissionDenied => ErrorKind::PermissionDenied.into(),
        NetworkError::TimedOut => ErrorKind::TimedOut.into(),
        NetworkError::UnexpectedEof => ErrorKind::UnexpectedEof.into(),
        NetworkError::WouldBlock => ErrorKind::WouldBlock.into(),
        NetworkError::WriteZero => ErrorKind::WriteZero.into(),
        NetworkError::Unsupported => ErrorKind::Unsupported.into(),
        NetworkError::UnknownError => ErrorKind::BrokenPipe.into(),
    }
}

pub fn io_err_into_net_error(net_error: std::io::Error) -> NetworkError {
    use std::io::ErrorKind;
    match net_error.kind() {
        ErrorKind::BrokenPipe => NetworkError::BrokenPipe,
        ErrorKind::AlreadyExists => NetworkError::AlreadyExists,
        ErrorKind::AddrInUse => NetworkError::AddressInUse,
        ErrorKind::AddrNotAvailable => NetworkError::AddressNotAvailable,
        ErrorKind::ConnectionAborted => NetworkError::ConnectionAborted,
        ErrorKind::ConnectionRefused => NetworkError::ConnectionRefused,
        ErrorKind::ConnectionReset => NetworkError::ConnectionReset,
        ErrorKind::Interrupted => NetworkError::Interrupted,
        ErrorKind::InvalidData => NetworkError::InvalidData,
        ErrorKind::InvalidInput => NetworkError::InvalidInput,
        ErrorKind::NotConnected => NetworkError::NotConnected,
        ErrorKind::PermissionDenied => NetworkError::PermissionDenied,
        ErrorKind::TimedOut => NetworkError::TimedOut,
        ErrorKind::UnexpectedEof => NetworkError::UnexpectedEof,
        ErrorKind::WouldBlock => NetworkError::WouldBlock,
        ErrorKind::WriteZero => NetworkError::WriteZero,
        ErrorKind::Unsupported => NetworkError::Unsupported,
        _ => NetworkError::UnknownError,
    }
}
