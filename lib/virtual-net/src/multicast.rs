use crate::{NetworkError, Result};
use crossbeam_queue::SegQueue;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::mem::MaybeUninit;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::task::Waker;
use virtual_mio::{InterestHandler, InterestType};

const MULTICAST_SEG_PACKET_CAPACITY: usize = 64;
const MULTICAST_READY_SEG_CAPACITY: usize = 1024;

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
    data: Arc<[u8]>,
    from: SocketAddr,
}

#[derive(Debug)]
struct PacketSeg {
    len: usize,
    packets: [Option<MulticastPacket>; MULTICAST_SEG_PACKET_CAPACITY],
}

impl PacketSeg {
    fn new() -> Self {
        Self {
            len: 0,
            packets: std::array::from_fn(|_| None),
        }
    }

    fn push(&mut self, packet: MulticastPacket) {
        debug_assert!(self.len < MULTICAST_SEG_PACKET_CAPACITY);
        self.packets[self.len] = Some(packet);
        self.len += 1;
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn packet(&self, index: usize) -> Option<MulticastPacket> {
        if index >= self.len {
            return None;
        }
        self.packets[index].clone()
    }
}

#[derive(Debug)]
struct MulticastReadState {
    read_seg: Option<Arc<PacketSeg>>,
    read_index: usize,
    queued_segs: usize,
    ready_segs: SegQueue<Arc<PacketSeg>>,
}

impl Default for MulticastReadState {
    fn default() -> Self {
        Self {
            read_seg: None,
            read_index: 0,
            queued_segs: 0,
            ready_segs: SegQueue::new(),
        }
    }
}

impl MulticastReadState {
    fn enqueue(&mut self, seg: Arc<PacketSeg>) {
        while self.queued_segs >= MULTICAST_READY_SEG_CAPACITY {
            if self.ready_segs.pop().is_some() {
                self.queued_segs -= 1;
            } else {
                self.queued_segs = 0;
                break;
            }
        }
        self.ready_segs.push(seg);
        self.queued_segs += 1;
    }

    fn ensure_read_seg(&mut self) -> Option<()> {
        loop {
            if let Some(seg) = &self.read_seg
                && self.read_index < seg.len
            {
                return Some(());
            }
            self.read_seg = None;
            let seg = self.ready_segs.pop()?;
            self.queued_segs = self.queued_segs.saturating_sub(1);
            self.read_index = 0;
            if !seg.is_empty() {
                self.read_seg = Some(seg);
            }
        }
    }

    fn next_packet_len(&mut self) -> Option<usize> {
        self.ensure_read_seg()?;
        self.read_seg
            .as_ref()?
            .packet(self.read_index)
            .map(|packet| packet.data.len())
    }

    fn recv(&mut self, peek: bool) -> Option<MulticastPacket> {
        self.ensure_read_seg()?;
        let packet = self.read_seg.as_ref()?.packet(self.read_index)?;
        if !peek {
            self.read_index += 1;
        }
        Some(packet)
    }
}

#[derive(Debug)]
struct MulticastGroup {
    write_seg: Mutex<Box<PacketSeg>>,
    members: Mutex<HashMap<u64, Weak<LocalUdpSocketShared>>>,
}

impl Default for MulticastGroup {
    fn default() -> Self {
        Self {
            write_seg: Mutex::new(Box::new(PacketSeg::new())),
            members: Default::default(),
        }
    }
}

impl MulticastGroup {
    fn publish(&self, data: &[u8], from: SocketAddr) -> Option<Arc<PacketSeg>> {
        let packet = MulticastPacket {
            data: Arc::from(data),
            from,
        };

        let mut write_seg = self.write_seg.lock();
        write_seg.push(packet);

        // UDP readiness must become visible after each send, so seal the current
        // segment immediately. Later batching can seal only on full/timer flush.
        let sealed = std::mem::replace(&mut *write_seg, Box::new(PacketSeg::new()));
        if sealed.is_empty() {
            None
        } else {
            Some(Arc::from(sealed))
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct MulticastCoordinator {
    groups: Mutex<HashMap<MulticastKey, Arc<MulticastGroup>>>,
}

impl MulticastCoordinator {
    pub(crate) fn join(&self, socket: &Arc<LocalUdpSocketShared>, key: MulticastKey) {
        let group = self.groups.lock().entry(key).or_default().clone();
        group
            .members
            .lock()
            .insert(socket.id, Arc::downgrade(socket));
        socket.join_multicast(key);
    }

    pub(crate) fn leave(&self, socket: &Arc<LocalUdpSocketShared>, key: MulticastKey) {
        let group = self.groups.lock().get(&key).cloned();
        if let Some(group) = group {
            let is_empty = {
                let mut members = group.members.lock();
                members.remove(&socket.id);
                members.is_empty()
            };
            if is_empty {
                self.groups.lock().remove(&key);
            }
        }
        socket.leave_multicast(key);
    }

    pub(crate) fn send(
        &self,
        sender: &Arc<LocalUdpSocketShared>,
        data: &[u8],
        from: SocketAddr,
        to: SocketAddr,
    ) -> Vec<Arc<LocalUdpSocketShared>> {
        let Some(key) = MulticastKey::from_socket_addr(to) else {
            return Vec::new();
        };
        let Some(group) = self.groups.lock().get(&key).cloned() else {
            return Vec::new();
        };

        let Some(seg) = group.publish(data, from) else {
            return Vec::new();
        };

        let mut stale = Vec::new();
        let mut subscribers = Vec::new();
        {
            let members = group.members.lock();
            for (&id, member) in members.iter() {
                let Some(member) = member.upgrade() else {
                    stale.push(id);
                    continue;
                };
                if Arc::ptr_eq(sender, &member) && !member.multicast_loop_for(key.addr) {
                    continue;
                }
                member.enqueue_multicast_segment(key, seg.clone());
                subscribers.push(member);
            }
        }
        if !stale.is_empty() {
            let mut members = group.members.lock();
            for id in stale {
                if members.get(&id).and_then(Weak::upgrade).is_none() {
                    members.remove(&id);
                }
            }
        }
        subscribers
    }

    pub(crate) fn next_packet_len(&self, socket: &Arc<LocalUdpSocketShared>) -> Option<usize> {
        socket.next_multicast_packet_len()
    }

    pub(crate) fn recv(
        &self,
        socket: &Arc<LocalUdpSocketShared>,
        buf: &mut [MaybeUninit<u8>],
        peek: bool,
    ) -> Result<(usize, SocketAddr)> {
        let Some(packet) = socket.recv_multicast_packet(peek) else {
            return Err(NetworkError::WouldBlock);
        };

        let amt = buf.len().min(packet.data.len());
        let buf: &mut [u8] = unsafe { std::mem::transmute(buf) };
        buf[..amt].copy_from_slice(&packet.data[..amt]);
        Ok((amt, packet.from))
    }
}

pub(crate) struct LocalUdpSocketShared {
    id: u64,
    multicast_reads: Mutex<HashMap<MulticastKey, MulticastReadState>>,
    multicast_loop_v4: AtomicBool,
    multicast_loop_v6: AtomicBool,
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
            multicast_loop_v4: AtomicBool::new(true),
            multicast_loop_v6: AtomicBool::new(true),
            read_wakers: Default::default(),
            handler: Default::default(),
        }
    }

    pub(crate) fn joined_keys(&self) -> Vec<MulticastKey> {
        self.multicast_reads.lock().keys().copied().collect()
    }

    fn join_multicast(&self, key: MulticastKey) {
        self.multicast_reads
            .lock()
            .entry(key)
            .or_insert_with(MulticastReadState::default);
    }

    fn leave_multicast(&self, key: MulticastKey) {
        self.multicast_reads.lock().remove(&key);
    }

    fn enqueue_multicast_segment(&self, key: MulticastKey, seg: Arc<PacketSeg>) {
        if let Some(state) = self.multicast_reads.lock().get_mut(&key) {
            state.enqueue(seg);
        }
    }

    fn next_multicast_packet_len(&self) -> Option<usize> {
        for state in self.multicast_reads.lock().values_mut() {
            if let Some(len) = state.next_packet_len() {
                return Some(len);
            }
        }
        None
    }

    fn recv_multicast_packet(&self, peek: bool) -> Option<MulticastPacket> {
        for state in self.multicast_reads.lock().values_mut() {
            if let Some(packet) = state.recv(peek) {
                return Some(packet);
            }
        }
        None
    }

    pub(crate) fn set_multicast_loop_v4(&self, val: bool) {
        self.multicast_loop_v4.store(val, Ordering::Relaxed);
    }

    pub(crate) fn multicast_loop_v4(&self) -> bool {
        self.multicast_loop_v4.load(Ordering::Relaxed)
    }

    pub(crate) fn set_multicast_loop_v6(&self, val: bool) {
        self.multicast_loop_v6.store(val, Ordering::Relaxed);
    }

    pub(crate) fn multicast_loop_v6(&self) -> bool {
        self.multicast_loop_v6.load(Ordering::Relaxed)
    }

    pub(crate) fn clear_read_wakers(&self) {
        self.read_wakers.lock().clear();
    }

    pub(crate) fn set_handler(&self, handler: Option<Box<dyn InterestHandler + Send + Sync>>) {
        *self.handler.lock() = handler;
    }

    pub(crate) fn add_read_waker(&self, waker: &Waker) {
        let mut wakers = self.read_wakers.lock();
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
        for waker in self.read_wakers.lock().drain(..) {
            waker.wake();
        }
        if let Some(handler) = self.handler.lock().as_mut() {
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
        if let Some(handler) = self.shared.handler.lock().as_mut() {
            handler.push_interest(interest);
        }
    }

    fn pop_interest(&mut self, interest: InterestType) -> bool {
        self.shared
            .handler
            .lock()
            .as_mut()
            .map(|handler| handler.pop_interest(interest))
            .unwrap_or(false)
    }

    fn has_interest(&self, interest: InterestType) -> bool {
        self.shared
            .handler
            .lock()
            .as_ref()
            .map(|handler| handler.has_interest(interest))
            .unwrap_or(false)
    }
}
