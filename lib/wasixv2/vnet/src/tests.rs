//! Socket-level tests against a loopback-only namespace.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use wasixv2_abi::{Errno, MsgFlags};
use wasixv2_wait::PollMask;

use crate::{Domain, IcmpLimit, Net, NetConfig, Network, SendMsg, Shutdown, Socket, SocketKind};

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
        now: Duration::ZERO,
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
        now: Duration::ZERO,
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
    /// Each ICMP endpoint.
    pings: Mutex<Vec<Arc<FakeDgram>>>,
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
    fn ping(&self, _: Domain, wq: WaitQueue) -> Result<Arc<dyn BackendDatagram>, Errno> {
        let d = FakeDgram::new(wq, false);
        self.pings.lock().unwrap().push(d.clone());
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
        now: Duration::ZERO,
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
pub(crate) fn veth_pair(v4: (&str, &str), v6: (&str, &str)) -> (Arc<Net>, Arc<Net>) {
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

// ---- ICMP ----

fn ping_socket(n: &Net, domain: Domain) -> Arc<dyn Socket> {
    let proto = match domain {
        Domain::Inet => 1,
        Domain::Inet6 => 58,
    };
    n.socket(domain, SocketKind::Datagram, proto, UID).unwrap()
}

fn raw_icmp(n: &Net, domain: Domain) -> Arc<dyn Socket> {
    let proto = match domain {
        Domain::Inet => 1,
        Domain::Inet6 => 58,
    };
    n.socket(domain, SocketKind::Raw, proto, UID).unwrap()
}

/// An ICMP checksum (RFC 1071) over `data`.
fn csum(data: &[u8]) -> u16 {
    let mut sum: u32 = data
        .chunks(2)
        .map(|c| u32::from(u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)])))
        .sum();
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// An IPv4 echo request with identifier `id`, sequence `seq` and `payload`, checksummed.
fn echo4(id: u16, seq: u16, payload: &[u8]) -> Vec<u8> {
    let mut m = vec![8, 0, 0, 0];
    m.extend(id.to_be_bytes());
    m.extend(seq.to_be_bytes());
    m.extend_from_slice(payload);
    let c = csum(&m);
    m[2..4].copy_from_slice(&c.to_be_bytes());
    m
}

/// `socket(AF_INET, SOCK_DGRAM, IPPROTO_ICMP)` and its IPv6 kin: an echo request gets the
/// socket's identifier and comes back from the address it was sent to, as the kernel of the
/// namespace answers it; anything but an echo request is `EINVAL`.
#[test]
fn ping_sockets_get_echo_replies_inside_the_namespace() {
    let n = net();
    let p = ping_socket(&n, Domain::Inet);
    assert_eq!(
        send_to(&*p, &[0; 8], Some(a("127.0.0.1:0"))),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        send_to(&*p, &[8, 0, 0], Some(a("127.0.0.1:0"))),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        send_to(&*p, &echo4(0x1234, 1, b"hello"), None),
        Err(Errno::EDESTADDRREQ)
    );
    send_to(&*p, &echo4(0x1234, 7, b"hello"), Some(a("127.0.0.1:0"))).unwrap();
    let ident = p.local_addr().unwrap().port();
    assert_ne!(ident, 0);
    let mut buf = [0; 64];
    let r = p.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!(r.from.unwrap(), a("127.0.0.1:0"));
    let reply = &buf[..r.len];
    assert_eq!(reply[0], 0);
    assert_eq!(u16::from_be_bytes([reply[4], reply[5]]), ident);
    assert_eq!(u16::from_be_bytes([reply[6], reply[7]]), 7);
    assert_eq!(&reply[8..], b"hello");
    assert_eq!(csum(reply), 0);
    // The identifier is the socket's port: taken once, unless both share it.
    let q = ping_socket(&n, Domain::Inet);
    q.bind(a("0.0.0.0:4242"), false).unwrap();
    assert_eq!(q.local_addr().unwrap(), a("0.0.0.0:4242"));
    assert_eq!(
        ping_socket(&n, Domain::Inet).bind(a("0.0.0.0:4242"), false),
        Err(Errno::EADDRINUSE)
    );
    assert_eq!(q.bind(a("0.0.0.0:4243"), false), Err(Errno::EINVAL));

    let p6 = ping_socket(&n, Domain::Inet6);
    let mut req = vec![128, 0, 0, 0, 0, 1, 0, 2];
    req.extend(b"six");
    send_to(&*p6, &req, Some(a("[::1]:0"))).unwrap();
    let r = p6.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!(r.from.unwrap(), a("[::1]:0"));
    assert_eq!(buf[0], 129);
    assert_eq!(&buf[8..r.len], b"six");
    assert_eq!(
        send_to(&*p6, &req, Some(a("127.0.0.1:0"))),
        Err(Errno::EAFNOSUPPORT)
    );
}

/// A raw ICMP socket sees every ICMP message arriving in its namespace, behind an IPv4
/// header: its own echo request on loopback and the reply. `ICMP_FILTER` keeps types out,
/// and a request with a bad checksum gets no answer.
#[test]
fn raw_icmp_sockets_see_what_arrives() {
    let n = net();
    let r = raw_icmp(&n, Domain::Inet);
    send_to(&*r, &echo4(77, 1, b"x"), Some(a("127.0.0.1:0"))).unwrap();
    let mut buf = [0; 64];
    let types: Vec<u8> = (0..2)
        .map(|_| {
            let got = r.recv(&mut buf, MsgFlags(0)).unwrap();
            assert_eq!(got.len, 20 + 9);
            assert_eq!(buf[0], 0x45);
            assert_eq!(buf[9], 1);
            buf[20]
        })
        .collect();
    assert_eq!(types, [8, 0]);
    // The reply keeps the identifier a raw socket chose.
    assert_eq!(u16::from_be_bytes([buf[24], buf[25]]), 77);
    assert_eq!(recv(&*r, 64), Err(Errno::EAGAIN));

    r.set_option(255, 1, &(1u32 << 8).to_le_bytes()).unwrap();
    let mut out = [0; 4];
    assert_eq!(r.get_option(255, 1, &mut out), Ok(4));
    assert_eq!(u32::from_le_bytes(out), 1 << 8);
    send_to(&*r, &echo4(77, 2, b"x"), Some(a("127.0.0.1:0"))).unwrap();
    r.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!(buf[20], 0);
    assert_eq!(recv(&*r, 64), Err(Errno::EAGAIN));
    let mut bad = echo4(77, 3, b"x");
    bad[2] ^= 0xff;
    send_to(&*r, &bad, Some(a("127.0.0.1:0"))).unwrap();
    assert_eq!(recv(&*r, 64), Err(Errno::EAGAIN));
}

/// Echo requests cross a veth to the namespace holding the address, which answers unless
/// `icmp_echo_ignore_all`; to the all-hosts group, both the sender's own namespace (looped
/// back) and the peer's answer from an address of the interface it arrived on, once
/// `icmp_echo_ignore_broadcasts` is off.
#[test]
fn echo_requests_cross_a_veth_and_reach_groups() {
    let (na, nb) = veth_pair(("10.25.0.1", "10.25.0.2"), ("fd00:25::1", "fd00:25::2"));
    let p = ping_socket(&na, Domain::Inet);
    let mut buf = [0; 64];
    send_to(&*p, &echo4(1, 1, b"a"), Some(a("10.25.0.2:0"))).unwrap();
    assert_eq!(
        p.recv(&mut buf, MsgFlags(0)).unwrap().from.unwrap(),
        a("10.25.0.2:0")
    );
    nb.set_icmp_echo_ignore(crate::EchoIgnore::All, true)
        .unwrap();
    send_to(&*p, &echo4(1, 2, b"a"), Some(a("10.25.0.2:0"))).unwrap();
    assert_eq!(recv(&*p, 64), Err(Errno::EAGAIN));
    nb.set_icmp_echo_ignore(crate::EchoIgnore::All, false)
        .unwrap();

    p.set_option(SOL_IP, IpOption::IP_MULTICAST_IF.0, &[10, 25, 0, 1])
        .unwrap();
    send_to(&*p, &echo4(1, 3, b"g"), Some(a("224.0.0.1:0"))).unwrap();
    assert_eq!(recv(&*p, 64), Err(Errno::EAGAIN));
    na.set_icmp_echo_ignore(crate::EchoIgnore::Broadcasts, false)
        .unwrap();
    nb.set_icmp_echo_ignore(crate::EchoIgnore::Broadcasts, false)
        .unwrap();
    send_to(&*p, &echo4(1, 4, b"g"), Some(a("224.0.0.1:0"))).unwrap();
    let mut from: Vec<SocketAddr> = (0..2)
        .map(|_| p.recv(&mut buf, MsgFlags(0)).unwrap().from.unwrap())
        .collect();
    from.sort();
    assert_eq!(from, [a("10.25.0.1:0"), a("10.25.0.2:0")]);

    let p6 = ping_socket(&na, Domain::Inet6);
    let req = [128, 0, 0, 0, 0, 1, 0, 1];
    send_to(&*p6, &req, Some(a("[fd00:25::2]:0"))).unwrap();
    assert_eq!(
        p6.recv(&mut buf, MsgFlags(0)).unwrap().from.unwrap(),
        a("[fd00:25::2]:0")
    );
}

/// A datagram to a port nobody holds is answered with a real ICMP port unreachable: raw
/// sockets see it quoting the datagram's IP and UDP headers, and the sender learns of it.
#[test]
fn port_unreachables_are_icmp_messages() {
    let n = net();
    let r = raw_icmp(&n, Domain::Inet);
    let u = udp(&n);
    u.connect(a("127.0.0.1:9")).unwrap();
    send(&*u, b"data").unwrap();
    assert_eq!(u.take_error(), Some(Errno::ECONNREFUSED));
    let mut buf = [0; 128];
    let got = r.recv(&mut buf, MsgFlags(0)).unwrap();
    let icmp = &buf[20..got.len];
    assert_eq!((icmp[0], icmp[1]), (3, 3));
    assert_eq!(csum(icmp), 0);
    // The quoted IPv4 header, the UDP header to port 9, and the payload.
    assert_eq!(icmp[8] >> 4, 4);
    assert_eq!(icmp[8 + 9], 17);
    assert_eq!(u16::from_be_bytes([icmp[30], icmp[31]]), 9);
    assert_eq!(&icmp[36..], b"data");
}

/// Echo requests to the uplink go through the backend's ICMP endpoint, and its replies come
/// back with the identifier the guest socket has.
#[test]
fn echo_requests_leave_through_the_backend() {
    let (n, rec) = backed();
    let p = ping_socket(&n, Domain::Inet);
    send_to(&*p, &echo4(9, 5, b"out"), Some(a("192.0.2.7:0"))).unwrap();
    let ident = p.local_addr().unwrap().port();
    let d = rec.pings.lock().unwrap()[0].clone();
    let (sent, to) = d.sent.lock().unwrap()[0].clone();
    assert_eq!(to, a("192.0.2.7:0"));
    assert_eq!(u16::from_be_bytes([sent[4], sent[5]]), ident);
    // The host's own identifier comes back, and the guest sees its own.
    let mut reply = sent.clone();
    reply[0] = 0;
    reply[4..6].copy_from_slice(&999u16.to_be_bytes());
    d.arrive(&reply, a("192.0.2.7:0"));
    assert!(p.poll(None).contains(PollMask::IN));
    let mut buf = [0; 64];
    let got = p.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!(got.from.unwrap(), a("192.0.2.7:0"));
    assert_eq!(u16::from_be_bytes([buf[4], buf[5]]), ident);
    assert_eq!(csum(&buf[..got.len]), 0);
}

/// `IPV6_PKTINFO` as an option (`ping -I <interface>`) gives sends the interface (and source)
/// they name none of: a link-local destination is reachable through it.
#[test]
fn sticky_pktinfo_names_the_interface_of_sends() {
    let (na, _nb) = veth_pair(("10.26.0.1", "10.26.0.2"), ("fd00:26::1", "fd00:26::2"));
    let p = ping_socket(&na, Domain::Inet6);
    assert_eq!(
        p.set_option(SOL_IPV6, Ipv6Option::IPV6_PKTINFO.0, &[0; 19]),
        Err(Errno::EINVAL)
    );
    let req = [128, 0, 0, 0, 0, 1, 0, 1];
    // Link-local all-nodes: needs an interface.
    assert_eq!(
        send_to(&*p, &req, Some(a("[fe80::1]:0"))),
        Err(Errno::EINVAL)
    );
    let mut pktinfo = [0u8; 20];
    pktinfo[16..].copy_from_slice(&index_of(&na, "veth0").to_le_bytes());
    p.set_option(SOL_IPV6, Ipv6Option::IPV6_PKTINFO.0, &pktinfo)
        .unwrap();
    assert_eq!(send_to(&*p, &req, Some(a("[fe80::1]:0"))), Ok(8));
    send_to(&*p, &req, Some(a("[fd00:26::2]:0"))).unwrap();
    let mut buf = [0; 16];
    assert_eq!(
        p.recv(&mut buf, MsgFlags(0)).unwrap().from.unwrap(),
        a("[fd00:26::2]:0")
    );
}

/// One `struct sock_filter`.
fn bpf(code: u16, jt: u8, jf: u8, k: u32) -> Vec<u8> {
    let mut b = code.to_le_bytes().to_vec();
    b.extend([jt, jf]);
    b.extend(k.to_le_bytes());
    b
}

const SO_ATTACH_FILTER: i32 = 26;
const SO_DETACH_FILTER: i32 = 27;
const SO_LOCK_FILTER: i32 = 44;
const SKF_NET_OFF: i32 = -0x100000;

/// A UDP socket's filter sees the UDP header and payload, the IP header below it
/// (`SKF_NET_OFF`): 0 drops a datagram, less than its length cuts the payload, never the
/// header (`sk_filter_trim_cap` with `sizeof(struct udphdr)`).
#[test]
fn udp_socket_filters_drop_and_trim() {
    let n = net();
    let rx = udp(&n);
    rx.bind(a("127.0.0.1:7100"), false).unwrap();
    let tx = udp(&n);
    // A = TTL; if A == 9 { A = P[8]; if A == 'k' return 8 + 3 } return 0; and a short
    // return keeps the header.
    let prog = [
        bpf(0x30, 0, 0, (SKF_NET_OFF + 8) as u32),
        bpf(0x15, 0, 3, 9),
        bpf(0x30, 0, 0, 8),
        bpf(0x15, 0, 1, u32::from(b'k')),
        bpf(0x06, 0, 0, 8 + 3),
        bpf(0x06, 0, 0, 0),
    ]
    .concat();
    rx.set_option(SOL_SOCKET, SO_ATTACH_FILTER, &prog).unwrap();
    let mut out = vec![0; prog.len()];
    assert_eq!(
        rx.get_option(SOL_SOCKET, SO_ATTACH_FILTER, &mut out),
        Ok(prog.len())
    );
    assert_eq!(out, prog);
    send_to(&*tx, b"kept long", Some(a("127.0.0.1:7100"))).unwrap();
    assert_eq!(recv(&*rx, 64), Err(Errno::EAGAIN));
    tx.set_option(0, 2 /* IP_TTL */, &9i32.to_le_bytes())
        .unwrap();
    send_to(&*tx, b"dropped", Some(a("127.0.0.1:7100"))).unwrap();
    send_to(&*tx, b"kept long", Some(a("127.0.0.1:7100"))).unwrap();
    assert_eq!(recv(&*rx, 64).unwrap(), b"kep");
    assert_eq!(recv(&*rx, 64), Err(Errno::EAGAIN));
    let one_byte = bpf(0x06, 0, 0, 1);
    rx.set_option(SOL_SOCKET, SO_ATTACH_FILTER, &one_byte)
        .unwrap();
    send_to(&*tx, b"whole header", Some(a("127.0.0.1:7100"))).unwrap();
    assert_eq!(recv(&*rx, 64).unwrap(), b"");
    // Detached, everything comes through; locked, the filter stays.
    rx.set_option(SOL_SOCKET, SO_DETACH_FILTER, &0i32.to_le_bytes())
        .unwrap();
    assert_eq!(
        rx.set_option(SOL_SOCKET, SO_DETACH_FILTER, &0i32.to_le_bytes()),
        Err(Errno::ENOENT)
    );
    send_to(&*tx, b"all", Some(a("127.0.0.1:7100"))).unwrap();
    assert_eq!(recv(&*rx, 64).unwrap(), b"all");
    rx.set_option(SOL_SOCKET, SO_ATTACH_FILTER, &one_byte)
        .unwrap();
    rx.set_option(SOL_SOCKET, SO_LOCK_FILTER, &1i32.to_le_bytes())
        .unwrap();
    assert_eq!(
        rx.set_option(SOL_SOCKET, SO_DETACH_FILTER, &0i32.to_le_bytes()),
        Err(Errno::EPERM)
    );
    assert_eq!(
        rx.set_option(SOL_SOCKET, SO_LOCK_FILTER, &0i32.to_le_bytes()),
        Err(Errno::EPERM)
    );
    assert_eq!(
        getopt(&*rx, SOL_SOCKET, SO_LOCK_FILTER, 4).unwrap(),
        1i32.to_le_bytes()
    );
    // An invalid program: no return at the end.
    let tcp_sock = tcp(&n);
    assert_eq!(
        tcp_sock.set_option(SOL_SOCKET, SO_ATTACH_FILTER, &bpf(0x00, 0, 0, 0)),
        Err(Errno::EINVAL)
    );
}

/// iputils ping's filter on a raw socket: only echo replies of its identifier pass, read
/// past the IP header the raw IPv4 socket's data starts with (`BPF_MSH`, indirect loads).
/// A ping socket's filter sees the ICMP message and can cut it short.
#[test]
fn icmp_socket_filters_see_what_raw_and_ping_sockets_get() {
    let n = net();
    let r = raw_icmp(&n, Domain::Inet);
    let prog = [
        bpf(0xb1, 0, 0, 0),        // ldxb 4*([0]&0xf)
        bpf(0x48, 0, 0, 4),        // ldh [x + 4]
        bpf(0x15, 0, 3, 77),       // jeq #77
        bpf(0x50, 0, 0, 0),        // ldb [x + 0]
        bpf(0x15, 0, 1, 0),        // jeq #ECHOREPLY
        bpf(0x06, 0, 0, u32::MAX), // ret #~0
        bpf(0x06, 0, 0, 0),        // ret #0
    ]
    .concat();
    r.set_option(SOL_SOCKET, SO_ATTACH_FILTER, &prog).unwrap();
    send_to(&*r, &echo4(78, 1, b"x"), Some(a("127.0.0.1:0"))).unwrap();
    assert_eq!(recv(&*r, 64), Err(Errno::EAGAIN));
    send_to(&*r, &echo4(77, 1, b"x"), Some(a("127.0.0.1:0"))).unwrap();
    let got = recv(&*r, 64).unwrap();
    assert_eq!((got.len(), got[20]), (20 + 9, 0));
    assert_eq!(recv(&*r, 64), Err(Errno::EAGAIN));

    let p = ping_socket(&n, Domain::Inet);
    p.set_option(SOL_SOCKET, SO_ATTACH_FILTER, &bpf(0x06, 0, 0, 4))
        .unwrap();
    send_to(&*p, &echo4(0, 1, b"hello"), Some(a("127.0.0.1:0"))).unwrap();
    let reply = recv(&*p, 64).unwrap();
    assert_eq!((reply.len(), reply[0]), (4, 0));

    // A raw IPv6 socket's data, and so its filter's, starts at the ICMPv6 header.
    let r6 = raw_icmp(&n, Domain::Inet6);
    let replies = [
        bpf(0x30, 0, 0, 0),
        bpf(0x15, 0, 1, 129),
        bpf(0x06, 0, 0, 0xffff),
        bpf(0x06, 0, 0, 0),
    ]
    .concat();
    r6.set_option(SOL_SOCKET, SO_ATTACH_FILTER, &replies)
        .unwrap();
    send_to(&*r6, &[128, 0, 0, 0, 0, 9, 0, 1], Some(a("[::1]:0"))).unwrap();
    assert_eq!(recv(&*r6, 64).unwrap()[0], 129);
    assert_eq!(recv(&*r6, 64), Err(Errno::EAGAIN));
}

/// A datagram out a veth to an address nobody across holds fails neighbour resolution: the
/// sender's own namespace answers it with a host unreachable from the sender's address
/// (`arp_error_report`, `ip6_link_failure`), which the socket's error queue holds with
/// `IP_RECVERR` and raw sockets see. Through a gateway the peer holds, the peer drops it
/// silently, as Linux does without forwarding.
#[test]
fn unresolved_neighbours_answer_with_host_unreachable() {
    use crate::iface::{AddMode, Route};
    let (na, _nb) = veth_pair(("10.26.0.1", "10.26.0.2"), ("fd00:26::1", "fd00:26::2"));
    let s = udp(&na);
    s.set_option(SOL_IP, IpOption::IP_RECVERR.0, &int(1))
        .unwrap();
    let raw = raw_icmp(&na, Domain::Inet);
    send_to(&*s, b"anyone?", Some(a("10.26.0.9:7"))).unwrap();
    let (data, m) = recv_error(&*s, 64).unwrap();
    assert_eq!(data, b"anyone?");
    assert_eq!(m.from, Some(a("10.26.0.9:7")));
    let (ee, offender) = extended_err(&m.control[0]);
    assert_eq!(ee, (Errno::EHOSTUNREACH.0 as u32, 2, 3, 1));
    assert_eq!(&offender[..8], &[2, 0, 0, 0, 10, 26, 0, 1]);
    let seen = recv(&*raw, 256).unwrap();
    assert_eq!(
        (&seen[12..16], seen[20], seen[21]),
        (&[10, 26, 0, 1][..], 3, 1)
    );

    // Through the peer as gateway: lost without a word.
    let veth0 = index_of(&na, "veth0");
    let gw = Route::unicast(
        "192.0.2.0".parse().unwrap(),
        24,
        Some("10.26.0.2".parse().unwrap()),
        veth0,
    );
    na.add_route(gw, AddMode::ADD).unwrap();
    s.take_error();
    send_to(&*s, b"routed", Some(a("192.0.2.1:7"))).unwrap();
    assert_eq!(recv_error(&*s, 64).unwrap_err(), Errno::EAGAIN);
    assert_eq!(recv(&*raw, 256), Err(Errno::EAGAIN));

    // An IPv6 ping socket hears of its echo request as an address unreachable.
    let p = ping_socket(&na, Domain::Inet6);
    p.set_option(SOL_IPV6, Ipv6Option::IPV6_RECVERR.0, &int(1))
        .unwrap();
    send_to(&*p, &[128, 0, 0, 0, 0, 0, 0, 1], Some(a("[fd00:26::9]:0"))).unwrap();
    let (_, m) = recv_error(&*p, 64).unwrap();
    let (ee, _) = extended_err(&m.control[0]);
    assert_eq!(ee, (Errno::EHOSTUNREACH.0 as u32, 3, 1, 3));
}

/// A datagram that may not be fragmented (`IP_PMTUDISC_DO`, `IPV6_DONTFRAG`) and does not fit
/// the MTU of the link it leaves by is `EMSGSIZE`, and with `IP_RECVERR` a local error naming
/// the MTU waits in the error queue (`ip_local_error`, `ipv6_local_error`); by default the
/// stack would fragment it, so it goes.
#[test]
fn datagrams_over_the_mtu_with_df_are_emsgsize_and_a_local_error() {
    let (na, nb) = veth_pair(("10.27.0.1", "10.27.0.2"), ("fd00:27::1", "fd00:27::2"));
    let rx = udp(&nb);
    rx.bind(a("0.0.0.0:7009"), false).unwrap();
    let s = udp(&na);
    let big = vec![7u8; 1473];
    send_to(&*s, &big, Some(a("10.27.0.2:7009"))).unwrap();
    assert_eq!(recv(&*rx, 2000).unwrap().len(), 1473);
    assert_eq!(
        s.set_option(SOL_IP, IpOption::IP_MTU_DISCOVER.0, &int(6)),
        Err(Errno::EINVAL)
    );
    s.set_option(SOL_IP, IpOption::IP_MTU_DISCOVER.0, &int(2))
        .unwrap();
    s.set_option(SOL_IP, IpOption::IP_RECVERR.0, &int(1))
        .unwrap();
    assert_eq!(
        send_to(&*s, &big, Some(a("10.27.0.2:7009"))),
        Err(Errno::EMSGSIZE)
    );
    send_to(&*s, &big[..1472], Some(a("10.27.0.2:7009"))).unwrap();
    let (data, m) = recv_error(&*s, 64).unwrap();
    assert!(data.is_empty());
    assert_eq!(m.from, Some(a("10.27.0.2:0")));
    let c = &m.control[0];
    let (ee, offender) = extended_err(c);
    assert_eq!(ee, (Errno::EMSGSIZE.0 as u32, 1, 0, 0));
    assert_eq!(u32::from_le_bytes(c.data[8..12].try_into().unwrap()), 1500);
    // `SO_EE_OFFENDER` is AF_UNSPEC for a local error.
    assert_eq!(&offender[..2], &[0, 0]);
    // A local error is no pending error.
    assert_eq!(s.take_error(), None);

    let rx6 = n6_udp(&nb);
    rx6.bind(a("[::]:7010"), false).unwrap();
    let s6 = n6_udp(&na);
    s6.set_option(SOL_IPV6, Ipv6Option::IPV6_RECVERR.0, &int(1))
        .unwrap();
    let to6 = Some(a("[fd00:27::2]:7010"));
    send_to(&*s6, &big[..1453], to6).unwrap();
    s6.set_option(SOL_IPV6, Ipv6Option::IPV6_DONTFRAG.0, &int(1))
        .unwrap();
    assert_eq!(send_to(&*s6, &big[..1453], to6), Err(Errno::EMSGSIZE));
    send_to(&*s6, &big[..1452], to6).unwrap();
    let (_, m) = recv_error(&*s6, 64).unwrap();
    assert_eq!(
        extended_err(&m.control[0]).0,
        (Errno::EMSGSIZE.0 as u32, 1, 0, 0)
    );

    // A raw socket's own IP header is never fragmented.
    let r = raw_icmp(&na, Domain::Inet);
    r.set_option(SOL_IP, IpOption::IP_HDRINCL.0, &int(1))
        .unwrap();
    let mut packet = vec![
        0x45, 0, 0, 0, 0, 0, 0, 0, 64, 1, 0, 0, 0, 0, 0, 0, 10, 27, 0, 2,
    ];
    packet.extend(echo4(1, 1, &[0; 1480]));
    assert_eq!(
        send_to(&*r, &packet, Some(a("10.27.0.2:0"))),
        Err(Errno::EMSGSIZE)
    );
}

fn n6_udp(n: &Net) -> Arc<dyn Socket> {
    n.socket(Domain::Inet6, SocketKind::Datagram, 0, UID)
        .unwrap()
}

/// `net.ipv6.icmp.echo_ignore_all` stops a namespace answering ICMPv6 echo requests, and
/// `echo_ignore_multicast` those to a group (`icmpv6_rcv`, `icmpv6_echo_reply`).
#[test]
fn ipv6_echo_sysctls_stop_replies() {
    use crate::EchoIgnore;
    let (na, nb) = veth_pair(("10.29.0.1", "10.29.0.2"), ("fd00:29::1", "fd00:29::2"));
    let p6 = ping_socket(&na, Domain::Inet6);
    let req = [128, 0, 0, 0, 0, 1, 0, 1];
    nb.set_icmp_echo_ignore(EchoIgnore::All6, true).unwrap();
    send_to(&*p6, &req, Some(a("[fd00:29::2]:0"))).unwrap();
    assert_eq!(recv(&*p6, 64), Err(Errno::EAGAIN));
    // IPv4's sysctl is another.
    nb.set_icmp_echo_ignore(EchoIgnore::All6, false).unwrap();
    nb.set_icmp_echo_ignore(EchoIgnore::All, true).unwrap();
    send_to(&*p6, &req, Some(a("[fd00:29::2]:0"))).unwrap();
    assert!(recv(&*p6, 64).is_ok());

    // To all nodes out the veth: both namespaces answer, until they ignore groups.
    let veth0 = index_of(&na, "veth0");
    let ff02 = |n: &Net| {
        n.set_icmp_echo_ignore(EchoIgnore::Multicast6, true)
            .unwrap()
    };
    let all_nodes = SocketAddr::V6(std::net::SocketAddrV6::new(
        "ff02::1".parse().unwrap(),
        0,
        0,
        veth0,
    ));
    send_to(&*p6, &req, Some(all_nodes)).unwrap();
    assert!(recv(&*p6, 64).is_ok() && recv(&*p6, 64).is_ok());
    ff02(&na);
    ff02(&nb);
    send_to(&*p6, &req, Some(all_nodes)).unwrap();
    assert_eq!(recv(&*p6, 64), Err(Errno::EAGAIN));
}

/// `struct ip_mreq_source`: group, interface address, source.
fn mreq_source(group: &str, address: &str, source: &str) -> Vec<u8> {
    let ip = |s: &str| s.parse::<std::net::Ipv4Addr>().unwrap().octets();
    [ip(group), ip(address), ip(source)].concat()
}

/// A `sockaddr_storage` holding `ip`.
fn storage(ip: &str) -> Vec<u8> {
    let mut ss = vec![0u8; 128];
    match ip.parse::<std::net::IpAddr>().unwrap() {
        std::net::IpAddr::V4(v4) => {
            ss[..2].copy_from_slice(&2u16.to_le_bytes());
            ss[4..8].copy_from_slice(&v4.octets());
        }
        std::net::IpAddr::V6(v6) => {
            ss[..2].copy_from_slice(&10u16.to_le_bytes());
            ss[8..24].copy_from_slice(&v6.octets());
        }
    }
    ss
}

/// `struct group_filter`: interface, group, mode, sources.
fn group_filter(index: u32, group: &str, include: bool, sources: &[&str]) -> Vec<u8> {
    let mut v = index.to_le_bytes().to_vec();
    v.extend(storage(group));
    v.extend(u32::from(include).to_le_bytes());
    v.extend((sources.len() as u32).to_le_bytes());
    for s in sources {
        v.extend(storage(s));
    }
    v
}

/// Source filters (`ip_mc_source`, `ip_mc_msfilter`): a source-specific membership takes
/// only its sources, a blocked source is kept out, and the interface lets in what some
/// membership's filter allows, so a socket that only has `IP_MULTICAST_ALL` hears just that.
#[test]
fn multicast_source_filters_choose_the_senders() {
    const IP_UNBLOCK_SOURCE: i32 = 37;
    const IP_BLOCK_SOURCE: i32 = 38;
    const IP_ADD_SOURCE_MEMBERSHIP: i32 = 39;
    const IP_DROP_SOURCE_MEMBERSHIP: i32 = 40;
    const IP_MSFILTER: i32 = 41;
    let n = net();
    let bound = |ip: &str, port: u16| {
        let s = udp(&n);
        s.set_option(SOL_SOCKET, SocketOption::SO_REUSEADDR.0, &int(1))
            .unwrap();
        s.bind(SocketAddr::new(ip.parse().unwrap(), port), false)
            .unwrap();
        s
    };
    let (ssm, all, blocker) = (
        bound("0.0.0.0", 6100),
        bound("0.0.0.0", 6100),
        bound("0.0.0.0", 6100),
    );
    let (a1, a2) = (bound("127.0.0.1", 0), bound("127.0.0.2", 0));
    for s in [&a1, &a2] {
        s.set_option(SOL_IP, IpOption::IP_MULTICAST_IF.0, &[127, 0, 0, 1])
            .unwrap();
    }
    let to = Some(a("239.2.2.2:6100"));
    let req = |s: &str| mreq_source("239.2.2.2", "127.0.0.1", s);
    // A filter needs a membership; a source membership makes one.
    assert_eq!(
        ssm.set_option(SOL_IP, IP_BLOCK_SOURCE, &req("127.0.0.2")),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        ssm.set_option(SOL_IP, IP_ADD_SOURCE_MEMBERSHIP, &req("127.0.0.1")[..8]),
        Err(Errno::EINVAL)
    );
    ssm.set_option(SOL_IP, IP_ADD_SOURCE_MEMBERSHIP, &req("127.0.0.1"))
        .unwrap();
    assert_eq!(
        ssm.set_option(SOL_IP, IP_ADD_SOURCE_MEMBERSHIP, &req("127.0.0.1")),
        Err(Errno::EADDRNOTAVAIL)
    );
    assert_eq!(
        ssm.set_option(SOL_IP, IP_BLOCK_SOURCE, &req("127.0.0.2")),
        Err(Errno::EINVAL)
    );
    send_to(&*a1, b"1", to).unwrap();
    send_to(&*a2, b"2", to).unwrap();
    assert_eq!(recv(&*ssm, 4).unwrap(), b"1");
    assert_eq!(recv(&*ssm, 4), Err(Errno::EAGAIN));
    // The interface takes only what the one filter allows.
    assert_eq!(recv(&*all, 4).unwrap(), b"1");
    assert_eq!(recv(&*all, 4), Err(Errno::EAGAIN));
    assert_eq!(recv(&*blocker, 4).unwrap(), b"1");
    let _ = recv(&*blocker, 4);

    // An EXCLUDE membership blocking 127.0.0.2: still nobody hears it, until unblocked.
    blocker
        .set_option(
            SOL_IP,
            IpOption::IP_ADD_MEMBERSHIP.0,
            &mreqn("239.2.2.2", "127.0.0.1", 0),
        )
        .unwrap();
    blocker
        .set_option(SOL_IP, IP_BLOCK_SOURCE, &req("127.0.0.2"))
        .unwrap();
    send_to(&*a2, b"2", to).unwrap();
    assert_eq!(recv(&*all, 4), Err(Errno::EAGAIN));
    blocker
        .set_option(SOL_IP, IP_UNBLOCK_SOURCE, &req("127.0.0.2"))
        .unwrap();
    assert_eq!(
        blocker.set_option(SOL_IP, IP_UNBLOCK_SOURCE, &req("127.0.0.2")),
        Err(Errno::EADDRNOTAVAIL)
    );
    send_to(&*a2, b"2", to).unwrap();
    assert_eq!(recv(&*blocker, 4).unwrap(), b"2");
    assert_eq!(recv(&*all, 4).unwrap(), b"2");
    assert_eq!(recv(&*ssm, 4), Err(Errno::EAGAIN));

    // IP_MSFILTER reads back the filter, and sets one whole; igmp_max_msf is 10.
    let mut msf = [&req("0.0.0.0")[..8], &[0; 4], &4u32.to_le_bytes(), &[0; 16]].concat();
    let len = ssm.get_option(SOL_IP, IP_MSFILTER, &mut msf).unwrap();
    assert_eq!(len, 20);
    assert_eq!(&msf[8..20], &[1, 0, 0, 0, 1, 0, 0, 0, 127, 0, 0, 1]);
    let many: Vec<u8> = (0..11u8).flat_map(|i| [10, 0, 0, i]).collect();
    let set = |numsrc: u32, list: &[u8]| {
        [
            &req("0.0.0.0")[..8],
            &1u32.to_le_bytes(),
            &numsrc.to_le_bytes(),
            list,
        ]
        .concat()
    };
    assert_eq!(
        ssm.set_option(SOL_IP, IP_MSFILTER, &set(11, &many)),
        Err(Errno::ENOBUFS)
    );
    assert_eq!(
        ssm.set_option(SOL_IP, IP_MSFILTER, &set(2, &many[..4])),
        Err(Errno::EINVAL)
    );
    ssm.set_option(SOL_IP, IP_MSFILTER, &set(2, &[127, 0, 0, 1, 127, 0, 0, 2]))
        .unwrap();
    send_to(&*a2, b"2", to).unwrap();
    assert_eq!(recv(&*ssm, 4).unwrap(), b"2");
    // Dropping every source of an INCLUDE filter leaves the group.
    ssm.set_option(SOL_IP, IP_DROP_SOURCE_MEMBERSHIP, &req("127.0.0.1"))
        .unwrap();
    ssm.set_option(SOL_IP, IP_DROP_SOURCE_MEMBERSHIP, &req("127.0.0.2"))
        .unwrap();
    assert_eq!(
        ssm.set_option(
            SOL_IP,
            IpOption::IP_DROP_MEMBERSHIP.0,
            &mreqn("239.2.2.2", "127.0.0.1", 0)
        ),
        Err(Errno::EADDRNOTAVAIL)
    );
}

/// IPv6 source filters through `MCAST_JOIN_SOURCE_GROUP` and `MCAST_MSFILTER`
/// (`ip6_mc_source`, `ip6_mc_msfilter`, `inet6_mc_check`) across a veth.
#[test]
fn ipv6_source_filters_through_mcast_options() {
    const MCAST_JOIN_SOURCE_GROUP: i32 = 46;
    const MCAST_LEAVE_SOURCE_GROUP: i32 = 47;
    const MCAST_MSFILTER: i32 = 48;
    let (na, nb) = veth_pair(("10.31.0.1", "10.31.0.2"), ("fd00:31::1", "fd00:31::2"));
    let (veth0, veth1) = (index_of(&na, "veth0"), index_of(&nb, "veth1"));
    let rx = nb
        .socket(Domain::Inet6, SocketKind::Datagram, 0, UID)
        .unwrap();
    rx.bind(a("[::]:6200"), false).unwrap();
    let gsr = |source: &str| {
        let mut v = veth1.to_le_bytes().to_vec();
        v.extend(storage("ff05::31"));
        v.extend(storage(source));
        v
    };
    assert_eq!(
        rx.set_option(SOL_IPV6, MCAST_JOIN_SOURCE_GROUP, &gsr("10.0.0.1")),
        Err(Errno::EADDRNOTAVAIL)
    );
    rx.set_option(SOL_IPV6, MCAST_JOIN_SOURCE_GROUP, &gsr("fd00:31::9"))
        .unwrap();
    let tx = na
        .socket(Domain::Inet6, SocketKind::Datagram, 0, UID)
        .unwrap();
    tx.bind(a("[fd00:31::1]:0"), false).unwrap();
    tx.set_option(
        SOL_IPV6,
        Ipv6Option::IPV6_MULTICAST_IF.0,
        &int(veth0 as i32),
    )
    .unwrap();
    let to = Some(a("[ff05::31]:6200"));
    send_to(&*tx, b"x", to).unwrap();
    assert_eq!(recv(&*rx, 4), Err(Errno::EAGAIN));
    // The whole filter at once; then read back.
    let f = group_filter(veth1, "ff05::31", true, &["fd00:31::9", "fd00:31::1"]);
    rx.set_option(SOL_IPV6, MCAST_MSFILTER, &f).unwrap();
    send_to(&*tx, b"y", to).unwrap();
    assert_eq!(recv(&*rx, 4).unwrap(), b"y");
    let mut out = group_filter(veth1, "ff05::31", false, &["::", "::", "::"]);
    let len = rx.get_option(SOL_IPV6, MCAST_MSFILTER, &mut out).unwrap();
    assert_eq!(len, 140 + 2 * 128);
    assert_eq!(&out[132..140], &[1, 0, 0, 0, 2, 0, 0, 0]);
    assert_eq!(
        &out[140 + 128 + 8..140 + 128 + 24],
        &storage("fd00:31::1")[8..24]
    );
    // EXCLUDE that sender.
    let f = group_filter(veth1, "ff05::31", false, &["fd00:31::1"]);
    rx.set_option(SOL_IPV6, MCAST_MSFILTER, &f).unwrap();
    send_to(&*tx, b"z", to).unwrap();
    assert_eq!(recv(&*rx, 4), Err(Errno::EAGAIN));
    assert_eq!(
        rx.set_option(SOL_IPV6, MCAST_LEAVE_SOURCE_GROUP, &gsr("fd00:31::1")),
        Err(Errno::EINVAL)
    );
}

fn send_at(s: &dyn Socket, data: &[u8], to: SocketAddr, ms: u64) -> Result<usize, Errno> {
    s.send(SendMsg {
        data,
        to: Some(to),
        flags: MsgFlags(0),
        control: &[],
        net_raw: false,
        now: Duration::from_millis(ms),
    })
}

/// How many messages `s` has queued.
fn drain(s: &dyn Socket) -> usize {
    std::iter::from_fn(|| recv(s, 256).ok()).count()
}

/// Port unreachables leaving through a veth are rate-limited per destination: a burst of six,
/// then one per `icmp_ratelimit` (`icmp/ratelimit` for IPv6, a tenth of a second); those
/// through loopback never are (`icmpv4_xrlim_allow`, `icmpv6_xrlim_allow`). Echo replies are
/// once `icmp_ratemask` names them, and `icmp_msgs_per_sec` caps everything limited.
#[test]
fn icmp_errors_through_a_veth_are_rate_limited() {
    let (na, nb) = veth_pair(("10.32.0.1", "10.32.0.2"), ("fd00:32::1", "fd00:32::2"));
    let (raw, raw6) = (raw_icmp(&na, Domain::Inet), raw_icmp(&na, Domain::Inet6));
    let (u, u6) = (udp(&na), n6_udp(&na));
    let t = 100_000;
    for _ in 0..10 {
        send_at(&*u, b"x", a("10.32.0.2:9"), t).unwrap();
    }
    assert_eq!(drain(&*raw), 6);
    send_at(&*u, b"x", a("10.32.0.2:9"), t + 999).unwrap();
    assert_eq!(drain(&*raw), 0);
    send_at(&*u, b"x", a("10.32.0.2:9"), t + 1000).unwrap();
    assert_eq!(drain(&*raw), 1);
    for _ in 0..10 {
        send_at(&*u, b"x", a("127.0.0.1:9"), t + 1000).unwrap();
    }
    assert_eq!(drain(&*raw), 10);

    for _ in 0..10 {
        send_at(&*u6, b"x", a("[fd00:32::2]:9"), t).unwrap();
    }
    assert_eq!(drain(&*raw6), 6);
    send_at(&*u6, b"x", a("[fd00:32::2]:9"), t + 100).unwrap();
    assert_eq!(drain(&*raw6), 1);

    nb.set_icmp_limit(IcmpLimit::Ratelimit(0)).unwrap();
    for _ in 0..10 {
        send_at(&*u, b"x", a("10.32.0.2:9"), t + 1000).unwrap();
    }
    assert_eq!(drain(&*raw), 10);

    let p = ping_socket(&na, Domain::Inet);
    let echo = |seq, ms| {
        send_at(&*p, &echo4(1, seq, b"e"), a("10.32.0.2:0"), ms)
            .map(drop)
            .unwrap()
    };
    (0..8u16).for_each(|seq| echo(seq, t));
    assert_eq!(drain(&*p), 8);
    nb.set_icmp_limit(IcmpLimit::Ratelimit(1000)).unwrap();
    nb.set_icmp_limit(IcmpLimit::Ratemask(1)).unwrap();
    (0..8u16).for_each(|seq| echo(seq, t + 10_000));
    assert_eq!(drain(&*p), 6);

    nb.set_icmp_limit(IcmpLimit::Ratemask(0x1818)).unwrap();
    nb.set_icmp_limit(IcmpLimit::Ratelimit(0)).unwrap();
    nb.set_icmp_limit(IcmpLimit::MsgsPerSec(0)).unwrap();
    for _ in 0..100_000 {
        send_at(&*u, b"x", a("10.32.0.2:9"), t + 20_000).unwrap();
    }
    let sent = drain(&*raw);
    assert!(sent < 100_000, "{sent}");
    send_at(&*u, b"x", a("10.32.0.2:9"), t + 30_000).unwrap();
    assert_eq!(drain(&*raw), 0);
}

/// On a stream socket only `IP_ADD_MEMBERSHIP`/`IP_DROP_MEMBERSHIP` (before their length) and
/// `IPV6_ADD_MEMBERSHIP`/`IPV6_DROP_MEMBERSHIP` (after it) are `EPROTO`; `MCAST_JOIN_GROUP`
/// joins as on any socket, the membership goes with the socket's close
/// (`ip_mc_drop_socket`), and an accepted socket has none of its listener's.
#[test]
fn stream_sockets_refuse_only_the_old_membership_options() {
    let n = net();
    let lo_groups = |n: &Net| n.interfaces()[0].groups.clone();
    let t = tcp(&n);
    assert_eq!(
        t.set_option(SOL_IP, IpOption::IP_ADD_MEMBERSHIP.0, &[0; 4]),
        Err(Errno::EPROTO)
    );
    let t6 = n.socket(Domain::Inet6, SocketKind::Stream, 0, UID).unwrap();
    let add6 = Ipv6Option::IPV6_ADD_MEMBERSHIP.0;
    assert_eq!(t6.set_option(SOL_IPV6, add6, &[0; 4]), Err(Errno::EINVAL));
    assert_eq!(
        t6.set_option(SOL_IPV6, add6, &mreq6("ff05::7", 1)),
        Err(Errno::EPROTO)
    );

    const MCAST_JOIN_GROUP: i32 = 42;
    let group: std::net::IpAddr = "239.7.7.7".parse().unwrap();
    let req = [&1u32.to_le_bytes()[..], &storage("239.7.7.7")].concat();
    t.set_option(SOL_IP, MCAST_JOIN_GROUP, &req).unwrap();
    assert!(lo_groups(&n).contains(&(group, 1)));
    let req6 = [&1u32.to_le_bytes()[..], &storage("ff05::7")].concat();
    t6.set_option(SOL_IPV6, MCAST_JOIN_GROUP, &req6).unwrap();
    let group6: std::net::IpAddr = "ff05::7".parse().unwrap();
    assert!(lo_groups(&n).contains(&(group6, 1)));
    drop_socket(t6);
    assert!(lo_groups(&n).iter().all(|(g, _)| *g != group6));

    t.bind(a("127.0.0.1:0"), false).unwrap();
    t.listen(1).unwrap();
    let c = tcp(&n);
    c.connect(t.local_addr().unwrap()).unwrap();
    let accepted = t.accept().unwrap();
    drop(accepted);
    assert!(lo_groups(&n).contains(&(group, 1)));
    drop_socket(t);
    assert!(lo_groups(&n).iter().all(|(g, _)| *g != group));
}

/// With `IPV6_RECVPATHMTU`, a send `IPV6_DONTFRAG` refuses leaves its path MTU for the next
/// `recvmsg` of a UDP or raw socket, ahead of queued data and only the latest one
/// (`ipv6_local_rxpmtu`, `ipv6_recv_rxpmtu`): no data, the destination as the sender, an
/// `IPV6_PATHMTU` control message. `IP_PMTUDISC_DO` alone leaves none. `IPV6_PATHMTU` reads
/// the MTU of a connected socket's route (`ENOTCONN` unconnected).
#[test]
fn ipv6_recvpathmtu_reports_the_mtu_a_dontfrag_send_exceeded() {
    let (na, _nb) = veth_pair(("10.33.0.1", "10.33.0.2"), ("fd00:33::1", "fd00:33::2"));
    let veth0 = index_of(&na, "veth0");
    na.set_mtu(veth0, 1400).unwrap();
    let s6 = n6_udp(&na);
    s6.bind(a("[::]:7012"), false).unwrap();
    let big = vec![7u8; 1400];
    let to = a("[fd00:33::2]:7011");
    let pathmtu = |s: &dyn Socket| getopt(s, SOL_IPV6, Ipv6Option::IPV6_PATHMTU.0, 32);
    assert_eq!(pathmtu(&*s6), Err(Errno::ENOTCONN));
    s6.set_option(SOL_IPV6, Ipv6Option::IPV6_RECVPATHMTU.0, &int(1))
        .unwrap();
    assert_eq!(
        getopt(&*s6, SOL_IPV6, Ipv6Option::IPV6_RECVPATHMTU.0, 4).unwrap(),
        int(1)
    );
    s6.set_option(SOL_IPV6, Ipv6Option::IPV6_MTU_DISCOVER.0, &int(2))
        .unwrap();
    assert_eq!(send_to(&*s6, &big, Some(to)), Err(Errno::EMSGSIZE));
    assert_eq!(recv(&*s6, 64), Err(Errno::EAGAIN));
    s6.set_option(SOL_IPV6, Ipv6Option::IPV6_DONTFRAG.0, &int(1))
        .unwrap();
    assert_eq!(send_to(&*s6, &big, Some(to)), Err(Errno::EMSGSIZE));
    na.set_mtu(veth0, 1300).unwrap();
    assert_eq!(send_to(&*s6, &big, Some(to)), Err(Errno::EMSGSIZE));
    // Itself, to itself: a datagram is queued, the path MTU still comes first.
    send_to(&*s6, b"q", Some(a("[::1]:7012"))).unwrap();
    let mut buf = [0u8; 16];
    let m = s6.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!((m.len, m.from), (0, Some(a("[fd00:33::2]:0"))));
    let c = &m.control[0];
    assert_eq!((c.level, c.kind), (SOL_IPV6, Ipv6Option::IPV6_PATHMTU.0));
    assert_eq!(c.data.len(), 32);
    assert_eq!(&c.data[..2], &10u16.to_le_bytes());
    assert_eq!(
        &c.data[8..24],
        &"fd00:33::2".parse::<std::net::Ipv6Addr>().unwrap().octets()
    );
    assert_eq!(u32::from_le_bytes(c.data[28..32].try_into().unwrap()), 1300);
    assert_eq!(recv(&*s6, 16).unwrap(), b"q");

    let u = n6_udp(&na);
    u.connect(to).unwrap();
    let mtu = pathmtu(&*u).unwrap();
    assert_eq!(u32::from_le_bytes(mtu[28..32].try_into().unwrap()), 1300);
    assert!(mtu[..28].iter().all(|&b| b == 0));
    assert_eq!(
        getopt(&*u, SOL_IPV6, Ipv6Option::IPV6_PATHMTU.0, 31),
        Err(Errno::EINVAL)
    );

    let r = raw_icmp(&na, Domain::Inet6);
    r.set_option(SOL_IPV6, Ipv6Option::IPV6_RECVPATHMTU.0, &int(1))
        .unwrap();
    r.set_option(SOL_IPV6, Ipv6Option::IPV6_DONTFRAG.0, &int(1))
        .unwrap();
    let mut req = vec![128, 0, 0, 0, 0, 1, 0, 1];
    req.resize(1300, 0);
    assert_eq!(
        send_to(&*r, &req, Some(a("[fd00:33::2]:0"))),
        Err(Errno::EMSGSIZE)
    );
    let m = r.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!(m.control[0].kind, Ipv6Option::IPV6_PATHMTU.0);
}

/// A backend whose resolver answers when the test says, from the test's thread, as the host's
/// would from its own; it carries nothing else.
#[derive(Default)]
struct Resolver {
    asked: Mutex<Vec<(crate::Lookup, crate::LookupDone)>>,
}

impl Resolver {
    /// The lookups asked so far, without their answers.
    fn questions(&self) -> Vec<crate::Lookup> {
        self.asked
            .lock()
            .unwrap()
            .iter()
            .map(|(l, _)| l.clone())
            .collect()
    }

    /// Answers the oldest lookup.
    fn answer(&self, found: Result<crate::Found, crate::LookupError>) {
        let (_, done) = self.asked.lock().unwrap().remove(0);
        done(found);
    }
}

impl Backend for Resolver {
    fn connect(&self, _: SocketAddr, _: WaitQueue) -> Result<Arc<dyn BackendStream>, Errno> {
        Err(Errno::ENETUNREACH)
    }
    fn datagram(&self, _: Domain, _: WaitQueue) -> Result<Arc<dyn BackendDatagram>, Errno> {
        Err(Errno::ENETUNREACH)
    }
    fn publish(
        &self,
        _: SocketAddr,
        _: u32,
        _: Arc<dyn InboundSink>,
    ) -> Result<Box<dyn Publication>, std::io::Error> {
        Err(std::io::ErrorKind::Unsupported.into())
    }
    fn lookup(&self, lookup: crate::Lookup, done: crate::LookupDone) {
        self.asked.lock().unwrap().push((lookup, done));
    }
}

fn with_resolver(config: NetConfig) -> (Arc<Net>, Arc<Resolver>) {
    let r = Arc::new(Resolver::default());
    (Net::new(config, Some(r.clone())), r)
}

fn addrs(canonical: &str, list: &[&str], ttl: u32) -> Result<crate::Found, crate::LookupError> {
    Ok(crate::Found {
        answer: crate::Answer::Addrs {
            canonical: canonical.into(),
            addrs: list.iter().map(|ip| ip.parse().unwrap()).collect(),
        },
        ttl,
    })
}

/// The records of type `kind` in the answer section of `msg`, as their data.
fn records(msg: &[u8], kind: u16) -> Vec<Vec<u8>> {
    let an = u16::from_be_bytes([msg[6], msg[7]]);
    let mut pos = 12;
    // The question: a name, type and class.
    while msg[pos] != 0 && msg[pos] & 0xc0 == 0 {
        pos += 1 + usize::from(msg[pos]);
    }
    pos += 1 + 4;
    let mut out = Vec::new();
    for _ in 0..an {
        while msg[pos] != 0 && msg[pos] & 0xc0 == 0 {
            pos += 1 + usize::from(msg[pos]);
        }
        pos += if msg[pos] == 0 { 1 } else { 2 };
        let t = u16::from_be_bytes([msg[pos], msg[pos + 1]]);
        let len = usize::from(u16::from_be_bytes([msg[pos + 8], msg[pos + 9]]));
        if t == kind {
            out.push(msg[pos + 10..pos + 10 + len].to_vec());
        }
        pos += 10 + len;
    }
    out
}

const DNS: &str = "10.0.2.3:53";
const TYPE_A: u16 = 1;
const TYPE_MX: u16 = 15;
const TYPE_PTR: u16 = 12;
const TYPE_AAAA: u16 = 28;

/// UDP to 10.0.2.3:53: the server asks the backend's resolver once per name, whatever the
/// type, answers once the resolver has, from 10.0.2.3:53 to the socket's address, and maps
/// what the host found to RCODEs. The send never waits for the lookup.
#[test]
fn the_dns_server_answers_udp_queries_from_the_resolver() {
    use crate::{Lookup, LookupError, dns::tests::query};
    let (n, r) = with_resolver(NetConfig::host_default());
    assert_eq!(n.resolv_conf(), "nameserver 10.0.2.3\n");
    let s = udp(&n);
    send_to(
        &*s,
        &query(0x1111, "Example.COM", TYPE_A, None),
        Some(a(DNS)),
    )
    .unwrap();
    assert_eq!(r.questions(), [Lookup::Name("example.com".into())]);
    assert!(!s.poll(None).contains(PollMask::IN));
    r.answer(addrs("example.com", &["192.0.2.7", "2001:db8::7"], 30));
    let mut buf = [0; 512];
    let got = s.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!(got.from, Some(a(DNS)));
    let answer = &buf[..got.len];
    assert_eq!(&answer[..2], &[0x11, 0x11]);
    // QR, RD and RA; NOERROR.
    assert_eq!(answer[2] & 0x81, 0x81);
    assert_eq!(answer[3], 0x80);
    assert_eq!(records(answer, TYPE_A), [vec![192, 0, 2, 7]]);

    // AAAA of a name with only IPv4: NODATA.
    send_to(&*s, &query(2, "v4.example", TYPE_AAAA, None), Some(a(DNS))).unwrap();
    r.answer(addrs("v4.example", &["192.0.2.8"], 30));
    let answer = recv(&*s, 512).unwrap();
    assert_eq!((answer[3] & 0xf, answer[7]), (0, 0));

    // NXDOMAIN, SERVFAIL.
    send_to(&*s, &query(3, "nope.example", TYPE_A, None), Some(a(DNS))).unwrap();
    r.answer(Err(LookupError::NotFound { ttl: 5 }));
    assert_eq!(recv(&*s, 512).unwrap()[3] & 0xf, 3);
    send_to(&*s, &query(4, "slow.example", TYPE_A, None), Some(a(DNS))).unwrap();
    r.answer(Err(LookupError::Failed));
    assert_eq!(recv(&*s, 512).unwrap()[3] & 0xf, 2);

    // PTR of a whole address asks for its name; MX goes to the name servers as it is.
    send_to(
        &*s,
        &query(5, "7.2.0.192.in-addr.arpa", TYPE_PTR, None),
        Some(a(DNS)),
    )
    .unwrap();
    assert_eq!(r.questions(), [Lookup::Addr("192.0.2.7".parse().unwrap())]);
    r.answer(Ok(crate::Found {
        answer: crate::Answer::Name("host.example".into()),
        ttl: 30,
    }));
    let answer = recv(&*s, 512).unwrap();
    assert_eq!(
        records(&answer, TYPE_PTR),
        [b"\x04host\x07example\x00".to_vec()]
    );
    let mx = query(6, "example.com", TYPE_MX, None);
    send_to(&*s, &mx, Some(a(DNS))).unwrap();
    assert_eq!(r.questions(), [Lookup::Forward(mx.clone())]);
    let mut upstream = mx.clone();
    upstream[..2].copy_from_slice(&[0xab, 0xcd]);
    upstream[2] |= 0x80;
    r.answer(Ok(crate::Found {
        answer: crate::Answer::Message(upstream),
        ttl: 0,
    }));
    let answer = recv(&*s, 512).unwrap();
    assert_eq!(&answer[..2], &[0, 6], "the query's identifier");
    assert_eq!(&answer[4..], &mx[4..]);

    // An answer for a socket that went is dropped.
    send_to(&*s, &query(7, "gone.example", TYPE_A, None), Some(a(DNS))).unwrap();
    drop(s);
    r.answer(addrs("gone.example", &["192.0.2.9"], 30));
}

/// An answer past 512 bytes keeps the records that fit and sets TC; the client asks again
/// over TCP, where the server answers in full with a length prefix and closes once the client
/// has closed and every query is answered. A connected UDP socket and an IPv6 socket get
/// their answers too.
#[test]
fn truncated_udp_answers_are_whole_over_tcp() {
    use crate::dns::tests::query;
    let (n, r) = with_resolver(NetConfig::host_default());
    let many: Vec<String> = (1..=40).map(|i| format!("2001:db8::{i:x}")).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    let s = udp(&n);
    s.connect(a(DNS)).unwrap();
    send(&*s, &query(1, "big.example", TYPE_AAAA, None)).unwrap();
    r.answer(addrs("big.example", &many, 30));
    let answer = recv(&*s, 4096).unwrap();
    assert!(answer.len() <= 512);
    assert_eq!(answer[2] & 0x02, 0x02, "TC");
    assert_eq!(records(&answer, TYPE_AAAA).len(), 17);

    let c = tcp(&n);
    assert_eq!(c.connect(a(DNS)), Err(Errno::EINPROGRESS));
    assert!(c.poll(None).contains(PollMask::OUT));
    assert_eq!(c.peer_addr(), Ok(a(DNS)));
    let q = query(2, "big.example", TYPE_AAAA, None);
    let mut framed = (q.len() as u16).to_be_bytes().to_vec();
    framed.extend(&q);
    // In two pieces, the second with a second query.
    send(&*c, &framed[..5]).unwrap();
    assert!(r.questions().is_empty());
    let mut rest = framed[5..].to_vec();
    rest.extend(&framed);
    rest[framed.len() - 5 + 3] = 3;
    send(&*c, &rest).unwrap();
    assert_eq!(r.questions().len(), 2);
    c.shutdown(Shutdown::Write).unwrap();
    assert_eq!(recv(&*c, 10), Err(Errno::EAGAIN));
    r.answer(addrs("big.example", &many, 30));
    let mut got = Vec::new();
    while got.len() < 2 {
        got.extend(recv(&*c, 2).unwrap());
    }
    let len = usize::from(u16::from_be_bytes([got[0], got[1]]));
    let answer = recv(&*c, len).unwrap();
    assert_eq!(answer.len(), len);
    assert_eq!(answer[2] & 0x02, 0, "not truncated");
    assert_eq!(records(&answer, TYPE_AAAA).len(), 40);
    // One query left: no end of stream yet.
    assert_eq!(recv(&*c, 10), Err(Errno::EAGAIN));
    r.answer(addrs("big.example", &many, 30));
    let len = u16::from_be_bytes(recv(&*c, 2).unwrap().try_into().unwrap());
    let answer = recv(&*c, usize::from(len)).unwrap();
    assert_eq!(&answer[..2], &[0, 3]);
    assert_eq!(recv(&*c, 10), Ok(Vec::new()), "the server closed");
    assert!(c.poll(None).contains(PollMask::HUP));

    let s6 = n
        .socket(Domain::Inet6, SocketKind::Datagram, 0, UID)
        .unwrap();
    send_to(
        &*s6,
        &query(9, "localhost", TYPE_A, None),
        Some(a("[::ffff:10.0.2.3]:53")),
    )
    .unwrap();
    let mut buf = [0; 512];
    let got = s6.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!(got.from, Some(a("[::ffff:10.0.2.3]:53")));
    assert_eq!(records(&buf[..got.len], TYPE_A), [vec![127, 0, 0, 1]]);
    assert!(r.questions().is_empty(), "localhost needs no lookup");
}

/// The server is a host: it answers pings, a datagram to another port gets a port
/// unreachable and a connection to another port is refused. Without a DNS server configured,
/// its address is an ordinary outside address.
#[test]
fn the_dns_server_refuses_other_ports_and_answers_pings() {
    let (n, _r) = with_resolver(NetConfig::host_default());
    let s = udp(&n);
    s.connect(a("10.0.2.3:54")).unwrap();
    send(&*s, b"x").unwrap();
    assert_eq!(recv(&*s, 8), Err(Errno::ECONNREFUSED));

    let c = tcp(&n);
    assert_eq!(c.connect(a("10.0.2.3:80")), Err(Errno::EINPROGRESS));
    assert!(c.poll(None).contains(PollMask::ERR));
    assert_eq!(c.connect(a("10.0.2.3:80")), Err(Errno::ECONNREFUSED));

    let p = ping_socket(&n, Domain::Inet);
    send_to(&*p, &echo4(0, 3, b"dns"), Some(a("10.0.2.3:0"))).unwrap();
    let mut buf = [0; 64];
    let r = p.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!(r.from, Some(a("10.0.2.3:0")));
    assert_eq!((buf[0], &buf[8..r.len]), (0, &b"dns"[..]));
    assert_eq!(csum(&buf[..r.len]), 0);

    let (n, rec) = backed_with(NetConfig {
        dns_server: Vec::new(),
        ..NetConfig::host_default()
    });
    let s = udp(&n);
    send_to(&*s, b"x", Some(a(DNS))).unwrap();
    assert_eq!(
        rec.dgrams.lock().unwrap().len(),
        1,
        "out through the backend"
    );
}

/// With an IPv6 uplink the server also sits at the prefix's host 3, as slirp's `fec0::3`.
#[test]
fn the_dns_server_has_an_ipv6_address_on_an_ipv6_uplink() {
    use crate::dns::tests::query;
    let uplink = crate::Uplink {
        v6: Some((
            "fec0::15".parse().unwrap(),
            64,
            Some("fec0::2".parse().unwrap()),
        )),
        ..crate::Uplink::default()
    };
    let servers = uplink.dns_server();
    assert_eq!(servers, [a("10.0.2.3:0").ip(), "fec0::3".parse().unwrap()]);
    let (n, r) = with_resolver(NetConfig {
        uplink: Some(uplink),
        dns_server: servers,
        ..NetConfig::default()
    });
    let s = n
        .socket(Domain::Inet6, SocketKind::Datagram, 0, UID)
        .unwrap();
    send_to(
        &*s,
        &query(1, "example.com", TYPE_AAAA, None),
        Some(a("[fec0::3]:53")),
    )
    .unwrap();
    r.answer(addrs("example.com", &["2001:db8::1"], 30));
    let mut buf = [0; 512];
    let got = s.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!(got.from, Some(a("[fec0::3]:53")));
    assert_eq!(records(&buf[..got.len], TYPE_AAAA).len(), 1);
}

/// A UDP header, then `payload`, with the checksum over the IPv4 pseudo header.
fn udp4(src: SocketAddr, dst: SocketAddr, payload: &[u8]) -> Vec<u8> {
    let len = 8 + payload.len();
    let mut u = Vec::new();
    u.extend(src.port().to_be_bytes());
    u.extend(dst.port().to_be_bytes());
    u.extend((len as u16).to_be_bytes());
    u.extend([0, 0]);
    u.extend_from_slice(payload);
    let (std::net::IpAddr::V4(s), std::net::IpAddr::V4(d)) = (src.ip(), dst.ip()) else {
        panic!("IPv4")
    };
    let mut pseudo = s.octets().to_vec();
    pseudo.extend(d.octets());
    pseudo.extend([0, 17]);
    pseudo.extend((len as u16).to_be_bytes());
    pseudo.extend(&u);
    let c = csum(&pseudo);
    u[6..8].copy_from_slice(&c.to_be_bytes());
    u
}

/// Raw sockets of other protocols (`raw.c`): a raw UDP socket gets a copy of every UDP
/// datagram arriving in the namespace, behind an IPv4 header; a raw socket's own UDP packet
/// reaches the UDP socket of its port; `IPPROTO_RAW` sends its own header; a protocol nobody
/// takes is answered with a protocol unreachable, one a raw socket listens to is not.
#[test]
fn raw_sockets_of_any_protocol_see_and_send_packets() {
    let n = net();
    let raw_udp = n.socket(Domain::Inet, SocketKind::Raw, 17, UID).unwrap();
    assert_eq!(raw_udp.protocol(), 17);
    let rx = n
        .socket(Domain::Inet, SocketKind::Datagram, 0, UID)
        .unwrap();
    rx.bind(a("127.0.0.1:7000"), false).unwrap();
    let tx = n
        .socket(Domain::Inet, SocketKind::Datagram, 0, UID)
        .unwrap();
    tx.bind(a("127.0.0.1:7001"), false).unwrap();
    send_to(&*tx, b"hi", Some(a("127.0.0.1:7000"))).unwrap();
    let mut buf = [0; 128];
    let got = raw_udp.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!(got.len, 20 + 8 + 2);
    assert_eq!((buf[0], buf[9]), (0x45, 17));
    assert_eq!(&buf[20..24], &[0x1b, 0x59, 0x1b, 0x58]);
    assert_eq!(&buf[28..30], b"hi");
    assert_eq!(recv(&*rx, 16).unwrap(), b"hi");

    // A UDP packet a raw UDP socket writes reaches the UDP socket of its port, as from the
    // port its header names.
    let packet = udp4(a("127.0.0.1:9999"), a("127.0.0.1:7000"), b"crafted");
    send_to(&*raw_udp, &packet, Some(a("127.0.0.1:0"))).unwrap();
    let mut from = [0; 16];
    let r = rx.recv(&mut from, MsgFlags(0)).unwrap();
    assert_eq!(&from[..r.len], b"crafted");
    assert_eq!(r.from.unwrap(), a("127.0.0.1:9999"));
    // A bad checksum is dropped.
    let mut bad = packet.clone();
    bad[8] ^= 1;
    send_to(&*raw_udp, &bad, Some(a("127.0.0.1:0"))).unwrap();
    assert_eq!(recv(&*rx, 16), Err(Errno::EAGAIN));
    while raw_udp.recv(&mut buf, MsgFlags::MSG_DONTWAIT).is_ok() {}

    // Protocol 253 (experimentation): a raw socket of 253 gets what another sends; the
    // sender's own raw socket of 254 takes its own packet, so no error comes back.
    let icmp = raw_icmp(&n, Domain::Inet);
    let exp = n.socket(Domain::Inet, SocketKind::Raw, 253, UID).unwrap();
    let mine = n.socket(Domain::Inet, SocketKind::Raw, 253, UID).unwrap();
    send_to(&*mine, b"payload", Some(a("127.0.0.1:0"))).unwrap();
    let got = exp.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!(buf[9], 253);
    assert_eq!(&buf[20..got.len], b"payload");
    assert_eq!(
        recv(&*icmp, 128),
        Err(Errno::EAGAIN),
        "no error for a taken protocol"
    );

    // IPPROTO_RAW sends with IP_HDRINCL: the header names protocol 253, then 252, which no
    // socket takes: a protocol unreachable comes back, quoting the packet.
    let raw = n.socket(Domain::Inet, SocketKind::Raw, 255, UID).unwrap();
    let mut out = [0; 4];
    let len = raw.get_option(0, 3, &mut out).unwrap();
    assert_eq!(&out[..len], &1i32.to_le_bytes(), "IP_HDRINCL is on");
    let mut packet = vec![
        0x45, 0, 0, 0, 0, 0, 0, 0, 33, 253, 0, 0, 0, 0, 0, 0, 127, 0, 0, 1,
    ];
    packet.extend(b"own");
    send_to(&*raw, &packet, Some(a("127.0.0.1:0"))).unwrap();
    let got = exp.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!(buf[8], 33, "the header's TTL");
    assert_eq!(&buf[20..got.len], b"own");
    packet[9] = 252;
    send_to(&*raw, &packet, Some(a("127.0.0.1:0"))).unwrap();
    let got = icmp.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!((buf[20], buf[21]), (3, 2), "protocol unreachable");
    assert_eq!(buf[28 + 9], 252, "quoting the packet's header");
    assert_eq!(&buf[48..got.len], b"own");
    assert_eq!(
        send_to(&*raw, &packet[..10], Some(a("127.0.0.1:0"))),
        Err(Errno::EINVAL)
    );
    // ICMP_FILTER is ICMP's alone.
    assert_eq!(exp.set_option(255, 1, &[0; 4]), Err(Errno::EOPNOTSUPP));
}

/// A raw IPv6 socket of another protocol: `IPV6_CHECKSUM` at the IPv6 level (refused on
/// ICMPv6 there), and the checksum the kernel computes over the pseudo header.
#[test]
fn raw_ipv6_sockets_compute_the_checksum_ipv6_checksum_asks_for() {
    let n = net();
    let r = n.socket(Domain::Inet6, SocketKind::Raw, 253, UID).unwrap();
    let mut out = [0; 4];
    let len = r.get_option(41, 7, &mut out).unwrap();
    assert_eq!(
        &out[..len],
        &(-1i32).to_le_bytes(),
        "no checksum by default"
    );
    assert_eq!(r.set_option(41, 7, &3i32.to_le_bytes()), Err(Errno::EINVAL));
    r.set_option(41, 7, &2i32.to_le_bytes()).unwrap();
    send_to(&*r, &[0xab, 0, 0, 0, 1, 2], Some(a("[::1]:0"))).unwrap();
    let mut buf = [0; 16];
    let got = r.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!(got.len, 6, "no IPv6 header");
    let mut pseudo = std::net::Ipv6Addr::LOCALHOST.octets().to_vec();
    pseudo.extend(std::net::Ipv6Addr::LOCALHOST.octets());
    pseudo.extend(6u32.to_be_bytes());
    pseudo.extend([0, 0, 0, 253]);
    pseudo.extend(&buf[..6]);
    assert_eq!(csum(&pseudo), 0);
    let icmp6 = raw_icmp(&n, Domain::Inet6);
    assert_eq!(
        icmp6.set_option(41, 7, &2i32.to_le_bytes()),
        Err(Errno::EINVAL)
    );
    assert_eq!(r.set_option(58, 1, &[0; 32]), Err(Errno::EOPNOTSUPP));
}

/// Sticky IPv6 extension headers (`ipv6_set_opt_hdr`): LTP's segment routing header of 640
/// bytes is taken and read back; a routing header of another type, a length that is not
/// whole 8-byte units, or one shorter than the header says are `EINVAL`; empty removes it.
#[test]
fn ipv6_sticky_routing_headers_are_checked_and_kept() {
    let n = net();
    let s = n
        .socket(Domain::Inet6, SocketKind::Datagram, 0, UID)
        .unwrap();
    let rthdr = 57;
    let size = 640;
    let segments = (size - 8) / 16;
    let mut srh = vec![0u8; size];
    srh[1] = ((size - 8) / 8) as u8;
    srh[2] = 4;
    srh[3] = (segments - 1) as u8;
    srh[4] = srh[3];
    for i in 0..segments {
        srh[8 + 16 * i..24 + 16 * i].copy_from_slice(&std::net::Ipv6Addr::LOCALHOST.octets());
    }
    let padding = size - 8 - 16 * segments;
    if padding > 0 {
        srh[9 + 16 * segments] = (padding - 2) as u8;
    }
    s.set_option(41, rthdr, &srh).unwrap();
    let mut out = vec![0; 1024];
    assert_eq!(s.get_option(41, rthdr, &mut out), Ok(size));
    assert_eq!(&out[..size], &srh[..]);
    let mut type0 = srh.clone();
    type0[2] = 0;
    assert_eq!(s.set_option(41, rthdr, &type0), Err(Errno::EINVAL));
    assert_eq!(s.set_option(41, rthdr, &srh[..12]), Err(Errno::EINVAL));
    assert_eq!(s.set_option(41, rthdr, &srh[..16]), Err(Errno::EINVAL));
    s.set_option(41, rthdr, &[]).unwrap();
    assert_eq!(s.get_option(41, rthdr, &mut out), Ok(0));
    // A destination options header is not checked further.
    s.set_option(41, 59, &[17, 0, 1, 4, 0, 0, 0, 0]).unwrap();
    let v4 = n
        .socket(Domain::Inet, SocketKind::Datagram, 0, UID)
        .unwrap();
    assert_eq!(v4.set_option(41, rthdr, &srh), Err(Errno::ENOPROTOOPT));
}
