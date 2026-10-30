//! Worker-local transport boundary. Implementations must bound their own JS/WebSocket queues.
use std::{
    net::SocketAddr,
    task::{Context, Poll},
};
use wasixv2_abi::Errno;
use wasixv2_vnet::{Found, Lookup, LookupError, Shutdown};

/// Capabilities of the actual provider, not of the browser or the guest socket layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub tcp: bool,
    pub udp: bool,
    pub ipv6: bool,
    pub listen: bool,
    pub dns_name: bool,
    pub dns_reverse: bool,
    pub dns_forward: bool,
    /// The relay sends EOF notifications (the stock v1 server currently does not).
    pub eof: bool,
    /// Additive v1 relay extension, selected out of band with a matching provider.
    pub fidelity: bool,
    pub urgent: bool,
    pub udp_publish: bool,
}

impl Capabilities {
    /// The classic relay's wire surface. IPv6 availability is a relay deployment choice.
    pub fn classic(ipv6: bool) -> Self {
        Self {
            tcp: true,
            udp: true,
            ipv6,
            listen: true,
            dns_name: true,
            ..Self::default()
        }
    }

    /// Our additive classic-fidelity profile; never use against a stock relay.
    pub fn classic_fidelity(ipv6: bool) -> Self {
        Self {
            eof: true,
            fidelity: true,
            urgent: true,
            udp_publish: true,
            dns_reverse: true,
            dns_forward: true,
            ..Self::classic(ipv6)
        }
    }

    /// The current SDK WISP provider carries TCP and DNS, not UDP or ingress.
    pub fn wisp(ipv6: bool) -> Self {
        Self {
            tcp: true,
            ipv6,
            dns_name: true,
            ..Self::default()
        }
    }
}

/// One ordered request. IDs are never reused within a backend, including across reconnects.
#[derive(Clone, Debug)]
pub enum Command {
    Connect {
        request: u64,
        socket: u64,
        peer: SocketAddr,
    },
    Bind {
        request: u64,
        socket: u64,
        addr: SocketAddr,
    },
    Send {
        request: u64,
        socket: u64,
        data: Vec<u8>,
    },
    SendUrgent {
        request: u64,
        socket: u64,
        data: Vec<u8>,
    },
    /// UDP has no success acknowledgment in the classic protocol.
    SendTo {
        socket: u64,
        data: Vec<u8>,
        addr: SocketAddr,
    },
    Shutdown {
        request: u64,
        socket: u64,
        how: Shutdown,
    },
    Close {
        socket: u64,
    },
    /// Sent before Close when unread bytes or receive overflow require an abort.
    Reset {
        socket: u64,
    },
    Listen {
        request: u64,
        socket: u64,
        addr: SocketAddr,
    },
    /// Existing classic GetAddrLocal request, after a successful bind/listen.
    LocalAddr {
        request: u64,
        socket: u64,
    },
    Accept {
        socket: u64,
        child: u64,
    },
    Lookup {
        request: u64,
        lookup: Lookup,
    },
}

/// An asynchronous completion produced by the owning I/O worker.
#[derive(Clone, Debug)]
pub enum Event {
    LocalAddr {
        request: u64,
        addr: SocketAddr,
    },
    Complete {
        request: u64,
        result: Result<(), Errno>,
    },
    Data {
        socket: u64,
        data: Vec<u8>,
    },
    Datagram {
        socket: u64,
        data: Vec<u8>,
        addr: SocketAddr,
    },
    Sent {
        request: u64,
        socket: u64,
        amount: usize,
    },
    SendError {
        request: u64,
        socket: u64,
        error: Errno,
    },
    /// Last byte is urgent, at this position in the ordered stream.
    Urgent {
        socket: u64,
        data: Vec<u8>,
    },
    /// A host UDP send failed; the endpoint remains usable.
    DatagramError {
        socket: u64,
        error: Errno,
    },
    /// Peer FIN, preserving the other direction. Reset is a distinct event.
    Eof {
        socket: u64,
    },
    Reset {
        socket: u64,
        error: Errno,
    },
    /// A provider send previously returned EAGAIN; it may now be retried.
    Writable {
        socket: u64,
    },
    Accepted {
        socket: u64,
        child: u64,
        peer: SocketAddr,
    },
    Lookup {
        request: u64,
        result: Result<Found, LookupError>,
    },
}

/// No Send/Sync bound: JS references stay on the I/O worker. Poll methods must not reenter
/// the backend synchronously; completion is delivered by `poll_recv`. Pending registers the
/// supplied waker, including for transport capacity (`WebSocket.bufferedAmount`).
///
/// `poll_send` accepts an entire command exactly once on Ready(Ok); Pending accepts nothing.
/// A failed send is ambiguous and ends the session: the driver never retries it on a new
/// connection. For WISP, the SDK adapter implements this trait using its provider callbacks
/// and reports only its actual capabilities. No WISP bytes are interpreted as classic frames.
pub trait Transport {
    fn capabilities(&self) -> Capabilities;
    fn poll_send(&mut self, cx: &mut Context<'_>, command: &Command) -> Poll<Result<(), Errno>>;
    fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Result<Event, Errno>>;
    /// Observe disconnect even when a full guest receive queue stops frame consumption.
    /// Pending registers the waker; Ready is terminal for this transport session.
    fn poll_closed(&mut self, cx: &mut Context<'_>) -> Poll<Errno>;
    /// Idempotent, releasing JS event handlers, pending callbacks and connection resources.
    fn close(&mut self);
}

/// Whole binary websocket frames. An implementation owns the websocket exclusively and
/// limits buffered bytes before returning Ready from `poll_send`. Receive frames must be
/// bounded by [`crate::codec::MAX_FRAME`] before allocation/copy from JS.
pub trait FrameTransport {
    fn poll_send(&mut self, cx: &mut Context<'_>, frame: &[u8]) -> Poll<Result<(), Errno>>;
    fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Result<Vec<u8>, Errno>>;
    /// Observe disconnect even when a full guest receive queue stops frame consumption.
    /// Pending registers the waker; Ready is terminal for this transport session.
    fn poll_closed(&mut self, cx: &mut Context<'_>) -> Poll<Errno>;
    /// Idempotent, releasing JS event handlers, pending callbacks and connection resources.
    fn close(&mut self);
}

/// Classic relay protocol adapter over a worker-local websocket implementation.
pub struct Classic<T> {
    transport: T,
    capabilities: Capabilities,
}

impl<T> Classic<T> {
    pub fn new(transport: T, capabilities: Capabilities) -> Self {
        Self {
            transport,
            capabilities,
        }
    }
}

impl<T: FrameTransport> Transport for Classic<T> {
    fn capabilities(&self) -> Capabilities {
        self.capabilities
    }
    fn poll_send(&mut self, cx: &mut Context<'_>, command: &Command) -> Poll<Result<(), Errno>> {
        if !self.capabilities.fidelity
            && matches!(
                command,
                Command::SendUrgent { .. }
                    | Command::Lookup {
                        lookup: Lookup::Addr(_) | Lookup::Forward(_),
                        ..
                    }
            )
        {
            return Poll::Ready(Err(Errno::EOPNOTSUPP));
        }
        let frame = match crate::codec::encode(command) {
            Ok(frame) => frame,
            Err(error) => return Poll::Ready(Err(error)),
        };
        self.transport.poll_send(cx, &frame)
    }
    fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Result<Event, Errno>> {
        self.transport.poll_recv(cx).map(|frame| {
            frame.and_then(|frame| {
                if !self.capabilities.fidelity && frame.starts_with(&7u32.to_le_bytes()) {
                    return Err(Errno::EPROTO);
                }
                crate::codec::decode(&frame)
            })
        })
    }
    fn poll_closed(&mut self, cx: &mut Context<'_>) -> Poll<Errno> {
        self.transport.poll_closed(cx)
    }
    fn close(&mut self) {
        self.transport.close();
    }
}
