//! In-runtime socket switch. Each port owns one unicast address; Machines retain their
//! independent namespace/port tables. Use [`Switch::network`] for automatic non-loopback
//! TCP/UDP publication. No host sockets, worker messages or packet stack are involved.
//! Stream source ports are backend-owned (as with the host backend), not guest bind ports.
mod datagram;
mod stream;

use datagram::Datagram;
use std::{
    collections::BTreeMap,
    io,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex, Weak},
};
use stream::Stream;
use wasixv2_abi::Errno;
use wasixv2_vnet::{
    Backend, BackendDatagram, BackendStream, Domain, InboundSink, Net, NetConfig, Publication,
    PublishedDatagram, Room, Uplink, WaitQueue,
};

/// Per-port bounds, including sockets waiting for an accept queue to have room.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub max_endpoints: usize,
    /// Datagram receive bytes, including a 768-byte charge per datagram.
    pub datagram_bytes: usize,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            max_endpoints: 1024,
            datagram_bytes: 212_992,
        }
    }
}

/// A read-only snapshot of a published TCP listener. `Full` listeners remain discoverable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Listener {
    pub address: SocketAddr,
    pub room: Room,
}

/// An isolated switch, shareable by several Machines in a native or browser runtime.
pub struct Switch {
    config: Config,
    ports: Mutex<BTreeMap<IpAddr, Weak<SwitchBackend>>>,
}
struct PortState {
    listeners: BTreeMap<u16, Weak<Published>>,
    datagrams: BTreeMap<u16, Weak<Datagram>>,
    streams: BTreeMap<u16, Weak<Lease>>,
    next: u16,
}
impl Default for PortState {
    fn default() -> Self {
        Self {
            listeners: BTreeMap::new(),
            datagrams: BTreeMap::new(),
            streams: BTreeMap::new(),
            next: 32768,
        }
    }
}
impl PortState {
    fn prune(&mut self) {
        self.listeners.retain(|_, w| w.strong_count() != 0);
        self.datagrams.retain(|_, w| w.strong_count() != 0);
        self.streams.retain(|_, w| w.strong_count() != 0);
    }
    fn room(&mut self, config: Config) -> Result<(), Errno> {
        self.prune();
        if self.listeners.len() + self.datagrams.len() + self.streams.len() >= config.max_endpoints
        {
            Err(Errno::ENOBUFS)
        } else {
            Ok(())
        }
    }
    fn port(&mut self, requested: u16, udp: bool) -> Result<u16, Errno> {
        let free = |p: &Self, n| {
            if udp {
                !p.datagrams.contains_key(&n)
            } else {
                !p.listeners.contains_key(&n) && !p.streams.contains_key(&n)
            }
        };
        if requested != 0 {
            return if free(self, requested) {
                Ok(requested)
            } else {
                Err(Errno::EADDRINUSE)
            };
        }
        // Backend source ports are independent of guest ports, as host NAT ports are.
        for _ in 32768..=60999 {
            let n = self.next;
            self.next = if n == 60999 { 32768 } else { n + 1 };
            if free(self, n) {
                return Ok(n);
            }
        }
        Err(Errno::EADDRINUSE)
    }
}

impl Switch {
    pub fn new(config: Config) -> io::Result<Arc<Self>> {
        if config.max_endpoints == 0 || config.datagram_bytes < 768 {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        Ok(Arc::new(Self {
            config,
            ports: Mutex::new(BTreeMap::new()),
        }))
    }

    /// Reserve an address until the backend and all its live endpoints have gone.
    /// Loopback is namespace-private; multicast/unspecified addresses cannot own a port.
    pub fn backend(self: &Arc<Self>, address: IpAddr) -> io::Result<Arc<SwitchBackend>> {
        if address.is_loopback()
            || address.is_unspecified()
            || address.is_multicast()
            || address == IpAddr::from([255, 255, 255, 255])
        {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let mut ports = self.ports.lock().unwrap();
        ports.retain(|_, w| w.strong_count() != 0);
        if ports.get(&address).and_then(Weak::upgrade).is_some() {
            return Err(io::ErrorKind::AddrInUse.into());
        }
        let backend = Arc::new_cyclic(|me| SwitchBackend {
            switch: self.clone(),
            address,
            me: me.clone(),
            state: Mutex::new(PortState::default()),
        });
        ports.insert(address, Arc::downgrade(&backend));
        Ok(backend)
    }

    /// Construct a single-family network with its own address, loopback and port tables.
    /// Non-loopback listeners and explicitly bound UDP sockets publish through the existing
    /// vnet lifecycle. Prefix routing reaches peers, with no gateway alias or external DNS.
    pub fn network(self: &Arc<Self>, address: IpAddr, prefix: u8) -> io::Result<Arc<Net>> {
        if prefix > if address.is_ipv4() { 32 } else { 128 } {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let backend = self.backend(address)?;
        let mut uplink = Uplink {
            v4: None,
            v6: None,
            ..Uplink::default()
        };
        match address {
            IpAddr::V4(a) => uplink.v4 = Some((a, prefix, None)),
            IpAddr::V6(a) => uplink.v6 = Some((a, prefix, None)),
        }
        Ok(Net::new(
            NetConfig {
                uplink: Some(uplink),
                auto_publish: Some(address),
                ..NetConfig::default()
            },
            Some(backend),
        ))
    }

    fn destination(&self, address: IpAddr) -> Option<Arc<SwitchBackend>> {
        self.ports
            .lock()
            .unwrap()
            .get(&address)
            .and_then(Weak::upgrade)
    }

    /// Observe live TCP listeners without consuming connections or changing guest state.
    /// Snapshot only: callers must still handle close/full races when connecting.
    pub fn listeners(&self) -> Vec<Listener> {
        let ports: Vec<_> = self
            .ports
            .lock()
            .unwrap()
            .values()
            .filter_map(Weak::upgrade)
            .collect();
        let mut snapshots = Vec::new();
        for port in ports {
            let listeners: Vec<_> = port
                .state
                .lock()
                .unwrap()
                .listeners
                .values()
                .filter_map(Weak::upgrade)
                .collect();
            for l in listeners {
                let room = l.sink.poll_room(None);
                if room != Room::Gone {
                    snapshots.push(Listener {
                        address: l.address,
                        room,
                    });
                }
            }
        }
        snapshots.sort_by_key(|l| l.address);
        snapshots
    }
}

/// A Machine's backend port. Dropping its last endpoint frees its address for reuse.
pub struct SwitchBackend {
    switch: Arc<Switch>,
    address: IpAddr,
    me: Weak<SwitchBackend>,
    state: Mutex<PortState>,
}
struct Lease {
    _port: Arc<SwitchBackend>,
}
struct Published {
    address: SocketAddr,
    sink: Arc<dyn InboundSink>,
    _port: Arc<SwitchBackend>,
}
struct PublicationHandle(Arc<Published>);
impl Publication for PublicationHandle {
    fn host_addr(&self) -> SocketAddr {
        self.0.address
    }
}
impl SwitchBackend {
    fn address(&self, host: SocketAddr) -> io::Result<SocketAddr> {
        if host.is_ipv4() != self.address.is_ipv4()
            || (!host.ip().is_unspecified() && host.ip() != self.address)
        {
            return Err(io::ErrorKind::AddrNotAvailable.into());
        }
        Ok(SocketAddr::new(self.address, host.port()))
    }
    fn datagram_at(
        &self,
        host: SocketAddr,
        wq: Option<WaitQueue>,
        published: bool,
    ) -> Result<Arc<Datagram>, Errno> {
        let mut state = self.state.lock().unwrap();
        state.room(self.switch.config)?;
        let port = state.port(host.port(), true)?;
        let ep = Datagram::new(
            self.me.upgrade().unwrap(),
            SocketAddr::new(self.address, port),
            wq,
            published,
        );
        state.datagrams.insert(port, Arc::downgrade(&ep));
        Ok(ep)
    }
}
impl Backend for SwitchBackend {
    fn connect(&self, peer: SocketAddr, wq: WaitQueue) -> Result<Arc<dyn BackendStream>, Errno> {
        if peer.is_ipv4() != self.address.is_ipv4() {
            return Err(Errno::EAFNOSUPPORT);
        }
        let target = self
            .switch
            .destination(peer.ip())
            .ok_or(Errno::EHOSTUNREACH)?;
        let listener = target
            .state
            .lock()
            .unwrap()
            .listeners
            .get(&peer.port())
            .and_then(Weak::upgrade)
            .ok_or(Errno::ECONNREFUSED)?;
        let (source, lease) = {
            let mut state = self.state.lock().unwrap();
            state.room(self.switch.config)?;
            let port = state.port(0, false)?;
            let lease = Arc::new(Lease {
                _port: self.me.upgrade().unwrap(),
            });
            state.streams.insert(port, Arc::downgrade(&lease));
            (SocketAddr::new(self.address, port), lease)
        };
        Ok(Stream::new(
            source,
            peer,
            wq,
            Arc::downgrade(&listener),
            lease,
        ))
    }
    fn datagram(&self, domain: Domain, wq: WaitQueue) -> Result<Arc<dyn BackendDatagram>, Errno> {
        if (domain == Domain::Inet) != self.address.is_ipv4() {
            return Err(Errno::EAFNOSUPPORT);
        }
        self.datagram_at(SocketAddr::new(self.address, 0), Some(wq), false)
            .map(|d| d as Arc<dyn BackendDatagram>)
    }
    fn publish(
        &self,
        host: SocketAddr,
        _: u32,
        sink: Arc<dyn InboundSink>,
    ) -> io::Result<Box<dyn Publication>> {
        let host = self.address(host)?;
        let mut state = self.state.lock().unwrap();
        state.room(self.switch.config).map_err(io_error)?;
        let port = state.port(host.port(), false).map_err(io_error)?;
        let publication = Arc::new(Published {
            address: SocketAddr::new(self.address, port),
            sink,
            _port: self.me.upgrade().unwrap(),
        });
        state.listeners.insert(port, Arc::downgrade(&publication));
        Ok(Box::new(PublicationHandle(publication)))
    }
    fn publish_datagram(&self, host: SocketAddr) -> io::Result<PublishedDatagram> {
        let host = self.address(host)?;
        let endpoint = self.datagram_at(host, None, true).map_err(io_error)?;
        let addr = endpoint.address;
        Ok((
            endpoint.clone(),
            Box::new(datagram::PublicationHandle { endpoint, addr }),
        ))
    }
}
fn io_error(e: Errno) -> io::Error {
    io::Error::from_raw_os_error(e.0)
}
