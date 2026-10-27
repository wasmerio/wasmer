//! Socket-level tests against a loopback-only namespace.

use std::{net::SocketAddr, sync::Arc};

use wasixv2_abi::{Errno, MsgFlags};
use wasixv2_wait::PollMask;

use crate::{Domain, Net, NetConfig, Network, SendMsg, Shutdown, Socket, SocketKind};

fn net() -> Arc<Net> {
    Net::new(NetConfig::default(), None)
}

fn a(s: &str) -> SocketAddr {
    s.parse().unwrap()
}

/// The uid sockets are made for unless a test says otherwise.
const UID: u32 = 1;

fn tcp(n: &Net) -> Arc<dyn Socket> {
    n.socket(Domain::Inet, SocketKind::Stream, 0, UID).unwrap()
}

fn udp(n: &Net) -> Arc<dyn Socket> {
    n.socket(Domain::Inet, SocketKind::Datagram, 0, UID)
        .unwrap()
}

fn send(s: &dyn Socket, data: &[u8]) -> Result<usize, Errno> {
    send_to(s, data, None)
}

fn send_to(s: &dyn Socket, data: &[u8], to: Option<SocketAddr>) -> Result<usize, Errno> {
    s.send(SendMsg {
        data,
        to,
        flags: MsgFlags(0),
        control: &[],
    })
}

fn recv(s: &dyn Socket, n: usize) -> Result<Vec<u8>, Errno> {
    let mut buf = vec![0; n];
    let r = s.recv(&mut buf, MsgFlags(0))?;
    buf.truncate(r.len);
    Ok(buf)
}

#[test]
fn fresh_stream_socket_polls_out_hup() {
    let n = net();
    let s = tcp(&n);
    assert_eq!(
        s.poll(None),
        PollMask::OUT | PollMask::WRNORM | PollMask::HUP
    );
    assert_eq!(recv(&*s, 4), Err(Errno::ENOTCONN));
    assert_eq!(send(&*s, b"x"), Err(Errno::EPIPE));
    assert_eq!(s.shutdown(Shutdown::Both), Err(Errno::ENOTCONN));
}

#[test]
fn loopback_tcp_lifecycle() {
    let n = net();
    let l = tcp(&n);
    l.bind(a("127.0.0.1:8080"), false).unwrap();
    l.listen(16).unwrap();
    assert_eq!(l.poll(None), PollMask::empty());
    assert_eq!(l.accept().unwrap_err(), Errno::EAGAIN);

    let c = tcp(&n);
    c.connect(a("127.0.0.1:8080")).unwrap();
    assert_eq!(c.connect(a("127.0.0.1:8080")), Err(Errno::EISCONN));
    assert!(l.poll(None).contains(PollMask::IN));
    let acc = l.accept().unwrap();
    let s = acc.socket;
    let cl = c.local_addr().unwrap();
    assert_eq!(acc.peer, cl);
    assert_eq!(cl.ip(), a("127.0.0.1:0").ip());
    assert!((32768..=60999).contains(&cl.port()));
    assert_eq!(s.local_addr().unwrap(), a("127.0.0.1:8080"));
    assert_eq!(c.peer_addr().unwrap(), a("127.0.0.1:8080"));

    assert_eq!(c.poll(None), PollMask::OUT | PollMask::WRNORM);
    send(&*c, b"hello").unwrap();
    assert!(s.poll(None).contains(PollMask::IN));
    assert_eq!(s.input_len().unwrap(), 5);
    assert_eq!(recv(&*s, 16).unwrap(), b"hello");
    assert_eq!(recv(&*s, 16), Err(Errno::EAGAIN));

    // Half close: the server sees EOF and can still answer.
    c.shutdown(Shutdown::Write).unwrap();
    assert!(s.poll(None).contains(PollMask::RDHUP));
    assert_eq!(recv(&*s, 16).unwrap(), b"");
    send(&*s, b"bye").unwrap();
    assert_eq!(recv(&*c, 16).unwrap(), b"bye");
    drop(s);
    assert!(c.poll(None).contains(PollMask::HUP));
    assert_eq!(recv(&*c, 16).unwrap(), b"");
}

#[test]
fn close_with_unread_data_resets_and_epipe_follows() {
    let n = net();
    let l = tcp(&n);
    l.bind(a("127.0.0.1:9000"), false).unwrap();
    l.listen(1).unwrap();
    let c = tcp(&n);
    c.connect(a("127.0.0.1:9000")).unwrap();
    let s = l.accept().unwrap().socket;
    send(&*c, b"unread").unwrap();
    drop(s);
    assert!(c.poll(None).contains(PollMask::ERR));
    assert_eq!(send(&*c, b"x"), Err(Errno::ECONNRESET));
    assert_eq!(send(&*c, b"x"), Err(Errno::EPIPE));
}

#[test]
fn connect_refused_and_unreachable() {
    let n = net();
    let c = tcp(&n);
    assert_eq!(c.connect(a("127.0.0.1:1")), Err(Errno::ECONNREFUSED));
    // No backend: anything off the namespace is unreachable.
    let c = tcp(&n);
    assert_eq!(c.connect(a("8.8.8.8:53")), Err(Errno::ENETUNREACH));
}

#[test]
fn full_backlog_queues_until_accept_and_listener_close_refuses() {
    let n = net();
    let l = tcp(&n);
    l.bind(a("127.0.0.1:7000"), false).unwrap();
    l.listen(0).unwrap();
    let c1 = tcp(&n);
    c1.connect(a("127.0.0.1:7000")).unwrap();
    let c2 = tcp(&n);
    assert_eq!(c2.connect(a("127.0.0.1:7000")), Err(Errno::EINPROGRESS));
    assert_eq!(c2.connect(a("127.0.0.1:7000")), Err(Errno::EALREADY));
    assert_eq!(c2.poll(None), PollMask::empty());
    l.accept().unwrap();
    assert!(c2.poll(None).contains(PollMask::OUT));
    assert_eq!(c2.take_error(), None);

    let c3 = tcp(&n);
    assert_eq!(c3.connect(a("127.0.0.1:7000")), Err(Errno::EINPROGRESS));
    drop(l);
    assert!(c3.poll(None).contains(PollMask::ERR));
    assert_eq!(c3.take_error(), Some(Errno::ECONNREFUSED));
    // The unaccepted, established connection was reset.
    assert_eq!(recv(&*c2, 4), Err(Errno::ECONNRESET));
}

#[test]
fn bind_rules_through_sockets() {
    let n = net();
    let s = tcp(&n);
    assert_eq!(s.bind(a("127.0.0.1:80"), false), Err(Errno::EACCES));
    assert_eq!(s.bind(a("10.1.2.3:8000"), false), Err(Errno::EADDRNOTAVAIL));
    s.bind(a("0.0.0.0:8000"), false).unwrap();
    assert_eq!(s.bind(a("0.0.0.0:8001"), false), Err(Errno::EINVAL));
    let t = tcp(&n);
    assert_eq!(t.bind(a("127.0.0.1:8000"), false), Err(Errno::EADDRINUSE));
    // UDP has its own port space.
    udp(&n).bind(a("127.0.0.1:8000"), false).unwrap();
    // Closing frees the port at once (no TIME_WAIT).
    drop(s);
    t.bind(a("127.0.0.1:8000"), false).unwrap();
    // A dual-stack IPv6 listener takes the IPv4 port too.
    let six = n.socket(Domain::Inet6, SocketKind::Stream, 0, UID).unwrap();
    assert_eq!(six.bind(a("[::]:8000"), false), Err(Errno::EADDRINUSE));
    six.bind(a("[::]:8002"), false).unwrap();
    six.listen(4).unwrap();
    let c = tcp(&n);
    c.connect(a("127.0.0.1:8002")).unwrap();
    let acc = six.accept().unwrap();
    assert_eq!(
        acc.peer,
        SocketAddr::new(
            "::ffff:127.0.0.1".parse().unwrap(),
            c.local_addr().unwrap().port()
        )
    );
}

#[test]
fn udp_datagrams_truncation_and_connected_filtering() {
    let n = net();
    let r = udp(&n);
    r.bind(a("127.0.0.1:5353"), false).unwrap();
    assert_eq!(
        r.poll(None),
        PollMask::OUT | PollMask::WRNORM | PollMask::WRBAND
    );
    let s = udp(&n);
    send_to(&*s, b"hello world", Some(a("127.0.0.1:5353"))).unwrap();
    send_to(&*s, b"", Some(a("127.0.0.1:5353"))).unwrap();
    assert!(r.poll(None).contains(PollMask::IN));
    assert_eq!(r.input_len().unwrap(), 11);

    let mut buf = [0u8; 5];
    let m = r.recv(&mut buf, MsgFlags::MSG_PEEK).unwrap();
    assert_eq!((m.len, m.full_len), (5, 11));
    let m = r.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!(&buf, b"hello");
    assert_eq!((m.len, m.full_len, m.flags), (5, 11, MsgFlags::MSG_TRUNC));
    // The sender is bound to the wildcard; the datagram carries the route's source.
    let port = s.local_addr().unwrap().port();
    assert_eq!(m.from, Some(SocketAddr::new(a("127.0.0.1:0").ip(), port)));
    let m = r.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!(m.full_len, 0);
    assert_eq!(r.recv(&mut buf, MsgFlags(0)).unwrap_err(), Errno::EAGAIN);

    // A connected socket only hears its peer.
    let other = udp(&n);
    r.connect(SocketAddr::new(a("127.0.0.1:0").ip(), port))
        .unwrap();
    send_to(&*other, b"x", Some(a("127.0.0.1:5353"))).unwrap();
    assert_eq!(r.recv(&mut buf, MsgFlags(0)).unwrap_err(), Errno::EAGAIN);
    send_to(&*s, b"y", Some(a("127.0.0.1:5353"))).unwrap();
    assert_eq!(r.recv(&mut buf, MsgFlags(0)).unwrap().len, 1);

    // Connected UDP to a closed port learns ECONNREFUSED.
    let c = udp(&n);
    c.connect(a("127.0.0.1:9")).unwrap();
    send(&*c, b"z").unwrap();
    assert!(c.poll(None).contains(PollMask::ERR));
    assert_eq!(send(&*c, b"z"), Err(Errno::ECONNREFUSED));
    assert_eq!(udp(&n).shutdown(Shutdown::Read), Err(Errno::ENOTCONN));
    assert_eq!(send(&*udp(&n), b"z"), Err(Errno::EDESTADDRREQ));
}

#[test]
fn reuseaddr_allows_rebind_while_connection_lives() {
    let n = net();
    let one = 1i32.to_le_bytes();
    let l = tcp(&n);
    l.set_option(1, 2, &one).unwrap();
    l.bind(a("127.0.0.1:6000"), false).unwrap();
    l.listen(4).unwrap();
    let c = tcp(&n);
    c.connect(a("127.0.0.1:6000")).unwrap();
    let _s = l.accept().unwrap().socket;
    drop(l);
    let plain = tcp(&n);
    assert_eq!(
        plain.bind(a("127.0.0.1:6000"), false),
        Err(Errno::EADDRINUSE)
    );
    let reuse = tcp(&n);
    reuse.set_option(1, 2, &one).unwrap();
    reuse.bind(a("127.0.0.1:6000"), false).unwrap();
    reuse.listen(4).unwrap();
}

#[test]
fn ports_lists_sockets_while_they_come_and_go() {
    let n = net();
    let l = tcp(&n);
    l.bind(a("127.0.0.1:7100"), false).unwrap();
    l.listen(4).unwrap();
    let u = udp(&n);
    u.bind(a("0.0.0.0:7101"), false).unwrap();
    let ports = n.ports();
    assert!(
        ports
            .iter()
            .any(|p| p.local == a("127.0.0.1:7100") && p.listening)
    );
    assert!(
        ports
            .iter()
            .any(|p| p.local == a("0.0.0.0:7101") && !p.listening)
    );

    // Sockets dropped while `ports` walks the tables unbind under the namespace lock; the walk
    // must not hold it then, nor drop their last reference inside it.
    let walker = {
        let n = n.clone();
        std::thread::spawn(move || {
            for _ in 0..2000 {
                n.ports();
            }
        })
    };
    for i in 0..2000u16 {
        let s = if i % 2 == 0 { tcp(&n) } else { udp(&n) };
        s.bind(a("127.0.0.1:0"), false).unwrap();
        drop(s);
    }
    walker.join().unwrap();
}

#[test]
fn udp_reuseport_group_spreads_by_source_port() {
    let n = net();
    let one = 1i32.to_le_bytes();
    let group: Vec<_> = (0..2)
        .map(|_| {
            let s = udp(&n);
            s.set_option(1, 15, &one).unwrap();
            s.bind(a("127.0.0.1:7200"), false).unwrap();
            s
        })
        .collect();
    for _ in 0..8 {
        let c = udp(&n);
        send_to(&*c, b"x", Some(a("127.0.0.1:7200"))).unwrap();
    }
    let got: Vec<usize> = group
        .iter()
        .map(|s| std::iter::from_fn(|| recv(&**s, 4).ok()).count())
        .collect();
    assert_eq!(got.iter().sum::<usize>(), 8);
}

// ---- a recording backend ----

use std::sync::Mutex;

use wasixv2_abi::{SocketLevel, SocketOption, TcpOption};
use wasixv2_wait::{PollTable, Pollable, WaitQueue};

use crate::{Backend, BackendDatagram, BackendStream, InboundSink, Publication, StreamState};

struct FakeStream {
    peer: SocketAddr,
    options: Mutex<Vec<(i32, i32, Vec<u8>)>>,
}

impl FakeStream {
    fn new(peer: SocketAddr) -> Arc<Self> {
        Arc::new(Self {
            peer,
            options: Mutex::default(),
        })
    }

    fn option(&self, level: i32, name: i32) -> Option<Vec<u8>> {
        let options = self.options.lock().unwrap();
        options
            .iter()
            .rev()
            .find(|o| o.0 == level && o.1 == name)
            .map(|o| o.2.clone())
    }
}

impl Pollable for FakeStream {
    fn poll(&self, _: Option<&mut PollTable<'_>>) -> PollMask {
        PollMask::OUT | PollMask::WRNORM
    }
}

impl BackendStream for FakeStream {
    fn state(&self) -> StreamState {
        StreamState::Established
    }
    fn read(&self, _: &mut [u8], _: bool) -> Result<usize, Errno> {
        Err(Errno::EAGAIN)
    }
    fn write(&self, data: &[u8]) -> Result<usize, Errno> {
        Ok(data.len())
    }
    fn shutdown(&self, _: Shutdown) -> Result<(), Errno> {
        Ok(())
    }
    fn take_error(&self) -> Option<Errno> {
        None
    }
    fn peer_addr(&self) -> SocketAddr {
        self.peer
    }
    fn input_len(&self) -> usize {
        0
    }
    fn set_option(&self, level: i32, name: i32, value: &[u8]) -> Result<(), Errno> {
        self.options
            .lock()
            .unwrap()
            .push((level, name, value.to_vec()));
        Ok(())
    }
}

/// A datagram endpoint whose host side is writable or not as the test says.
#[derive(Default)]
struct Recorder {
    streams: Mutex<Vec<Arc<FakeStream>>>,
}

impl Backend for Recorder {
    fn connect(&self, peer: SocketAddr, _: WaitQueue) -> Result<Arc<dyn BackendStream>, Errno> {
        let s = FakeStream::new(peer);
        self.streams.lock().unwrap().push(s.clone());
        Ok(s)
    }
    fn datagram(&self, _: Domain, _: WaitQueue) -> Result<Arc<dyn BackendDatagram>, Errno> {
        Err(Errno::ENETUNREACH)
    }
    fn publish(
        &self,
        _: SocketAddr,
        _: Arc<dyn InboundSink>,
    ) -> Result<Box<dyn Publication>, std::io::Error> {
        Err(std::io::ErrorKind::Unsupported.into())
    }
}

fn backed() -> (Arc<Net>, Arc<Recorder>) {
    let rec = Arc::new(Recorder::default());
    let config = NetConfig {
        auto_publish: Some(a("0.0.0.0:0").ip()),
        ..NetConfig::host_default()
    };
    (Net::new(config, Some(rec.clone())), rec)
}

fn int(v: i32) -> [u8; 4] {
    v.to_le_bytes()
}

const SOL_SOCKET: i32 = SocketLevel::SOL_SOCKET.0;
const SOL_TCP: i32 = SocketLevel::SOL_TCP.0;

#[test]
fn options_set_before_connect_reach_the_backend_stream() {
    let (n, rec) = backed();
    let s = tcp(&n);
    s.set_option(SOL_TCP, TcpOption::TCP_NODELAY.0, &int(1))
        .unwrap();
    s.set_option(SOL_TCP, TcpOption::TCP_KEEPIDLE.0, &int(30))
        .unwrap();
    s.set_option(SOL_SOCKET, SocketOption::SO_KEEPALIVE.0, &int(1))
        .unwrap();
    let linger = [int(1), int(5)].concat();
    s.set_option(SOL_SOCKET, SocketOption::SO_LINGER.0, &linger)
        .unwrap();
    assert_eq!(s.connect(a("10.0.2.2:80")), Err(Errno::EINPROGRESS));
    let stream = rec.streams.lock().unwrap()[0].clone();
    assert_eq!(
        stream.option(SOL_TCP, TcpOption::TCP_NODELAY.0),
        Some(int(1).to_vec())
    );
    assert_eq!(
        stream.option(SOL_TCP, TcpOption::TCP_KEEPIDLE.0),
        Some(int(30).to_vec())
    );
    assert_eq!(
        stream.option(SOL_SOCKET, SocketOption::SO_KEEPALIVE.0),
        Some(int(1).to_vec())
    );
    assert_eq!(
        stream.option(SOL_SOCKET, SocketOption::SO_LINGER.0),
        Some(linger.clone())
    );
    // Options never set are left to the host's defaults.
    assert_eq!(stream.option(SOL_TCP, TcpOption::TCP_KEEPCNT.0), None);
}

#[test]
fn disconnect_resets_the_connection_and_returns_port_and_address() {
    let n = net();
    let l = tcp(&n);
    l.bind(a("127.0.0.1:7300"), false).unwrap();
    l.listen(4).unwrap();
    let c = tcp(&n);
    c.connect(a("127.0.0.1:7300")).unwrap();
    let s = l.accept().unwrap().socket;
    let port = c.local_addr().unwrap().port();
    c.disconnect().unwrap();
    // `tcp_disconnect` from ESTABLISHED: RST to the peer, ECONNRESET on our own socket.
    assert_eq!(c.take_error(), Some(Errno::ECONNRESET));
    assert_eq!(recv(&*s, 4), Err(Errno::ECONNRESET));
    // The autobound port went back (`inet_put_port`); the source address is the wildcard
    // again (`inet_bhash2_reset_saddr`), the port still shows.
    assert_eq!(
        c.local_addr().unwrap(),
        SocketAddr::new(a("0.0.0.0:0").ip(), port)
    );
    assert!(n.ports().iter().all(|p| p.local.port() != port));
    // Disconnecting a closed socket sets nothing.
    c.disconnect().unwrap();
    assert_eq!(c.take_error(), None);
    // A port and address chosen by bind stay.
    let b = tcp(&n);
    b.bind(a("127.0.0.1:7301"), false).unwrap();
    b.connect(a("127.0.0.1:7300")).unwrap();
    b.disconnect().unwrap();
    assert_eq!(b.local_addr().unwrap(), a("127.0.0.1:7301"));
}

#[test]
fn shutdown_of_a_reset_connection_is_enotconn() {
    let n = net();
    let l = tcp(&n);
    l.bind(a("127.0.0.1:7400"), false).unwrap();
    l.listen(4).unwrap();
    let c = tcp(&n);
    c.connect(a("127.0.0.1:7400")).unwrap();
    let s = l.accept().unwrap().socket;
    send(&*c, b"unread").unwrap();
    drop(s);
    assert!(c.poll(None).contains(PollMask::ERR));
    assert_eq!(c.shutdown(Shutdown::Write), Err(Errno::ENOTCONN));
}

#[test]
fn reuseport_groups_need_one_owner() {
    let n = net();
    let one = int(1);
    let as_uid = |uid: u32| {
        let s = n
            .socket(Domain::Inet, SocketKind::Datagram, 0, uid)
            .unwrap();
        s.set_option(SOL_SOCKET, SocketOption::SO_REUSEPORT.0, &one)
            .unwrap();
        s
    };
    let first = as_uid(1);
    first.bind(a("127.0.0.1:7600"), false).unwrap();
    assert_eq!(
        as_uid(2).bind(a("127.0.0.1:7600"), false),
        Err(Errno::EADDRINUSE)
    );
    as_uid(1).bind(a("127.0.0.1:7600"), false).unwrap();
    // TCP the same, and the accepted socket keeps the listener's owner.
    let l = n.socket(Domain::Inet, SocketKind::Stream, 0, 1).unwrap();
    l.set_option(SOL_SOCKET, SocketOption::SO_REUSEPORT.0, &one)
        .unwrap();
    l.bind(a("127.0.0.1:7601"), false).unwrap();
    l.listen(1).unwrap();
    let other = n.socket(Domain::Inet, SocketKind::Stream, 0, 2).unwrap();
    other
        .set_option(SOL_SOCKET, SocketOption::SO_REUSEPORT.0, &one)
        .unwrap();
    assert_eq!(
        other.bind(a("127.0.0.1:7601"), false),
        Err(Errno::EADDRINUSE)
    );
}
