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
        net_raw: false,
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

/// A connected pair on loopback port `port`: (client, accepted).
fn tcp_pair(n: &Net, port: u16) -> (Arc<dyn Socket>, Arc<dyn Socket>) {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let l = tcp(n);
    l.bind(addr, false).unwrap();
    l.listen(1).unwrap();
    let c = tcp(n);
    c.connect(addr).unwrap();
    let s = l.accept().unwrap().socket;
    (c, s)
}

fn send_oob(s: &dyn Socket, data: &[u8]) -> Result<usize, Errno> {
    s.send(SendMsg {
        data,
        to: None,
        flags: MsgFlags::MSG_OOB,
        control: &[],
        net_raw: false,
    })
}

fn recv_flags(s: &dyn Socket, n: usize, flags: MsgFlags) -> Result<(Vec<u8>, MsgFlags), Errno> {
    let mut buf = vec![0; n];
    let r = s.recv(&mut buf, flags)?;
    buf.truncate(r.len);
    Ok((buf, r.flags))
}

/// Records the events of every wakeup of a socket's queue.
struct Wakeups(std::sync::Mutex<Vec<PollMask>>);

impl wasixv2_wait::WaitEntry for Wakeups {
    fn wake(&self, events: PollMask) -> bool {
        self.0.lock().unwrap().push(events);
        false
    }
}

/// `MSG_OOB` makes the last byte sent urgent: the reader's stream stops in front of it and
/// steps over it, `recv(MSG_OOB)` reads it once, `SIOCATMARK` says when the stream is there,
/// and the reader polls `PRI` until it is read (`tcp_recvmsg`, `tcp_recv_urg`, `tcp_poll`).
#[test]
fn tcp_urgent_byte_leaves_the_stream() {
    let n = net();
    let (c, s) = tcp_pair(&n, 9100);
    let seen = Arc::new(Wakeups(Default::default()));
    let mut queue = None;
    s.poll(Some(&mut wasixv2_wait::PollTable::new(
        PollMask::empty(),
        &mut |q: &wasixv2_wait::WaitQueue, _| queue = Some(q.add(seen.clone())),
    )));
    assert_eq!(recv_flags(&*s, 1, MsgFlags::MSG_OOB), Err(Errno::EINVAL));
    assert_eq!(send(&*c, b"ab"), Ok(2));
    assert_eq!(send_oob(&*c, b"cU"), Ok(2));
    assert_eq!(send(&*c, b"de"), Ok(2));
    // The wakeup of the urgent send says a new mark arrived; the others do not.
    let events = seen.0.lock().unwrap().clone();
    assert_eq!(
        events.iter().filter(|e| e.contains(PollMask::PRI)).count(),
        1
    );
    assert!(s.poll(None).contains(PollMask::PRI | PollMask::IN));
    assert_eq!(s.input_len(), Ok(3));
    assert_eq!(s.at_mark(), Ok(false));
    assert_eq!(recv(&*s, 16).unwrap(), b"abc");
    assert_eq!(s.at_mark(), Ok(true));
    assert_eq!(
        recv_flags(&*s, 4, MsgFlags::MSG_OOB | MsgFlags::MSG_PEEK),
        Ok((b"U".to_vec(), MsgFlags::MSG_OOB))
    );
    assert_eq!(
        recv_flags(&*s, 0, MsgFlags::MSG_OOB),
        Ok((Vec::new(), MsgFlags::MSG_OOB | MsgFlags::MSG_TRUNC))
    );
    assert_eq!(recv_flags(&*s, 1, MsgFlags::MSG_OOB), Err(Errno::EINVAL));
    assert!(!s.poll(None).contains(PollMask::PRI));
    assert_eq!(recv(&*s, 16).unwrap(), b"de");
    assert_eq!(s.at_mark(), Ok(false));
    // The sender had none.
    assert_eq!(recv_flags(&*c, 1, MsgFlags::MSG_OOB), Err(Errno::EINVAL));
    drop(queue);
}

/// With `SO_OOBINLINE` the urgent byte stays in the stream and `MSG_OOB` finds nothing.
#[test]
fn tcp_urgent_byte_inline() {
    let n = net();
    let (c, s) = tcp_pair(&n, 9101);
    let on = 1i32.to_le_bytes();
    s.set_option(
        wasixv2_abi::SocketLevel::SOL_SOCKET.0,
        wasixv2_abi::SocketOption::SO_OOBINLINE.0,
        &on,
    )
    .unwrap();
    send(&*c, b"a").unwrap();
    send_oob(&*c, b"U").unwrap();
    send(&*c, b"b").unwrap();
    assert_eq!(s.input_len(), Ok(3));
    assert_eq!(recv_flags(&*s, 1, MsgFlags::MSG_OOB), Err(Errno::EINVAL));
    assert_eq!(recv(&*s, 16).unwrap(), b"a");
    assert_eq!(s.at_mark(), Ok(true));
    assert_eq!(recv(&*s, 16).unwrap(), b"Ub");
}

/// A listener's `recv(MSG_OOB)` is `ENOTCONN`; a socket reset before it read its urgent byte
/// gets `ENOTCONN` too, one that saw the FIN gets the byte.
#[test]
fn tcp_urgent_byte_errors() {
    let n = net();
    let l = tcp(&n);
    l.bind(a("127.0.0.1:9102"), false).unwrap();
    l.listen(1).unwrap();
    assert_eq!(recv_flags(&*l, 1, MsgFlags::MSG_OOB), Err(Errno::ENOTCONN));
    drop(l);

    let (c, s) = tcp_pair(&n, 9103);
    send_oob(&*s, b"U").unwrap();
    send(&*c, b"unread").unwrap();
    drop(s);
    assert_eq!(recv_flags(&*c, 1, MsgFlags::MSG_OOB), Err(Errno::ENOTCONN));

    let (c, s) = tcp_pair(&n, 9104);
    send_oob(&*s, b"U").unwrap();
    drop(s);
    assert_eq!(
        recv_flags(&*c, 1, MsgFlags::MSG_OOB),
        Ok((b"U".to_vec(), MsgFlags::MSG_OOB))
    );
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

use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

use wasixv2_abi::{IpOption, Ipv6Option, SoEeOrigin, SocketLevel, SocketOption, TcpOption};
use wasixv2_wait::{PollTable, Pollable, WaitEntry, WaitQueue};

use crate::{
    Backend, BackendDatagram, BackendStream, Control, InboundSink, Publication, QueuedError, Room,
    StreamState,
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
    fn write_urgent(&self, data: &[u8]) -> Result<usize, Errno> {
        Ok(data.len())
    }
    fn read_urgent(&self, _: bool) -> Result<u8, Errno> {
        Err(Errno::EINVAL)
    }
    fn at_mark(&self) -> bool {
        false
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

/// A datagram endpoint whose host side is writable or not as the test says, and whose
/// receive and error queues hold what the test puts there. A published one records whom it
/// is attached to.
struct FakeDgram {
    writable: AtomicBool,
    wq: WaitQueue,
    sent: Mutex<Vec<(Vec<u8>, SocketAddr)>>,
    options: Mutex<Vec<(i32, i32, Vec<u8>)>>,
    errors: Mutex<std::collections::VecDeque<QueuedError>>,
    inbound: Mutex<std::collections::VecDeque<(Vec<u8>, SocketAddr)>>,
    /// `Some` for a published endpoint: the queue it wakes, if attached.
    attached: Option<Mutex<Option<WaitQueue>>>,
}

impl FakeDgram {
    fn new(wq: WaitQueue, published: bool) -> Arc<Self> {
        Arc::new(Self {
            writable: AtomicBool::new(true),
            wq,
            sent: Mutex::default(),
            options: Mutex::default(),
            errors: Mutex::default(),
            inbound: Mutex::default(),
            attached: published.then(Mutex::default),
        })
    }

    fn is_attached(&self) -> bool {
        self.attached.as_ref().unwrap().lock().unwrap().is_some()
    }

    /// A datagram from `from` arrives from the host.
    fn arrive(&self, data: &[u8], from: SocketAddr) {
        let wq = match &self.attached {
            Some(a) => a.lock().unwrap().clone(),
            None => Some(self.wq.clone()),
        };
        // Detached, it is dropped.
        if let Some(wq) = wq {
            self.inbound
                .lock()
                .unwrap()
                .push_back((data.to_vec(), from));
            wq.wake(PollMask::IN);
        }
    }
}

impl Pollable for FakeDgram {
    fn poll(&self, _: Option<&mut PollTable<'_>>) -> PollMask {
        let mut m = match self.writable.load(Ordering::SeqCst) {
            true => PollMask::OUT | PollMask::WRNORM,
            false => PollMask::empty(),
        };
        if !self.inbound.lock().unwrap().is_empty() {
            m |= PollMask::IN;
        }
        m
    }
}

impl BackendDatagram for FakeDgram {
    fn send_to(&self, data: &[u8], to: SocketAddr) -> Result<usize, Errno> {
        if !self.writable.load(Ordering::SeqCst) {
            return Err(Errno::EAGAIN);
        }
        self.sent.lock().unwrap().push((data.to_vec(), to));
        Ok(data.len())
    }
    fn connect(&self, _: Option<SocketAddr>) -> Result<(), Errno> {
        Ok(())
    }
    fn take_error(&self) -> Option<Errno> {
        None
    }
    fn recv_from(&self, buf: &mut [u8], _: bool) -> Result<(usize, usize, SocketAddr), Errno> {
        let (data, from) = self
            .inbound
            .lock()
            .unwrap()
            .pop_front()
            .ok_or(Errno::EAGAIN)?;
        let n = buf.len().min(data.len());
        buf[..n].copy_from_slice(&data[..n]);
        Ok((n, data.len(), from))
    }
    fn input_len(&self) -> usize {
        self.inbound
            .lock()
            .unwrap()
            .front()
            .map_or(0, |d| d.0.len())
    }
    fn set_option(&self, level: i32, name: i32, value: &[u8]) -> Result<(), Errno> {
        self.options
            .lock()
            .unwrap()
            .push((level, name, value.to_vec()));
        Ok(())
    }
    fn take_queued_error(&self) -> Option<QueuedError> {
        self.errors.lock().unwrap().pop_front()
    }
    fn attach(&self, wq: Option<WaitQueue>) -> Result<(), Errno> {
        let attached = self.attached.as_ref().ok_or(Errno::EOPNOTSUPP)?;
        *attached.lock().unwrap() = wq;
        self.inbound.lock().unwrap().clear();
        self.options.lock().unwrap().clear();
        Ok(())
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
    /// Each UDP publication's host address and endpoint.
    published_dgrams: Mutex<Vec<(SocketAddr, Arc<FakeDgram>)>>,
}

impl Backend for Recorder {
    fn connect(&self, peer: SocketAddr, _: WaitQueue) -> Result<Arc<dyn BackendStream>, Errno> {
        let s = FakeStream::new(peer);
        self.streams.lock().unwrap().push(s.clone());
        Ok(s)
    }
    fn datagram(&self, _: Domain, wq: WaitQueue) -> Result<Arc<dyn BackendDatagram>, Errno> {
        let d = FakeDgram::new(wq, false);
        self.dgrams.lock().unwrap().push(d.clone());
        Ok(d)
    }
    fn publish_datagram(
        &self,
        host: SocketAddr,
    ) -> Result<crate::PublishedDatagram, std::io::Error> {
        let d = FakeDgram::new(WaitQueue::new(), true);
        self.published_dgrams
            .lock()
            .unwrap()
            .push((host, d.clone()));
        Ok((d, Box::new(FakePublication(Arc::default()))))
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
    backed_with(NetConfig {
        auto_publish: Some(a("0.0.0.0:0").ip()),
        ..NetConfig::host_default()
    })
}

fn backed_with(config: NetConfig) -> (Arc<Net>, Arc<Recorder>) {
    let rec = Arc::new(Recorder::default());
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

const SOL_IP: i32 = SocketLevel::SOL_IP.0;
const SOL_IPV6: i32 = SocketLevel::SOL_IPV6.0;

/// `recv(MSG_ERRQUEUE)` into `n` bytes: the payload, and the message.
fn recv_error(s: &dyn Socket, n: usize) -> Result<(Vec<u8>, crate::RecvMsg), Errno> {
    let mut buf = vec![0; n];
    let m = s.recv(&mut buf, MsgFlags::MSG_ERRQUEUE)?;
    buf.truncate(m.len);
    Ok((buf, m))
}

/// The `sock_extended_err` fields of an `IP_RECVERR`/`IPV6_RECVERR` payload: errno, origin,
/// type, code; and the offender's bytes after it.
fn extended_err(c: &Control) -> ((u32, u8, u8, u8), &[u8]) {
    let errno = u32::from_le_bytes(c.data[..4].try_into().unwrap());
    ((errno, c.data[4], c.data[5], c.data[6]), &c.data[16..])
}

#[test]
fn port_unreachables_queue_with_ip_recverr_and_keep_so_error_in_step() {
    let n = net();
    let s = udp(&n);
    // Without IP_RECVERR an unconnected socket never hears of a refused datagram.
    send_to(&*s, b"lost", Some(a("127.0.0.1:9"))).unwrap();
    assert!(!s.poll(None).contains(PollMask::ERR));
    assert_eq!(s.take_error(), None);
    assert_eq!(recv_error(&*s, 64).unwrap_err(), Errno::EAGAIN);

    s.set_option(SOL_IP, IpOption::IP_RECVERR.0, &int(1))
        .unwrap();
    send_to(&*s, b"first", Some(a("127.0.0.1:9"))).unwrap();
    assert!(s.poll(None).contains(PollMask::ERR));
    // Unconnected too, the next send reports the pending error instead of sending.
    assert_eq!(
        send_to(&*s, b"second", Some(a("127.0.0.1:10"))),
        Err(Errno::ECONNREFUSED)
    );
    send_to(&*s, b"second", Some(a("127.0.0.1:10"))).unwrap();
    // The pending error is taken by SO_ERROR; the queue stays and still polls ERR.
    assert_eq!(s.take_error(), Some(Errno::ECONNREFUSED));
    assert_eq!(s.take_error(), None);
    assert!(s.poll(None).contains(PollMask::ERR));

    let (data, m) = recv_error(&*s, 3).unwrap();
    assert_eq!(data, b"fir");
    assert_eq!(m.flags, MsgFlags::MSG_ERRQUEUE | MsgFlags::MSG_TRUNC);
    assert_eq!((m.len, m.full_len), (3, 3));
    assert_eq!(m.from, Some(a("127.0.0.1:9")));
    let [c] = &m.control[..] else {
        panic!("one control message: {:?}", m.control)
    };
    assert_eq!(
        (c.level, c.kind, c.data.len()),
        (SOL_IP, IpOption::IP_RECVERR.0, 32)
    );
    let (ee, offender) = extended_err(c);
    assert_eq!(ee, (Errno::ECONNREFUSED.0 as u32, 2, 3, 3));
    // `SO_EE_OFFENDER`: a sockaddr_in for 127.0.0.1.
    assert_eq!(&offender[..8], &[2, 0, 0, 0, 127, 0, 0, 1]);
    // `sock_dequeue_err_skb`: the next entry is an ICMP one, so its errno is pending again.
    assert_eq!(s.take_error(), Some(Errno::ECONNREFUSED));

    let (data, m) = recv_error(&*s, 64).unwrap();
    assert_eq!(data, b"second");
    assert_eq!(m.from, Some(a("127.0.0.1:10")));
    assert_eq!(m.flags, MsgFlags::MSG_ERRQUEUE);
    assert_eq!(s.take_error(), None);
    assert_eq!(recv_error(&*s, 64).unwrap_err(), Errno::EAGAIN);
    assert!(!s.poll(None).contains(PollMask::ERR));

    // Turning IP_RECVERR off purges the queue but leaves the pending error.
    send_to(&*s, b"third", Some(a("127.0.0.1:9"))).unwrap();
    s.set_option(SOL_IP, IpOption::IP_RECVERR.0, &int(0))
        .unwrap();
    assert_eq!(recv_error(&*s, 64).unwrap_err(), Errno::EAGAIN);
    assert_eq!(s.take_error(), Some(Errno::ECONNREFUSED));
}

#[test]
fn ipv6_sockets_report_errors_with_ipv6_recverr_and_mapped_addresses() {
    let n = net();
    let s = n
        .socket(Domain::Inet6, SocketKind::Datagram, 0, UID)
        .unwrap();
    s.set_option(SOL_IPV6, Ipv6Option::IPV6_RECVERR.0, &int(1))
        .unwrap();
    send_to(&*s, b"six", Some(a("[::1]:9"))).unwrap();
    assert_eq!(s.take_error(), Some(Errno::ECONNREFUSED));
    // An IPv4 destination answers through the IPv4 error path, which IP_RECVERR controls.
    send_to(&*s, b"four", Some(a("[::ffff:127.0.0.1]:9"))).unwrap();
    let (data, m) = recv_error(&*s, 64).unwrap();
    assert_eq!(data, b"six");
    assert_eq!(m.from, Some(a("[::1]:9")));
    let c = &m.control[0];
    assert_eq!(
        (c.level, c.kind, c.data.len()),
        (SOL_IPV6, Ipv6Option::IPV6_RECVERR.0, 16 + 28)
    );
    assert_eq!(extended_err(c).0, (Errno::ECONNREFUSED.0 as u32, 3, 1, 4));
    assert_eq!(recv_error(&*s, 64).unwrap_err(), Errno::EAGAIN);

    s.set_option(SOL_IP, IpOption::IP_RECVERR.0, &int(1))
        .unwrap();
    send_to(&*s, b"four", Some(a("[::ffff:127.0.0.1]:9"))).unwrap();
    let (_, m) = recv_error(&*s, 64).unwrap();
    assert_eq!(m.from, Some(a("[::ffff:127.0.0.1]:9")));
    // Still the socket family's message, with the offender mapped.
    let c = &m.control[0];
    assert_eq!((c.level, c.kind), (SOL_IPV6, Ipv6Option::IPV6_RECVERR.0));
    let (ee, offender) = extended_err(c);
    assert_eq!((ee.1, ee.2, ee.3), (2, 3, 3));
    assert_eq!(&offender[..2], &10u16.to_le_bytes());
    let mapped = std::net::Ipv4Addr::LOCALHOST.to_ipv6_mapped().octets();
    assert_eq!(&offender[8..24], &mapped);
}

#[test]
fn backend_errors_queue_behind_local_ones_in_the_guest_view() {
    let (n, rec) = backed();
    let s = udp(&n);
    // Set before the endpoint exists: it learns it when it is made.
    s.set_option(SOL_IP, IpOption::IP_RECVERR.0, &int(1))
        .unwrap();
    send_to(&*s, b"out", Some(a("10.0.2.2:53"))).unwrap();
    let d = rec.dgrams.lock().unwrap()[0].clone();
    assert_eq!(
        d.options.lock().unwrap()[..],
        [(SOL_IP, IpOption::IP_RECVERR.0, int(1).to_vec())]
    );
    // The host's report: its loopback is the guest's gateway alias.
    d.errors.lock().unwrap().push_back(QueuedError {
        errno: Errno::ECONNREFUSED,
        origin: SoEeOrigin::SO_EE_ORIGIN_ICMP,
        type_: 3,
        code: 3,
        info: 0,
        data: 0,
        offender: Some(a("127.0.0.1:0")),
        dst: Some(a("127.0.0.1:53")),
        payload: b"out".to_vec(),
    });
    send_to(&*s, b"in", Some(a("127.0.0.1:9"))).unwrap();
    let (data, _) = recv_error(&*s, 64).unwrap();
    assert_eq!(data, b"in");
    let (data, m) = recv_error(&*s, 64).unwrap();
    assert_eq!(data, b"out");
    assert_eq!(m.from, Some(a("10.0.2.2:53")));
    assert_eq!(&extended_err(&m.control[0]).1[4..8], &[10, 0, 2, 2]);
    assert_eq!(recv_error(&*s, 64).unwrap_err(), Errno::EAGAIN);
    // Changes reach the endpoint too.
    s.set_option(SOL_IP, IpOption::IP_RECVERR.0, &int(0))
        .unwrap();
    assert_eq!(d.options.lock().unwrap().len(), 2);
}

#[test]
fn a_published_udp_port_serves_the_first_socket_bound_where_it_reaches() {
    let (n, rec) = backed_with(NetConfig::host_default());
    let p = n
        .publish_udp(a("127.0.0.1:5353"), a("0.0.0.0:5300"))
        .unwrap();
    let (host, d) = rec.published_dgrams.lock().unwrap()[0].clone();
    assert_eq!(host, a("127.0.0.1:5353"));
    assert!(!d.is_attached());
    // A wildcard publication arrives on the uplink address, which loopback is not.
    let lo = udp(&n);
    lo.set_option(SOL_SOCKET, SocketOption::SO_REUSEADDR.0, &int(1))
        .unwrap();
    lo.bind(a("127.0.0.1:5300"), false).unwrap();
    assert!(!d.is_attached());

    let s = udp(&n);
    s.set_option(SOL_SOCKET, SocketOption::SO_REUSEADDR.0, &int(1))
        .unwrap();
    s.set_option(SOL_IP, IpOption::IP_RECVERR.0, &int(1))
        .unwrap();
    s.bind(a("0.0.0.0:5300"), false).unwrap();
    assert!(d.is_attached());
    // The endpoint learns the options set before it served the socket.
    assert_eq!(
        d.options.lock().unwrap()[..],
        [(SOL_IP, IpOption::IP_RECVERR.0, int(1).to_vec())]
    );
    // A query from the host's loopback, which the guest sees as the gateway alias.
    d.arrive(b"query", a("127.0.0.1:40000"));
    assert!(s.poll(None).contains(PollMask::IN));
    let mut buf = [0u8; 16];
    let m = s.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!(&buf[..m.len], b"query");
    assert_eq!(m.from, Some(a("10.0.2.2:40000")));
    // The reply leaves from the published endpoint, not a fresh one.
    assert_eq!(send_to(&*s, b"answer", m.from), Ok(6));
    assert_eq!(
        d.sent.lock().unwrap()[..],
        [(b"answer".to_vec(), a("127.0.0.1:40000"))]
    );
    assert!(rec.dgrams.lock().unwrap().is_empty());

    // A second socket on the port waits until the first goes.
    let next = udp(&n);
    next.set_option(SOL_SOCKET, SocketOption::SO_REUSEADDR.0, &int(1))
        .unwrap();
    next.bind(a("10.0.2.15:5300"), false).unwrap();
    d.arrive(b"one", a("127.0.0.1:40000"));
    assert!(!next.poll(None).contains(PollMask::IN));
    drop(s);
    assert!(d.is_attached());
    assert!(
        !next.poll(None).contains(PollMask::IN),
        "dropped on the switch"
    );
    d.arrive(b"two", a("127.0.0.1:40000"));
    assert_eq!(recv(&*next, 16).unwrap(), b"two");

    // Ending the publication leaves the socket with an endpoint of its own.
    drop(p);
    assert!(!d.is_attached());
    assert_eq!(send_to(&*next, b"x", Some(a("10.0.2.2:53"))), Ok(1));
    assert_eq!(rec.dgrams.lock().unwrap().len(), 1);
    assert!(n.state().udp_pubs.is_empty());
}

#[test]
fn a_udp_publication_follows_binds_and_connections() {
    let (n, rec) = backed_with(NetConfig::host_default());
    // Bound before the publication: it takes it when it is made.
    let s = udp(&n);
    s.bind(a("10.0.2.15:5400"), false).unwrap();
    let _p = n
        .publish_udp(a("127.0.0.1:5454"), a("10.0.2.15:5400"))
        .unwrap();
    let d = rec.published_dgrams.lock().unwrap()[0].1.clone();
    assert!(d.is_attached());
    // Connected inside the namespace, nothing from outside is for the socket.
    let peer = udp(&n);
    peer.bind(a("10.0.2.15:5401"), false).unwrap();
    s.connect(a("10.0.2.15:5401")).unwrap();
    assert!(!d.is_attached());
    s.disconnect().unwrap();
    assert!(d.is_attached());
    d.arrive(b"back", a("127.0.0.1:40000"));
    assert_eq!(recv(&*s, 16).unwrap(), b"back");
    // Connected outside, the published endpoint carries the association.
    s.connect(a("10.0.2.2:53")).unwrap();
    assert!(d.is_attached());
    assert_eq!(send(&*s, b"q"), Ok(1));
    assert_eq!(d.sent.lock().unwrap().len(), 1);
    assert!(rec.dgrams.lock().unwrap().is_empty());
}

#[test]
fn explicitly_bound_udp_sockets_are_auto_published() {
    let (n, rec) = backed();
    let s = udp(&n);
    s.bind(a("0.0.0.0:6000"), false).unwrap();
    let (host, d) = rec.published_dgrams.lock().unwrap()[0].clone();
    assert_eq!(host, a("0.0.0.0:6000"));
    assert!(d.is_attached());
    // Loopback stays private, and an implicit bind publishes nothing.
    udp(&n).bind(a("127.0.0.1:6001"), false).unwrap();
    send_to(&*udp(&n), b"x", Some(a("10.0.2.2:53"))).unwrap();
    assert_eq!(rec.published_dgrams.lock().unwrap().len(), 1);
    drop(s);
    assert!(n.state().udp_pubs.is_empty());
}

/// Two namespaces joined by a veth pair, as `ip link add veth0 type veth peer name veth1`,
/// `ip link set veth1 netns ...`, addresses on both ends and both up make them.
fn veth_pair(v4: (&str, &str), v6: (&str, &str)) -> (Arc<Net>, Arc<Net>) {
    use crate::{
        NewLink, NewLinkKind,
        iface::{AddMode, IfAddr},
        move_link,
    };
    let (na, nb) = (net(), net());
    let (veth0, veth1) = na
        .new_link(NewLink {
            name: Some("veth0".into()),
            mtu: None,
            mac: None,
            kind: NewLinkKind::Veth {
                peer_name: Some("veth1".into()),
                peer_mtu: None,
                peer_mac: None,
            },
        })
        .unwrap();
    let veth1 = move_link(&*na, veth1.unwrap(), &*nb).unwrap();
    for (n, index, (ip4, ip6)) in [(&na, veth0, (v4.0, v6.0)), (&nb, veth1, (v4.1, v6.1))] {
        for (ip, len) in [(ip4, 24), (ip6, 64)] {
            n.add_address(index, IfAddr::new(ip.parse().unwrap(), len), AddMode::ADD)
                .unwrap();
        }
        n.change_flags(index, crate::iface::flags::IFF_UP).unwrap();
    }
    (na, nb)
}

/// TCP and UDP cross a veth pair into the namespace at its other end, which sees the
/// connecting end's address as the peer's; the way back takes the route back.
#[test]
fn veth_carries_streams_and_datagrams_between_namespaces() {
    let (na, nb) = veth_pair(("10.23.0.1", "10.23.0.2"), ("fd00:23::1", "fd00:23::2"));
    let l = tcp(&nb);
    l.bind(a("10.23.0.2:7000"), false).unwrap();
    l.listen(4).unwrap();
    let c = tcp(&na);
    c.connect(a("10.23.0.2:7000")).unwrap();
    assert_eq!(c.local_addr().unwrap().ip(), a("10.23.0.1:0").ip());
    let acc = l.accept().unwrap();
    assert_eq!(acc.peer, c.local_addr().unwrap());
    send(&*c, b"ping").unwrap();
    assert_eq!(recv(&*acc.socket, 8).unwrap(), b"ping");

    let u = nb
        .socket(Domain::Inet6, SocketKind::Datagram, 0, UID)
        .unwrap();
    u.bind(a("[fd00:23::2]:5000"), false).unwrap();
    let v = na
        .socket(Domain::Inet6, SocketKind::Datagram, 0, UID)
        .unwrap();
    send_to(&*v, b"hi", Some(a("[fd00:23::2]:5000"))).unwrap();
    let mut buf = [0; 8];
    let got = u.recv(&mut buf, MsgFlags(0)).unwrap();
    let from = got.from.unwrap();
    assert_eq!(from.ip(), a("[fd00:23::1]:0").ip());
    send_to(&*u, b"back", Some(from)).unwrap();
    assert_eq!(recv(&*v, 8).unwrap(), b"back");

    // The namespaces stay apart: neither has the other's addresses.
    let x = tcp(&na);
    assert_eq!(x.bind(a("10.23.0.2:0"), false), Err(Errno::EADDRNOTAVAIL));
    // An address on the link that nobody has: nothing answers.
    assert_eq!(
        tcp(&na).connect(a("10.23.0.9:7000")),
        Err(Errno::EHOSTUNREACH)
    );
    send_to(&*udp(&na), b"lost", Some(a("10.23.0.9:9"))).unwrap();
}

/// Carrier needs both ends up; deleting one end deletes the other; a namespace that goes takes
/// its ends' peers with it.
#[test]
fn veth_carrier_deletion_and_namespace_exit() {
    use crate::iface::flags::{IFF_LOWER_UP, IFF_UP};
    let (na, nb) = veth_pair(("10.23.0.1", "10.23.0.2"), ("fd00:23::1", "fd00:23::2"));
    let veth = |n: &Net, name: &str| n.interfaces().into_iter().find(|i| i.name == name);
    assert_ne!(veth(&na, "veth0").unwrap().flags & IFF_LOWER_UP, 0);
    let peer = veth(&na, "veth0").unwrap().peer.unwrap();
    assert_eq!(peer, (veth(&nb, "veth1").unwrap().index, Some(0)));
    nb.change_flags(veth(&nb, "veth1").unwrap().index, 0)
        .unwrap();
    let veth0 = veth(&na, "veth0").unwrap();
    assert_eq!(veth0.flags & (IFF_UP | IFF_LOWER_UP), IFF_UP);
    assert_eq!(
        tcp(&na).connect(a("10.23.0.2:7000")),
        Err(Errno::EHOSTUNREACH)
    );

    assert_eq!(nb.del_link(1), Err(Errno::EOPNOTSUPP));
    na.del_link(veth0.index).unwrap();
    assert!(veth(&nb, "veth1").is_none());

    let (na, nb) = veth_pair(("10.23.0.1", "10.23.0.2"), ("fd00:23::1", "fd00:23::2"));
    drop(nb);
    assert!(veth(&na, "veth0").is_none());
    assert_eq!(na.interfaces().len(), 1);
}

/// `move_link`: the interface arrives down without addresses, keeping its index; names in
/// use and interfaces that stay home refuse.
#[test]
fn moving_an_interface_between_namespaces() {
    use crate::{NewLink, NewLinkKind, iface::flags::IFF_UP, move_link};
    let (na, nb) = (net(), net());
    let dummy = |n: &Net| {
        n.new_link(NewLink {
            name: Some("dummy0".into()),
            mtu: None,
            mac: None,
            kind: NewLinkKind::Dummy,
        })
    };
    let (d, _) = dummy(&na).unwrap();
    na.change_flags(d, IFF_UP).unwrap();
    assert_eq!(move_link(&*na, 1, &*nb), Err(Errno::EINVAL));
    dummy(&nb).unwrap();
    assert_eq!(move_link(&*na, d, &*nb), Err(Errno::EEXIST));
    let c = net();
    assert_eq!(move_link(&*na, d, &*c), Ok(d));
    let moved = c.interfaces().into_iter().find(|i| i.index == d).unwrap();
    assert_eq!((moved.name.as_str(), moved.flags & IFF_UP), ("dummy0", 0));
    assert!(moved.addresses.is_empty());
    assert_eq!(na.interfaces().len(), 1);
    // The move gave `c` an id in `na` (`peernet2id_alloc`).
    assert_eq!(na.peer_id(&*c), Some(0));
    assert_eq!(c.peer_id(&*na), None);
}

/// `IP_BIND_ADDRESS_NO_PORT`: `bind` with port 0 keeps the address and takes no port; the
/// port comes with the first `connect`, `listen` or send, from that address.
#[test]
fn bind_address_no_port_defers_the_port() {
    let n = net();
    let no_port = |s: &dyn Socket| {
        s.set_option(SOL_IP, IpOption::IP_BIND_ADDRESS_NO_PORT.0, &int(1))
            .unwrap();
        s.bind(a("127.0.0.2:0"), false).unwrap();
        assert_eq!(s.local_addr().unwrap(), a("127.0.0.2:0"));
    };
    let (l1, l2) = (tcp(&n), tcp(&n));
    l1.bind(a("127.0.0.1:7001"), false).unwrap();
    l2.bind(a("127.0.0.1:7002"), false).unwrap();
    l1.listen(4).unwrap();
    l2.listen(4).unwrap();
    let (c1, c2) = (tcp(&n), tcp(&n));
    no_port(&*c1);
    no_port(&*c2);
    // A bind without a port leaves the socket free to bind again (`inet_num` is 0).
    c2.bind(a("127.0.0.2:0"), false).unwrap();
    c1.connect(a("127.0.0.1:7001")).unwrap();
    c2.connect(a("127.0.0.1:7002")).unwrap();
    let (s1, s2) = (c1.local_addr().unwrap(), c2.local_addr().unwrap());
    assert_eq!(
        (s1.ip(), s2.ip()),
        (a("127.0.0.2:0").ip(), a("127.0.0.2:0").ip())
    );
    assert_ne!(s1.port(), 0);
    assert_eq!(l1.accept().unwrap().peer, s1);

    let l3 = tcp(&n);
    no_port(&*l3);
    l3.listen(4).unwrap();
    assert_eq!(l3.local_addr().unwrap().ip(), a("127.0.0.2:0").ip());
    assert_ne!(l3.local_addr().unwrap().port(), 0);

    // UDP: the first send binds from the address; a disconnect gives the port back and
    // keeps the address.
    let (r, u) = (udp(&n), udp(&n));
    r.bind(a("127.0.0.1:7003"), false).unwrap();
    no_port(&*u);
    send_to(&*u, b"x", Some(a("127.0.0.1:7003"))).unwrap();
    let mut buf = [0; 4];
    let from = r.recv(&mut buf, MsgFlags(0)).unwrap().from.unwrap();
    assert_eq!(from, u.local_addr().unwrap());
    assert_eq!(from.ip(), a("127.0.0.2:0").ip());
    u.connect(a("127.0.0.1:7003")).unwrap();
    u.disconnect().unwrap();
    assert_eq!(u.local_addr().unwrap(), a("127.0.0.2:0"));
}

fn index_of(n: &Net, name: &str) -> u32 {
    n.interfaces()
        .into_iter()
        .find(|i| i.name == name)
        .unwrap()
        .index
}

/// `ip_mreqn`: group, address, interface index.
fn mreqn(group: &str, address: &str, index: u32) -> Vec<u8> {
    let ip = |s: &str| s.parse::<std::net::Ipv4Addr>().unwrap().octets();
    [&ip(group)[..], &ip(address), &index.to_le_bytes()].concat()
}

/// `ipv6_mreq`: group, interface index.
fn mreq6(group: &str, index: u32) -> Vec<u8> {
    let g = group.parse::<std::net::Ipv6Addr>().unwrap().octets();
    [&g[..], &index.to_le_bytes()].concat()
}

fn getopt(s: &dyn Socket, level: i32, name: i32, len: usize) -> Result<Vec<u8>, Errno> {
    let mut out = vec![0; len];
    let n = s.get_option(level, name, &mut out)?;
    out.truncate(n);
    Ok(out)
}

/// The multicast options as `do_ip_setsockopt` and `do_ipv6_setsockopt` check them: a byte
/// is enough for IPv4's integers, TTL -1 is the default, an interface is found by address or
/// index, and stream sockets refuse what only datagrams use.
#[test]
fn multicast_options_are_checked_and_read_back() {
    let n = net();
    let s = udp(&n);
    assert_eq!(
        getopt(&*s, SOL_IP, IpOption::IP_MULTICAST_TTL.0, 4),
        Ok(int(1).to_vec())
    );
    s.set_option(SOL_IP, IpOption::IP_MULTICAST_TTL.0, &[7])
        .unwrap();
    assert_eq!(
        getopt(&*s, SOL_IP, IpOption::IP_MULTICAST_TTL.0, 1),
        Ok(vec![7])
    );
    s.set_option(SOL_IP, IpOption::IP_MULTICAST_TTL.0, &int(-1))
        .unwrap();
    assert_eq!(
        getopt(&*s, SOL_IP, IpOption::IP_MULTICAST_TTL.0, 4),
        Ok(int(1).to_vec())
    );
    assert_eq!(
        s.set_option(SOL_IP, IpOption::IP_MULTICAST_TTL.0, &int(256)),
        Err(Errno::EINVAL)
    );
    s.set_option(SOL_IP, IpOption::IP_MULTICAST_LOOP.0, &[0])
        .unwrap();
    assert_eq!(
        getopt(&*s, SOL_IP, IpOption::IP_MULTICAST_LOOP.0, 4),
        Ok(int(0).to_vec())
    );
    let lo = [127, 0, 0, 1];
    s.set_option(SOL_IP, IpOption::IP_MULTICAST_IF.0, &lo)
        .unwrap();
    assert_eq!(
        getopt(&*s, SOL_IP, IpOption::IP_MULTICAST_IF.0, 4),
        Ok(lo.to_vec())
    );
    assert_eq!(
        s.set_option(SOL_IP, IpOption::IP_MULTICAST_IF.0, &[10, 9, 9, 9]),
        Err(Errno::EADDRNOTAVAIL)
    );
    let t = tcp(&n);
    assert_eq!(
        t.set_option(SOL_IP, IpOption::IP_MULTICAST_TTL.0, &int(2)),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        t.set_option(
            SOL_IP,
            IpOption::IP_ADD_MEMBERSHIP.0,
            &mreqn("239.1.2.3", "0.0.0.0", 1)
        ),
        Err(Errno::EPROTO)
    );
    let s6 = n
        .socket(Domain::Inet6, SocketKind::Datagram, 0, UID)
        .unwrap();
    assert_eq!(
        s6.set_option(SOL_IPV6, Ipv6Option::IPV6_MULTICAST_LOOP.0, &int(2)),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        s6.set_option(SOL_IPV6, Ipv6Option::IPV6_MULTICAST_IF.0, &int(42)),
        Err(Errno::ENODEV)
    );
    s6.set_option(SOL_IPV6, Ipv6Option::IPV6_MULTICAST_HOPS.0, &int(5))
        .unwrap();
    assert_eq!(
        getopt(&*s6, SOL_IPV6, Ipv6Option::IPV6_MULTICAST_HOPS.0, 4),
        Ok(int(5).to_vec())
    );
    assert_eq!(
        s.set_option(SOL_IPV6, Ipv6Option::IPV6_MULTICAST_HOPS.0, &int(5)),
        Err(Errno::ENOPROTOOPT)
    );
}

/// Memberships (`ip_mc_join_group`, `ip_mc_leave_group`): a group only, once per interface,
/// counted on the interface while a socket holds them, and gone with the socket.
#[test]
fn memberships_join_the_interface_while_held() {
    let n = net();
    let lo_groups = |n: &Net| n.interfaces()[0].groups.clone();
    let group: std::net::IpAddr = "239.1.2.3".parse().unwrap();
    let add = IpOption::IP_ADD_MEMBERSHIP.0;
    let s = udp(&n);
    assert_eq!(
        s.set_option(SOL_IP, add, &mreqn("10.0.0.1", "127.0.0.1", 0)),
        Err(Errno::EINVAL)
    );
    assert_eq!(s.set_option(SOL_IP, add, &[0; 7]), Err(Errno::EINVAL));
    // An `ip_mreq`, by the interface's address.
    s.set_option(SOL_IP, add, &mreqn("239.1.2.3", "127.0.0.1", 0)[..8])
        .unwrap();
    assert_eq!(
        s.set_option(SOL_IP, add, &mreqn("239.1.2.3", "0.0.0.0", 1)),
        Err(Errno::EADDRINUSE)
    );
    let t = udp(&n);
    t.set_option(SOL_IP, add, &mreqn("239.1.2.3", "0.0.0.0", 1))
        .unwrap();
    assert_eq!(lo_groups(&n)[0], (group, 2));
    let drop = IpOption::IP_DROP_MEMBERSHIP.0;
    assert_eq!(
        t.set_option(SOL_IP, drop, &mreqn("239.9.9.9", "0.0.0.0", 1)),
        Err(Errno::EADDRNOTAVAIL)
    );
    t.set_option(SOL_IP, drop, &mreqn("239.1.2.3", "0.0.0.0", 1))
        .unwrap();
    assert_eq!(lo_groups(&n)[0], (group, 1));
    drop_socket(s);
    assert!(lo_groups(&n).iter().all(|(g, _)| *g != group));
    // The kernel's own: all-hosts, and IPv6's all-nodes groups.
    assert!(lo_groups(&n).contains(&("224.0.0.1".parse().unwrap(), 1)));
    assert!(lo_groups(&n).contains(&("ff02::1".parse().unwrap(), 1)));
}

fn drop_socket(s: Arc<dyn Socket>) {
    drop(s);
}

/// Inside a namespace, a datagram to a group reaches every socket bound to its port that
/// allows the group, by membership or `IP_MULTICAST_ALL`, if the interface it is sent out of
/// is in the group and `IP_MULTICAST_LOOP` loops it back.
#[test]
fn multicast_reaches_member_sockets_of_the_namespace() {
    let n = net();
    let bound = |port: u16| {
        let s = udp(&n);
        s.set_option(SOL_SOCKET, SocketOption::SO_REUSEADDR.0, &int(1))
            .unwrap();
        s.bind(SocketAddr::new("0.0.0.0".parse().unwrap(), port), false)
            .unwrap();
        s
    };
    let (member, other, no_all, elsewhere) = (bound(6000), bound(6000), bound(6000), bound(6001));
    member
        .set_option(
            SOL_IP,
            IpOption::IP_ADD_MEMBERSHIP.0,
            &mreqn("239.1.2.3", "127.0.0.1", 0),
        )
        .unwrap();
    no_all
        .set_option(SOL_IP, IpOption::IP_MULTICAST_ALL.0, &int(0))
        .unwrap();
    let sender = udp(&n);
    sender
        .set_option(SOL_IP, IpOption::IP_MULTICAST_IF.0, &[127, 0, 0, 1])
        .unwrap();
    send_to(&*sender, b"m", Some(a("239.1.2.3:6000"))).unwrap();
    let mut buf = [0; 4];
    let got = member.recv(&mut buf, MsgFlags(0)).unwrap();
    let from = got.from.unwrap();
    assert_eq!(from.port(), sender.local_addr().unwrap().port());
    assert_eq!(from.ip(), a("127.0.0.1:0").ip());
    assert_eq!(recv(&*other, 4).unwrap(), b"m");
    assert_eq!(recv(&*no_all, 4), Err(Errno::EAGAIN));
    assert_eq!(recv(&*elsewhere, 4), Err(Errno::EAGAIN));
    // A group the interface is not in reaches nobody.
    send_to(&*sender, b"x", Some(a("239.7.7.7:6000"))).unwrap();
    assert_eq!(recv(&*other, 4), Err(Errno::EAGAIN));
    // No interface to send from: the route to the group decides, and there is none.
    assert_eq!(
        send_to(&*udp(&n), b"x", Some(a("239.1.2.3:6000"))),
        Err(Errno::ENETUNREACH)
    );
}

/// Across a veth, a datagram to a group reaches the peer's namespace if its end is in the
/// group and the hop limit lets it leave, and loops back only to members of the sender's
/// own interface; IPv6 alike.
#[test]
fn multicast_crosses_a_veth_to_member_interfaces() {
    let (na, nb) = veth_pair(("10.24.0.1", "10.24.0.2"), ("fd00:24::1", "fd00:24::2"));
    let (veth0, veth1) = (index_of(&na, "veth0"), index_of(&nb, "veth1"));
    let recv_on = |n: &Net, domain, at: &str| {
        let s = n.socket(domain, SocketKind::Datagram, 0, UID).unwrap();
        s.bind(a(at), false).unwrap();
        s
    };
    let r = recv_on(&nb, Domain::Inet, "0.0.0.0:7000");
    let own = recv_on(&na, Domain::Inet, "0.0.0.0:7000");
    let sender = udp(&na);
    sender
        .set_option(SOL_IP, IpOption::IP_MULTICAST_IF.0, &[10, 24, 0, 1])
        .unwrap();
    // Nobody across is in the group yet.
    send_to(&*sender, b"0", Some(a("239.4.4.4:7000"))).unwrap();
    assert_eq!(recv(&*r, 4), Err(Errno::EAGAIN));
    r.set_option(
        SOL_IP,
        IpOption::IP_ADD_MEMBERSHIP.0,
        &mreqn("239.4.4.4", "0.0.0.0", veth1),
    )
    .unwrap();
    send_to(&*sender, b"1", Some(a("239.4.4.4:7000"))).unwrap();
    let mut buf = [0; 4];
    let got = r.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!(got.from.unwrap().ip(), a("10.24.0.1:0").ip());
    assert_eq!(recv(&*own, 4), Err(Errno::EAGAIN));
    // TTL 0 stays on the host; all-hosts loops back to the sender's namespace.
    sender
        .set_option(SOL_IP, IpOption::IP_MULTICAST_TTL.0, &int(0))
        .unwrap();
    send_to(&*sender, b"2", Some(a("239.4.4.4:7000"))).unwrap();
    assert_eq!(recv(&*r, 4), Err(Errno::EAGAIN));
    send_to(&*sender, b"3", Some(a("224.0.0.1:7000"))).unwrap();
    assert_eq!(recv(&*own, 4).unwrap(), b"3");

    let r6 = recv_on(&nb, Domain::Inet6, "[::]:7001");
    r6.set_option(
        SOL_IPV6,
        Ipv6Option::IPV6_ADD_MEMBERSHIP.0,
        &mreq6("ff02::1:3", veth1),
    )
    .unwrap();
    let s6 = na
        .socket(Domain::Inet6, SocketKind::Datagram, 0, UID)
        .unwrap();
    s6.set_option(
        SOL_IPV6,
        Ipv6Option::IPV6_MULTICAST_IF.0,
        &int(veth0 as i32),
    )
    .unwrap();
    send_to(&*s6, b"6", Some(a("[ff02::1:3]:7001"))).unwrap();
    let got = r6.recv(&mut buf, MsgFlags(0)).unwrap();
    assert!(
        matches!(got.from.unwrap().ip(), std::net::IpAddr::V6(v6) if v6.is_unicast_link_local())
    );
}
