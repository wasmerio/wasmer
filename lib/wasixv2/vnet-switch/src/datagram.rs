use crate::SwitchBackend;
use std::{
    collections::VecDeque,
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use wasixv2_abi::Errno;
use wasixv2_vnet::{
    BackendDatagram, DatagramErrors, NetResult, PollMask, PollTable, Pollable, Publication,
    QueuedError, WaitQueue,
};

struct State {
    queue: VecDeque<(Vec<u8>, SocketAddr)>,
    bytes: usize,
    peer: Option<SocketAddr>,
    errors: DatagramErrors,
    wq: Option<WaitQueue>,
}
pub(super) struct Datagram {
    port: Arc<SwitchBackend>,
    pub address: SocketAddr,
    published: bool,
    state: Mutex<State>,
}
impl Datagram {
    pub fn new(
        port: Arc<SwitchBackend>,
        address: SocketAddr,
        wq: Option<WaitQueue>,
        published: bool,
    ) -> Arc<Self> {
        Arc::new(Self {
            port,
            address,
            published,
            state: Mutex::new(State {
                queue: VecDeque::new(),
                bytes: 0,
                peer: None,
                errors: DatagramErrors::default(),
                wq,
            }),
        })
    }
    fn incoming(&self, data: &[u8], from: SocketAddr) -> bool {
        let mut state = self.state.lock().unwrap();
        // Linux net/ipv4/udp.c:__udp4_lib_lookup/udp_queue_rcv_one_skb. An
        // unbound publication acts like a closed port; a full socket drops silently.
        if state.wq.is_none() || state.peer.is_some_and(|p| p != from) {
            return false;
        }
        if state.bytes + data.len() + 768 > self.port.switch.config.datagram_bytes {
            return true;
        }
        state.queue.push_back((data.to_vec(), from));
        state.bytes += data.len() + 768;
        let wq = state.wq.clone();
        drop(state);
        if let Some(wq) = wq {
            wq.wake(PollMask::IN | PollMask::RDNORM);
        }
        true
    }
}
impl Pollable for Datagram {
    fn poll(&self, _: Option<&mut PollTable<'_>>) -> PollMask {
        let state = self.state.lock().unwrap();
        let mut mask = PollMask::OUT | PollMask::WRNORM;
        if !state.queue.is_empty() {
            mask |= PollMask::IN | PollMask::RDNORM;
        }
        if state.errors.has_error() {
            mask |= PollMask::ERR;
        }
        mask
    }
}
impl BackendDatagram for Datagram {
    fn send_to(&self, data: &[u8], to: SocketAddr) -> NetResult<usize> {
        if to.is_ipv4() != self.address.is_ipv4() {
            return Err(Errno::EAFNOSUPPORT);
        }
        if data.len() > if to.is_ipv4() { 65507 } else { 65527 } {
            return Err(Errno::EMSGSIZE);
        }
        if let Some(error) = self.state.lock().unwrap().errors.take_error() {
            return Err(error);
        }
        let target = self
            .port
            .switch
            .destination(to.ip())
            .ok_or(Errno::EHOSTUNREACH)?;
        let endpoint = target
            .state
            .lock()
            .unwrap()
            .datagrams
            .get(&to.port())
            .and_then(std::sync::Weak::upgrade);
        if !endpoint.is_some_and(|e| e.incoming(data, self.address)) {
            // udp_sendmsg accepted this packet; the ICMP error affects the NEXT I/O.
            let mut state = self.state.lock().unwrap();
            let connected = state.peer == Some(to);
            let changed = state.errors.refused(
                to,
                Some(SocketAddr::new(to.ip(), 0)),
                data,
                connected,
                self.port.switch.config.datagram_bytes,
            );
            let wake = state.wq.clone().filter(|_| changed);
            drop(state);
            if let Some(wq) = wake {
                wq.wake(PollMask::ERR);
            }
        }
        Ok(data.len())
    }
    fn connect(&self, peer: Option<SocketAddr>) -> NetResult<()> {
        if peer.is_some_and(|p| p.is_ipv4() != self.address.is_ipv4()) {
            return Err(Errno::EAFNOSUPPORT);
        }
        self.state.lock().unwrap().peer = peer;
        Ok(())
    }
    fn take_error(&self) -> Option<Errno> {
        self.state.lock().unwrap().errors.take_error()
    }
    fn recv_from(&self, buf: &mut [u8], peek: bool) -> NetResult<(usize, usize, SocketAddr)> {
        let mut state = self.state.lock().unwrap();
        if let Some(e) = state.errors.take_error() {
            return Err(e);
        }
        let (data, from) = state.queue.front().ok_or(Errno::EAGAIN)?;
        let n = data.len().min(buf.len());
        buf[..n].copy_from_slice(&data[..n]);
        let result = (n, data.len(), *from);
        if !peek {
            state.bytes -= data.len() + 768;
            state.queue.pop_front();
        }
        Ok(result)
    }
    fn input_len(&self) -> usize {
        self.state
            .lock()
            .unwrap()
            .queue
            .front()
            .map_or(0, |(d, _)| d.len())
    }
    fn set_option(&self, level: i32, name: i32, value: &[u8]) -> NetResult<()> {
        self.state
            .lock()
            .unwrap()
            .errors
            .set_option(level, name, value)
    }
    fn take_queued_error(&self) -> Option<QueuedError> {
        self.state.lock().unwrap().errors.take_queued_error()
    }
    fn attach(&self, wq: Option<WaitQueue>) -> NetResult<()> {
        if !self.published {
            return Err(Errno::EOPNOTSUPP);
        }
        *self.state.lock().unwrap() = State {
            queue: VecDeque::new(),
            bytes: 0,
            peer: None,
            errors: DatagramErrors::default(),
            wq,
        };
        Ok(())
    }
}
pub(super) struct PublicationHandle {
    pub endpoint: Arc<Datagram>,
    pub addr: SocketAddr,
}
impl Publication for PublicationHandle {
    fn host_addr(&self) -> SocketAddr {
        let _ = &self.endpoint;
        self.addr
    }
}
