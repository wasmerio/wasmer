use crate::{Lease, Published};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex, Weak},
};
use wasixv2_abi::Errno;
use wasixv2_vnet::{
    BackendStream, MemoryStream, NetResult, PollMask, PollTable, Pollable, Room, Shutdown,
    StreamState, WaitQueue,
};
use wasixv2_wait::{WaitEntry, WaitRegistration};

struct Forward(WaitQueue);
impl WaitEntry for Forward {
    fn wake(&self, _: PollMask) -> bool {
        self.0.wake(PollMask::empty());
        true
    }
}
struct Pending {
    listener: Weak<Published>,
    accepted: Arc<MemoryStream>,
    queue: WaitQueue,
    registrations: Vec<WaitRegistration>,
}
struct State {
    phase: StreamState,
    error: Option<Errno>,
    pending: Option<Pending>,
}
pub(super) struct Stream {
    inner: Arc<MemoryStream>,
    state: Mutex<State>,
    queue: WaitQueue,
    _lease: Arc<Lease>,
}
impl Stream {
    pub fn new(
        source: SocketAddr,
        peer: SocketAddr,
        queue: WaitQueue,
        listener: Weak<Published>,
        lease: Arc<Lease>,
    ) -> Arc<Self> {
        let accepted_queue = WaitQueue::new();
        let [inner, accepted] =
            MemoryStream::pair([source, peer], [queue.clone(), accepted_queue.clone()]);
        let stream = Arc::new(Self {
            inner,
            queue,
            _lease: lease,
            state: Mutex::new(State {
                phase: StreamState::Connecting,
                error: None,
                pending: Some(Pending {
                    listener,
                    accepted,
                    queue: accepted_queue,
                    registrations: Vec::new(),
                }),
            }),
        });
        stream.progress();
        stream
    }
    fn progress(&self) {
        let Some(mut pending) = self.state.lock().unwrap().pending.take() else {
            return;
        };
        pending.registrations.clear();
        let entry: Arc<dyn WaitEntry> = Arc::new(Forward(self.queue.clone()));
        let mut register = |q: &WaitQueue, _| pending.registrations.push(q.add(entry.clone()));
        let listener = pending.listener.upgrade();
        // Linux net/ipv4/tcp_ipv4.c:tcp_v4_syn_recv_sock leaves a SYN pending while
        // sk_acceptq_is_full. Register first; retry from the socket poll after a room wake.
        let room = listener.as_ref().map_or(Room::Gone, |l| {
            l.sink
                .poll_room(Some(&mut PollTable::new(PollMask::empty(), &mut register)))
        });
        let phase = match room {
            Room::Full => {
                self.state.lock().unwrap().pending = Some(pending);
                return;
            }
            Room::Ready
                if listener
                    .as_ref()
                    .unwrap()
                    .sink
                    .incoming(pending.accepted.clone(), pending.queue.clone()) =>
            {
                StreamState::Established
            }
            Room::Ready => {
                self.state.lock().unwrap().pending = Some(pending);
                self.queue.wake(PollMask::empty());
                return;
            }
            Room::Gone => StreamState::Closed,
        };
        let mut state = self.state.lock().unwrap();
        state.phase = phase;
        if phase == StreamState::Closed {
            state.error = Some(Errno::ECONNREFUSED);
        }
        drop(state);
        drop(pending);
        self.queue.wake(PollMask::empty());
    }
    fn ready(&self) -> NetResult<()> {
        self.progress();
        let phase = self.state.lock().unwrap().phase;
        match phase {
            StreamState::Connecting => Err(Errno::EAGAIN),
            StreamState::Closed => Err(self.take_error().unwrap_or(Errno::EPIPE)),
            StreamState::Established => Ok(()),
        }
    }
}
impl Pollable for Stream {
    fn poll(&self, _: Option<&mut PollTable<'_>>) -> PollMask {
        match self.state() {
            StreamState::Connecting => PollMask::empty(),
            StreamState::Closed => {
                PollMask::HUP
                    | PollMask::OUT
                    | PollMask::IN
                    | if self.state.lock().unwrap().error.is_some() {
                        PollMask::ERR
                    } else {
                        PollMask::empty()
                    }
            }
            StreamState::Established => self.inner.poll(None),
        }
    }
}
impl BackendStream for Stream {
    fn state(&self) -> StreamState {
        self.progress();
        let state = self.state.lock().unwrap();
        if state.phase == StreamState::Established {
            self.inner.state()
        } else {
            state.phase
        }
    }
    fn tcp_established(&self) -> bool {
        self.state() == StreamState::Established && self.inner.tcp_established()
    }
    fn read(&self, buf: &mut [u8], peek: bool) -> NetResult<usize> {
        self.ready()?;
        self.inner.read(buf, peek)
    }
    fn write(&self, data: &[u8]) -> NetResult<usize> {
        self.ready()?;
        self.inner.write(data)
    }
    fn write_more(&self, data: &[u8], more: bool, mss: usize) -> NetResult<usize> {
        self.ready()?;
        self.inner.write_more(data, more, mss)
    }
    fn push(&self) {
        self.inner.push();
    }
    fn corked_len(&self) -> usize {
        self.inner.corked_len()
    }
    fn write_urgent(&self, data: &[u8]) -> NetResult<usize> {
        self.ready()?;
        self.inner.write_urgent(data)
    }
    fn read_urgent(&self, peek: bool) -> NetResult<u8> {
        self.ready()?;
        self.inner.read_urgent(peek)
    }
    fn at_mark(&self) -> bool {
        self.inner.at_mark()
    }
    fn shutdown(&self, how: Shutdown) -> NetResult<()> {
        self.inner.shutdown(how)
    }
    fn take_error(&self) -> Option<Errno> {
        self.state
            .lock()
            .unwrap()
            .error
            .take()
            .or_else(|| self.inner.take_error())
    }
    fn peer_addr(&self) -> SocketAddr {
        self.inner.peer_addr()
    }
    fn input_len(&self) -> usize {
        self.inner.input_len()
    }
    fn output_len(&self) -> usize {
        self.inner.output_len()
    }
    fn set_option(&self, level: i32, name: i32, value: &[u8]) -> NetResult<()> {
        self.inner.set_option(level, name, value)
    }
}
