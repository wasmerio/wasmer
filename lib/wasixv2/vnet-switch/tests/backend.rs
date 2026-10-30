use std::{net::SocketAddr, sync::Arc, time::Duration};
use wasixv2_abi::{Errno, IpOption, MsgFlags, SocketLevel, SocketOption};
use wasixv2_vnet::{
    Backend, Domain, Network, PollMask, SendMsg, Shutdown, Socket, SocketKind, WaitQueue,
};
use wasixv2_vnet_switch::{Config, Switch};

fn addr(s: &str) -> SocketAddr {
    s.parse().unwrap()
}
fn switch() -> Arc<Switch> {
    Switch::new(Config::default()).unwrap()
}
fn socket(net: &dyn Network, domain: Domain, kind: SocketKind) -> Arc<dyn Socket> {
    net.socket(domain, kind, 0, 1).unwrap()
}
fn send(s: &dyn Socket, bytes: &[u8], to: Option<SocketAddr>) -> Result<usize, Errno> {
    s.send(SendMsg {
        data: bytes,
        to,
        flags: MsgFlags(0),
        control: &[],
        net_raw: false,
        now: Duration::ZERO,
    })
}
fn receive(s: &dyn Socket) -> Vec<u8> {
    let mut b = [0; 64];
    let r = s.recv(&mut b, MsgFlags(0)).unwrap();
    b[..r.len].to_vec()
}
#[test]
fn machines_route_tcp_and_discover_only_live_nonloopback_listeners() {
    let sw = switch();
    let a = sw.network("192.0.2.10".parse().unwrap(), 24).unwrap();
    let b = sw.network("192.0.2.11".parse().unwrap(), 24).unwrap();
    let listener = socket(&*b, Domain::Inet, SocketKind::Stream);
    listener.bind(addr("0.0.0.0:9000"), false).unwrap();
    listener.listen(4).unwrap();
    let private = socket(&*b, Domain::Inet, SocketKind::Stream);
    private.bind(addr("127.0.0.1:9001"), false).unwrap();
    private.listen(1).unwrap();
    assert_eq!(
        sw.listeners().iter().map(|l| l.address).collect::<Vec<_>>(),
        [addr("192.0.2.11:9000")]
    );
    let client = socket(&*a, Domain::Inet, SocketKind::Stream);
    let r = client.connect(addr("192.0.2.11:9000"));
    assert!(r.is_ok() || r == Err(Errno::EINPROGRESS));
    let accepted = listener.accept().unwrap();
    assert_eq!(
        accepted.peer,
        client.local_addr().unwrap(),
        "switch routes preserve the guest source port"
    );
    assert_eq!(
        accepted.peer.ip(),
        "192.0.2.10".parse::<std::net::IpAddr>().unwrap()
    );
    assert_eq!(send(&*client, b"hello", None), Ok(5));
    assert_eq!(receive(&*accepted.socket), b"hello");
    accepted.socket.shutdown(Shutdown::Write).unwrap();
    assert!(client.poll(None).contains(PollMask::RDHUP));
    assert_eq!(receive(&*client), b"");
    assert_eq!(send(&*client, b"back", None), Ok(4));
    assert_eq!(receive(&*accepted.socket), b"back");
    drop(listener);
    assert!(sw.listeners().is_empty());
    let refused = socket(&*a, Domain::Inet, SocketKind::Stream);
    assert_eq!(
        refused.connect(addr("192.0.2.11:9001")),
        Err(Errno::ECONNREFUSED)
    );
}
#[test]
fn udp_peers_preserve_bound_source_empty_peek_and_truncation() {
    let sw = switch();
    let a = sw.network("192.0.2.10".parse().unwrap(), 24).unwrap();
    let b = sw.network("192.0.2.11".parse().unwrap(), 24).unwrap();
    let left = socket(&*a, Domain::Inet, SocketKind::Datagram);
    let right = socket(&*b, Domain::Inet, SocketKind::Datagram);
    left.bind(addr("0.0.0.0:8000"), false).unwrap();
    right.bind(addr("0.0.0.0:8000"), false).unwrap();
    assert_eq!(send(&*left, b"", Some(addr("192.0.2.11:8000"))), Ok(0));
    let mut bytes = [0; 2];
    let r = right.recv(&mut bytes, MsgFlags::MSG_PEEK).unwrap();
    assert_eq!(r.full_len, 0);
    assert_eq!(r.from, Some(addr("192.0.2.10:8000")));
    assert_eq!(right.recv(&mut bytes, MsgFlags(0)).unwrap().len, 0);
    send(&*right, b"four", r.from).unwrap();
    let r = left.recv(&mut bytes, MsgFlags::MSG_TRUNC).unwrap();
    assert_eq!(r.full_len, 4);
    assert_eq!(r.len, 2);
    assert_eq!(&bytes, b"fo");
    assert!(r.flags.contains(MsgFlags::MSG_TRUNC));
}
#[test]
fn connected_udp_refusal_is_consumed_once_by_next_send_or_recv() {
    let sw = switch();
    let a = sw.network("192.0.2.10".parse().unwrap(), 24).unwrap();
    let _b = sw.network("192.0.2.11".parse().unwrap(), 24).unwrap();
    let s = socket(&*a, Domain::Inet, SocketKind::Datagram);
    s.connect(addr("192.0.2.11:9000")).unwrap();
    assert_eq!(send(&*s, b"probe", None), Ok(5));
    assert!(s.poll(None).contains(PollMask::ERR));
    assert_eq!(send(&*s, b"probe", None), Err(Errno::ECONNREFUSED));
    assert!(!s.poll(None).contains(PollMask::ERR));
    assert_eq!(send(&*s, b"probe", None), Ok(5));
    assert_eq!(s.recv(&mut [0; 8], MsgFlags(0)), Err(Errno::ECONNREFUSED));
    assert_eq!(s.recv(&mut [0; 8], MsgFlags(0)), Err(Errno::EAGAIN));
    s.disconnect().unwrap();
    assert_eq!(send(&*s, b"probe", Some(addr("192.0.2.11:9000"))), Ok(5));
    assert_eq!(s.take_error(), None);
}
#[test]
fn recverr_quotes_unconnected_refusal_and_dequeue_clears_pollerr() {
    let sw = switch();
    let a = sw.backend("192.0.2.10".parse().unwrap()).unwrap();
    let _b = sw.backend("192.0.2.11".parse().unwrap()).unwrap();
    let ep = a.datagram(Domain::Inet, WaitQueue::new()).unwrap();
    ep.set_option(
        SocketLevel::SOL_IP.0,
        IpOption::IP_RECVERR.0,
        &1i32.to_le_bytes(),
    )
    .unwrap();
    ep.send_to(b"quote", addr("192.0.2.11:9000")).unwrap();
    assert!(ep.poll(None).contains(PollMask::ERR));
    let err = ep.take_queued_error().unwrap();
    assert_eq!(err.errno, Errno::ECONNREFUSED);
    assert_eq!((err.type_, err.code), (3, 3));
    assert_eq!(err.dst, Some(addr("192.0.2.11:9000")));
    assert_eq!(err.payload, b"quote");
    assert!(!ep.poll(None).contains(PollMask::ERR));
}
#[test]
fn ipv6_peers_use_the_same_backend_and_independent_ports() {
    let sw = switch();
    let a = sw.network("fd00::10".parse().unwrap(), 64).unwrap();
    let b = sw.network("fd00::11".parse().unwrap(), 64).unwrap();
    let s = socket(&*b, Domain::Inet6, SocketKind::Stream);
    s.bind(addr("[::]:8000"), false).unwrap();
    s.listen(1).unwrap();
    let c = socket(&*a, Domain::Inet6, SocketKind::Stream);
    let _ = c.connect(addr("[fd00::11]:8000"));
    let accepted = s.accept().unwrap().socket;
    send(&*c, b"v6", None).unwrap();
    assert_eq!(receive(&*accepted), b"v6");
    let d = socket(&*a, Domain::Inet6, SocketKind::Datagram);
    d.connect(addr("[fd00::11]:8001")).unwrap();
    send(&*d, b"v6", None).unwrap();
    assert_eq!(d.take_error(), Some(Errno::ECONNREFUSED));
}
#[test]
fn full_accept_queue_wakes_pending_connect_and_close_refuses_it() {
    let sw = switch();
    let a = sw.backend("192.0.2.10".parse().unwrap()).unwrap();
    let b = sw.network("192.0.2.11".parse().unwrap(), 24).unwrap();
    let l = socket(&*b, Domain::Inet, SocketKind::Stream);
    l.bind(addr("0.0.0.0:9000"), false).unwrap();
    l.listen(0).unwrap();
    let first = a
        .connect(addr("192.0.2.11:9000"), WaitQueue::new())
        .unwrap();
    let second = a
        .connect(addr("192.0.2.11:9000"), WaitQueue::new())
        .unwrap();
    assert_eq!(second.state(), wasixv2_vnet::StreamState::Connecting);
    let one = l.accept().unwrap();
    assert!(second.poll(None).contains(PollMask::OUT));
    let two = l.accept().unwrap();
    first.write(b"a").unwrap();
    second.write(b"b").unwrap();
    assert_eq!(receive(&*one.socket), b"a");
    assert_eq!(receive(&*two.socket), b"b");
    let third = a
        .connect(addr("192.0.2.11:9000"), WaitQueue::new())
        .unwrap();
    let fourth = a
        .connect(addr("192.0.2.11:9000"), WaitQueue::new())
        .unwrap();
    assert_eq!(fourth.state(), wasixv2_vnet::StreamState::Connecting);
    drop(l);
    assert!(fourth.poll(None).contains(PollMask::ERR));
    assert_eq!(fourth.take_error(), Some(Errno::ECONNREFUSED));
    drop(third);
}
#[test]
fn port_lifetime_duplicate_ownership_limits_and_switch_isolation() {
    let sw = Switch::new(Config {
        max_endpoints: 1,
        datagram_bytes: 768,
    })
    .unwrap();
    let a = sw.backend("192.0.2.10".parse().unwrap()).unwrap();
    assert!(sw.backend("192.0.2.10".parse().unwrap()).is_err());
    let ep = a.datagram(Domain::Inet, WaitQueue::new()).unwrap();
    assert!(a.datagram(Domain::Inet, WaitQueue::new()).is_err());
    drop(a);
    assert!(sw.backend("192.0.2.10".parse().unwrap()).is_err());
    drop(ep);
    assert!(sw.backend("192.0.2.10".parse().unwrap()).is_ok());
    assert!(sw.backend("127.0.0.1".parse().unwrap()).is_err());
    let foreign = switch();
    let x = foreign.backend("192.0.2.11".parse().unwrap()).unwrap();
    assert!(matches!(
        x.connect(addr("192.0.2.10:8000"), WaitQueue::new()),
        Err(Errno::EHOSTUNREACH)
    ));
}
#[test]
fn published_datagrams_drop_until_attached_reset_state_and_stay_bound() {
    let sw = switch();
    let a = sw.backend("192.0.2.10".parse().unwrap()).unwrap();
    let b = sw.backend("192.0.2.11".parse().unwrap()).unwrap();
    let sender = a.datagram(Domain::Inet, WaitQueue::new()).unwrap();
    let (ep, publication) = b.publish_datagram(addr("192.0.2.11:0")).unwrap();
    let dst = publication.host_addr();
    assert_ne!(dst.port(), 0);
    sender.send_to(b"lost", dst).unwrap();
    assert_eq!(ep.recv_from(&mut [0; 8], false), Err(Errno::EAGAIN));
    ep.attach(Some(WaitQueue::new())).unwrap();
    sender.send_to(b"", dst).unwrap();
    assert_eq!(ep.recv_from(&mut [], false).unwrap().1, 0);
    sender.send_to(b"old", dst).unwrap();
    ep.attach(None).unwrap();
    ep.attach(Some(WaitQueue::new())).unwrap();
    assert_eq!(ep.recv_from(&mut [0; 8], false), Err(Errno::EAGAIN));
    drop(publication);
    assert!(b.publish_datagram(dst).is_err());
    drop(ep);
    assert!(b.publish_datagram(dst).is_ok());
}
#[test]
fn memory_stream_reuses_bounded_buffers_urgent_marks_and_unread_reset() {
    use wasixv2_vnet::{BackendStream, MemoryStream, Pollable};
    let [a, b] = MemoryStream::pair(
        [addr("192.0.2.10:1234"), addr("192.0.2.11:1234")],
        [WaitQueue::new(), WaitQueue::new()],
    );
    a.write_urgent(b"ab!").unwrap();
    let mut buf = [0; 8];
    assert_eq!(b.read(&mut buf, false), Ok(2));
    assert!(b.at_mark());
    assert!(b.poll(None).contains(PollMask::PRI));
    assert_eq!(b.read_urgent(true), Ok(b'!'));
    assert_eq!(b.read_urgent(false), Ok(b'!'));
    a.set_option(
        SocketLevel::SOL_SOCKET.0,
        SocketOption::SO_SNDBUF.0,
        &1i32.to_le_bytes(),
    )
    .unwrap();
    b.set_option(
        SocketLevel::SOL_SOCKET.0,
        SocketOption::SO_RCVBUF.0,
        &1i32.to_le_bytes(),
    )
    .unwrap();
    let n = a.write(&vec![0; 32768]).unwrap();
    assert!(n < 32768);
    assert_eq!(a.write(b"x"), Err(Errno::EAGAIN));
    drop(b);
    assert_eq!(a.take_error(), Some(Errno::ECONNRESET));
}

#[test]
fn already_resolved_udp_endpoint_attaches_through_net_publication_hook() {
    use wasixv2_vnet::{Net, NetConfig, Uplink};
    let sw = switch();
    let backend = sw.backend("192.0.2.11".parse().unwrap()).unwrap();
    let net = Net::new(
        NetConfig {
            uplink: Some(Uplink {
                v4: Some(("192.0.2.11".parse().unwrap(), 24, None)),
                ..Uplink::default()
            }),
            ..NetConfig::default()
        },
        Some(backend.clone()),
    );
    let s = socket(&*net, Domain::Inet, SocketKind::Datagram);
    s.bind(addr("0.0.0.0:8080"), false).unwrap();
    let pair = backend.publish_datagram(addr("192.0.2.11:0")).unwrap();
    let host = pair.1.host_addr();
    let publication = net.publish_udp_endpoint(pair, addr("0.0.0.0:8080"));
    assert_eq!(publication.host_addr(), host);
    let source = sw.backend("192.0.2.10".parse().unwrap()).unwrap();
    let ep = source.datagram(Domain::Inet, WaitQueue::new()).unwrap();
    ep.send_to(b"adopted", host).unwrap();
    assert_eq!(receive(&*s), b"adopted");
}
#[test]
fn switched_established_stream_reads_reset_then_eof() {
    let sw = switch();
    let a = sw.backend("192.0.2.10".parse().unwrap()).unwrap();
    let b = sw.network("192.0.2.11".parse().unwrap(), 24).unwrap();
    let listener = socket(&*b, Domain::Inet, SocketKind::Stream);
    listener.bind(addr("0.0.0.0:9000"), false).unwrap();
    listener.listen(1).unwrap();
    let stream = a
        .connect(addr("192.0.2.11:9000"), WaitQueue::new())
        .unwrap();
    let accepted = listener.accept().unwrap().socket;
    stream.write(b"unread").unwrap();
    drop(accepted);
    assert_eq!(stream.read(&mut [0; 8], false), Err(Errno::ECONNRESET));
    assert_eq!(stream.read(&mut [0; 8], false), Ok(0));
}

#[test]
fn unbound_udp_source_port_and_reuseport_listener_survive_peer_routing() {
    let sw = switch();
    let a = sw.network("192.0.2.10".parse().unwrap(), 24).unwrap();
    let b = sw.network("192.0.2.11".parse().unwrap(), 24).unwrap();
    let left = socket(&*a, Domain::Inet, SocketKind::Datagram);
    let right = socket(&*b, Domain::Inet, SocketKind::Datagram);
    right.bind(addr("0.0.0.0:9000"), false).unwrap();
    left.connect(addr("192.0.2.11:9000")).unwrap();
    send(&*left, b"tuple", None).unwrap();
    let mut buf = [0; 8];
    let received = right.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!(received.from, Some(left.local_addr().unwrap()));
    send(&*right, b"reply", received.from).unwrap();
    assert_eq!(receive(&*left), b"reply");
    let make_listener = || {
        let s = socket(&*b, Domain::Inet, SocketKind::Stream);
        s.set_option(
            SocketLevel::SOL_SOCKET.0,
            SocketOption::SO_REUSEPORT.0,
            &1i32.to_le_bytes(),
        )
        .unwrap();
        s.bind(addr("0.0.0.0:8080"), false).unwrap();
        s.listen(1).unwrap();
        s
    };
    let first = make_listener();
    let second = make_listener();
    drop(first);
    assert!(
        sw.listeners()
            .iter()
            .any(|l| l.address == addr("192.0.2.11:8080"))
    );
    let client = socket(&*a, Domain::Inet, SocketKind::Stream);
    let _ = client.connect(addr("192.0.2.11:8080"));
    assert!(second.accept().is_ok());
}
