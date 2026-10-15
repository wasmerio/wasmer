//! The host backend against real host sockets on 127.0.0.1.
#![cfg(unix)]

use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream, UdpSocket},
    sync::{Arc, mpsc},
    time::Duration,
};

use wasixv2_abi::{Errno, MsgFlags};
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
    let s = net.socket(Domain::Inet, SocketKind::Stream, 0).unwrap();
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
    let s = net.socket(Domain::Inet, SocketKind::Stream, 0).unwrap();
    let dst: SocketAddr = format!("10.0.2.2:{port}").parse().unwrap();
    assert_eq!(s.connect(dst), Err(Errno::EINPROGRESS));
    assert!(wait_for(&*s, PollMask::ERR).contains(PollMask::HUP));
    assert_eq!(s.take_error(), Some(Errno::ECONNREFUSED));
}

#[test]
fn published_port_reaches_guest_listener() {
    let net = setup();
    let l = net.socket(Domain::Inet, SocketKind::Stream, 0).unwrap();
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
    let s = net.socket(Domain::Inet, SocketKind::Datagram, 0).unwrap();
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
