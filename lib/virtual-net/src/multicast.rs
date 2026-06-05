use crate::{NetworkError, Result};
use std::collections::{HashMap, VecDeque};
use std::mem::MaybeUninit;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex, Weak};
use std::task::Waker;
use virtual_mio::{InterestHandler, InterestType};

const MULTICAST_RING_CAPACITY: usize = 1024;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(crate) struct MulticastKey {
    addr: IpAddr,
    port: u16,
}

impl MulticastKey {
    pub(crate) fn new(addr: IpAddr, port: u16) -> Self {
        Self { addr, port }
    }

    pub(crate) fn from_socket_addr(addr: SocketAddr) -> Option<Self> {
        match addr.ip() {
            IpAddr::V4(ip) if ip.is_multicast() => Some(Self {
                addr: IpAddr::V4(ip),
                port: addr.port(),
            }),
            IpAddr::V6(ip) if ip.is_multicast() => Some(Self {
                addr: IpAddr::V6(ip),
                port: addr.port(),
            }),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
struct MulticastPacket {
    seq: u64,
    data: Arc<[u8]>,
    from: SocketAddr,
}

#[derive(Debug, Default)]
struct MulticastGroup {
    next_seq: u64,
    packets: VecDeque<MulticastPacket>,
    members: HashMap<u64, Weak<LocalUdpSocketShared>>,
}

#[derive(Debug, Default)]
pub(crate) struct MulticastCoordinator {
    groups: HashMap<MulticastKey, MulticastGroup>,
}

impl MulticastCoordinator {
    pub(crate) fn join(&mut self, socket: &Arc<LocalUdpSocketShared>, key: MulticastKey) {
        let group = self.groups.entry(key).or_default();
        group.members.insert(socket.id, Arc::downgrade(socket));
        socket
            .multicast_reads
            .lock()
            .unwrap()
            .insert(key, group.next_seq);
    }

    pub(crate) fn leave(&mut self, socket: &Arc<LocalUdpSocketShared>, key: MulticastKey) {
        if let Some(group) = self.groups.get_mut(&key) {
            group.members.remove(&socket.id);
            if group.members.is_empty() {
                self.groups.remove(&key);
            }
        }
        socket.multicast_reads.lock().unwrap().remove(&key);
    }

    pub(crate) fn send(
        &mut self,
        sender: &Arc<LocalUdpSocketShared>,
        data: &[u8],
        from: SocketAddr,
        to: SocketAddr,
    ) -> Vec<Arc<LocalUdpSocketShared>> {
        let Some(key) = MulticastKey::from_socket_addr(to) else {
            return Vec::new();
        };
        let Some(group) = self.groups.get_mut(&key) else {
            return Vec::new();
        };

        let packet = MulticastPacket {
            seq: group.next_seq,
            data: Arc::from(data),
            from,
        };
        group.next_seq = group.next_seq.wrapping_add(1);
        group.packets.push_back(packet);
        while group.packets.len() > MULTICAST_RING_CAPACITY {
            group.packets.pop_front();
        }

        let mut stale = Vec::new();
        let mut subscribers = Vec::new();
        for (&id, member) in &group.members {
            let Some(member) = member.upgrade() else {
                stale.push(id);
                continue;
            };
            if Arc::ptr_eq(sender, &member) && !member.multicast_loop_for(key.addr) {
                if let Some(cursor) = member.multicast_reads.lock().unwrap().get_mut(&key) {
                    *cursor = group.next_seq;
                }
                continue;
            }
            subscribers.push(member);
        }
        for id in stale {
            group.members.remove(&id);
        }
        subscribers
    }

    pub(crate) fn next_packet_len(&mut self, socket: &Arc<LocalUdpSocketShared>) -> Option<usize> {
        let mut reads = socket.multicast_reads.lock().unwrap();
        for (key, cursor) in reads.iter_mut() {
            let Some(group) = self.groups.get(key) else {
                continue;
            };
            if !group.members.contains_key(&socket.id) {
                continue;
            }
            let Some(front) = group.packets.front() else {
                continue;
            };
            if *cursor < front.seq {
                *cursor = front.seq;
            }
            if let Some(packet) = group.packets.iter().find(|packet| packet.seq >= *cursor) {
                return Some(packet.data.len());
            }
        }
        None
    }

    pub(crate) fn recv(
        &mut self,
        socket: &Arc<LocalUdpSocketShared>,
        buf: &mut [MaybeUninit<u8>],
        peek: bool,
    ) -> Result<(usize, SocketAddr)> {
        let mut reads = socket.multicast_reads.lock().unwrap();
        for (key, cursor) in reads.iter_mut() {
            let Some(group) = self.groups.get(key) else {
                continue;
            };
            if !group.members.contains_key(&socket.id) {
                continue;
            }
            let Some(front) = group.packets.front() else {
                continue;
            };
            if *cursor < front.seq {
                *cursor = front.seq;
            }
            let Some(packet) = group.packets.iter().find(|packet| packet.seq >= *cursor) else {
                continue;
            };

            let amt = buf.len().min(packet.data.len());
            let buf: &mut [u8] = unsafe { std::mem::transmute(buf) };
            buf[..amt].copy_from_slice(&packet.data[..amt]);
            if !peek {
                *cursor = packet.seq.wrapping_add(1);
            }
            return Ok((amt, packet.from));
        }
        Err(NetworkError::WouldBlock)
    }
}

pub(crate) struct LocalUdpSocketShared {
    id: u64,
    multicast_reads: Mutex<HashMap<MulticastKey, u64>>,
    multicast_loop_v4: Mutex<bool>,
    multicast_loop_v6: Mutex<bool>,
    read_wakers: Mutex<Vec<Waker>>,
    handler: Mutex<Option<Box<dyn InterestHandler + Send + Sync>>>,
}

impl std::fmt::Debug for LocalUdpSocketShared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalUdpSocketShared")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl LocalUdpSocketShared {
    pub(crate) fn new(id: u64) -> Self {
        Self {
            id,
            multicast_reads: Default::default(),
            multicast_loop_v4: Mutex::new(true),
            multicast_loop_v6: Mutex::new(true),
            read_wakers: Default::default(),
            handler: Default::default(),
        }
    }

    pub(crate) fn joined_keys(&self) -> Vec<MulticastKey> {
        self.multicast_reads
            .lock()
            .unwrap()
            .keys()
            .copied()
            .collect()
    }

    pub(crate) fn set_multicast_loop_v4(&self, val: bool) {
        *self.multicast_loop_v4.lock().unwrap() = val;
    }

    pub(crate) fn multicast_loop_v4(&self) -> bool {
        *self.multicast_loop_v4.lock().unwrap()
    }

    pub(crate) fn set_multicast_loop_v6(&self, val: bool) {
        *self.multicast_loop_v6.lock().unwrap() = val;
    }

    pub(crate) fn multicast_loop_v6(&self) -> bool {
        *self.multicast_loop_v6.lock().unwrap()
    }

    pub(crate) fn clear_read_wakers(&self) {
        self.read_wakers.lock().unwrap().clear();
    }

    pub(crate) fn set_handler(&self, handler: Option<Box<dyn InterestHandler + Send + Sync>>) {
        *self.handler.lock().unwrap() = handler;
    }

    pub(crate) fn add_read_waker(&self, waker: &Waker) {
        let mut wakers = self.read_wakers.lock().unwrap();
        if !wakers.iter().any(|existing| existing.will_wake(waker)) {
            wakers.push(waker.clone());
        }
    }

    fn multicast_loop_for(&self, addr: IpAddr) -> bool {
        match addr {
            IpAddr::V4(_) => self.multicast_loop_v4(),
            IpAddr::V6(_) => self.multicast_loop_v6(),
        }
    }

    pub(crate) fn notify_readable(&self) {
        for waker in self.read_wakers.lock().unwrap().drain(..) {
            waker.wake();
        }
        if let Some(handler) = self.handler.lock().unwrap().as_mut() {
            handler.push_interest(InterestType::Readable);
        }
    }
}

#[derive(Debug)]
pub(crate) struct LocalUdpSocketInterestHandler {
    shared: Arc<LocalUdpSocketShared>,
}

impl LocalUdpSocketInterestHandler {
    pub(crate) fn new(shared: Arc<LocalUdpSocketShared>) -> Self {
        Self { shared }
    }
}

impl InterestHandler for LocalUdpSocketInterestHandler {
    fn push_interest(&mut self, interest: InterestType) {
        if let Some(handler) = self.shared.handler.lock().unwrap().as_mut() {
            handler.push_interest(interest);
        }
    }

    fn pop_interest(&mut self, interest: InterestType) -> bool {
        self.shared
            .handler
            .lock()
            .unwrap()
            .as_mut()
            .map(|handler| handler.pop_interest(interest))
            .unwrap_or(false)
    }

    fn has_interest(&self, interest: InterestType) -> bool {
        self.shared
            .handler
            .lock()
            .unwrap()
            .as_ref()
            .map(|handler| handler.has_interest(interest))
            .unwrap_or(false)
    }
}
