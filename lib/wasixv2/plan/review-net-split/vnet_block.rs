// ---- a recording backend ----

use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

use wasixv2_abi::{IpOption, SocketLevel, SocketOption, TcpOption};
use wasixv2_wait::{PollTable, Pollable, WaitEntry, WaitQueue};

use crate::{
    Backend, BackendDatagram, BackendStream, Control, InboundSink, Publication, Room, StreamState,
};

/// A connected stream that records the options applied to it.
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
struct FakeDgram {
    writable: AtomicBool,
    wq: WaitQueue,
    sent: Mutex<Vec<Vec<u8>>>,
}

impl Pollable for FakeDgram {
    fn poll(&self, _: Option<&mut PollTable<'_>>) -> PollMask {
        match self.writable.load(Ordering::SeqCst) {
            true => PollMask::OUT | PollMask::WRNORM,
            false => PollMask::empty(),
        }
    }
}

impl BackendDatagram for FakeDgram {
    fn send_to(&self, data: &[u8], _: SocketAddr) -> Result<usize, Errno> {
        if !self.writable.load(Ordering::SeqCst) {
            return Err(Errno::EAGAIN);
        }
        self.sent.lock().unwrap().push(data.to_vec());
        Ok(data.len())
    }
    fn connect(&self, _: Option<SocketAddr>) -> Result<(), Errno> {
        Ok(())
    }
    fn take_error(&self) -> Option<Errno> {
        None
    }
    fn recv_from(&self, _: &mut [u8], _: bool) -> Result<(usize, usize, SocketAddr), Errno> {
        Err(Errno::EAGAIN)
    }
    fn input_len(&self) -> usize {
        0
    }
}

struct FakePublication(Arc<AtomicU32>);

impl Publication for FakePublication {
    fn host_addr(&self) -> SocketAddr {
        a("127.0.0.1:1")
    }
    fn set_backlog(&self, backlog: u32) {
        self.0.store(backlog, Ordering::SeqCst);
    }
}

/// A publication's sink and the host backlog it was given.
type Published = (Arc<dyn InboundSink>, Arc<AtomicU32>);

#[derive(Default)]
struct Recorder {
    streams: Mutex<Vec<Arc<FakeStream>>>,
    dgrams: Mutex<Vec<Arc<FakeDgram>>>,
    /// Each publication's sink and host backlog.
    published: Mutex<Vec<Published>>,
}

impl Backend for Recorder {
    fn connect(&self, peer: SocketAddr, _: WaitQueue) -> Result<Arc<dyn BackendStream>, Errno> {
        let s = FakeStream::new(peer);
        self.streams.lock().unwrap().push(s.clone());
        Ok(s)
    }
    fn datagram(&self, _: Domain, wq: WaitQueue) -> Result<Arc<dyn BackendDatagram>, Errno> {
        let d = Arc::new(FakeDgram {
            writable: AtomicBool::new(true),
            wq,
            sent: Mutex::default(),
        });
        self.dgrams.lock().unwrap().push(d.clone());
        Ok(d)
    }
    fn publish(
        &self,
        _: SocketAddr,
        backlog: u32,
        sink: Arc<dyn InboundSink>,
    ) -> Result<Box<dyn Publication>, std::io::Error> {
        let b = Arc::new(AtomicU32::new(backlog));
        self.published.lock().unwrap().push((sink, b.clone()));
        Ok(Box::new(FakePublication(b)))
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

struct Flag(AtomicBool);

impl WaitEntry for Flag {
    fn wake(&self, _: PollMask) -> bool {
        self.0.store(true, Ordering::SeqCst);
        true
    }
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
fn published_connections_wait_on_the_host_while_the_accept_queue_is_full() {
    let (n, rec) = backed();
    let l = tcp(&n);
    l.set_option(SOL_TCP, TcpOption::TCP_NODELAY.0, &int(1))
        .unwrap();
    l.bind(a("0.0.0.0:8080"), false).unwrap();
    l.listen(0).unwrap();
    let (sink, backlog) = rec.published.lock().unwrap()[0].clone();
    // The host listener takes the guest's backlog.
    assert_eq!(backlog.load(Ordering::SeqCst), 0);
    assert_eq!(sink.poll_room(None), Room::Ready);
    let first = FakeStream::new(a("127.0.0.1:40000"));
    assert!(sink.incoming(first.clone(), WaitQueue::new()));
    // The accepted socket inherits the listener's options (`inet_csk_clone_lock`).
    assert_eq!(
        first.option(SOL_TCP, TcpOption::TCP_NODELAY.0),
        Some(int(1).to_vec())
    );
    // Backlog 0 holds one connection: the next waits on the host, registered for room.
    let woken = Arc::new(Flag(AtomicBool::new(false)));
    let entry: Arc<dyn WaitEntry> = woken.clone();
    let mut regs = Vec::new();
    let mut proc = |q: &WaitQueue, _| regs.push(q.add(entry.clone()));
    let mut table = PollTable::new(PollMask::empty(), &mut proc);
    assert_eq!(sink.poll_room(Some(&mut table)), Room::Full);
    assert!(!woken.0.load(Ordering::SeqCst));
    l.accept().unwrap();
    assert!(woken.0.load(Ordering::SeqCst), "accept makes room");
    assert_eq!(sink.poll_room(None), Room::Ready);
    // A second `listen` changes the backlog, on the host too.
    l.listen(5).unwrap();
    assert_eq!(backlog.load(Ordering::SeqCst), 5);
    drop(l);
    assert_eq!(sink.poll_room(None), Room::Gone);
}

#[test]
fn backend_datagrams_are_writable_only_while_the_host_takes_them() {
    let (n, rec) = backed();
    let s = udp(&n);
    let dst = a("10.0.2.2:53");
    assert_eq!(send_to(&*s, b"one", Some(dst)), Ok(3));
    let d = rec.dgrams.lock().unwrap()[0].clone();
    assert!(s.poll(None).contains(PollMask::OUT));
    d.writable.store(false, Ordering::SeqCst);
    assert_eq!(send_to(&*s, b"two", Some(dst)), Err(Errno::EAGAIN));
    assert!(!s.poll(None).intersects(PollMask::OUT | PollMask::WRNORM));
    // A blocked sender sleeps on the socket's queue, which the endpoint wakes.
    let woken = Arc::new(Flag(AtomicBool::new(false)));
    let entry: Arc<dyn WaitEntry> = woken.clone();
    let mut regs = Vec::new();
    let mut proc = |q: &WaitQueue, _| regs.push(q.add(entry.clone()));
    s.poll(Some(&mut PollTable::new(PollMask::OUT, &mut proc)));
    d.writable.store(true, Ordering::SeqCst);
    d.wq.wake(PollMask::OUT | PollMask::WRNORM);
    assert!(woken.0.load(Ordering::SeqCst));
    assert!(s.poll(None).contains(PollMask::OUT));
    assert_eq!(send_to(&*s, b"two", Some(dst)), Ok(3));
    assert_eq!(d.sent.lock().unwrap().len(), 2);
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
fn udp_send_fails_in_linux_order() {
    let n = net();
    let s = udp(&n);
    assert_eq!(s.shutdown(Shutdown::Write), Err(Errno::ENOTCONN));
    // The address is checked before the shutdown, and the socket bound before either.
    assert_eq!(send(&*s, b"x"), Err(Errno::EDESTADDRREQ));
    assert_ne!(s.local_addr().unwrap().port(), 0);
    assert_eq!(
        send_to(&*s, b"x", Some(a("127.0.0.1:0"))),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        send_to(&*s, b"x", Some(a("8.8.8.8:53"))),
        Err(Errno::ENETUNREACH)
    );
    assert_eq!(
        send_to(&*s, &[0; 65508], Some(a("127.0.0.1:9"))),
        Err(Errno::EMSGSIZE)
    );
    assert_eq!(
        send_to(&*s, b"x", Some(a("127.0.0.1:9"))),
        Err(Errno::EPIPE)
    );
}

fn send_ctl(
    s: &dyn Socket,
    data: &[u8],
    to: SocketAddr,
    control: &[Control],
) -> Result<usize, Errno> {
    s.send(SendMsg {
        data,
        to: Some(to),
        flags: MsgFlags(0),
        control,
        net_raw: false,
    })
}

#[test]
fn udp_control_messages_are_checked_and_applied() {
    let n = net();
    let r = udp(&n);
    r.bind(a("0.0.0.0:7500"), false).unwrap();
    let s = udp(&n);
    let to = a("127.0.0.1:7500");
    let ctl = |level: i32, kind: i32, data: Vec<u8>| Control { level, kind, data };
    let pktinfo = |index: u32, src: [u8; 4]| {
        let mut v = index.to_le_bytes().to_vec();
        v.extend(src);
        v.extend([0; 4]);
        ctl(0, IpOption::IP_PKTINFO.0, v)
    };
    // `IP_PKTINFO`'s address is the source the receiver sees.
    assert_eq!(
        send_ctl(&*s, b"a", to, &[pktinfo(0, [127, 0, 0, 2])]),
        Ok(1)
    );
    let mut buf = [0u8; 16];
    let m = r.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!(m.from.unwrap().ip(), a("127.0.0.2:0").ip());
    // Not the machine's address, no such interface, a bad hop limit, an unknown type.
    assert_eq!(
        send_ctl(&*s, b"a", to, &[pktinfo(0, [10, 9, 9, 9])]),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        send_ctl(&*s, b"a", to, &[pktinfo(99, [0; 4])]),
        Err(Errno::ENODEV)
    );
    let ttl = |v: i32| ctl(0, IpOption::IP_TTL.0, v.to_le_bytes().to_vec());
    assert_eq!(send_ctl(&*s, b"a", to, &[ttl(0)]), Err(Errno::EINVAL));
    assert_eq!(send_ctl(&*s, b"a", to, &[ttl(64)]), Ok(1));
    assert_eq!(
        send_ctl(&*s, b"a", to, &[ctl(0, 1234, Vec::new())]),
        Err(Errno::EINVAL)
    );
    assert_eq!(r.recv(&mut buf, MsgFlags(0)).unwrap().len, 1);
    // `UDP_SEGMENT` splits the payload into datagrams of that size.
    let seg = ctl(17, 103, 4u16.to_le_bytes().to_vec());
    assert_eq!(send_ctl(&*s, b"0123456789", to, &[seg]), Ok(10));
    let lens: Vec<usize> = std::iter::from_fn(|| r.recv(&mut buf, MsgFlags(0)).ok())
        .map(|m| m.len)
        .collect();
    assert_eq!(lens, [4, 4, 2]);
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
