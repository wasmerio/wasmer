//! The host backend against real host sockets on 127.0.0.1.
#![cfg(unix)]

use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream, UdpSocket},
    sync::{Arc, mpsc},
    time::Duration,
};

use wasixv2_abi::{Errno, MsgFlags, SocketLevel, SocketOption, TcpOption};
use wasixv2_vnet::{
    Domain, Net, NetConfig, Network, PollMask, PollTable, SendMsg, Shutdown, Socket, SocketKind,
};
use wasixv2_vnet_host::HostBackend;
use wasixv2_wait::{WaitEntry, WaitQueue, WaitRegistration};

struct Signal(mpsc::Sender<()>);

impl WaitEntry for Signal {
    fn wake(&self, _: PollMask) -> bool {
        let _ = self.0.send(());
        true
    }
}

/// Waits (woken through the socket's wait queues) until `s` reports any of `want`.
fn wait_for(s: &dyn Socket, want: PollMask) -> PollMask {
    let (tx, rx) = mpsc::channel();
    let entry: Arc<dyn WaitEntry> = Arc::new(Signal(tx));
    for _ in 0..100 {
        let mut regs: Vec<WaitRegistration> = Vec::new();
        let mut proc = |q: &WaitQueue, _| regs.push(q.add(entry.clone()));
        let m = s.poll(Some(&mut PollTable::new(PollMask::empty(), &mut proc)));
        if m.intersects(want) {
            return m;
        }
        let _ = rx.recv_timeout(Duration::from_millis(100));
    }
    panic!("timed out waiting for {want:?}");
}

fn setup() -> Arc<Net> {
    let backend = HostBackend::new().unwrap();
    Net::new(NetConfig::host_default(), Some(backend))
}

fn send(s: &dyn Socket, data: &[u8], to: Option<SocketAddr>) -> Result<usize, Errno> {
    s.send(SendMsg {
        data,
        to,
        flags: MsgFlags(0),
        control: &[],
    })
}

fn recv_all(s: &dyn Socket, n: usize) -> Vec<u8> {
    let mut out = Vec::new();
    while out.len() < n {
        wait_for(s, PollMask::IN);
        let mut buf = [0u8; 64];
        let r = s.recv(&mut buf, MsgFlags(0)).unwrap();
        assert!(r.len > 0, "unexpected EOF");
        out.extend_from_slice(&buf[..r.len]);
    }
    out
}

#[test]
fn outbound_connect_through_gateway_alias() {
    let net = setup();
    let host = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = host.local_addr().unwrap().port();
    let s = net.socket(Domain::Inet, SocketKind::Stream, 0, 1).unwrap();
    let dst: SocketAddr = format!("10.0.2.2:{port}").parse().unwrap();
    assert_eq!(s.connect(dst), Err(Errno::EINPROGRESS));
    let (mut peer, _) = host.accept().unwrap();
    assert!(wait_for(&*s, PollMask::OUT | PollMask::ERR).contains(PollMask::OUT));
    assert_eq!(s.take_error(), None);
    assert_eq!(s.peer_addr().unwrap(), dst);
    assert_eq!(s.local_addr().unwrap().ip().to_string(), "10.0.2.15");

    assert_eq!(send(&*s, b"ping", None), Ok(4));
    let mut buf = [0u8; 4];
    peer.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"ping");
    peer.write_all(b"pong").unwrap();
    assert_eq!(recv_all(&*s, 4), b"pong");

    s.shutdown(Shutdown::Write).unwrap();
    let mut rest = Vec::new();
    peer.read_to_end(&mut rest).unwrap();
    assert!(rest.is_empty());
    drop(peer);
    assert!(wait_for(&*s, PollMask::RDHUP).contains(PollMask::HUP));
    let mut buf = [0u8; 4];
    assert_eq!(s.recv(&mut buf, MsgFlags(0)).unwrap().len, 0);
}

#[test]
fn outbound_connect_refused() {
    let net = setup();
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let s = net.socket(Domain::Inet, SocketKind::Stream, 0, 1).unwrap();
    let dst: SocketAddr = format!("10.0.2.2:{port}").parse().unwrap();
    assert_eq!(s.connect(dst), Err(Errno::EINPROGRESS));
    assert!(wait_for(&*s, PollMask::ERR).contains(PollMask::HUP));
    assert_eq!(s.take_error(), Some(Errno::ECONNREFUSED));
}

#[test]
fn published_port_reaches_guest_listener() {
    let net = setup();
    let l = net.socket(Domain::Inet, SocketKind::Stream, 0, 1).unwrap();
    l.bind("0.0.0.0:8080".parse().unwrap(), false).unwrap();
    l.listen(8).unwrap();
    let publication = net
        .publish(
            "127.0.0.1:0".parse().unwrap(),
            "0.0.0.0:8080".parse().unwrap(),
        )
        .unwrap();
    let mut client = TcpStream::connect(publication.host_addr()).unwrap();
    client.write_all(b"hello").unwrap();
    wait_for(&*l, PollMask::IN);
    let acc = l.accept().unwrap();
    // The host peer is on the host's loopback, which the guest knows as the gateway alias.
    assert_eq!(acc.peer.ip().to_string(), "10.0.2.2");
    assert_eq!(acc.peer.port(), client.local_addr().unwrap().port());
    assert_eq!(
        acc.socket.local_addr().unwrap(),
        "10.0.2.15:8080".parse().unwrap()
    );
    assert_eq!(recv_all(&*acc.socket, 5), b"hello");
    send(&*acc.socket, b"world", None).unwrap();
    let mut buf = [0u8; 5];
    client.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"world");
    drop(acc);
    let mut rest = Vec::new();
    client.read_to_end(&mut rest).unwrap();
    assert!(rest.is_empty());
}

#[test]
fn udp_through_the_backend() {
    let net = setup();
    let host = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = host.local_addr().unwrap().port();
    let s = net
        .socket(Domain::Inet, SocketKind::Datagram, 0, 1)
        .unwrap();
    let dst: SocketAddr = format!("10.0.2.2:{port}").parse().unwrap();
    assert_eq!(send(&*s, b"query", Some(dst)), Ok(5));
    let mut buf = [0u8; 16];
    host.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let (n, from) = host.recv_from(&mut buf).unwrap();
    assert_eq!(&buf[..n], b"query");
    host.send_to(b"answer", from).unwrap();
    wait_for(&*s, PollMask::IN);
    let r = s.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!(&buf[..r.len], b"answer");
    assert_eq!(r.from, Some(dst));
}

/// One registration on a socket's queues, made once and never renewed: what an epoll item
/// added before the traffic, or a task asleep in `recv`, holds.
struct Registered {
    rx: mpsc::Receiver<()>,
    _regs: Vec<WaitRegistration>,
}

fn register_once(s: &dyn Socket) -> Registered {
    let (tx, rx) = mpsc::channel();
    let entry: Arc<dyn WaitEntry> = Arc::new(Signal(tx));
    let mut regs = Vec::new();
    let mut proc = |q: &WaitQueue, _| regs.push(q.add(entry.clone()));
    s.poll(Some(&mut PollTable::new(PollMask::empty(), &mut proc)));
    Registered { rx, _regs: regs }
}

impl Registered {
    /// Waits for wakeups of the one registration until `s` reports any of `want`.
    fn until(&self, s: &dyn Socket, want: PollMask) -> PollMask {
        loop {
            let m = s.poll(None);
            if m.intersects(want) {
                return m;
            }
            self.rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap_or_else(|_| panic!("no wakeup for {want:?}"));
        }
    }
}

fn tcp_state(s: &dyn Socket) -> u8 {
    let mut info = [0u8; 8];
    s.get_option(SocketLevel::SOL_TCP.0, TcpOption::TCP_INFO.0, &mut info)
        .unwrap();
    info[0]
}

#[test]
fn udp_waiter_registered_before_the_first_send_is_woken() {
    let net = setup();
    let host = UdpSocket::bind("127.0.0.1:0").unwrap();
    let dst: SocketAddr = format!("10.0.2.2:{}", host.local_addr().unwrap().port())
        .parse()
        .unwrap();
    let s = net
        .socket(Domain::Inet, SocketKind::Datagram, 0, 1)
        .unwrap();
    // As an event loop does: watch the socket first, send the query after.
    let watch = register_once(&*s);
    assert_eq!(send(&*s, b"query", Some(dst)), Ok(5));
    let mut buf = [0u8; 16];
    host.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let (_, from) = host.recv_from(&mut buf).unwrap();
    host.send_to(b"answer", from).unwrap();
    assert!(watch.until(&*s, PollMask::IN).contains(PollMask::IN));
    assert_eq!(s.recv(&mut buf, MsgFlags(0)).unwrap().len, 6);
}

#[test]
fn tcp_waiter_registered_before_connect_is_woken() {
    let net = setup();
    let host = TcpListener::bind("127.0.0.1:0").unwrap();
    let dst: SocketAddr = format!("10.0.2.2:{}", host.local_addr().unwrap().port())
        .parse()
        .unwrap();
    let s = net.socket(Domain::Inet, SocketKind::Stream, 0, 1).unwrap();
    let watch = register_once(&*s);
    assert_eq!(s.connect(dst), Err(Errno::EINPROGRESS));
    let (mut peer, _) = host.accept().unwrap();
    // Woken (on the registration made while the socket was still closed) once connected.
    while s.peer_addr().is_err() {
        watch
            .rx
            .recv_timeout(Duration::from_secs(5))
            .expect("woken on connect");
    }
    peer.write_all(b"hi").unwrap();
    watch.until(&*s, PollMask::IN);
    let mut buf = [0u8; 4];
    assert_eq!(s.recv(&mut buf, MsgFlags(0)).unwrap().len, 2);
}

#[test]
fn refused_host_connect_closes_the_socket() {
    let net = setup();
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let dst: SocketAddr = format!("10.0.2.2:{port}").parse().unwrap();
    let s = net.socket(Domain::Inet, SocketKind::Stream, 0, 1).unwrap();
    assert_eq!(s.connect(dst), Err(Errno::EINPROGRESS));
    let ephemeral = s.local_addr().unwrap().port();
    assert_ne!(ephemeral, 0);
    assert!(wait_for(&*s, PollMask::ERR).contains(PollMask::HUP));
    // `tcp_done`: no peer, state CLOSE, the autobound port released (`getsockname` still
    // shows it, as Linux's `inet_sport` does).
    assert_eq!(s.peer_addr(), Err(Errno::ENOTCONN));
    assert_eq!(tcp_state(&*s), 7);
    assert_eq!(s.local_addr().unwrap().port(), ephemeral);
    assert!(net.ports().iter().all(|p| p.local.port() != ephemeral));
    assert_eq!(s.take_error(), Some(Errno::ECONNREFUSED));
    // The next connect reports the (already taken) error once; the one after retries.
    assert_eq!(s.connect(dst), Err(Errno::ECONNABORTED));
    let host = TcpListener::bind(("127.0.0.1", port)).unwrap();
    assert_eq!(s.connect(dst), Err(Errno::EINPROGRESS));
    let _peer = host.accept().unwrap();
    wait_for(&*s, PollMask::OUT);
    assert_eq!(s.peer_addr(), Ok(dst));
    assert_eq!(tcp_state(&*s), 1);
}

#[test]
fn reset_host_stream_has_no_peer() {
    let net = setup();
    let host = TcpListener::bind("127.0.0.1:0").unwrap();
    let dst: SocketAddr = format!("10.0.2.2:{}", host.local_addr().unwrap().port())
        .parse()
        .unwrap();
    let s = net.socket(Domain::Inet, SocketKind::Stream, 0, 1).unwrap();
    assert_eq!(s.connect(dst), Err(Errno::EINPROGRESS));
    let (peer, _) = host.accept().unwrap();
    wait_for(&*s, PollMask::OUT);
    socket2::SockRef::from(&peer)
        .set_linger(Some(Duration::ZERO))
        .unwrap();
    drop(peer);
    wait_for(&*s, PollMask::ERR | PollMask::HUP);
    assert_eq!(s.peer_addr(), Err(Errno::ENOTCONN));
    assert_eq!(tcp_state(&*s), 7);
}

#[test]
fn connected_udp_filters_the_source_and_learns_refusals() {
    let net = setup();
    let host = UdpSocket::bind("127.0.0.1:0").unwrap();
    let other = UdpSocket::bind("127.0.0.1:0").unwrap();
    let dst: SocketAddr = format!("10.0.2.2:{}", host.local_addr().unwrap().port())
        .parse()
        .unwrap();
    let s = net
        .socket(Domain::Inet, SocketKind::Datagram, 0, 1)
        .unwrap();
    s.connect(dst).unwrap();
    assert_eq!(send(&*s, b"query", None), Ok(5));
    let mut buf = [0u8; 16];
    host.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let (_, from) = host.recv_from(&mut buf).unwrap();
    // A stray datagram from another port never reaches the connected socket.
    other.send_to(b"spoof", from).unwrap();
    host.send_to(b"answer", from).unwrap();
    wait_for(&*s, PollMask::IN);
    let r = s.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!(&buf[..r.len], b"answer");
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(s.recv(&mut buf, MsgFlags(0)), Err(Errno::EAGAIN));

    // Nobody listens any more: the ICMP port unreachable comes back as ECONNREFUSED.
    drop(host);
    assert_eq!(send(&*s, b"again", None), Ok(5));
    assert!(wait_for(&*s, PollMask::ERR).contains(PollMask::ERR));
    assert_eq!(s.recv(&mut buf, MsgFlags(0)), Err(Errno::ECONNREFUSED));

    // Disconnected, it hears from anyone again.
    s.disconnect().unwrap();
    let local = UdpSocket::bind("127.0.0.1:0").unwrap();
    let to: SocketAddr = format!("10.0.2.2:{}", local.local_addr().unwrap().port())
        .parse()
        .unwrap();
    assert_eq!(send(&*s, b"x", Some(to)), Ok(1));
    local
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let (_, from) = local.recv_from(&mut buf).unwrap();
    other.send_to(b"any", from).unwrap();
    wait_for(&*s, PollMask::IN);
    let r = s.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!(&buf[..r.len], b"any");
}

#[test]
fn closed_stream_with_a_stalled_peer_is_reset_after_its_linger_time() {
    let net = setup();
    let host = TcpListener::bind("127.0.0.1:0").unwrap();
    let dst: SocketAddr = format!("10.0.2.2:{}", host.local_addr().unwrap().port())
        .parse()
        .unwrap();
    let s = net.socket(Domain::Inet, SocketKind::Stream, 0, 1).unwrap();
    assert_eq!(s.connect(dst), Err(Errno::EINPROGRESS));
    let (mut peer, _) = host.accept().unwrap();
    socket2::SockRef::from(&peer)
        .set_recv_buffer_size(4096)
        .unwrap();
    wait_for(&*s, PollMask::OUT);
    let mut linger = [0u8; 8];
    linger[..4].copy_from_slice(&1i32.to_le_bytes());
    linger[4..].copy_from_slice(&1i32.to_le_bytes());
    s.set_option(
        SocketLevel::SOL_SOCKET.0,
        SocketOption::SO_LINGER.0,
        &linger,
    )
    .unwrap();
    // Fill everything between us and the peer, which never reads.
    let chunk = vec![7u8; 64 * 1024];
    let mut total = 0;
    for _ in 0..200 {
        match send(&*s, &chunk, None) {
            Ok(n) => total += n,
            Err(Errno::EAGAIN) => {
                std::thread::sleep(Duration::from_millis(20));
                if send(&*s, &chunk, None) == Err(Errno::EAGAIN) {
                    break;
                }
            }
            Err(e) => panic!("send: {e:?}"),
        }
    }
    assert!(total > 0);
    drop(s);
    // The orphan gives up after its linger second: the peer, reading at last, is reset
    // before it has everything.
    std::thread::sleep(Duration::from_millis(1500));
    peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut got = 0;
    let mut buf = vec![0u8; 64 * 1024];
    let end = loop {
        match peer.read(&mut buf) {
            Ok(0) => break None,
            Ok(n) => got += n,
            Err(e) => break Some(e.kind()),
        }
    };
    assert_eq!(end, Some(std::io::ErrorKind::ConnectionReset));
    assert!(got < total);
}

#[test]
fn published_connections_wait_for_room_in_the_guest_accept_queue() {
    let net = setup();
    let l = net.socket(Domain::Inet, SocketKind::Stream, 0, 1).unwrap();
    l.bind("0.0.0.0:8081".parse().unwrap(), false).unwrap();
    // Room for one connection (`sk_acceptq_is_full` is `> backlog`).
    l.listen(0).unwrap();
    let publication = net
        .publish(
            "127.0.0.1:0".parse().unwrap(),
            "0.0.0.0:8081".parse().unwrap(),
        )
        .unwrap();
    let mut first = TcpStream::connect(publication.host_addr()).unwrap();
    first.write_all(b"one").unwrap();
    wait_for(&*l, PollMask::IN);
    // The second waits in the host's backlog: neither reset nor closed while the guest is
    // full, as a SYN to a full Linux accept queue goes unanswered.
    let mut second = TcpStream::connect(publication.host_addr()).unwrap();
    second.write_all(b"two").unwrap();
    second
        .set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    let mut buf = [0u8; 8];
    let early = second.read(&mut buf).map_err(|e| e.kind());
    assert!(
        matches!(
            early,
            Err(std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut)
        ),
        "the waiting connection ended early: {early:?}"
    );
    let a = l.accept().unwrap();
    assert_eq!(recv_all(&*a.socket, 3), b"one");
    // The accept made room: the reactor takes the waiting connection now.
    let watch = register_once(&*l);
    watch.until(&*l, PollMask::IN);
    let b = l.accept().unwrap();
    assert_eq!(recv_all(&*b.socket, 3), b"two");
    send(&*b.socket, b"ok", None).unwrap();
    second
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    second.read_exact(&mut buf[..2]).unwrap();
    assert_eq!(&buf[..2], b"ok");
    drop(first);
}

#[test]
fn published_connection_without_a_guest_listener_is_reset() {
    let net = setup();
    let publication = net
        .publish(
            "127.0.0.1:0".parse().unwrap(),
            "0.0.0.0:8082".parse().unwrap(),
        )
        .unwrap();
    let mut client = TcpStream::connect(publication.host_addr()).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut buf = [0u8; 4];
    let got = client.read(&mut buf).map_err(|e| e.kind());
    assert_eq!(got, Err(std::io::ErrorKind::ConnectionReset));
}
