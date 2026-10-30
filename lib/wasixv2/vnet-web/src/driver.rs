use crate::{
    backend::{Flow, Kind, Listener, Packet, Shared, Stream, unbound},
    transport::{Command, Event, Transport},
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
    task::{Context, Poll},
};
use wasixv2_abi::Errno;
use wasixv2_vnet::{Answer, Lookup, LookupError, Room, Shutdown, StreamState};
use wasixv2_wait::{PollMask, PollTable, WaitEntry, WaitQueue, WaitRegistration};

enum Pending {
    Open(u64),
    Address(u64, bool),
    Listen(u64),
    Send(u64, usize),
    Shutdown(u64),
}
struct DriverWake(Arc<Shared>);
impl WaitEntry for DriverWake {
    fn wake(&self, _: PollMask) -> bool {
        self.0.wake.wake();
        true
    }
}

/// One session's event pump. It contains no JS references and may be moved to the I/O worker
/// before polling. The transport itself stays local to that worker. Dropping this driver
/// cancels every pending operation of this session and wakes all affected socket waiters.
pub struct Driver {
    shared: Arc<Shared>,
    generation: u64,
    pending: BTreeMap<u64, Pending>,
    command: Option<Command>,
    registrations: Vec<WaitRegistration>,
    cleanup: VecDeque<Command>,
    retired: Vec<Listener>,
    cursor: u64,
    stopped: bool,
    input: Option<(u64, Vec<u8>, usize, bool)>,
}
impl Driver {
    pub(crate) fn new(shared: Arc<Shared>, generation: u64) -> Self {
        Self {
            shared,
            generation,
            pending: BTreeMap::new(),
            command: None,
            registrations: Vec::new(),
            cleanup: VecDeque::new(),
            retired: Vec::new(),
            cursor: 0,
            stopped: false,
            input: None,
        }
    }

    /// Owns the worker-local transport so cancellation also closes its JS resources.
    pub async fn run(mut self, transport: impl Transport) -> Result<(), Errno> {
        struct Close<T: Transport>(T);
        impl<T: Transport> Drop for Close<T> {
            fn drop(&mut self) {
                self.0.close();
            }
        }
        let mut transport = Close(transport);
        futures_util::future::poll_fn(|cx| self.poll(cx, &mut transport.0)).await
    }

    /// Runs until transport failure. There is no automatic reconnection or retransmission
    /// across sessions. Bounded work per poll yields to the worker's event loop.
    pub fn poll(
        &mut self,
        cx: &mut Context<'_>,
        transport: &mut dyn Transport,
    ) -> Poll<Result<(), Errno>> {
        if self.stopped {
            return Poll::Ready(Ok(()));
        }
        self.shared.wake.register(cx.waker());
        if self.shared.inner.lock().unwrap().capabilities != transport.capabilities() {
            return self.stop(transport, Errno::EINVAL);
        }
        for _ in 0..64 {
            let mut progress = false;
            if let Poll::Ready(error) = transport.poll_closed(cx) {
                return self.stop(transport, error);
            }
            if self.input.is_none() {
                match transport.poll_recv(cx) {
                    Poll::Ready(Ok(event)) => {
                        if let Event::Data { socket, data } = event {
                            if data.len() > crate::codec::MAX_FRAME {
                                return self.stop(transport, Errno::EMSGSIZE);
                            }
                            self.input = Some((socket, data, 0, false));
                        } else if let Event::Urgent { socket, data } = event {
                            if data.is_empty() || data.len() > crate::codec::MAX_FRAME {
                                return self.stop(transport, Errno::EPROTO);
                            }
                            self.input = Some((socket, data, 0, true));
                        } else if let Err(error) = self.event(event) {
                            return self.stop(transport, error);
                        }
                        progress = true;
                    }
                    Poll::Ready(Err(error)) => return self.stop(transport, error),
                    Poll::Pending => {}
                }
            }
            match self.copy_input() {
                Ok(copied) => progress |= copied,
                Err(error) => return self.stop(transport, error),
            }
            if self.command.is_none() {
                match self.next(cx) {
                    Ok(command) => self.command = command,
                    Err(error) => return self.stop(transport, error),
                }
            }
            // Trait-object destructors may reenter the backend, just like sink callbacks.
            self.retired.clear();
            if !self.command_valid() {
                self.discard_command();
                progress = true;
            }
            if let Some(command) = &self.command {
                match transport.poll_send(cx, command) {
                    Poll::Ready(Ok(())) => {
                        let command = self.command.take().unwrap();
                        self.sent(command);
                        progress = true;
                    }
                    Poll::Ready(Err(error)) => return self.stop(transport, error),
                    Poll::Pending => {}
                }
            }
            if !progress {
                return Poll::Pending;
            }
        }
        cx.waker().wake_by_ref();
        Poll::Pending
    }

    fn stop(&mut self, transport: &mut dyn Transport, error: Errno) -> Poll<Result<(), Errno>> {
        transport.close();
        // Transport failure does not prove a peer RST. Linux net/ipv4/tcp.c:tcp_done's
        // asynchronous error path is represented by ECONNABORTED for lost infrastructure.
        self.disconnect();
        Poll::Ready(Err(error))
    }

    fn next(&mut self, cx: &mut Context<'_>) -> Result<Option<Command>, Errno> {
        self.registrations.clear();
        if let Some(command) = self.cleanup.pop_front() {
            return Ok(Some(command));
        }
        let listeners: Vec<_> = {
            let inner = self.shared.inner.lock().unwrap();
            inner
                .listeners
                .iter()
                .filter(|(_, l)| l.ready && !l.released && l.child.is_none())
                .map(|(&id, l)| (id, l.sink.clone()))
                .collect()
        };
        let entry: Arc<dyn WaitEntry> = Arc::new(DriverWake(self.shared.clone()));
        let mut accepting = BTreeSet::new();
        for (id, sink) in listeners {
            let mut register = |q: &WaitQueue, _| self.registrations.push(q.add(entry.clone()));
            let room = sink.poll_room(Some(&mut PollTable::new(PollMask::empty(), &mut register)));
            // Like vnet-host's listener, accept and reset when no guest listener exists;
            // only a full accept queue leaves the connection in the relay backlog.
            if room != Room::Full {
                accepting.insert(id);
            }
        }
        // Registration precedes checking the predicate. A room/guest-I/O wake before this
        // point is retained by AtomicWaker; no kernel/backend lock spans a transport call.
        self.shared.wake.register(cx.waker());
        let mut inner = self.shared.inner.lock().unwrap();
        if let Some((&request, query)) = inner.lookups.iter_mut().find(|(_, q)| !q.sent) {
            query.sent = true;
            return Ok(Some(Command::Lookup {
                request,
                lookup: query.lookup.clone(),
            }));
        }
        let listener_ids: Vec<_> = inner.listeners.keys().copied().collect();
        for id in listener_ids {
            let listener = &inner.listeners[&id];
            if listener.released {
                if listener.opening {
                    continue;
                }
                let listener = inner.listeners.remove(&id).unwrap();
                if let Some(child) = listener.child
                    && let Some(flow) = inner.flows.get_mut(&child)
                {
                    flow.released = true;
                    flow.reset = true;
                    // This child was reserved, not yet delivered to a guest.
                    flow.state = StreamState::Closed;
                    flow.opening = false;
                }
                self.retired.push(listener);
                return Ok(Some(Command::Close { socket: id }));
            }
            if !listener.opening && !listener.ready {
                let addr = listener.addr;
                let request = inner.id()?;
                inner.listeners.get_mut(&id).unwrap().opening = true;
                self.pending.insert(request, Pending::Listen(id));
                return Ok(Some(Command::Listen {
                    request,
                    socket: id,
                    addr,
                }));
            }
            if accepting.contains(&id)
                && listener.child.is_none()
                && inner.flows.len() + inner.listeners.len() < self.shared.config.max_sockets
            {
                let child = inner.id()?;
                let mut flow = Flow::new(
                    Kind::Stream,
                    unbound(wasixv2_vnet::Domain::Inet),
                    WaitQueue::new(),
                );
                flow.opening = true;
                flow.remote = true;
                inner.flows.insert(child, flow);
                let listener = inner.listeners.get_mut(&id).unwrap();
                listener.child = Some(child);
                return Ok(Some(Command::Accept { socket: id, child }));
            }
        }
        // Round-robin selection keeps one busy stream from starving other endpoints.
        let ids: Vec<_> = inner
            .flows
            .range((
                std::ops::Bound::Excluded(self.cursor),
                std::ops::Bound::Unbounded,
            ))
            .chain(inner.flows.range(..=self.cursor))
            .map(|(&id, _)| id)
            .collect();
        for id in ids {
            let request = inner.id()?;
            let flow = inner.flows.get_mut(&id).unwrap();
            let command = if flow.released || flow.reset {
                if flow.reset {
                    if let Some((request, _)) = flow.inflight.take() {
                        self.pending.remove(&request);
                    }
                    flow.tx.clear();
                }
                if flow.opening || flow.inflight.is_some() {
                    continue;
                }
                if flow.reset && !flow.reset_sent && flow.remote {
                    flow.reset_sent = true;
                    Some(Command::Reset { socket: id })
                } else if flow.reset || (flow.tx.is_empty() && flow.outgoing.is_empty()) {
                    if !flow.remote {
                        if flow.released {
                            inner.flows.remove(&id);
                        }
                        continue;
                    }
                    // No wrapper still references a released endpoint. Reset endpoints with
                    // a live wrapper stay queryable after their remote socket is closed.
                    flow.remote = false;
                    let released = flow.released;
                    if released {
                        inner.flows.remove(&id);
                    }
                    Some(Command::Close { socket: id })
                } else {
                    None
                }
            } else {
                None
            };
            if command.is_some() {
                self.cursor = id;
                return Ok(command);
            }
            let Some(flow) = inner.flows.get_mut(&id) else {
                continue;
            };
            if flow.reset || flow.state == StreamState::Closed {
                continue;
            }
            if flow.state == StreamState::Connecting && !flow.opening && !flow.released {
                flow.opening = true;
                flow.remote = true;
                self.pending.insert(request, Pending::Open(id));
                self.cursor = id;
                return Ok(Some(match flow.kind {
                    Kind::Stream => Command::Connect {
                        request,
                        socket: id,
                        peer: flow.peer,
                    },
                    Kind::Datagram(_) => Command::Bind {
                        request,
                        socket: id,
                        addr: flow.peer,
                    },
                }));
            }
            if flow.state != StreamState::Established {
                continue;
            }
            if flow.kind != Kind::Stream {
                if let Some(packet) = flow.outgoing.front() {
                    self.cursor = id;
                    return Ok(Some(Command::SendTo {
                        socket: id,
                        data: packet.data.clone(),
                        addr: packet.addr,
                    }));
                }
                continue;
            }
            if flow.inflight.is_some() || flow.send_blocked {
                continue;
            }
            if !flow.tx.is_empty() {
                let mut size = flow.tx.len().min(self.shared.config.chunk_bytes);
                let urgent = flow
                    .tx_urgent
                    .front()
                    .is_some_and(|mark| *mark == flow.tx_head);
                if let Some(mark) = flow.tx_urgent.front() {
                    size = size.min(if urgent {
                        1
                    } else {
                        (*mark - flow.tx_head) as usize
                    });
                }
                let data = flow.tx.iter().take(size).copied().collect();
                flow.inflight = Some((request, size));
                self.pending.insert(request, Pending::Send(id, size));
                self.cursor = id;
                return Ok(Some(if urgent {
                    Command::SendUrgent {
                        request,
                        socket: id,
                        data,
                    }
                } else {
                    Command::Send {
                        request,
                        socket: id,
                        data,
                    }
                }));
            }
            let read = flow.read_shut && !flow.sent_read_shut;
            let write = flow.write_shut && !flow.sent_write_shut;
            if read || write {
                flow.sent_read_shut |= read;
                flow.sent_write_shut |= write;
                flow.inflight = Some((request, 0));
                self.pending.insert(request, Pending::Shutdown(id));
                self.cursor = id;
                return Ok(Some(Command::Shutdown {
                    request,
                    socket: id,
                    how: match (read, write) {
                        (true, true) => Shutdown::Both,
                        (true, false) => Shutdown::Read,
                        _ => Shutdown::Write,
                    },
                }));
            }
        }
        Ok(None)
    }

    fn copy_input(&mut self) -> Result<bool, Errno> {
        let Some((socket, data, offset, urgent)) = &mut self.input else {
            return Ok(false);
        };
        let mut inner = self.shared.inner.lock().unwrap();
        let Some(flow) = inner.flows.get_mut(socket) else {
            self.input = None;
            return Ok(true);
        };
        if flow.kind != Kind::Stream {
            return Err(Errno::EPROTO);
        }
        if flow.released {
            // Linux tcp_close counts unread bytes, including bytes already received from
            // the relay but still waiting for room in the endpoint's receive queue.
            flow.reset |= *offset < data.len();
            self.input = None;
            return Ok(true);
        }
        if flow.peer_fin || flow.state == StreamState::Closed {
            self.input = None;
            return Ok(true);
        }
        let n = (data.len() - *offset).min(self.shared.config.buffer_bytes - flow.rx.len());
        let bytes = &data[*offset..*offset + n];
        let marked = *urgent && n != 0 && *offset + n == data.len();
        if marked {
            flow.rx.push_urgent(bytes, flow.oob_inline);
        } else {
            flow.rx.push(bytes);
        }
        *offset += n;
        if *offset == data.len() {
            self.input = None;
        }
        let wq = flow.wq.clone();
        drop(inner);
        if n != 0 {
            wq.wake(
                PollMask::READABLE
                    | if marked {
                        PollMask::PRI
                    } else {
                        PollMask::empty()
                    },
            );
        }
        Ok(n != 0)
    }

    fn command_valid(&self) -> bool {
        let inner = self.shared.inner.lock().unwrap();
        match &self.command {
            Some(
                Command::Send {
                    request, socket, ..
                }
                | Command::Shutdown {
                    request, socket, ..
                }
                | Command::SendUrgent {
                    request, socket, ..
                },
            ) => inner
                .flows
                .get(socket)
                .is_some_and(|f| f.inflight.is_some_and(|(id, _)| id == *request) && !f.reset),
            Some(Command::Connect { socket, .. } | Command::Bind { socket, .. }) => inner
                .flows
                .get(socket)
                .is_some_and(|f| f.state == StreamState::Connecting && !f.released),
            Some(Command::Listen { socket, .. }) => {
                inner.listeners.get(socket).is_some_and(|l| !l.released)
            }
            Some(Command::Lookup { request, .. }) => inner.lookups.contains_key(request),
            _ => true,
        }
    }

    fn discard_command(&mut self) {
        let command = self.command.take();
        let mut inner = self.shared.inner.lock().unwrap();
        match command {
            Some(
                Command::Connect {
                    request, socket, ..
                }
                | Command::Bind {
                    request, socket, ..
                },
            ) => {
                self.pending.remove(&request);
                if let Some(flow) = inner.flows.get_mut(&socket) {
                    flow.opening = false;
                    flow.remote = false;
                    if flow.released {
                        inner.flows.remove(&socket);
                    }
                }
            }
            Some(Command::Listen {
                request, socket, ..
            }) => {
                self.pending.remove(&request);
                if let Some(listener) = inner.listeners.remove(&socket) {
                    self.retired.push(listener);
                }
            }
            Some(
                Command::Send {
                    request, socket, ..
                }
                | Command::Shutdown {
                    request, socket, ..
                }
                | Command::SendUrgent {
                    request, socket, ..
                },
            ) => {
                self.pending.remove(&request);
                if let Some(flow) = inner.flows.get_mut(&socket) {
                    flow.inflight = None;
                }
            }
            _ => {}
        }
    }

    fn sent(&mut self, command: Command) {
        if let Command::SendTo { socket, .. } = command {
            let wq = {
                let mut inner = self.shared.inner.lock().unwrap();
                inner.flows.get_mut(&socket).map(|flow| {
                    if let Some(packet) = flow.outgoing.pop_front() {
                        flow.outgoing_bytes -= packet.data.len().max(1);
                    }
                    flow.wq.clone()
                })
            };
            if let Some(wq) = wq {
                wq.wake(PollMask::WRITABLE);
            }
        }
    }

    fn event(&mut self, event: Event) -> Result<(), Errno> {
        let mut binding_wakes = Vec::new();
        let mut address_query = None;
        let mut wakes = Vec::new();
        let mut complete = None;
        let mut incoming = None;
        let mut inner = self.shared.inner.lock().unwrap();
        match event {
            Event::Complete { request, result } => {
                if let Some(query) = inner.lookups.remove(&request) {
                    complete = Some((
                        query.done,
                        Err(crate::codec::lookup_error(
                            result.err().unwrap_or(Errno::EPROTO),
                        )),
                    ));
                } else if let Some(pending) = self.pending.remove(&request) {
                    match pending {
                        Pending::Open(id) => {
                            if let Some(flow) = inner.flows.get_mut(&id) {
                                flow.opening = false;
                                match result {
                                    Ok(()) if flow.binding.is_some() => {
                                        flow.opening = true;
                                        address_query = Some((id, true));
                                    }
                                    Ok(()) => flow.state = StreamState::Established,
                                    Err(error) => {
                                        if let Some(binding) = flow.binding.take() {
                                            binding_wakes.extend(binding.complete(Err(error)));
                                        }
                                        if flow.published {
                                            self.publication_failure(flow.peer, error);
                                        }
                                        flow.fail(error)
                                    }
                                }
                                wakes.push((flow.wq.clone(), flow.mask(self.shared.config)));
                            }
                        }
                        Pending::Listen(id) => {
                            if let Some(listener) = inner.listeners.get_mut(&id) {
                                listener.opening = false;
                                listener.ready = result.is_ok();
                                if result.is_ok() && listener.binding.is_some() {
                                    listener.opening = true;
                                    listener.ready = false;
                                    address_query = Some((id, false));
                                }
                                // A failed publication never affects guest bind/listen. Keep
                                // the failure visible to the embedding driver caller.
                                if let Err(error) = result {
                                    listener.released = true;
                                    if let Some(binding) = listener.binding.take() {
                                        binding_wakes.extend(binding.complete(Err(error)));
                                    }
                                    self.publication_failure(listener.addr, error);
                                }
                            }
                        }
                        Pending::Shutdown(id) => {
                            if let Some(flow) = inner.flows.get_mut(&id) {
                                flow.inflight = None;
                                if let Err(error) = result {
                                    flow.fail(error);
                                }
                                wakes.push((flow.wq.clone(), flow.mask(self.shared.config)));
                            }
                        }
                        Pending::Address(id, udp) => {
                            let error = result.err().ok_or(Errno::EPROTO)?;
                            self.bound(&mut inner, id, udp, Err(error), &mut binding_wakes)?;
                        }
                        Pending::Send(_, _) => return Err(Errno::EPROTO),
                    }
                }
            }
            Event::LocalAddr { request, addr } => {
                if let Some(Pending::Address(id, udp)) = self.pending.remove(&request) {
                    self.bound(&mut inner, id, udp, Ok(addr), &mut binding_wakes)?;
                } else {
                    return Err(Errno::EPROTO);
                }
            }
            Event::Lookup {
                request,
                mut result,
            } => {
                if let Some(query) = inner.lookups.get(&request) {
                    if let Ok(found) = &mut result {
                        match (&query.lookup, &mut found.answer) {
                            (Lookup::Name(name), Answer::Addrs { canonical, addrs }) => {
                                if addrs.len() > 256 || canonical.len() > 253 {
                                    return Err(Errno::EPROTO);
                                }
                                if canonical.is_empty() {
                                    *canonical = name.clone();
                                }
                            }
                            (Lookup::Addr(_), Answer::Name(name)) if name.len() <= 253 => {}
                            (Lookup::Forward(_), Answer::Message(message))
                                if message.len() <= 65535 => {}
                            _ => return Err(Errno::EPROTO),
                        }
                    }
                    let query = inner.lookups.remove(&request).unwrap();
                    complete = Some((query.done, result));
                }
            }
            Event::Sent {
                request,
                socket,
                amount,
            } => {
                if let Some(Pending::Send(id, size)) = self.pending.remove(&request) {
                    if id != socket || amount == 0 || amount > size {
                        return Err(Errno::EPROTO);
                    }
                    if let Some(flow) = inner.flows.get_mut(&id) {
                        if flow.inflight != Some((request, size)) {
                            return Err(Errno::EPROTO);
                        }
                        flow.inflight = None;
                        flow.tx.drain(..amount);
                        flow.tx_head += amount as u64;
                        while flow
                            .tx_urgent
                            .front()
                            .is_some_and(|mark| *mark < flow.tx_head)
                        {
                            flow.tx_urgent.pop_front();
                        }
                        wakes.push((flow.wq.clone(), PollMask::WRITABLE));
                    }
                }
            }
            Event::SendError {
                request,
                socket,
                error,
            } => {
                if let Some(Pending::Send(id, _)) = self.pending.remove(&request) {
                    if id != socket {
                        return Err(Errno::EPROTO);
                    }
                    if let Some(flow) = inner.flows.get_mut(&id) {
                        flow.inflight = None;
                        if error == Errno::EAGAIN {
                            flow.send_blocked = true;
                        } else {
                            flow.fail(error);
                            flow.reset = true;
                        }
                        wakes.push((flow.wq.clone(), flow.mask(self.shared.config)));
                    }
                }
            }
            Event::Data { .. } | Event::Urgent { .. } => {
                unreachable!("data is copied through the bounded input slot")
            }
            Event::DatagramError { socket, error } => {
                if let Some(flow) = inner.flows.get_mut(&socket) {
                    if flow.kind == Kind::Stream {
                        return Err(Errno::EPROTO);
                    }
                    if flow.attached {
                        flow.error = Some(error);
                        wakes.push((flow.wq.clone(), PollMask::ERR));
                    }
                }
            }
            Event::DatagramRefused {
                socket,
                destination,
                offender,
                payload,
            } => {
                if !inner.capabilities.udp_icmp
                    || payload.len() > 65535
                    || offender.is_some_and(|a| a.is_ipv4() != destination.is_ipv4())
                {
                    return Err(Errno::EPROTO);
                }
                if let Some(flow) = inner.flows.get_mut(&socket) {
                    let Kind::Datagram(domain) = flow.kind else {
                        return Err(Errno::EPROTO);
                    };
                    if (domain == wasixv2_vnet::Domain::Inet) != destination.is_ipv4() {
                        return Err(Errno::EPROTO);
                    }
                    // Linux net/ipv4/udp.c:__udp4_lib_err (now udp_err) looks up the
                    // quoted tuple, then reports hard errors only to connected sockets
                    // unless RECVERR is enabled. A stale peer's error cannot hit a new peer.
                    if flow.attached
                        && !flow.released
                        && flow.connected_peer.is_none_or(|p| p == destination)
                    {
                        let changed = flow.datagram_errors.refused(
                            destination,
                            offender,
                            &payload,
                            flow.connected_peer.is_some(),
                            self.shared.config.buffer_bytes,
                        );
                        if changed {
                            wakes.push((flow.wq.clone(), PollMask::ERR));
                        }
                    }
                }
            }
            Event::Writable { socket } => {
                if let Some(flow) = inner.flows.get_mut(&socket) {
                    flow.send_blocked = false;
                    wakes.push((flow.wq.clone(), PollMask::WRITABLE));
                }
            }
            Event::Datagram { socket, data, addr } => {
                if let Some(flow) = inner.flows.get_mut(&socket) {
                    let Kind::Datagram(domain) = flow.kind else {
                        return Err(Errno::EPROTO);
                    };
                    if data.len() > 65535 {
                        return Err(Errno::EMSGSIZE);
                    }
                    // Linux net/ipv4/udp.c:__udp_enqueue_schedule_skb drops excess datagrams.
                    if (domain == wasixv2_vnet::Domain::Inet) == addr.is_ipv4()
                        && flow.connected_peer.is_none_or(|peer| peer == addr)
                        && flow.incoming_bytes + data.len().max(1)
                            <= self.shared.config.buffer_bytes
                        && flow.attached
                        && !flow.released
                        && flow.state == StreamState::Established
                    {
                        flow.incoming_bytes += data.len().max(1);
                        flow.incoming.push_back(Packet { data, addr });
                        wakes.push((flow.wq.clone(), PollMask::READABLE));
                    }
                }
            }
            Event::Eof { socket } => {
                if let Some(flow) = inner.flows.get_mut(&socket) {
                    flow.read_shut = true;
                    flow.peer_fin = true;
                    // A peer FIN is not a local SHUT_RD request.
                    flow.sent_read_shut = true;
                    wakes.push((flow.wq.clone(), flow.mask(self.shared.config)));
                }
            }
            Event::Reset { socket, error } => {
                if let Some(flow) = inner.flows.get_mut(&socket) {
                    flow.fail(error);
                    flow.reset = true;
                    self.pending.retain(|_, p| !matches!(p, Pending::Open(id) | Pending::Send(id, _) | Pending::Shutdown(id) if *id == socket));
                    wakes.push((flow.wq.clone(), flow.mask(self.shared.config)));
                }
            }
            Event::Accepted {
                socket,
                child,
                peer,
            } => {
                let sink = if let Some(listener) = inner.listeners.get_mut(&socket) {
                    if listener.child != Some(child) {
                        return Err(Errno::EPROTO);
                    }
                    listener.child = None;
                    if listener.released {
                        None
                    } else {
                        Some(listener.sink.clone())
                    }
                } else {
                    None
                };
                if let Some(flow) = inner.flows.get_mut(&child) {
                    flow.opening = false;
                    flow.peer = peer;
                    flow.state = StreamState::Established;
                    if let Some(sink) = sink {
                        incoming = Some((sink, child, peer, flow.wq.clone()));
                    } else {
                        flow.released = true;
                        flow.reset = true;
                    }
                } else {
                    if self.cleanup.len() >= self.shared.config.max_sockets * 2 {
                        return Err(Errno::ENOBUFS);
                    }
                    self.cleanup.push_back(Command::Reset { socket: child });
                    self.cleanup.push_back(Command::Close { socket: child });
                }
            }
        }
        if let Some((socket, udp)) = address_query {
            let request = inner.id()?;
            self.pending.insert(request, Pending::Address(socket, udp));
            self.cleanup
                .push_back(Command::LocalAddr { request, socket });
        }
        drop(inner);
        for waker in binding_wakes {
            waker.wake();
        }
        for (wq, mask) in wakes {
            wq.wake(mask);
        }
        if let Some((done, result)) = complete {
            done(result);
        }
        if let Some((sink, id, peer, wq)) = incoming {
            let stream = Arc::new(Stream {
                shared: self.shared.clone(),
                id,
                peer,
            });
            // Linux tcp_v4_syn_recv_sock/inet_csk_accept use current listener state.
            // Accept credit may predate listen; no callback runs under the backend mutex.
            if sink.poll_room(None) != Room::Ready || !sink.incoming(stream.clone(), wq) {
                let mut inner = self.shared.inner.lock().unwrap();
                if let Some(flow) = inner.flows.get_mut(&id) {
                    flow.reset = true;
                }
            }
        }
        Ok(())
    }

    fn bound(
        &self,
        inner: &mut crate::backend::Inner,
        id: u64,
        udp: bool,
        result: Result<std::net::SocketAddr, Errno>,
        wakes: &mut Vec<std::task::Waker>,
    ) -> Result<(), Errno> {
        let (requested, binding) = if udp {
            let Some(flow) = inner.flows.get_mut(&id) else {
                return Ok(());
            };
            (flow.peer, flow.binding.clone())
        } else {
            let Some(listener) = inner.listeners.get_mut(&id) else {
                return Ok(());
            };
            (listener.addr, listener.binding.clone())
        };
        let Some(binding) = binding else {
            return Err(Errno::EPROTO);
        };
        if let Ok(addr) = result
            && (addr.port() == 0
                || addr.is_ipv4() != requested.is_ipv4()
                || (requested.port() != 0 && requested.port() != addr.port())
                || (!requested.ip().is_unspecified() && requested.ip() != addr.ip()))
        {
            return Err(Errno::EPROTO);
        }
        if udp {
            let flow = inner.flows.get_mut(&id).unwrap();
            flow.binding = None;
            flow.opening = false;
            match result {
                Ok(addr) => {
                    flow.peer = addr;
                    flow.state = StreamState::Established;
                }
                Err(error) => flow.fail(error),
            }
        } else {
            let listener = inner.listeners.get_mut(&id).unwrap();
            listener.binding = None;
            listener.opening = false;
            match result {
                Ok(addr) => {
                    listener.addr = addr;
                    listener.ready = true;
                }
                Err(_) => listener.released = true,
            }
        }
        if let Err(error) = result {
            self.publication_failure(requested, error);
        }
        wakes.extend(binding.complete(result));
        Ok(())
    }

    fn publication_failure(&self, addr: std::net::SocketAddr, error: Errno) {
        // Returned through the driver, never as a guest socket error.
        let mut failures = self.shared.failures.lock().unwrap();
        if failures.len() == self.shared.config.max_sockets {
            failures.pop_front();
        }
        failures.push_back((addr, error));
    }

    fn disconnect(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        self.command = None;
        self.input = None;
        self.pending.clear();
        self.registrations.clear();
        let mut inner = self.shared.inner.lock().unwrap();
        if inner.generation != self.generation {
            return;
        }
        inner.online = false;
        let mut queues = Vec::new();
        let mut binding_wakes = Vec::new();
        for flow in inner.flows.values_mut() {
            if let Some(binding) = flow.binding.take() {
                binding_wakes.extend(binding.complete(Err(Errno::ECONNABORTED)));
            }
            if flow.state != StreamState::Closed {
                flow.fail(Errno::ECONNABORTED);
                queues.push(flow.wq.clone());
            }
            flow.remote = false;
            flow.opening = false;
        }
        inner.flows.retain(|_, flow| !flow.released);
        let listeners = std::mem::take(&mut inner.listeners);
        for listener in listeners.values() {
            if let Some(binding) = &listener.binding {
                binding_wakes.extend(binding.complete(Err(Errno::ECONNABORTED)));
            }
        }
        let queries = std::mem::take(&mut inner.lookups);
        drop(inner);
        drop(listeners);
        for waker in binding_wakes {
            waker.wake();
        }
        for wq in queues {
            wq.wake(PollMask::ERR | PollMask::HUP | PollMask::READABLE | PollMask::WRITABLE);
        }
        for (_, query) in queries {
            (query.done)(Err(LookupError::Failed));
        }
    }
}
impl Drop for Driver {
    fn drop(&mut self) {
        self.disconnect();
    }
}
