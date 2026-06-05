use crate::{NetworkError, Result};
use crossbeam_queue::ArrayQueue;
use dashmap::DashMap;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::mem::MaybeUninit;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::task::Waker;
use virtual_mio::{InterestHandler, InterestType};

const MULTICAST_SEG_PACKET_CAPACITY: usize = 64;
const MULTICAST_WRITE_QUEUE_CAPACITY: usize = 64;
const MULTICAST_READY_SEG_CAPACITY: usize = 256;

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
    group_addr: IpAddr,
    sender_id: u64,
}

impl MulticastPacket {
    fn is_deliverable_to(&self, socket_id: u64, loop_v4: bool, loop_v6: bool) -> bool {
        if self.sender_id != socket_id {
            return true;
        }
        match self.group_addr {
            IpAddr::V4(_) => loop_v4,
            IpAddr::V6(_) => loop_v6,
        }
    }
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

    fn is_full(&self) -> bool {
        self.len == MULTICAST_SEG_PACKET_CAPACITY
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

    fn has_deliverable_for(&self, socket_id: u64, loop_v4: bool, loop_v6: bool) -> bool {
        self.packets[..self.len]
            .iter()
            .flatten()
            .any(|packet| packet.is_deliverable_to(socket_id, loop_v4, loop_v6))
    }
}

#[derive(Debug)]
struct MulticastReadState {
    read_seg: Option<Arc<PacketSeg>>,
    read_index: usize,
    ready_segs: ArrayQueue<Arc<PacketSeg>>,
}

impl Default for MulticastReadState {
    fn default() -> Self {
        Self {
            read_seg: None,
            read_index: 0,
            ready_segs: ArrayQueue::new(MULTICAST_READY_SEG_CAPACITY),
        }
    }
}

impl MulticastReadState {
    fn enqueue(&mut self, seg: Arc<PacketSeg>) {
        if let Some(_dropped) = self.ready_segs.force_push(seg) {
            // UDP drops are expected when a subscriber cannot keep up.
        }
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
            self.read_index = 0;
            if !seg.is_empty() {
                self.read_seg = Some(seg);
            }
        }
    }

    fn next_packet(
        &mut self,
        socket_id: u64,
        loop_v4: bool,
        loop_v6: bool,
        peek: bool,
    ) -> Option<MulticastPacket> {
        loop {
            self.ensure_read_seg()?;
            let packet = self.read_seg.as_ref()?.packet(self.read_index)?;
            if !packet.is_deliverable_to(socket_id, loop_v4, loop_v6) {
                self.read_index += 1;
                continue;
            }
            if !peek {
                self.read_index += 1;
            }
            return Some(packet);
        }
    }

    fn next_packet_len(&mut self, socket_id: u64, loop_v4: bool, loop_v6: bool) -> Option<usize> {
        self.next_packet(socket_id, loop_v4, loop_v6, true)
            .map(|packet| packet.data.len())
    }

    fn recv(
        &mut self,
        socket_id: u64,
        loop_v4: bool,
        loop_v6: bool,
        peek: bool,
    ) -> Option<MulticastPacket> {
        self.next_packet(socket_id, loop_v4, loop_v6, peek)
    }
}

#[derive(Debug)]
struct MulticastGroup {
    write_queue: ArrayQueue<MulticastPacket>,
    flushing: AtomicBool,
    members: DashMap<u64, Weak<LocalUdpSocketShared>>,
}

impl Default for MulticastGroup {
    fn default() -> Self {
        Self {
            write_queue: ArrayQueue::new(MULTICAST_WRITE_QUEUE_CAPACITY),
            flushing: AtomicBool::new(false),
            members: Default::default(),
        }
    }
}

impl MulticastGroup {
    fn publish(
        &self,
        key: MulticastKey,
        sender: &Arc<LocalUdpSocketShared>,
        data: &[u8],
        from: SocketAddr,
    ) -> Vec<Arc<LocalUdpSocketShared>> {
        let mut packet = MulticastPacket {
            data: Arc::from(data),
            from,
            group_addr: key.addr,
            sender_id: sender.id,
        };
        let mut subscribers = self.subscribers_for_packet(&packet);

        loop {
            match self.write_queue.push(packet) {
                Ok(()) => break,
                Err(returned) => {
                    packet = returned;
                    subscribers.extend(self.flush(key));
                    if self.write_queue.is_full() {
                        let _ = self.write_queue.pop();
                    }
                }
            }
        }

        subscribers
    }

    fn flush(&self, key: MulticastKey) -> Vec<Arc<LocalUdpSocketShared>> {
        if self
            .flushing
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Vec::new();
        }

        let mut subscribers = Vec::new();
        while let Some(seg) = self.pop_segment() {
            subscribers.extend(self.fanout(key, seg));
        }
        self.flushing.store(false, Ordering::Release);

        if !self.write_queue.is_empty() {
            subscribers.extend(self.flush(key));
        }
        subscribers
    }

    fn pop_segment(&self) -> Option<Arc<PacketSeg>> {
        let mut seg = PacketSeg::new();
        let first = self.write_queue.pop()?;
        seg.push(first);
        while !seg.is_full() {
            let Some(packet) = self.write_queue.pop() else {
                break;
            };
            seg.push(packet);
        }
        Some(Arc::new(seg))
    }

    fn fanout(&self, key: MulticastKey, seg: Arc<PacketSeg>) -> Vec<Arc<LocalUdpSocketShared>> {
        let mut stale = Vec::new();
        let mut subscribers = Vec::new();
        for member in self.members.iter() {
            let id = *member.key();
            let Some(member) = member.value().upgrade() else {
                stale.push(id);
                continue;
            };
            if !seg.has_deliverable_for(
                member.id,
                member.multicast_loop_v4(),
                member.multicast_loop_v6(),
            ) {
                continue;
            }
            member.enqueue_multicast_segment(key, seg.clone());
            subscribers.push(member);
        }
        for id in stale {
            self.members.remove(&id);
        }
        subscribers
    }

    fn subscribers_for_packet(&self, packet: &MulticastPacket) -> Vec<Arc<LocalUdpSocketShared>> {
        let mut stale = Vec::new();
        let mut subscribers = Vec::new();
        for member in self.members.iter() {
            let id = *member.key();
            let Some(member) = member.value().upgrade() else {
                stale.push(id);
                continue;
            };
            if packet.is_deliverable_to(
                member.id,
                member.multicast_loop_v4(),
                member.multicast_loop_v6(),
            ) {
                subscribers.push(member);
            }
        }
        for id in stale {
            self.members.remove(&id);
        }
        subscribers
    }
}

#[derive(Debug, Default)]
pub(crate) struct MulticastCoordinator {
    groups: DashMap<MulticastKey, Arc<MulticastGroup>>,
}

impl MulticastCoordinator {
    pub(crate) fn join(&self, socket: &Arc<LocalUdpSocketShared>, key: MulticastKey) {
        let group = self
            .groups
            .entry(key)
            .or_insert_with(|| Arc::new(MulticastGroup::default()))
            .clone();
        group.members.insert(socket.id, Arc::downgrade(socket));
        socket.join_multicast(key);
    }

    pub(crate) fn leave(&self, socket: &Arc<LocalUdpSocketShared>, key: MulticastKey) {
        let group = self.groups.get(&key).map(|group| group.clone());
        if let Some(group) = group {
            group.members.remove(&socket.id);
            if group.members.is_empty() {
                self.groups
                    .remove_if(&key, |_, group| group.members.is_empty());
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
        let Some(group) = self.groups.get(&key).map(|group| group.clone()) else {
            return Vec::new();
        };

        group.publish(key, sender, data, from)
    }

    pub(crate) fn next_packet_len(&self, socket: &Arc<LocalUdpSocketShared>) -> Option<usize> {
        if let Some(len) = socket.next_multicast_packet_len() {
            return Some(len);
        }
        self.flush_socket_groups(socket);
        socket.next_multicast_packet_len()
    }

    pub(crate) fn recv(
        &self,
        socket: &Arc<LocalUdpSocketShared>,
        buf: &mut [MaybeUninit<u8>],
        peek: bool,
    ) -> Result<(usize, SocketAddr)> {
        let packet = match socket.recv_multicast_packet(peek) {
            Some(packet) => packet,
            None => {
                self.flush_socket_groups(socket);
                let Some(packet) = socket.recv_multicast_packet(peek) else {
                    return Err(NetworkError::WouldBlock);
                };
                packet
            }
        };

        let amt = buf.len().min(packet.data.len());
        let buf: &mut [u8] = unsafe { std::mem::transmute(buf) };
        buf[..amt].copy_from_slice(&packet.data[..amt]);
        Ok((amt, packet.from))
    }

    fn flush_socket_groups(&self, socket: &Arc<LocalUdpSocketShared>) {
        for key in socket.joined_keys() {
            let Some(group) = self.groups.get(&key).map(|group| group.clone()) else {
                continue;
            };
            for subscriber in group.flush(key) {
                if !Arc::ptr_eq(&subscriber, socket) {
                    subscriber.notify_readable();
                }
            }
        }
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
        let socket_id = self.id;
        let loop_v4 = self.multicast_loop_v4();
        let loop_v6 = self.multicast_loop_v6();
        for state in self.multicast_reads.lock().values_mut() {
            if let Some(len) = state.next_packet_len(socket_id, loop_v4, loop_v6) {
                return Some(len);
            }
        }
        None
    }

    fn recv_multicast_packet(&self, peek: bool) -> Option<MulticastPacket> {
        let socket_id = self.id;
        let loop_v4 = self.multicast_loop_v4();
        let loop_v6 = self.multicast_loop_v6();
        for state in self.multicast_reads.lock().values_mut() {
            if let Some(packet) = state.recv(socket_id, loop_v4, loop_v6, peek) {
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
