use std::fmt;
use std::mem::MaybeUninit;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::Ipv6Addr;
use std::net::Shutdown;
use std::net::SocketAddr;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;
use thiserror::Error;

pub use bytes::Bytes;
pub use bytes::BytesMut;

pub type Result<T> = std::result::Result<T, NetworkError>;

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
#[async_trait::async_trait]
#[allow(unused_variables)]
pub trait VirtualNetworking: fmt::Debug + Send + Sync + 'static {
    /// Bridges this local network with a remote network, which is required in
    /// order to make lower level networking calls (such as UDP/TCP)
    async fn bridge(
        &self,
        network: &str,
        access_token: &str,
        security: StreamSecurity,
    ) -> Result<()> {
        Err(NetworkError::Unsupported)
    }

    /// Disconnects from the remote network essentially unbridging it
    async fn unbridge(&self) -> Result<()> {
        Err(NetworkError::Unsupported)
    }

    /// Acquires an IP address on the network and configures the routing tables
    async fn dhcp_acquire(&self) -> Result<Vec<IpAddr>> {
        Err(NetworkError::Unsupported)
    }

    /// Adds a static IP address to the interface with a netmask prefix
    fn ip_add(&self, ip: IpAddr, prefix: u8) -> Result<()> {
        Err(NetworkError::Unsupported)
    }

    /// Removes a static (or dynamic) IP address from the interface
    fn ip_remove(&self, ip: IpAddr) -> Result<()> {
        Err(NetworkError::Unsupported)
    }

    /// Clears all the assigned IP addresses for this interface
    fn ip_clear(&self) -> Result<()> {
        Err(NetworkError::Unsupported)
    }

    /// Lists all the IP addresses currently assigned to this interface
    fn ip_list(&self) -> Result<Vec<IpCidr>> {
        Err(NetworkError::Unsupported)
    }

    /// Returns the hardware MAC address for this interface
    fn mac(&self) -> Result<[u8; 6]> {
        Err(NetworkError::Unsupported)
    }

    /// Adds a default gateway to the routing table
    fn gateway_set(&self, ip: IpAddr) -> Result<()> {
        Err(NetworkError::Unsupported)
    }

    /// Adds a specific route to the routing table
    fn route_add(
        &self,
        cidr: IpCidr,
        via_router: IpAddr,
        preferred_until: Option<Duration>,
        expires_at: Option<Duration>,
    ) -> Result<()> {
        Err(NetworkError::Unsupported)
    }

    /// Removes a routing rule from the routing table
    fn route_remove(&self, cidr: IpAddr) -> Result<()> {
        Err(NetworkError::Unsupported)
    }

    /// Clears the routing table for this interface
    fn route_clear(&self) -> Result<()> {
        Err(NetworkError::Unsupported)
    }

    /// Lists all the routes defined in the routing table for this interface
    fn route_list(&self) -> Result<Vec<IpRoute>> {
        Err(NetworkError::Unsupported)
    }

    /// Creates a low level socket that can read and write Ethernet packets
    /// directly to the interface
    async fn bind_raw(&self) -> Result<Box<dyn VirtualRawSocket + Sync>> {
        Err(NetworkError::Unsupported)
    }

    /// Lists for TCP connections on a specific IP and Port combination
    /// Multiple servers (processes or threads) can bind to the same port if they each set
    /// the reuse-port and-or reuse-addr flags
    async fn listen_tcp(
        &self,
        addr: SocketAddr,
        only_v6: bool,
        reuse_port: bool,
        reuse_addr: bool,
    ) -> Result<Box<dyn VirtualTcpListener + Sync>> {
        Err(NetworkError::Unsupported)
    }

    /// Opens a UDP socket that listens on a specific IP and Port combination
    /// Multiple servers (processes or threads) can bind to the same port if they each set
    /// the reuse-port and-or reuse-addr flags
    async fn bind_udp(
        &self,
        addr: SocketAddr,
        reuse_port: bool,
        reuse_addr: bool,
    ) -> Result<Box<dyn VirtualUdpSocket + Sync>> {
        Err(NetworkError::Unsupported)
    }

    /// Creates a socket that can be used to send and receive ICMP packets
    /// from a paritcular IP address
    async fn bind_icmp(&self, addr: IpAddr) -> Result<Box<dyn VirtualIcmpSocket + Sync>> {
        Err(NetworkError::Unsupported)
    }

    /// Opens a TCP connection to a particular destination IP address and port
    async fn connect_tcp(
        &self,
        addr: SocketAddr,
        peer: SocketAddr,
    ) -> Result<Box<dyn VirtualTcpSocket + Sync>> {
        Err(NetworkError::Unsupported)
    }

    /// Performs DNS resolution for a specific hostname
    async fn resolve(
        &self,
        host: &str,
        port: Option<u16>,
        dns_server: Option<IpAddr>,
    ) -> Result<Vec<IpAddr>> {
        Err(NetworkError::Unsupported)
    }
}

pub type DynVirtualNetworking = Arc<dyn VirtualNetworking>;

pub trait VirtualTcpListener: fmt::Debug + Send + Sync + 'static {
    /// Tries to accept a new connection
    fn try_accept(&mut self) -> Option<Result<(Box<dyn VirtualTcpSocket + Sync>, SocketAddr)>>;

    /// Polls the socket for new connections
    fn poll_accept(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(Box<dyn VirtualTcpSocket + Sync>, SocketAddr)>>;

    /// Polls the socket for when there is data to be received
    fn poll_accept_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<usize>>;

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

    /// Polls the socket for when there is data to be received
    fn poll_read_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<usize>>;

    /// Polls the socket for when the backpressure allows for writing to the socket
    fn poll_write_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<usize>>;
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

    /// Tries to send out a datagram or stream of bytes on this socket
    fn try_send(&mut self, data: &[u8]) -> Result<usize>;

    /// Sends out a datagram or stream of bytes on this socket
    fn poll_send(&mut self, cx: &mut Context<'_>, data: &[u8]) -> Poll<Result<usize>>;

    /// Attempts to flush the object, ensuring that any buffered data reach
    /// their destination.
    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<()>>;

    /// Closes the socket
    fn close(&mut self) -> Result<()>;

    /// Recv a packet from the socket
    fn poll_recv<'a>(
        &mut self,
        cx: &mut Context<'_>,
        buf: &'a mut [MaybeUninit<u8>],
    ) -> Poll<Result<usize>>;

    /// Recv a packet from the socket
    fn try_recv(&mut self, buf: &mut [MaybeUninit<u8>]) -> Result<usize>;
}

/// Connectionless sockets are able to send and receive datagrams and stream
/// bytes to multiple addresses at the same time (peer-to-peer)
pub trait VirtualConnectionlessSocket: VirtualSocket + fmt::Debug + Send + Sync + 'static {
    /// Sends out a datagram or stream of bytes on this socket
    /// to a specific address
    fn poll_send_to(
        &mut self,
        cx: &mut Context<'_>,
        data: &[u8],
        addr: SocketAddr,
    ) -> Poll<Result<usize>>;

    /// Sends out a datagram or stream of bytes on this socket
    /// to a specific address
    fn try_send_to(&mut self, data: &[u8], addr: SocketAddr) -> Result<usize>;

    /// Recv a packet from the socket
    fn poll_recv_from<'a>(
        &mut self,
        cx: &mut Context<'_>,
        buf: &'a mut [MaybeUninit<u8>],
    ) -> Poll<Result<(usize, SocketAddr)>>;

    /// Recv a packet from the socket
    fn try_recv_from(&mut self, buf: &mut [MaybeUninit<u8>]) -> Result<(usize, SocketAddr)>;
}

/// ICMP sockets are low level devices bound to a specific address
/// that can send and receive ICMP packets
pub trait VirtualIcmpSocket:
    VirtualConnectionlessSocket + fmt::Debug + Send + Sync + 'static
{
}

pub trait VirtualRawSocket: VirtualSocket + fmt::Debug + Send + Sync + 'static {
    /// Sends out a datagram or stream of bytes on this socket
    fn poll_send(&mut self, cx: &mut Context<'_>, data: &[u8]) -> Poll<Result<usize>>;

    /// Sends out a datagram or stream of bytes on this socket
    fn try_send(&mut self, data: &[u8]) -> Result<usize>;

    /// Attempts to flush the object, ensuring that any buffered data reach
    /// their destination.
    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<()>>;

    /// Attempts to flush the object, ensuring that any buffered data reach
    /// their destination.
    fn try_flush(&mut self) -> Result<()>;

    /// Recv a packet from the socket
    fn poll_recv<'a>(
        &mut self,
        cx: &mut Context<'_>,
        buf: &'a mut [MaybeUninit<u8>],
    ) -> Poll<Result<usize>>;

    /// Recv a packet from the socket
    fn try_recv(&mut self, buf: &mut [MaybeUninit<u8>]) -> Result<usize>;

    /// Tells the raw socket and its backing switch that all packets
    /// should be received by this socket even if they are not
    /// destined for this device
    fn set_promiscuous(&mut self, promiscuous: bool) -> Result<()>;

    /// Returns if the socket is running in promiscuous mode whereby it
    /// will receive all packets even if they are not destined for the
    /// local interface
    fn promiscuous(&self) -> Result<bool>;
}

pub trait VirtualTcpSocket: VirtualConnectedSocket + fmt::Debug + Send + Sync + 'static {
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

    /// Shuts down either the READER or WRITER sides of the socket
    /// connection.
    fn shutdown(&mut self, how: Shutdown) -> Result<()>;

    /// Return true if the socket is closed
    fn is_closed(&self) -> bool;
}

pub trait VirtualUdpSocket:
    VirtualConnectionlessSocket + fmt::Debug + Send + Sync + 'static
{
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

#[async_trait::async_trait]
impl VirtualNetworking for UnsupportedVirtualNetworking {}

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
    /// Insufficient memory
    #[error("Insufficient memory")]
    InsufficientMemory,
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
    /// Too many open files
    #[error("too many open files")]
    TooManyOpenFiles,
    /// The operation is not supported.
    #[error("unsupported")]
    Unsupported,
    /// Some other unhandled error. If you see this, it's probably a bug.
    #[error("unknown error found")]
    UnknownError,
}
