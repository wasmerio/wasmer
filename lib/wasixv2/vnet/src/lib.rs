//! Socket-level network, one per network namespace: port tables, interfaces and routes,
//! in-memory streams and datagram queues between local sockets, publishing, and the trait that
//! backends implement for traffic leaving the namespace. There is no packet-level stack.
//!
//! `api` is the contract with the kernel and with backends; see its module docs.

mod api;

pub use api::{
    Accepted, Backend, BackendDatagram, BackendStream, Control, Domain, InboundSink, Interface,
    Network, PortEntry, Publication, RecvMsg, SendMsg, Shutdown, Socket, SocketKind,
};
pub use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue};
