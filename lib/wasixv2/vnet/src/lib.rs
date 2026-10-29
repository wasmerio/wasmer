//! Socket-level network, one per network namespace: port tables, interfaces and routes,
//! in-memory streams and datagram queues between local sockets, publishing, and the trait that
//! backends implement for traffic leaving the namespace. There is no packet-level stack.
//!
//! `api` is the contract with the kernel and with backends; see its module docs.

mod addr;
mod api;
pub mod bpf;
mod cmsg;
mod dns;
mod icmp;
pub mod iface;
mod mcast;
mod net;
pub mod opts;
pub mod ports;
mod ratelimit;
mod raw;
mod stream;
mod sync;
mod tcp;
mod udp;

#[cfg(test)]
mod tests;

pub use api::{
    Accepted, Answer, Backend, BackendDatagram, BackendStream, Control, Domain, EchoIgnore, Found,
    GroupSources, IcmpEntry, InboundSink, Interface, Lookup, LookupDone, LookupError, Network,
    NewLink, NewLinkKind, PortEntry, Publication, PublishedDatagram, QueuedError, RecvMsg, Room,
    SendMsg, Shutdown, Socket, SocketKind, StreamState,
};
pub use icmp::IcmpSocket;
pub use net::{MovedLink, Net, NetConfig, Rtnl, Uplink, move_link, rtnl};
pub use ratelimit::IcmpLimit;
pub use tcp::TcpSocket;
pub use udp::UdpSocket;
pub use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue};

/// Result of a network operation: a guest-visible errno on failure.
pub type NetResult<T> = Result<T, wasixv2_abi::Errno>;
