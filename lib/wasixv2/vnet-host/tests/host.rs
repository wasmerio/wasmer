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
    Backend, Domain, Net, NetConfig, Network, PollMask, PollTable, SendMsg, Shutdown, Socket,
    SocketKind, StreamState,
};
use wasixv2_vnet_host::{HostBackend, ResolverConfig, VsockConfig, VsockRoute};
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
        net_raw: false,
        now: std::time::Duration::ZERO,
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

fn send_oob(s: &dyn Socket, data: &[u8]) -> Result<usize, Errno> {
    s.send(SendMsg {
        data,
        to: None,
        flags: MsgFlags::MSG_OOB,
        control: &[],
        net_raw: false,
        now: Duration::ZERO,
    })
}

/// Urgent data crosses the host bridge both ways: a host `MSG_OOB` byte is the guest's urgent
/// byte, out of its stream and polling `PRI` until `recv(MSG_OOB)` reads it, and the last byte
/// of a guest `MSG_OOB` send is urgent for the host peer.
#[test]
fn urgent_data_crosses_the_bridge() {
    use std::os::fd::AsRawFd;

    let net = setup();
    let host = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = host.local_addr().unwrap().port();
    let s = net.socket(Domain::Inet, SocketKind::Stream, 0, 1).unwrap();
    let dst: SocketAddr = format!("10.0.2.2:{port}").parse().unwrap();
    assert_eq!(s.connect(dst), Err(Errno::EINPROGRESS));
    let (mut peer, _) = host.accept().unwrap();
    assert!(wait_for(&*s, PollMask::OUT | PollMask::ERR).contains(PollMask::OUT));

    peer.write_all(b"ab").unwrap();
    socket2::SockRef::from(&peer)
        .send_out_of_band(b"U")
        .unwrap();
    peer.write_all(b"cd").unwrap();
    wait_for(&*s, PollMask::PRI);
    assert_eq!(s.input_len(), Ok(2));
    assert_eq!(s.at_mark(), Ok(false));
    let mut buf = [0u8; 16];
    assert_eq!(s.recv(&mut buf, MsgFlags(0)).unwrap().len, 2);
    assert_eq!(&buf[..2], b"ab");
    assert_eq!(s.at_mark(), Ok(true));
    let r = s.recv(&mut buf, MsgFlags::MSG_OOB).unwrap();
    assert_eq!((r.len, buf[0], r.flags), (1, b'U', MsgFlags::MSG_OOB));
    assert!(!s.poll(None).contains(PollMask::PRI));
    assert_eq!(recv_all(&*s, 2), b"cd");

    assert_eq!(send(&*s, b"xy", None), Ok(2));
    assert_eq!(send_oob(&*s, b"zW"), Ok(2));
    let mut got = [0u8; 3];
    peer.read_exact(&mut got).unwrap();
    assert_eq!(&got, b"xyz");
    let mut pfd = libc::pollfd {
        fd: peer.as_raw_fd(),
        events: libc::POLLPRI,
        revents: 0,
    };
    // SAFETY: one valid pollfd.
    assert_eq!(unsafe { libc::poll(&mut pfd, 1, 5000) }, 1);
    let mut oob = 0u8;
    // SAFETY: one writable byte.
    let n = unsafe {
        libc::recv(
            peer.as_raw_fd(),
            (&mut oob as *mut u8).cast(),
            1,
            libc::MSG_OOB,
        )
    };
    assert_eq!((n, oob), (1, b'W'));
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
fn published_udp_port_reaches_the_guest_socket_and_replies_leave_from_it() {
    let net = setup();
    let s = net
        .socket(Domain::Inet, SocketKind::Datagram, 0, 1)
        .unwrap();
    s.bind("0.0.0.0:5300".parse().unwrap(), false).unwrap();
    let publication = net
        .publish_udp(
            "127.0.0.1:0".parse().unwrap(),
            "0.0.0.0:5300".parse().unwrap(),
        )
        .unwrap();
    let published = publication.host_addr();
    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    client.send_to(b"query", published).unwrap();
    wait_for(&*s, PollMask::IN);
    let mut buf = [0u8; 16];
    let r = s.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!(&buf[..r.len], b"query");
    // The host peer is on the host's loopback, which the guest knows as the gateway alias.
    let peer = r.from.unwrap();
    assert_eq!(peer.ip().to_string(), "10.0.2.2");
    assert_eq!(peer.port(), client.local_addr().unwrap().port());
    // The reply comes from the published port.
    assert_eq!(send(&*s, b"answer", Some(peer)), Ok(6));
    let (n, from) = client.recv_from(&mut buf).unwrap();
    assert_eq!(&buf[..n], b"answer");
    assert_eq!(from, published);

    // With the socket gone, what arrives is dropped, not kept for the next one.
    drop(s);
    client.send_to(b"stale", published).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    let next = net
        .socket(Domain::Inet, SocketKind::Datagram, 0, 1)
        .unwrap();
    next.bind("0.0.0.0:5300".parse().unwrap(), false).unwrap();
    client.send_to(b"fresh", published).unwrap();
    wait_for(&*next, PollMask::IN);
    let r = next.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!(&buf[..r.len], b"fresh");
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

#[cfg(target_os = "linux")]
#[test]
fn host_port_unreachables_reach_the_error_queue_in_the_guest_view() {
    let net = setup();
    let s = net
        .socket(Domain::Inet, SocketKind::Datagram, 0, 1)
        .unwrap();
    s.set_option(
        SocketLevel::SOL_IP.0,
        wasixv2_abi::IpOption::IP_RECVERR.0,
        &1i32.to_le_bytes(),
    )
    .unwrap();
    let closed = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let dst: SocketAddr = format!("10.0.2.2:{port}").parse().unwrap();
    assert_eq!(send(&*s, b"query", Some(dst)), Ok(5));
    wait_for(&*s, PollMask::ERR);
    let mut buf = [0u8; 16];
    let r = loop {
        // The pending error and the queued entry come from one reactor pass, but the wakeup
        // may fall between them.
        match s.recv(&mut buf, MsgFlags::MSG_ERRQUEUE) {
            Err(Errno::EAGAIN) => std::thread::sleep(Duration::from_millis(10)),
            r => break r.unwrap(),
        }
    };
    assert_eq!(&buf[..r.len], b"query");
    assert_eq!(r.flags, MsgFlags::MSG_ERRQUEUE);
    // The host's loopback is the guest's gateway alias, for the address and the offender.
    assert_eq!(r.from, Some(dst));
    let c = &r.control[0];
    assert_eq!(c.data[..8], [111, 0, 0, 0, 2, 3, 3, 0]);
    assert_eq!(c.data[16..24], [2, 0, 0, 0, 10, 0, 2, 2]);
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

/// A guest ping socket's echo request to a host address leaves through a host ICMP datagram
/// socket where the host allows one (Linux's `ping_group_range`), and the reply comes back
/// with the guest's identifier; where the host refuses, the send fails with `EPERM`.
#[test]
fn echo_requests_through_the_host() {
    let net = setup();
    let p = net
        .socket(Domain::Inet, SocketKind::Datagram, 1, 1)
        .unwrap();
    let mut req = vec![8, 0, 0, 0, 0, 0, 0, 3];
    req.extend(b"wasixv2");
    let dst: SocketAddr = "10.0.2.2:0".parse().unwrap();
    let host_allows = std::fs::read_to_string("/proc/sys/net/ipv4/ping_group_range")
        .ok()
        .and_then(|r| {
            let mut it = r.split_whitespace().map(|v| v.parse::<u64>().ok());
            Some((it.next()??, it.next()??))
        })
        .is_none_or(|(lo, hi)| {
            // SAFETY: getegid has no preconditions.
            let gid = u64::from(unsafe { libc::getegid() });
            lo <= gid && gid <= hi
        });
    match send(&*p, &req, Some(dst)) {
        Err(Errno::EPERM) if !host_allows => return,
        sent => assert_eq!(sent, Ok(req.len())),
    }
    wait_for(&*p, PollMask::IN);
    let mut buf = [0u8; 64];
    let r = p.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!(r.from, Some(dst));
    assert_eq!(buf[0], 0);
    let ident = p.local_addr().unwrap().port();
    assert_eq!(u16::from_be_bytes([buf[4], buf[5]]), ident);
    assert_eq!(&buf[8..r.len], b"wasixv2");
}

/// A query for `name` of `qtype`, class IN, with recursion desired.
fn dns_query(id: u16, name: &str, qtype: u16) -> Vec<u8> {
    let mut q = id.to_be_bytes().to_vec();
    q.extend([1, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    for label in name.split('.') {
        q.push(label.len() as u8);
        q.extend(label.as_bytes());
    }
    q.push(0);
    q.extend(qtype.to_be_bytes());
    q.extend([0, 1]);
    q
}

/// A stub name server on 127.0.0.1, UDP and TCP on one port: over UDP it answers every query
/// with TC set and nothing else, over TCP with one TXT record "hello", as a server whose
/// answer is too long for a datagram would. It reports the identifiers it saw.
fn stub_name_server(seen: mpsc::Sender<u16>) -> u16 {
    let (tcp, udp) = loop {
        let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = tcp.local_addr().unwrap().port();
        if let Ok(udp) = UdpSocket::bind(("127.0.0.1", port)) {
            break (tcp, udp);
        }
    };
    let port = udp.local_addr().unwrap().port();
    let answer = |q: &[u8], truncated: bool| {
        let mut a = q.to_vec();
        a[2] |= 0x80 | if truncated { 0x02 } else { 0 };
        a[3] = 0x80;
        if !truncated {
            a[7] = 1;
            a.extend([0xc0, 12, 0, 16, 0, 1, 0, 0, 0, 60, 0, 6, 5]);
            a.extend(b"hello");
        }
        a
    };
    let udp_seen = seen.clone();
    std::thread::spawn(move || {
        let mut buf = [0; 512];
        while let Ok((n, from)) = udp.recv_from(&mut buf) {
            let _ = udp_seen.send(u16::from_be_bytes([buf[0], buf[1]]));
            let _ = udp.send_to(&answer(&buf[..n], true), from);
        }
    });
    std::thread::spawn(move || {
        for mut c in tcp.incoming().flatten() {
            let mut len = [0; 2];
            c.read_exact(&mut len).unwrap();
            let mut q = vec![0; usize::from(u16::from_be_bytes(len))];
            c.read_exact(&mut q).unwrap();
            let _ = seen.send(u16::from_be_bytes([q[0], q[1]]));
            let a = answer(&q, false);
            c.write_all(&(a.len() as u16).to_be_bytes()).unwrap();
            c.write_all(&a).unwrap();
        }
    });
    port
}

/// The guest's query to 10.0.2.3 and its answer, over UDP.
fn ask(s: &dyn Socket, query: &[u8]) -> Vec<u8> {
    send(s, query, Some("10.0.2.3:53".parse().unwrap())).unwrap();
    wait_for(s, PollMask::IN);
    let mut buf = [0; 512];
    let r = s.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!(r.from, Some("10.0.2.3:53".parse().unwrap()));
    buf[..r.len].to_vec()
}

/// The DNS server with the host backend's resolver: names in the host's hosts file are
/// answered from it, with the CNAME of their canonical name; a query it does not answer
/// itself goes to the host's name servers with an identifier of its own, over TCP once the
/// UDP answer comes back truncated, and its answer goes to the guest with the guest's.
#[test]
fn the_dns_server_asks_the_hosts_file_and_the_host_name_servers() {
    let dir = std::env::temp_dir().join(format!("wasixv2-vnet-host-dns-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("hosts"), "192.0.2.10 stub.test alias.test\n").unwrap();
    std::fs::write(dir.join("resolv.conf"), "nameserver 127.0.0.1\n").unwrap();
    let (tx, seen) = mpsc::channel();
    let config = ResolverConfig {
        hosts: dir.join("hosts"),
        resolv_conf: dir.join("resolv.conf"),
        port: stub_name_server(tx),
    };
    let net = Net::new(
        NetConfig::host_default(),
        Some(HostBackend::with_resolver(config).unwrap()),
    );
    let s = net
        .socket(Domain::Inet, SocketKind::Datagram, 0, 1)
        .unwrap();

    let a = ask(&*s, &dns_query(0x0102, "Alias.Test", 1));
    assert_eq!((&a[..2], a[3] & 0xf, a[7]), (&[1, 2][..], 0, 2));
    // CNAME alias.test -> stub.test, then stub.test A 192.0.2.10.
    assert!(a.windows(11).any(|w| w == b"\x04stub\x04test\x00"));
    assert!(a.ends_with(&[192, 0, 2, 10]));

    let a = ask(&*s, &dns_query(0x0304, "txt.test", 16));
    assert_eq!((&a[..2], a[2] & 0x02, a[7]), (&[3, 4][..], 0, 1));
    assert!(a.ends_with(b"\x05hello"));
    let (udp_id, tcp_id) = (seen.recv().unwrap(), seen.recv().unwrap());
    assert_eq!(udp_id, tcp_id);
    assert_ne!(udp_id, 0x0304);

    // A name nobody has: NXDOMAIN, or SERVFAIL on a host without a working resolver.
    let a = ask(&*s, &dns_query(5, "nothing.invalid", 1));
    assert!(matches!(a[3] & 0xf, 2 | 3), "rcode {}", a[3] & 0xf);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The vsock bridge by firecracker's convention: a connection to port P of a host CID is one to
/// the host Unix socket `<prefix>_P`; nobody there is the host's reset.
#[test]
fn vsock_streams_reach_host_unix_sockets_by_port() {
    let dir = std::env::temp_dir().join(format!("wasixv2-vsock-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let prefix = dir.join("v.sock");
    let listener = std::os::unix::net::UnixListener::bind(dir.join("v.sock_5000")).unwrap();
    let vsock = VsockConfig {
        cid: 3,
        routes: vec![(Some(2), VsockRoute::UnixPrefix(prefix))],
    };
    let backend = HostBackend::with_vsock(ResolverConfig::default(), vsock).unwrap();
    assert_eq!(backend.vsock_cid(), Some(3));
    let (tx, rx) = mpsc::channel();
    let wq = WaitQueue::new();
    let _reg = wq.add(Arc::new(Signal(tx)));
    let stream = backend.vsock_connect(2, 5000, wq).unwrap();
    let (mut host, _) = listener.accept().unwrap();
    while stream.state() != StreamState::Established {
        rx.recv_timeout(Duration::from_secs(5)).expect("connected");
    }
    assert_eq!(stream.write(b"ping"), Ok(4));
    let mut buf = [0; 4];
    host.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"ping");
    host.write_all(b"pong").unwrap();
    while stream.input_len() < 4 {
        rx.recv_timeout(Duration::from_secs(5)).expect("data");
    }
    assert_eq!(stream.read(&mut buf, false), Ok(4));
    assert_eq!(&buf, b"pong");
    // No host socket for the port, and no route for the CID.
    assert_eq!(
        backend.vsock_connect(2, 5001, WaitQueue::new()).err(),
        Some(Errno::ECONNRESET)
    );
    assert_eq!(
        backend.vsock_connect(7, 5000, WaitQueue::new()).err(),
        Some(Errno::ECONNRESET)
    );
    assert_eq!(HostBackend::new().unwrap().vsock_cid(), None);
    let _ = std::fs::remove_dir_all(&dir);
}
