use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::Ipv6Addr;
use std::net::SocketAddr;
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub use super::IpCidr;
pub use super::IpRoute;
pub use super::NetworkError;
pub use super::SocketStatus;
pub use super::StreamSecurity;

/// Represents a socket ID
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SocketId(u64);

impl From<u64> for SocketId {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum FrameSerializationFormat {
    Bincode,
    #[cfg(feature = "json")]
    Json,
    #[cfg(feature = "messagepack")]
    MessagePack,
    #[cfg(feature = "cbor")]
    Cbor,
}

/// Possible values which can be passed to the `TcpStream::shutdown` method.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Shutdown {
    /// Shut down the reading portion of the stream.
    Read,
    /// Shut down the writing portion of the stream.
    Write,
    /// Shut down both the reading and writing portions of the stream.
    Both,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RequestType {
    /// Bridges this local network with a remote network, which is required in
    /// order to make lower level networking calls (such as UDP/TCP)
    Bridge {
        network: String,
        access_token: String,
        security: StreamSecurity,
    },
    /// Flushes all the data by ensuring a full round trip is completed
    Flush,
    /// Disconnects from the remote network essentially unbridging it
    Unbridge,
    /// Acquires an IP address on the network and configures the routing tables
    DhcpAcquire,
    /// Adds a static IP address to the interface with a netmask prefix
    IpAdd { ip: IpAddr, prefix: u8 },
    /// Removes a static (or dynamic) IP address from the interface
    IpRemove(IpAddr),
    /// Clears all the assigned IP addresses for this interface
    IpClear,
    /// Lists all the IP addresses currently assigned to this interface
    GetIpList,
    /// Returns the hardware MAC address for this interface
    GetMac,
    /// Adds a default gateway to the routing table
    GatewaySet(IpAddr),
    /// Adds a specific route to the routing table
    RouteAdd {
        cidr: IpCidr,
        via_router: IpAddr,
        preferred_until: Option<Duration>,
        expires_at: Option<Duration>,
    },
    /// Removes a routing rule from the routing table
    RouteRemove(IpAddr),
    /// Clears the routing table for this interface
    RouteClear,
    /// Lists all the routes defined in the routing table for this interface
    GetRouteList,
    /// Creates a low level socket that can read and write Ethernet packets
    /// directly to the interface
    BindRaw(SocketId),
    /// Lists for TCP connections on a specific IP and Port combination
    /// Multiple servers (processes or threads) can bind to the same port if they each set
    /// the reuse-port and-or reuse-addr flags
    ListenTcp {
        socket_id: SocketId,
        addr: SocketAddr,
        only_v6: bool,
        reuse_port: bool,
        reuse_addr: bool,
    },
    /// Binds a TCP socket without immediately listening or connecting.
    BindTcp {
        socket_id: SocketId,
        addr: SocketAddr,
        only_v6: bool,
        reuse_port: bool,
        reuse_addr: bool,
    },
    /// Opens a UDP socket that listens on a specific IP and Port combination
    /// Multiple servers (processes or threads) can bind to the same port if they each set
    /// the reuse-port and-or reuse-addr flags
    BindUdp {
        socket_id: SocketId,
        addr: SocketAddr,
        reuse_port: bool,
        reuse_addr: bool,
    },
    /// Creates a socket that can be used to send and receive ICMP packets
    /// from a paritcular IP address
    BindIcmp { socket_id: SocketId, addr: IpAddr },
    /// Opens a TCP connection to a particular destination IP address and port
    ConnectTcp {
        socket_id: SocketId,
        addr: SocketAddr,
        peer: SocketAddr,
    },
    /// Performs DNS resolution for a specific hostname
    Resolve {
        host: String,
        port: Option<u16>,
        dns_server: Option<IpAddr>,
    },
    /// Closes the socket
    Close,
    /// Converts a bound TCP socket into a listening socket.
    ListenBound,
    /// Converts a bound TCP socket into a connected TCP stream.
    ConnectBound { peer: SocketAddr },
    /// Begins the process of accepting a socket and returns it later
    BeginAccept(SocketId),
    /// Returns the local address of this TCP listener
    GetAddrLocal,
    /// Returns the address (IP and Port) of the peer socket that this
    /// is connected to
    GetAddrPeer,
    /// Sets how many network hops the packets are permitted for new connections
    SetTtl(u32),
    /// Returns the maximum number of network hops before packets are dropped
    GetTtl,
    /// Returns the status/state of the socket
    GetStatus,
    /// Determines how long the socket will remain in a TIME_WAIT
    /// after it disconnects (only the one that initiates the close will
    /// be in a TIME_WAIT state thus the clients should always do this rather
    /// than the server)
    SetLinger(Option<Duration>),
    /// Returns how long the socket will remain in a TIME_WAIT
    /// after it disconnects
    GetLinger,
    /// Tells the raw socket and its backing switch that all packets
    /// should be received by this socket even if they are not
    /// destined for this device
    SetPromiscuous(bool),
    /// Returns if the socket is running in promiscuous mode whereby it
    /// will receive all packets even if they are not destined for the
    /// local interface
    GetPromiscuous,
    /// Sets the receive buffer size which acts as a throttle for how
    /// much data is buffered on this side of the pipe
    SetRecvBufSize(u64),
    /// Size of the receive buffer that holds all data that has not
    /// yet been read
    GetRecvBufSize,
    /// Sets the size of the send buffer which will hold the bytes of
    /// data while they are being sent over to the peer
    SetSendBufSize(u64),
    /// Size of the send buffer that holds all data that is currently
    /// being transmitted.
    GetSendBufSize,
    /// When NO_DELAY is set the data that needs to be transmitted to
    /// the peer is sent immediately rather than waiting for a bigger
    /// batch of data, this reduces latency but increases encapsulation
    /// overhead.
    SetNoDelay(bool),
    /// Indicates if the NO_DELAY flag is set which means that data
    /// is immediately sent to the peer without waiting. This reduces
    /// latency but increases encapsulation overhead.
    GetNoDelay,
    /// When KEEP_ALIVE is set the connection will periodically send
    /// an empty data packet to the server to make sure the connection
    /// stays alive.
    SetKeepAlive(bool),
    /// Indicates if the KEEP_ALIVE flag is set which means that the
    /// socket will periodically send an empty data packet to keep
    /// the connection alive.
    GetKeepAlive,
    /// When DONT_ROUTE is set the packet will be sent directly
    /// to the interface without passing through the routing logic.
    SetDontRoute(bool),
    /// Indicates if the packet will pass straight through to
    /// the interface bypassing the routing logic.
    GetDontRoute,
    /// Shuts down either the READER or WRITER sides of the socket
    /// connection.
    Shutdown(Shutdown),
    /// Return true if the socket is closed
    IsClosed,
    /// Sets a flag that means that the UDP socket is able
    /// to receive and process broadcast packets.
    SetBroadcast(bool),
    /// Indicates if the SO_BROADCAST flag is set which means
    /// that the UDP socket will receive and process broadcast
    /// packets
    GetBroadcast,
    /// Sets a flag that indicates if multicast packets that
    /// this socket is a member of will be looped back to
    /// the sending socket. This applies to IPv4 addresses
    SetMulticastLoopV4(bool),
    /// Gets a flag that indicates if multicast packets that
    /// this socket is a member of will be looped back to
    /// the sending socket. This applies to IPv4 addresses
    GetMulticastLoopV4,
    /// Sets a flag that indicates if multicast packets that
    /// this socket is a member of will be looped back to
    /// the sending socket. This applies to IPv6 addresses
    SetMulticastLoopV6(bool),
    /// Gets a flag that indicates if multicast packets that
    /// this socket is a member of will be looped back to
    /// the sending socket. This applies to IPv6 addresses
    GetMulticastLoopV6,
    /// Sets the TTL for IPv4 multicast packets which is the
    /// number of network hops before the packet is dropped
    SetMulticastTtlV4(u32),
    /// Gets the TTL for IPv4 multicast packets which is the
    /// number of network hops before the packet is dropped
    GetMulticastTtlV4,
    /// Tells this interface that it will subscribe to a
    /// particular multicast address. This applies to IPv4 addresses
    JoinMulticastV4 {
        multiaddr: Ipv4Addr,
        iface: Ipv4Addr,
    },
    /// Tells this interface that it will unsubscribe to a
    /// particular multicast address. This applies to IPv4 addresses
    LeaveMulticastV4 {
        multiaddr: Ipv4Addr,
        iface: Ipv4Addr,
    },
    /// Tells this interface that it will subscribe to a
    /// particular multicast address. This applies to IPv6 addresses
    JoinMulticastV6 { multiaddr: Ipv6Addr, iface: u32 },
    /// Tells this interface that it will unsubscribe to a
    /// particular multicast address. This applies to IPv6 addresses
    LeaveMulticastV6 { multiaddr: Ipv6Addr, iface: u32 },
    /// Binds a UDP socket with explicit IPv6-only behavior.
    BindUdpV2 {
        socket_id: SocketId,
        addr: SocketAddr,
        only_v6: bool,
        reuse_port: bool,
        reuse_addr: bool,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ResponseType {
    /// Nothing is returned (or noop)
    None,
    /// An error has occurred
    Err(NetworkError),
    /// Represents a duration of time
    Duration(Duration),
    /// Represents an amount (e.g. amount of bytes)
    Amount(u64),
    /// Returns a flag of true or false
    Flag(bool),
    /// List of IP addresses
    IpAddressList(Vec<IpAddr>),
    /// A single IP address
    IpAddress(IpAddr),
    /// List of socket addresses
    SocketAddrList(Vec<SocketAddr>),
    /// A single IP address
    SocketAddr(SocketAddr),
    /// Represents a MAC address
    Mac([u8; 6]),
    /// List of CIDR routes from a routing table
    CidrList(Vec<IpCidr>),
    /// List of IP routes from a routing table
    RouteList(Vec<IpRoute>),
    /// Reference to a socket
    Socket(SocketId),
    /// The TTL of a packet
    Ttl(u32),
    /// The status of the socket
    Status(SocketStatus),
}

/// Message sent by the client to the server
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MessageRequest {
    Interface {
        req: RequestType,
        req_id: Option<u64>,
    },
    Socket {
        socket: SocketId,
        req: RequestType,
        req_id: Option<u64>,
    },
    Send {
        socket: SocketId,
        data: Vec<u8>,
        req_id: Option<u64>,
    },
    SendTo {
        socket: SocketId,
        data: Vec<u8>,
        addr: SocketAddr,
        req_id: Option<u64>,
    },
    Reconnect,
}

/// Message sent by the server back to a client
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MessageResponse {
    ResponseToRequest {
        req_id: u64,
        res: ResponseType,
    },
    Recv {
        socket_id: SocketId,
        data: Vec<u8>,
    },
    RecvWithAddr {
        socket_id: SocketId,
        data: Vec<u8>,
        addr: SocketAddr,
    },
    Sent {
        socket_id: SocketId,
        req_id: u64,
        amount: u64,
    },
    SendError {
        socket_id: SocketId,
        req_id: u64,
        error: NetworkError,
    },
    FinishAccept {
        socket_id: SocketId,
        child_id: SocketId,
        addr: SocketAddr,
    },
    Closed {
        socket_id: SocketId,
    },
}

#[cfg(all(test, feature = "remote"))]
mod wire_tests {
    use std::pin::Pin;

    use bytes::BytesMut;
    use tokio_serde::{Deserializer, Serializer, formats::SymmetricalBincode};

    use super::*;

    fn legacy_bind_udp_request() -> MessageRequest {
        MessageRequest::Interface {
            req: RequestType::BindUdp {
                socket_id: 0x0102_0304_0506_0708_u64.into(),
                addr: "[2001:db8::1234]:4242".parse().unwrap(),
                reuse_port: true,
                reuse_addr: false,
            },
            req_id: Some(0x1112_1314_1516_1718),
        }
    }

    fn frame(payload: &[u8]) -> Vec<u8> {
        let mut frame = Vec::with_capacity(4 + payload.len());
        frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(payload);
        frame
    }

    async fn assert_server_accepts_legacy_frame(frame: &[u8], format: FrameSerializationFormat) {
        use std::sync::{Arc, Mutex};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        #[derive(Debug, Default)]
        struct RecordingNetworking(Mutex<Vec<(SocketAddr, bool, bool, bool)>>);

        #[async_trait::async_trait]
        impl crate::VirtualNetworking for RecordingNetworking {
            async fn bind_udp(
                &self,
                addr: SocketAddr,
                only_v6: bool,
                reuse_port: bool,
                reuse_addr: bool,
            ) -> crate::Result<Box<dyn crate::VirtualUdpSocket + Sync>> {
                self.0
                    .lock()
                    .unwrap()
                    .push((addr, only_v6, reuse_port, reuse_addr));
                Err(NetworkError::Unsupported)
            }
        }

        let networking = Arc::new(RecordingNetworking::default());
        let (mut peer, transport) = tokio::io::duplex(1024);
        let (rx, tx) = tokio::io::split(transport);
        let (_server, driver) =
            crate::RemoteNetworkingServer::new_from_async_io(tx, rx, format, networking.clone());
        let driver = tokio::spawn(driver);
        peer.write_all(frame).await.unwrap();
        let response_length = tokio::time::timeout(Duration::from_secs(2), peer.read_u32())
            .await
            .unwrap()
            .unwrap();
        assert!(response_length > 0);
        assert_eq!(
            networking.0.lock().unwrap().as_slice(),
            &[("[2001:db8::1234]:4242".parse().unwrap(), false, true, false)],
        );
        driver.abort();
    }

    fn assert_legacy_bind_udp(request: MessageRequest) {
        let MessageRequest::Interface {
            req:
                RequestType::BindUdp {
                    socket_id,
                    addr,
                    reuse_port,
                    reuse_addr,
                },
            req_id,
        } = request
        else {
            panic!("expected legacy BindUdp interface request");
        };
        assert_eq!(socket_id, 0x0102_0304_0506_0708_u64.into());
        assert_eq!(addr, "[2001:db8::1234]:4242".parse::<SocketAddr>().unwrap());
        assert!(reuse_port);
        assert!(!reuse_addr);
        assert_eq!(req_id, Some(0x1112_1314_1516_1718));
    }

    #[tokio::test]
    async fn legacy_bind_udp_bincode_frame_is_stable() {
        const FRAME: &[u8] = &[
            0, 0, 0, 43, 0, 17, 253, 8, 7, 6, 5, 4, 3, 2, 1, 1, 32, 1, 13, 184, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 18, 52, 251, 146, 16, 1, 0, 1, 253, 24, 23, 22, 21, 20, 19, 18, 17,
        ];
        let request = legacy_bind_udp_request();
        let mut bincode = SymmetricalBincode::<MessageRequest>::default();
        let bytes = Pin::new(&mut bincode).serialize(&request).unwrap();
        assert_eq!(frame(&bytes), FRAME);

        let payload = BytesMut::from(&FRAME[4..]);
        let decoded = Pin::new(&mut bincode).deserialize(&payload).unwrap();
        assert_legacy_bind_udp(decoded);
        assert_server_accepts_legacy_frame(FRAME, FrameSerializationFormat::Bincode).await;
    }

    #[cfg(feature = "messagepack")]
    #[tokio::test]
    async fn legacy_bind_udp_messagepack_frame_is_stable() {
        use tokio_serde::formats::SymmetricalMessagePack;

        const FRAME: &[u8] = &[
            0, 0, 0, 70, 129, 169, 73, 110, 116, 101, 114, 102, 97, 99, 101, 146, 129, 167, 66,
            105, 110, 100, 85, 100, 112, 148, 207, 1, 2, 3, 4, 5, 6, 7, 8, 129, 162, 86, 54, 146,
            220, 0, 16, 32, 1, 13, 204, 184, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 18, 52, 205, 16, 146,
            195, 194, 207, 17, 18, 19, 20, 21, 22, 23, 24,
        ];
        let request = legacy_bind_udp_request();
        let mut messagepack = SymmetricalMessagePack::<MessageRequest>::default();
        let bytes = Pin::new(&mut messagepack).serialize(&request).unwrap();
        assert_eq!(frame(&bytes), FRAME);

        let payload = BytesMut::from(&FRAME[4..]);
        let decoded = Pin::new(&mut messagepack).deserialize(&payload).unwrap();
        assert_legacy_bind_udp(decoded);
        assert_server_accepts_legacy_frame(FRAME, FrameSerializationFormat::MessagePack).await;
    }
}
