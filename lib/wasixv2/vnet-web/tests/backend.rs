use std::{
    collections::VecDeque,
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll},
};
use wasixv2_abi::Errno;
use wasixv2_vnet::{
    Answer, Backend, BackendStream, Domain, Found, InboundSink, Lookup, LookupError, Room,
    Shutdown, StreamState,
};
use wasixv2_vnet_web::{Capabilities, Command, Config, Driver, Event, Transport, WebBackend};
use wasixv2_wait::{PollMask, PollTable, WaitEntry, WaitQueue};

fn peer() -> SocketAddr {
    "192.0.2.1:1234".parse().unwrap()
}
struct Mock {
    capabilities: Capabilities,
    commands: Vec<Command>,
    events: VecDeque<Event>,
    blocked: bool,
    error: Option<Errno>,
    closed: Arc<AtomicBool>,
}
impl Transport for Mock {
    fn capabilities(&self) -> Capabilities {
        self.capabilities
    }
    fn poll_send(&mut self, _: &mut Context<'_>, command: &Command) -> Poll<Result<(), Errno>> {
        if self.blocked {
            return Poll::Pending;
        }
        self.commands.push(command.clone());
        Poll::Ready(Ok(()))
    }
    fn poll_recv(&mut self, _: &mut Context<'_>) -> Poll<Result<Event, Errno>> {
        if let Some(error) = self.error.take() {
            return Poll::Ready(Err(error));
        }
        self.events
            .pop_front()
            .map_or(Poll::Pending, |event| Poll::Ready(Ok(event)))
    }
    fn poll_closed(&mut self, _: &mut Context<'_>) -> Poll<Errno> {
        self.error.take().map_or(Poll::Pending, Poll::Ready)
    }
    fn close(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
    }
}
struct Harness {
    backend: Arc<WebBackend>,
    driver: Driver,
    transport: Mock,
}
impl Harness {
    fn new() -> Self {
        Self::with_capabilities(Capabilities::classic(true))
    }
    fn with_capabilities(capabilities: Capabilities) -> Self {
        let (backend, driver) = WebBackend::new(
            Config {
                max_sockets: 8,
                max_lookups: 2,
                buffer_bytes: 8,
                chunk_bytes: 4,
            },
            capabilities,
        )
        .unwrap();
        Self {
            backend,
            driver,
            transport: Mock {
                capabilities,
                commands: Vec::new(),
                events: VecDeque::new(),
                blocked: false,
                error: None,
                closed: Arc::new(AtomicBool::new(false)),
            },
        }
    }
    fn pump(&mut self) -> Poll<Result<(), Errno>> {
        let mut cx = Context::from_waker(futures_util::task::noop_waker_ref());
        self.driver.poll(&mut cx, &mut self.transport)
    }
    fn event(&mut self, event: Event) {
        self.transport.events.push_back(event);
        assert!(self.pump().is_pending());
    }
    fn connect(&mut self) -> (Arc<dyn BackendStream>, u64) {
        let stream = self.backend.connect(peer(), WaitQueue::new()).unwrap();
        assert!(self.pump().is_pending());
        let (request, socket) = self
            .transport
            .commands
            .iter()
            .rev()
            .find_map(|c| match c {
                Command::Connect {
                    request, socket, ..
                } => Some((*request, *socket)),
                _ => None,
            })
            .unwrap();
        self.event(Event::Complete {
            request,
            result: Ok(()),
        });
        (stream, socket)
    }
    fn send_request(&self) -> (u64, u64, Vec<u8>) {
        self.transport
            .commands
            .iter()
            .rev()
            .find_map(|c| match c {
                Command::Send {
                    request,
                    socket,
                    data,
                } => Some((*request, *socket, data.clone())),
                _ => None,
            })
            .unwrap()
    }
    fn ack(&mut self, amount: usize) {
        let (request, socket, _) = self.send_request();
        self.event(Event::Sent {
            request,
            socket,
            amount,
        });
    }
}
#[test]
fn short_writes_and_acknowledgments_preserve_bytes_and_readiness() {
    let mut h = Harness::new();
    let (s, _) = h.connect();
    assert_eq!(s.state(), StreamState::Established);
    assert_eq!(s.write(b"abcdefghij"), Ok(8));
    assert_eq!(s.write(b"x"), Err(Errno::EAGAIN));
    assert!(!s.poll(None).contains(PollMask::OUT));
    assert!(h.pump().is_pending());
    assert_eq!(h.send_request().2, b"abcd");
    assert_eq!(s.output_len(), 8);
    h.ack(2);
    assert_eq!(h.send_request().2, b"cdef");
    assert_eq!(s.output_len(), 6);
    assert!(s.poll(None).contains(PollMask::OUT));
    assert_eq!(s.write(b"ij"), Ok(2));
    h.ack(4);
    assert_eq!(h.send_request().2, b"ghij");
    h.ack(4);
    assert_eq!(s.output_len(), 0);
}
#[test]
fn eof_keeps_write_direction_and_shutdown_orders_fin_after_data() {
    let mut h = Harness::new();
    let (s, socket) = h.connect();
    h.event(Event::Data {
        socket,
        data: b"abc".to_vec(),
    });
    h.event(Event::Eof { socket });
    assert!(s.poll(None).contains(PollMask::RDHUP));
    assert!(!s.poll(None).contains(PollMask::HUP));
    assert!(!s.tcp_established());
    let mut buf = [0; 8];
    assert_eq!(s.read(&mut buf, true), Ok(3));
    assert_eq!(s.input_len(), 3);
    assert_eq!(s.read(&mut buf, false), Ok(3));
    assert_eq!(s.read(&mut buf, false), Ok(0));
    assert_eq!(s.write(b"reply"), Ok(5));
    s.shutdown(Shutdown::Write).unwrap();
    assert_eq!(s.write(b"x"), Err(Errno::EPIPE));
    assert!(h.pump().is_pending());
    assert!(
        !h.transport
            .commands
            .iter()
            .any(|c| matches!(c, Command::Shutdown { .. }))
    );
    h.ack(4);
    h.ack(1);
    let request = h
        .transport
        .commands
        .iter()
        .find_map(|c| match c {
            Command::Shutdown {
                request,
                how: Shutdown::Write,
                ..
            } => Some(*request),
            _ => None,
        })
        .unwrap();
    h.event(Event::Complete {
        request,
        result: Ok(()),
    });
    assert!(s.poll(None).contains(PollMask::HUP | PollMask::RDHUP));
}
#[test]
fn reset_drains_readable_bytes_then_reports_error_once() {
    let mut h = Harness::new();
    let (s, socket) = h.connect();
    h.event(Event::Data {
        socket,
        data: b"data".to_vec(),
    });
    h.event(Event::Reset {
        socket,
        error: Errno::ECONNRESET,
    });
    assert_eq!(s.state(), StreamState::Closed);
    assert!(s.poll(None).contains(PollMask::ERR | PollMask::HUP));
    let mut buf = [0; 8];
    assert_eq!(s.read(&mut buf, false), Ok(4));
    assert_eq!(s.read(&mut buf, false), Err(Errno::ECONNRESET));
    assert_eq!(s.read(&mut buf, false), Ok(0));
    assert_eq!(s.write(b"x"), Err(Errno::EPIPE));
    assert_eq!(s.take_error(), None);
}
struct WakeCounter(AtomicUsize);
impl WaitEntry for WakeCounter {
    fn wake(&self, _: PollMask) -> bool {
        self.0.fetch_add(1, Ordering::SeqCst);
        true
    }
}
#[test]
fn refusal_wakes_the_original_wait_queue_and_so_error_clears() {
    let mut h = Harness::new();
    let wq = WaitQueue::new();
    let counter = Arc::new(WakeCounter(AtomicUsize::new(0)));
    let _registration = wq.add(counter.clone());
    let s = h.backend.connect(peer(), wq).unwrap();
    assert_eq!(s.state(), StreamState::Connecting);
    assert_eq!(s.poll(None), PollMask::empty());
    assert_eq!(s.read(&mut [0; 1], false), Err(Errno::EAGAIN));
    assert!(h.pump().is_pending());
    let request = match h.transport.commands[0] {
        Command::Connect { request, .. } => request,
        _ => unreachable!(),
    };
    h.event(Event::Complete {
        request,
        result: Err(Errno::ECONNREFUSED),
    });
    assert!(counter.0.load(Ordering::SeqCst) > 0);
    assert_eq!(s.take_error(), Some(Errno::ECONNREFUSED));
    assert_eq!(s.take_error(), None);
    assert!(!s.poll(None).contains(PollMask::ERR));
    assert_eq!(s.write(b"x"), Err(Errno::EPIPE));
}
#[test]
fn disconnect_and_reconnect_never_replay_queued_bytes() {
    let mut h = Harness::new();
    let (old, _) = h.connect();
    h.transport.blocked = true;
    assert_eq!(old.write(b"old"), Ok(3));
    assert!(h.pump().is_pending());
    let done = Arc::new(Mutex::new(None));
    let output = done.clone();
    h.backend.lookup(
        Lookup::Name("cancel.test".into()),
        Box::new(move |r| *output.lock().unwrap() = Some(r)),
    );
    h.transport.error = Some(Errno::ENETDOWN);
    assert_eq!(h.pump(), Poll::Ready(Err(Errno::ENETDOWN)));
    assert!(h.transport.closed.load(Ordering::SeqCst));
    assert_eq!(old.output_len(), 0);
    assert_eq!(old.take_error(), Some(Errno::ECONNABORTED));
    assert_eq!(*done.lock().unwrap(), Some(Err(LookupError::Failed)));
    let capabilities = h.backend.capabilities();
    h.driver = h.backend.reconnect(capabilities).unwrap();
    assert!(h.backend.reconnect(capabilities).is_err());
    h.transport.blocked = false;
    h.transport.commands.clear();
    let (new, _) = h.connect();
    assert_eq!(old.write(b"bad"), Err(Errno::EPIPE));
    assert_eq!(new.write(b"new"), Ok(3));
    assert!(h.pump().is_pending());
    assert_eq!(h.send_request().2, b"new");
    assert!(
        !h.transport
            .commands
            .iter()
            .any(|c| matches!(c, Command::Send { data, .. } if data == b"old"))
    );
    drop(old);
    assert_eq!(new.state(), StreamState::Established);
}
#[test]
fn last_reference_drains_writes_but_unread_data_aborts() {
    let mut h = Harness::new();
    let (s, socket) = h.connect();
    s.write(b"abc").unwrap();
    let held = s.clone();
    drop(s);
    assert!(h.pump().is_pending());
    assert!(
        !h.transport
            .commands
            .iter()
            .any(|c| matches!(c, Command::Close { .. }))
    );
    drop(held);
    h.ack(3);
    assert!(
        h.transport
            .commands
            .iter()
            .any(|c| matches!(c, Command::Close { socket: id } if *id == socket))
    );
    let (s, socket) = h.connect();
    h.event(Event::Data {
        socket,
        data: vec![1],
    });
    drop(s);
    assert!(h.pump().is_pending());
    let reset = h
        .transport
        .commands
        .iter()
        .position(|c| matches!(c, Command::Reset { socket: id } if *id == socket))
        .unwrap();
    let close = h
        .transport
        .commands
        .iter()
        .position(|c| matches!(c, Command::Close { socket: id } if *id == socket))
        .unwrap();
    assert!(reset < close);
}
#[test]
fn receive_saturation_retains_frame_suffix_and_disconnect_still_progresses() {
    let mut h = Harness::new();
    let (s, socket) = h.connect();
    h.event(Event::Data {
        socket,
        data: b"abcdefghijkl".to_vec(),
    });
    assert_eq!(s.input_len(), 8);
    let mut buf = [0; 8];
    assert_eq!(s.read(&mut buf, false), Ok(8));
    assert_eq!(&buf, b"abcdefgh");
    assert!(h.pump().is_pending());
    assert_eq!(s.read(&mut buf, false), Ok(4));
    assert_eq!(&buf[..4], b"ijkl");
    h.event(Event::Data {
        socket,
        data: vec![1; 12],
    });
    h.transport.error = Some(Errno::ENETDOWN);
    assert_eq!(h.pump(), Poll::Ready(Err(Errno::ENETDOWN)));
    assert_eq!(s.take_error(), Some(Errno::ECONNABORTED));
}
#[test]
fn provider_eagain_waits_for_writable_notification_without_busy_retry() {
    let mut h = Harness::new();
    let (s, socket) = h.connect();
    s.write(b"abc").unwrap();
    assert!(h.pump().is_pending());
    let (request, _, _) = h.send_request();
    let count = h.transport.commands.len();
    h.event(Event::SendError {
        request,
        socket,
        error: Errno::EAGAIN,
    });
    assert_eq!(h.transport.commands.len(), count);
    assert!(!s.poll(None).contains(PollMask::OUT));
    h.event(Event::Writable { socket });
    assert_eq!(h.send_request().2, b"abc");
    h.ack(3);
    assert_eq!(s.output_len(), 0);
}
#[test]
fn udp_retains_boundaries_source_peek_truncation_and_empty_datagrams() {
    let mut h = Harness::new();
    let s = h.backend.datagram(Domain::Inet, WaitQueue::new()).unwrap();
    assert_eq!(s.send_to(b"x", peer()), Err(Errno::EAGAIN));
    assert!(h.pump().is_pending());
    let (request, socket) = h
        .transport
        .commands
        .iter()
        .find_map(|c| match c {
            Command::Bind {
                request, socket, ..
            } => Some((*request, *socket)),
            _ => None,
        })
        .unwrap();
    h.event(Event::Complete {
        request,
        result: Ok(()),
    });
    for data in [b"abc".to_vec(), vec![]] {
        h.event(Event::Datagram {
            socket,
            data,
            addr: peer(),
        });
    }
    let mut buf = [0; 2];
    assert_eq!(s.recv_from(&mut buf, true), Ok((2, 3, peer())));
    assert_eq!(&buf, b"ab");
    assert_eq!(s.input_len(), 3);
    assert_eq!(s.recv_from(&mut buf, false), Ok((2, 3, peer())));
    assert!(s.poll(None).contains(PollMask::IN));
    assert_eq!(s.recv_from(&mut buf, false), Ok((0, 0, peer())));
    assert!(!s.poll(None).contains(PollMask::IN));
    s.connect(Some(peer())).unwrap();
    h.event(Event::Datagram {
        socket,
        data: vec![1],
        addr: "192.0.2.2:1".parse().unwrap(),
    });
    assert_eq!(s.recv_from(&mut buf, false), Err(Errno::EAGAIN));
    for _ in 0..8 {
        assert_eq!(s.send_to(&[], peer()), Ok(0));
    }
    assert_eq!(s.send_to(&[], peer()), Err(Errno::EAGAIN));
    assert!(!s.poll(None).contains(PollMask::OUT));
    assert!(h.pump().is_pending());
    assert!(s.poll(None).contains(PollMask::OUT));
    assert_eq!(s.send_to(&[0; 9], peer()), Err(Errno::EMSGSIZE));
}
#[test]
fn provider_capabilities_and_dns_answers_are_explicit_and_bounded() {
    let mut h = Harness::with_capabilities(Capabilities::wisp(false));
    assert!(matches!(
        h.backend.datagram(Domain::Inet, WaitQueue::new()),
        Err(Errno::EOPNOTSUPP)
    ));
    assert!(matches!(
        h.backend
            .connect("[::1]:1".parse().unwrap(), WaitQueue::new()),
        Err(Errno::EAFNOSUPPORT)
    ));
    let calls = Arc::new(AtomicUsize::new(0));
    for lookup in [Lookup::Addr(peer().ip()), Lookup::Forward(vec![])] {
        let calls = calls.clone();
        h.backend.lookup(
            lookup,
            Box::new(move |r| {
                assert_eq!(r, Err(LookupError::Failed));
                calls.fetch_add(1, Ordering::SeqCst);
            }),
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let answer = Arc::new(Mutex::new(None));
    let output = answer.clone();
    let backend = h.backend.clone();
    h.backend.lookup(
        Lookup::Name("a.test".into()),
        Box::new(move |r| {
            // Callback may reenter the backend: it must never run under its mutex.
            assert!(backend.capabilities().dns_name);
            *output.lock().unwrap() = Some(r);
        }),
    );
    assert!(h.pump().is_pending());
    let request = h
        .transport
        .commands
        .iter()
        .find_map(|c| match c {
            Command::Lookup { request, .. } => Some(*request),
            _ => None,
        })
        .unwrap();
    h.event(Event::Lookup {
        request,
        result: Ok(Found {
            answer: Answer::Addrs {
                canonical: String::new(),
                addrs: vec![peer().ip()],
            },
            ttl: 30,
        }),
    });
    assert!(
        matches!(&*answer.lock().unwrap(), Some(Ok(Found { answer: Answer::Addrs { canonical, .. }, .. })) if canonical == "a.test")
    );
    for _ in 0..3 {
        let calls = calls.clone();
        h.backend.lookup(
            Lookup::Name("pending.test".into()),
            Box::new(move |_| {
                calls.fetch_add(1, Ordering::SeqCst);
            }),
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    drop(h.driver);
    assert_eq!(calls.load(Ordering::SeqCst), 5);
}
#[test]
fn resolver_provider_supports_reverse_and_raw_forward_without_host_files() {
    let mut caps = Capabilities::wisp(true);
    caps.dns_reverse = true;
    caps.dns_forward = true;
    let mut h = Harness::with_capabilities(caps);
    for (lookup, expected) in [
        (Lookup::Addr(peer().ip()), Answer::Name("a.test".into())),
        (Lookup::Forward(vec![1, 2]), Answer::Message(vec![1, 2, 3])),
    ] {
        let result = Arc::new(Mutex::new(None));
        let output = result.clone();
        h.backend
            .lookup(lookup, Box::new(move |r| *output.lock().unwrap() = Some(r)));
        assert!(h.pump().is_pending());
        let request = h
            .transport
            .commands
            .iter()
            .rev()
            .find_map(|c| match c {
                Command::Lookup { request, .. } => Some(*request),
                _ => None,
            })
            .unwrap();
        let found = Found {
            answer: expected,
            ttl: 60,
        };
        h.event(Event::Lookup {
            request,
            result: Ok(found.clone()),
        });
        assert_eq!(*result.lock().unwrap(), Some(Ok(found)));
    }
}
struct Sink {
    room: AtomicBool,
    queue: WaitQueue,
    incoming: Mutex<Vec<Arc<dyn BackendStream>>>,
}
impl Sink {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            room: AtomicBool::new(false),
            queue: WaitQueue::new(),
            incoming: Mutex::new(Vec::new()),
        })
    }
}
impl InboundSink for Sink {
    fn poll_room(&self, table: Option<&mut PollTable<'_>>) -> Room {
        if let Some(table) = table {
            table.wait_on(&self.queue);
        }
        if self.room.load(Ordering::SeqCst) {
            Room::Ready
        } else {
            Room::Full
        }
    }
    fn incoming(&self, stream: Arc<dyn BackendStream>, _: WaitQueue) -> bool {
        assert_eq!(stream.read(&mut [0; 1], false), Err(Errno::EAGAIN));
        self.room.store(false, Ordering::SeqCst);
        self.incoming.lock().unwrap().push(stream);
        true
    }
}
#[test]
fn publication_obeys_accept_room_and_drop_closes_late_accepted_children() {
    let mut h = Harness::new();
    let sink = Sink::new();
    let publication = h.backend.publish(peer(), 1, sink.clone()).unwrap();
    assert_eq!(publication.host_addr(), peer());
    assert!(h.pump().is_pending());
    let (request, socket) = h
        .transport
        .commands
        .iter()
        .find_map(|c| match c {
            Command::Listen {
                request, socket, ..
            } => Some((*request, *socket)),
            _ => None,
        })
        .unwrap();
    h.event(Event::Complete {
        request,
        result: Ok(()),
    });
    assert!(
        !h.transport
            .commands
            .iter()
            .any(|c| matches!(c, Command::Accept { .. }))
    );
    sink.room.store(true, Ordering::SeqCst);
    sink.queue.wake(PollMask::OUT);
    assert!(h.pump().is_pending());
    let child = h
        .transport
        .commands
        .iter()
        .find_map(|c| match c {
            Command::Accept { child, .. } => Some(*child),
            _ => None,
        })
        .unwrap();
    h.event(Event::Accepted {
        socket,
        child,
        peer: peer(),
    });
    assert_eq!(sink.incoming.lock().unwrap().len(), 1);
    assert_eq!(sink.incoming.lock().unwrap()[0].peer_addr(), peer());
    assert_eq!(
        h.transport
            .commands
            .iter()
            .filter(|c| matches!(c, Command::Accept { .. }))
            .count(),
        1
    );
    sink.room.store(true, Ordering::SeqCst);
    assert!(h.pump().is_pending());
    let child = h
        .transport
        .commands
        .iter()
        .rev()
        .find_map(|c| match c {
            Command::Accept { child, .. } => Some(*child),
            _ => None,
        })
        .unwrap();
    drop(publication);
    assert!(h.pump().is_pending());
    h.transport.commands.clear();
    h.event(Event::Accepted {
        socket,
        child,
        peer: peer(),
    });
    assert!(
        h.transport
            .commands
            .iter()
            .any(|c| matches!(c, Command::Close { socket } if *socket == child))
    );
}
#[test]
fn publication_failure_is_visible_only_to_embedder() {
    let mut h = Harness::new();
    let sink = Sink::new();
    let publication = h.backend.publish(peer(), 1, sink).unwrap();
    assert!(h.pump().is_pending());
    let request = h
        .transport
        .commands
        .iter()
        .find_map(|c| match c {
            Command::Listen { request, .. } => Some(*request),
            _ => None,
        })
        .unwrap();
    h.event(Event::Complete {
        request,
        result: Err(Errno::EADDRINUSE),
    });
    assert_eq!(
        h.backend.take_publication_error(),
        Some((peer(), Errno::EADDRINUSE))
    );
    assert_eq!(publication.host_addr(), peer());
    drop(publication);
}
#[test]
fn bad_ack_fails_session_and_pending_dns_exactly_once() {
    let mut h = Harness::new();
    let (s, socket) = h.connect();
    s.write(b"abc").unwrap();
    assert!(h.pump().is_pending());
    let (request, _, _) = h.send_request();
    let count = Arc::new(AtomicUsize::new(0));
    let output = count.clone();
    h.backend.lookup(
        Lookup::Name("a.test".into()),
        Box::new(move |r| {
            assert!(r.is_err());
            output.fetch_add(1, Ordering::SeqCst);
        }),
    );
    h.transport.events.push_back(Event::Sent {
        request,
        socket,
        amount: 4,
    });
    assert_eq!(h.pump(), Poll::Ready(Err(Errno::EPROTO)));
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(s.take_error(), Some(Errno::ECONNABORTED));
}
#[test]
fn canceling_run_closes_transport_and_wakes_connect_waiter() {
    use std::{future::Future, pin::pin};
    let h = Harness::new();
    let s = h.backend.connect(peer(), WaitQueue::new()).unwrap();
    let closed = h.transport.closed.clone();
    {
        let mut run = pin!(h.driver.run(h.transport));
        assert!(
            run.as_mut()
                .poll(&mut Context::from_waker(
                    futures_util::task::noop_waker_ref()
                ))
                .is_pending()
        );
    }
    assert!(closed.load(Ordering::SeqCst));
    assert_eq!(s.take_error(), Some(Errno::ECONNABORTED));
}
#[test]
fn socket_and_config_limits_are_enforced() {
    let h = Harness::new();
    let sockets: Vec<_> = (0..8)
        .map(|_| h.backend.connect(peer(), WaitQueue::new()).unwrap())
        .collect();
    assert!(matches!(
        h.backend.connect(peer(), WaitQueue::new()),
        Err(Errno::ENOBUFS)
    ));
    assert_eq!(sockets.len(), 8);
    assert!(
        WebBackend::new(
            Config {
                buffer_bytes: 0,
                ..Config::default()
            },
            Capabilities::classic(true)
        )
        .is_err()
    );
}

#[test]
fn cancel_before_transport_acceptance_never_opens_remote_socket() {
    let mut h = Harness::new();
    h.transport.blocked = true;
    let socket = h.backend.connect(peer(), WaitQueue::new()).unwrap();
    assert!(h.pump().is_pending());
    drop(socket);
    assert!(h.pump().is_pending());
    h.transport.blocked = false;
    assert!(h.pump().is_pending());
    assert!(h.transport.commands.is_empty());
    let _sockets: Vec<_> = (0..8)
        .map(|_| h.backend.connect(peer(), WaitQueue::new()).unwrap())
        .collect();
}
#[test]
fn unread_close_aborts_even_while_an_acknowledgment_is_pending() {
    let mut h = Harness::new();
    let (s, socket) = h.connect();
    s.write(b"abc").unwrap();
    assert!(h.pump().is_pending());
    let (request, _, _) = h.send_request();
    h.event(Event::Data {
        socket,
        data: b"in".to_vec(),
    });
    drop(s);
    assert!(h.pump().is_pending());
    assert!(
        h.transport
            .commands
            .iter()
            .any(|c| matches!(c, Command::Close { socket: id } if *id == socket))
    );
    h.event(Event::Sent {
        request,
        socket,
        amount: 3,
    });
}

#[test]
fn udp_connect_preserves_already_queued_datagrams() {
    let mut h = Harness::new();
    let s = h.backend.datagram(Domain::Inet, WaitQueue::new()).unwrap();
    assert!(h.pump().is_pending());
    let (request, socket) = h
        .transport
        .commands
        .iter()
        .find_map(|c| match c {
            Command::Bind {
                request, socket, ..
            } => Some((*request, *socket)),
            _ => None,
        })
        .unwrap();
    h.event(Event::Complete {
        request,
        result: Ok(()),
    });
    let other = "192.0.2.2:1".parse().unwrap();
    h.event(Event::Datagram {
        socket,
        data: vec![1],
        addr: other,
    });
    s.connect(Some(peer())).unwrap();
    assert_eq!(s.recv_from(&mut [0; 8], false), Ok((1, 1, other)));
    h.event(Event::Datagram {
        socket,
        data: vec![2],
        addr: other,
    });
    assert_eq!(s.recv_from(&mut [0; 8], false), Err(Errno::EAGAIN));
}
#[test]
fn vnet_socket_layer_drives_backend_connect_send_receive_and_shutdown() {
    use wasixv2_abi::MsgFlags;
    use wasixv2_vnet::{Net, NetConfig, Network, SendMsg, SocketKind};
    let mut h = Harness::new();
    let net = Net::new(NetConfig::host_default(), Some(h.backend.clone()));
    let s = net.socket(Domain::Inet, SocketKind::Stream, 0, 1).unwrap();
    assert_eq!(s.connect(peer()), Err(Errno::EINPROGRESS));
    assert!(h.pump().is_pending());
    let (request, socket) = h
        .transport
        .commands
        .iter()
        .find_map(|c| match c {
            Command::Connect {
                request, socket, ..
            } => Some((*request, *socket)),
            _ => None,
        })
        .unwrap();
    h.event(Event::Complete {
        request,
        result: Ok(()),
    });
    assert!(s.poll(None).contains(PollMask::OUT));
    assert_eq!(s.take_error(), None);
    assert_eq!(
        s.send(SendMsg {
            data: b"abc",
            to: None,
            flags: MsgFlags(0),
            control: &[],
            net_raw: false,
            now: std::time::Duration::ZERO
        }),
        Ok(3)
    );
    assert!(h.pump().is_pending());
    h.ack(3);
    h.event(Event::Data {
        socket,
        data: b"reply".to_vec(),
    });
    let mut buf = [0; 8];
    assert_eq!(s.recv(&mut buf, MsgFlags(0)).unwrap().len, 5);
    assert_eq!(&buf[..5], b"reply");
    s.shutdown(Shutdown::Write).unwrap();
    assert!(h.pump().is_pending());
    assert!(h.transport.commands.iter().any(|c| matches!(
        c,
        Command::Shutdown {
            how: Shutdown::Write,
            ..
        }
    )));
}

struct ReenterOnDrop {
    backend: Arc<WebBackend>,
    dropped: Arc<AtomicBool>,
}
impl InboundSink for ReenterOnDrop {
    fn poll_room(&self, _: Option<&mut PollTable<'_>>) -> Room {
        Room::Gone
    }
    fn incoming(&self, _: Arc<dyn BackendStream>, _: WaitQueue) -> bool {
        false
    }
}
impl Drop for ReenterOnDrop {
    fn drop(&mut self) {
        assert!(self.backend.capabilities().tcp);
        self.dropped.store(true, Ordering::SeqCst);
    }
}
#[test]
fn sink_destructors_can_reenter_after_publication_close_and_disconnect() {
    // The timeout detects a lock-order regression without hanging the entire test runner.
    let (done, result) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        for disconnect in [false, true] {
            let mut h = Harness::new();
            let dropped = Arc::new(AtomicBool::new(false));
            let publication = h
                .backend
                .publish(
                    peer(),
                    1,
                    Arc::new(ReenterOnDrop {
                        backend: h.backend.clone(),
                        dropped: dropped.clone(),
                    }),
                )
                .unwrap();
            if disconnect {
                drop(h.driver);
            } else {
                drop(publication);
                assert!(h.pump().is_pending());
            }
            assert!(dropped.load(Ordering::SeqCst));
        }
        done.send(()).unwrap();
    });
    result
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    thread.join().unwrap();
}

struct GoneSink;
impl InboundSink for GoneSink {
    fn poll_room(&self, _: Option<&mut PollTable<'_>>) -> Room {
        Room::Gone
    }
    fn incoming(&self, _: Arc<dyn BackendStream>, _: WaitQueue) -> bool {
        panic!("no guest listener exists")
    }
}
#[test]
fn publication_without_guest_listener_accepts_and_resets_instead_of_stalling() {
    let mut h = Harness::new();
    let _publication = h.backend.publish(peer(), 1, Arc::new(GoneSink)).unwrap();
    assert!(h.pump().is_pending());
    let (request, socket) = h
        .transport
        .commands
        .iter()
        .find_map(|c| match c {
            Command::Listen {
                request, socket, ..
            } => Some((*request, *socket)),
            _ => None,
        })
        .unwrap();
    h.event(Event::Complete {
        request,
        result: Ok(()),
    });
    let child = h
        .transport
        .commands
        .iter()
        .find_map(|c| match c {
            Command::Accept { child, .. } => Some(*child),
            _ => None,
        })
        .unwrap();
    h.event(Event::Accepted {
        socket,
        child,
        peer: peer(),
    });
    assert!(
        h.transport
            .commands
            .iter()
            .any(|c| matches!(c, Command::Reset { socket } if *socket == child))
    );
    assert!(
        h.transport
            .commands
            .iter()
            .any(|c| matches!(c, Command::Close { socket } if *socket == child))
    );
}

#[test]
fn read_shutdown_preserves_queued_bytes_and_the_established_tcp_state() {
    let mut h = Harness::new();
    let (s, socket) = h.connect();
    h.event(Event::Data {
        socket,
        data: b"abcdefghijkl".to_vec(),
    });
    s.shutdown(Shutdown::Read).unwrap();
    assert!(s.tcp_established(), "SHUT_RD sends no FIN");
    let mut buf = [0; 8];
    assert_eq!(s.read(&mut buf, false), Ok(8));
    assert_eq!(&buf, b"abcdefgh");
    assert!(h.pump().is_pending());
    assert_eq!(s.read(&mut buf, false), Ok(4));
    assert_eq!(&buf[..4], b"ijkl");
    assert_eq!(s.read(&mut buf, false), Ok(0));
    assert_eq!(s.write(b"out"), Ok(3));
}
#[test]
fn close_resets_when_unread_bytes_remain_in_the_driver_input_slot() {
    let mut h = Harness::new();
    let (s, socket) = h.connect();
    h.event(Event::Data {
        socket,
        data: b"abcdefghijkl".to_vec(),
    });
    assert_eq!(s.read(&mut [0; 8], false), Ok(8));
    drop(s);
    assert!(h.pump().is_pending());
    assert!(
        h.transport
            .commands
            .iter()
            .any(|c| matches!(c, Command::Reset { socket: id } if *id == socket))
    );
}

#[test]
fn fidelity_urgent_marks_preserve_order_peek_inline_and_bounded_writes() {
    let mut h = Harness::with_capabilities(Capabilities::classic_fidelity(true));
    let (stream, socket) = h.connect();
    assert_eq!(stream.write(b"ab"), Ok(2));
    assert_eq!(stream.write_urgent(b"c!"), Ok(2));
    assert_eq!(stream.write(b"de"), Ok(2));
    assert!(h.pump().is_pending());
    assert_eq!(h.send_request().2, b"abc");
    h.ack(3);
    let (request, data) = h
        .transport
        .commands
        .iter()
        .find_map(|c| match c {
            Command::SendUrgent { request, data, .. } => Some((*request, data.clone())),
            _ => None,
        })
        .unwrap();
    assert_eq!(data, b"!");
    h.event(Event::Sent {
        request,
        socket,
        amount: 1,
    });
    assert_eq!(h.send_request().2, b"de");
    h.ack(2);
    h.event(Event::Urgent {
        socket,
        data: b"abc!".to_vec(),
    });
    h.event(Event::Data {
        socket,
        data: b"de".to_vec(),
    });
    assert!(stream.poll(None).contains(PollMask::PRI));
    let mut buf = [0; 8];
    assert_eq!(stream.read(&mut buf, false), Ok(3));
    assert_eq!(&buf[..3], b"abc");
    assert!(stream.at_mark());
    assert_eq!(stream.read_urgent(true), Ok(b'!'));
    assert_eq!(stream.read_urgent(false), Ok(b'!'));
    assert!(!stream.poll(None).contains(PollMask::PRI));
    assert_eq!(stream.read(&mut buf, false), Ok(2));
    assert_eq!(&buf[..2], b"de");
    stream
        .set_option(
            wasixv2_abi::SocketLevel::SOL_SOCKET.0,
            wasixv2_abi::SocketOption::SO_OOBINLINE.0,
            &1i32.to_le_bytes(),
        )
        .unwrap();
    h.event(Event::Urgent {
        socket,
        data: vec![b'?'],
    });
    assert_eq!(stream.read_urgent(false), Err(Errno::EINVAL));
    assert_eq!(stream.read(&mut buf, false), Ok(1));
    assert_eq!(buf[0], b'?');
    assert_eq!(stream.write_urgent(b"123456789"), Ok(8));
    assert_eq!(stream.write_urgent(b"!"), Err(Errno::EAGAIN));
}

#[test]
fn published_udp_drops_unattached_packets_resets_attachment_and_preserves_errors() {
    let mut h = Harness::with_capabilities(Capabilities::classic_fidelity(true));
    let (endpoint, publication) = h.backend.publish_datagram(peer()).unwrap();
    assert_eq!(publication.host_addr(), peer());
    assert!(h.pump().is_pending());
    let (request, socket) = h
        .transport
        .commands
        .iter()
        .find_map(|c| match c {
            Command::Bind {
                request,
                socket,
                addr,
            } => {
                assert_eq!(*addr, peer());
                Some((*request, *socket))
            }
            _ => None,
        })
        .unwrap();
    h.event(Event::Complete {
        request,
        result: Ok(()),
    });
    h.event(Event::Datagram {
        socket,
        data: vec![],
        addr: peer(),
    });
    assert!(!endpoint.poll(None).contains(PollMask::READABLE));
    endpoint.attach(Some(WaitQueue::new())).unwrap();
    h.event(Event::Datagram {
        socket,
        data: vec![],
        addr: peer(),
    });
    assert_eq!(endpoint.recv_from(&mut [], true), Ok((0, 0, peer())));
    assert_eq!(endpoint.recv_from(&mut [], false), Ok((0, 0, peer())));
    assert_eq!(endpoint.recv_from(&mut [], false), Err(Errno::EAGAIN));
    h.event(Event::DatagramError {
        socket,
        error: Errno::EMSGSIZE,
    });
    assert!(endpoint.poll(None).contains(PollMask::ERR));
    assert_eq!(endpoint.take_error(), Some(Errno::EMSGSIZE));
    assert_eq!(endpoint.send_to(b"ok", peer()), Ok(2));
    h.event(Event::Datagram {
        socket,
        data: b"stale".to_vec(),
        addr: peer(),
    });
    endpoint.attach(None).unwrap();
    endpoint.attach(Some(WaitQueue::new())).unwrap();
    assert_eq!(endpoint.recv_from(&mut [0; 8], false), Err(Errno::EAGAIN));
    drop(endpoint);
    assert!(h.pump().is_pending());
    assert!(
        !h.transport
            .commands
            .iter()
            .any(|c| matches!(c,Command::Close{socket:id} if *id==socket))
    );
    drop(publication);
    assert!(h.pump().is_pending());
    assert!(
        h.transport
            .commands
            .iter()
            .any(|c| matches!(c,Command::Close{socket:id} if *id==socket))
    );
    assert!(
        h.backend
            .publish_datagram("127.0.0.1:0".parse().unwrap())
            .is_err()
    );
    let stock = Harness::new();
    assert!(stock.backend.publish_datagram(peer()).is_err());
}

#[test]
fn dropping_udp_drains_accepted_datagrams_before_close() {
    let mut h = Harness::new();
    let endpoint = h.backend.datagram(Domain::Inet, WaitQueue::new()).unwrap();
    assert!(h.pump().is_pending());
    let (request, socket) = h
        .transport
        .commands
        .iter()
        .find_map(|c| match c {
            Command::Bind {
                request, socket, ..
            } => Some((*request, *socket)),
            _ => None,
        })
        .unwrap();
    h.event(Event::Complete {
        request,
        result: Ok(()),
    });
    assert_eq!(endpoint.send_to(b"last", peer()), Ok(4));
    drop(endpoint);
    assert!(h.pump().is_pending());
    let send = h
        .transport
        .commands
        .iter()
        .position(
            |c| matches!(c,Command::SendTo {socket:id,data,..} if *id==socket && data==b"last"),
        )
        .unwrap();
    let close = h
        .transport
        .commands
        .iter()
        .position(|c| matches!(c,Command::Close {socket:id} if *id==socket))
        .unwrap();
    assert!(send < close);
}

#[test]
fn publication_rechecks_a_listener_created_after_accept_credit() {
    struct LaterListener {
        ready: AtomicBool,
        accepted: AtomicUsize,
    }
    impl InboundSink for LaterListener {
        fn poll_room(&self, _: Option<&mut PollTable<'_>>) -> Room {
            if self.ready.load(Ordering::SeqCst) {
                Room::Ready
            } else {
                Room::Gone
            }
        }
        fn incoming(&self, _: Arc<dyn BackendStream>, _: WaitQueue) -> bool {
            self.accepted.fetch_add(1, Ordering::SeqCst);
            true
        }
    }
    let mut h = Harness::new();
    let sink = Arc::new(LaterListener {
        ready: AtomicBool::new(false),
        accepted: AtomicUsize::new(0),
    });
    let _publication = h.backend.publish(peer(), 1, sink.clone()).unwrap();
    assert!(h.pump().is_pending());
    let (request, socket) = h
        .transport
        .commands
        .iter()
        .find_map(|c| match c {
            Command::Listen {
                request, socket, ..
            } => Some((*request, *socket)),
            _ => None,
        })
        .unwrap();
    h.event(Event::Complete {
        request,
        result: Ok(()),
    });
    let child = h
        .transport
        .commands
        .iter()
        .find_map(|c| match c {
            Command::Accept { child, .. } => Some(*child),
            _ => None,
        })
        .unwrap();
    sink.ready.store(true, Ordering::SeqCst);
    h.event(Event::Accepted {
        socket,
        child,
        peer: peer(),
    });
    assert_eq!(sink.accepted.load(Ordering::SeqCst), 1);
    assert!(
        !h.transport
            .commands
            .iter()
            .any(|c| matches!(c,Command::Reset {socket:id} if *id==child))
    );
}

#[test]
fn skipping_an_urgent_only_full_queue_wakes_the_input_driver() {
    use futures_util::task::{ArcWake, waker_ref};
    struct Wake(AtomicUsize);
    impl ArcWake for Wake {
        fn wake_by_ref(this: &Arc<Self>) {
            this.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let caps = Capabilities::classic_fidelity(false);
    let mut h = Harness::with_capabilities(caps);
    let (backend, driver) = WebBackend::new(
        Config {
            max_sockets: 8,
            max_lookups: 2,
            buffer_bytes: 1,
            chunk_bytes: 1,
        },
        caps,
    )
    .unwrap();
    h.backend = backend;
    h.driver = driver;
    let (stream, socket) = h.connect();
    h.event(Event::Urgent {
        socket,
        data: vec![b'!'],
    });
    h.transport.events.push_back(Event::Data {
        socket,
        data: vec![b'a'],
    });
    let wake = Arc::new(Wake(AtomicUsize::new(0)));
    let waker = waker_ref(&wake);
    let mut cx = Context::from_waker(&waker);
    assert!(h.driver.poll(&mut cx, &mut h.transport).is_pending());
    wake.0.store(0, Ordering::SeqCst);
    assert_eq!(stream.read(&mut [0; 1], false), Err(Errno::EAGAIN));
    assert!(
        wake.0.load(Ordering::SeqCst) > 0,
        "skipping the urgent byte released receive capacity"
    );
    assert!(h.pump().is_pending());
    let mut data = [0; 1];
    assert_eq!(stream.read(&mut data, false), Ok(1));
    assert_eq!(data[0], b'a');
}
