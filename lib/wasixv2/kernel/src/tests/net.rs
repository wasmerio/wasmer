//! The socket syscalls against a small in-test network implementing the vnet traits.

use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    sync::Weak,
};

use futures::FutureExt as _;
use wasixv2_abi::{AddressFamily, MsgFlags, SocketFlags, SocketLevel, SocketOption, UserMsghdr};
use wasixv2_syscalls::{read_value, write_value};
use wasixv2_vnet::{
    Accepted, Control, Domain, Network, RecvMsg, SendMsg, Shutdown, Socket, SocketKind,
};
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, poll_wait};

use super::{
    ipc::{errno, get, machine, put},
    *,
};
use crate::sync::MutexExt;

/// IPv4 only. Stream connects complete at once but report `EINPROGRESS` first; every
/// received message carries one control message (level 0, type 2, an int 64: `IP_TTL`).
struct FakeNet {
    me: Weak<FakeNet>,
    bound: Mutex<HashMap<SocketAddr, Weak<FakeSocket>>>,
    next_port: Mutex<u16>,
}

#[derive(Default)]
struct SockState {
    local: Option<SocketAddr>,
    peer: Option<SocketAddr>,
    peer_socket: Weak<FakeSocket>,
    backlog: VecDeque<Accepted>,
    rx: VecDeque<(Vec<u8>, SocketAddr)>,
}

struct FakeSocket {
    me: Weak<FakeSocket>,
    kind: SocketKind,
    net: Weak<FakeNet>,
    state: Mutex<SockState>,
    queue: WaitQueue,
}

impl FakeNet {
    fn new() -> Arc<Self> {
        Arc::new_cyclic(|me| Self {
            me: me.clone(),
            bound: Mutex::default(),
            next_port: Mutex::new(40000),
        })
    }

    fn new_socket(&self, kind: SocketKind) -> Arc<FakeSocket> {
        Arc::new_cyclic(|me| FakeSocket {
            me: me.clone(),
            kind,
            net: self.me.clone(),
            state: Mutex::default(),
            queue: WaitQueue::new(),
        })
    }

    fn lookup(&self, addr: SocketAddr) -> Option<Arc<FakeSocket>> {
        let bound = self.bound.lock_or_recover();
        let any = SocketAddr::new([0, 0, 0, 0].into(), addr.port());
        bound
            .get(&addr)
            .and_then(Weak::upgrade)
            .or_else(|| bound.get(&any).and_then(Weak::upgrade))
    }
}

impl Network for FakeNet {
    fn socket(
        &self,
        _: Domain,
        kind: SocketKind,
        protocol: i32,
        _: u32,
    ) -> Result<Arc<dyn Socket>, Errno> {
        if protocol != 0 {
            return Err(Errno::EPROTONOSUPPORT);
        }
        Ok(self.new_socket(kind))
    }
}

impl FakeSocket {
    fn net(&self) -> Arc<FakeNet> {
        self.net.upgrade().unwrap()
    }

    fn register(&self, mut addr: SocketAddr) -> Result<SocketAddr, Errno> {
        let net = self.net();
        if addr.port() == 0 {
            let mut next = net.next_port.lock_or_recover();
            addr.set_port(*next);
            *next += 1;
        }
        let mut bound = net.bound.lock_or_recover();
        if bound.get(&addr).is_some_and(|s| s.strong_count() > 0) {
            return Err(Errno::EADDRINUSE);
        }
        bound.insert(addr, self.me.clone());
        self.state.lock_or_recover().local = Some(addr);
        Ok(addr)
    }

    fn autobind(&self) -> SocketAddr {
        let local = self.state.lock_or_recover().local;
        local.unwrap_or_else(|| self.register("127.0.0.1:0".parse().unwrap()).unwrap())
    }

    fn deliver(&self, data: &[u8], from: SocketAddr) {
        self.state
            .lock_or_recover()
            .rx
            .push_back((data.to_vec(), from));
        self.queue.wake(PollMask::READABLE);
    }
}

impl Pollable for FakeSocket {
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, &self.queue);
        let state = self.state.lock_or_recover();
        let mut mask = PollMask::WRITABLE;
        if !state.rx.is_empty() || !state.backlog.is_empty() {
            mask |= PollMask::READABLE;
        }
        mask
    }
}

impl Socket for FakeSocket {
    fn domain(&self) -> Domain {
        Domain::Inet
    }

    fn kind(&self) -> SocketKind {
        self.kind
    }

    fn protocol(&self) -> i32 {
        match self.kind {
            SocketKind::Stream => 6,
            SocketKind::Datagram => 17,
            SocketKind::Raw => 255,
        }
    }

    fn bind(&self, addr: SocketAddr, privileged: bool) -> Result<(), Errno> {
        if (1..1024).contains(&addr.port()) && !privileged {
            return Err(Errno::EACCES);
        }
        if self.state.lock_or_recover().local.is_some() {
            return Err(Errno::EINVAL);
        }
        self.register(addr).map(drop)
    }

    fn listen(&self, _: u32) -> Result<(), Errno> {
        self.autobind();
        Ok(())
    }

    fn accept(&self) -> Result<Accepted, Errno> {
        let mut state = self.state.lock_or_recover();
        state.backlog.pop_front().ok_or(Errno::EAGAIN)
    }

    fn connect(&self, addr: SocketAddr) -> Result<(), Errno> {
        let local = self.autobind();
        if self.kind == SocketKind::Datagram {
            self.state.lock_or_recover().peer = Some(addr);
            return Ok(());
        }
        if self.state.lock_or_recover().peer.is_some() {
            return Err(Errno::EISCONN);
        }
        let listener = self.net().lookup(addr).ok_or(Errno::ECONNREFUSED)?;
        let server = self.net().new_socket(SocketKind::Stream);
        {
            let mut state = server.state.lock_or_recover();
            state.local = Some(addr);
            state.peer = Some(local);
            state.peer_socket = self.me.clone();
        }
        {
            let mut state = self.state.lock_or_recover();
            state.peer = Some(addr);
            state.peer_socket = Arc::downgrade(&server);
        }
        listener
            .state
            .lock_or_recover()
            .backlog
            .push_back(Accepted {
                socket: server,
                peer: local,
            });
        listener.queue.wake(PollMask::READABLE);
        Err(Errno::EINPROGRESS)
    }

    fn disconnect(&self) -> Result<(), Errno> {
        self.state.lock_or_recover().peer = None;
        Ok(())
    }

    fn shutdown(&self, _: Shutdown) -> Result<(), Errno> {
        Ok(())
    }

    fn send(&self, msg: SendMsg<'_>) -> Result<usize, Errno> {
        let from = self.autobind();
        let (peer, peer_socket) = {
            let state = self.state.lock_or_recover();
            (state.peer, state.peer_socket.upgrade())
        };
        let target = match (self.kind, msg.to.or(peer)) {
            (SocketKind::Stream, _) => peer_socket.ok_or(Errno::ENOTCONN)?,
            (SocketKind::Datagram | SocketKind::Raw, Some(to)) => match self.net().lookup(to) {
                Some(target) => target,
                None => return Ok(msg.data.len()),
            },
            (SocketKind::Datagram | SocketKind::Raw, None) => return Err(Errno::EDESTADDRREQ),
        };
        target.deliver(msg.data, from);
        Ok(msg.data.len())
    }

    fn recv(&self, buf: &mut [u8], flags: MsgFlags) -> Result<RecvMsg, Errno> {
        let mut state = self.state.lock_or_recover();
        let (data, from) = state.rx.front().cloned().ok_or(Errno::EAGAIN)?;
        let len = data.len().min(buf.len());
        if !flags.contains(MsgFlags::MSG_PEEK) {
            match self.kind {
                SocketKind::Stream if len < data.len() => state.rx[0].0.drain(..len).for_each(drop),
                _ => drop(state.rx.pop_front()),
            }
        }
        buf[..len].copy_from_slice(&data[..len]);
        let truncated = self.kind == SocketKind::Datagram && len < data.len();
        Ok(RecvMsg {
            len,
            full_len: if truncated { data.len() } else { len },
            from: Some(from),
            flags: if truncated {
                MsgFlags::MSG_TRUNC
            } else {
                MsgFlags(0)
            },
            control: vec![Control {
                level: 0,
                kind: 2,
                data: 64i32.to_le_bytes().to_vec(),
            }],
        })
    }

    fn local_addr(&self) -> Result<SocketAddr, Errno> {
        let local = self.state.lock_or_recover().local;
        Ok(local.unwrap_or("0.0.0.0:0".parse().unwrap()))
    }

    fn peer_addr(&self) -> Result<SocketAddr, Errno> {
        self.state.lock_or_recover().peer.ok_or(Errno::ENOTCONN)
    }

    fn take_error(&self) -> Option<Errno> {
        None
    }

    fn input_len(&self) -> Result<usize, Errno> {
        let state = self.state.lock_or_recover();
        Ok(state.rx.front().map_or(0, |(data, _)| data.len()))
    }

    fn set_option(&self, _: i32, _: i32, _: &[u8]) -> Result<(), Errno> {
        Err(Errno::ENOPROTOOPT)
    }

    fn get_option(&self, _: i32, _: i32, _: &mut [u8]) -> Result<usize, Errno> {
        Err(Errno::ENOPROTOOPT)
    }
}

fn networked() -> (Kernel, TaskId, FlatMemory) {
    super::cred::machine_with(|kernel| kernel.with_network(FakeNet::new()))
}

const STREAM: i32 = 1;
const DGRAM: i32 = 2;

fn socket(kernel: &Kernel, caller: &Caller<'_>, r#type: i32) -> Fd {
    block_on(kernel.socket(caller, AddressFamily::AF_INET, r#type, 0)).unwrap()
}

/// A sockaddr_in for 127.0.0.1:port at `at`.
fn sockaddr(memory: &FlatMemory, at: u32, port: u16) {
    let addr: SocketAddr = SocketAddr::new([127, 0, 0, 1].into(), port);
    put(memory, at, &crate::socket::encode(addr));
}

fn read_sockaddr(memory: &FlatMemory, at: u32) -> SocketAddr {
    crate::socket::decode(Domain::Inet, &get(memory, at, 16), false).unwrap()
}

#[test]
fn socket_checks_type_flags_and_families() {
    let (kernel, task, memory) = networked();
    let caller = Caller::new(task, &memory);
    let create = |family, r#type| errno(block_on(kernel.socket(&caller, family, r#type, 0)));

    assert_eq!(
        create(AddressFamily::AF_INET, STREAM | 0x4000),
        Errno::EINVAL
    );
    assert_eq!(create(AddressFamily::AF_INET, 12), Errno::EINVAL);
    // A raw socket of protocol 0 is none (`inet_create`), before the capability check.
    assert_eq!(create(AddressFamily::AF_INET, 3), Errno::EPROTONOSUPPORT);
    assert_eq!(create(AddressFamily::AF_INET, 5), Errno::ESOCKTNOSUPPORT);
    assert_eq!(create(AddressFamily(99), STREAM), Errno::EAFNOSUPPORT);
    let bad_protocol = block_on(kernel.socket(&caller, AddressFamily::AF_INET, STREAM, 99));
    assert_eq!(errno(bad_protocol), Errno::EPROTONOSUPPORT);

    let nonblocking = 0x800 | 0x80000;
    let fd = block_on(kernel.socket(&caller, AddressFamily::AF_INET, STREAM | nonblocking, 0));
    let fd = fd.unwrap();
    let file = kernel.file(task, fd).unwrap();
    assert!(file.flags().contains(OpenFlags::O_NONBLOCK));
    assert_eq!(kernel.files(task).lock_or_recover().cloexec(fd), Some(true));

    // A pipe is not a socket.
    let (r, _) = super::ipc::pipe(&kernel, &caller, OpenFlags(0));
    assert_eq!(
        errno(block_on(kernel.listen(&caller, r, 1))),
        Errno::ENOTSOCK
    );
    assert_eq!(
        errno(block_on(kernel.listen(&caller, Fd(77), 1))),
        Errno::EBADF
    );
}

#[test]
fn root_binds_privileged_ports_and_raw_sockets_need_cap_net_raw() {
    let (kernel, task, memory) = super::cred::machine_with(|kernel| {
        kernel
            .with_credentials(Credentials::new(0, 0))
            .with_network(FakeNet::new())
    });
    let caller = Caller::new(task, &memory);
    let fd = socket(&kernel, &caller, STREAM);
    sockaddr(&memory, 0x200, 80);

    assert_eq!(
        block_on(kernel.bind(&caller, fd, GuestPtr::new(0x200), 16)),
        Ok(0)
    );
    // A raw ICMP socket for root; raw sockets of other protocols carry nothing, and one of
    // protocol 0 is none (`inet_create`).
    let vnet = wasixv2_vnet::Net::new(wasixv2_vnet::NetConfig::default(), None);
    let (kernel, task, memory) = super::cred::machine_with(|kernel| {
        kernel
            .with_credentials(Credentials::new(0, 0))
            .with_network(vnet)
    });
    let caller = Caller::new(task, &memory);
    let raw = |protocol| block_on(kernel.socket(&caller, AddressFamily::AF_INET, 3, protocol));
    assert!(raw(1).is_ok());
    assert_eq!(errno(raw(17)), Errno::ESOCKTNOSUPPORT);
    assert_eq!(errno(raw(0)), Errno::EPROTONOSUPPORT);
    let (kernel, task, memory) = networked();
    let caller = Caller::new(task, &memory);
    let raw = |protocol| block_on(kernel.socket(&caller, AddressFamily::AF_INET, 3, protocol));
    assert_eq!(errno(raw(1)), Errno::EPERM);
    assert_eq!(errno(raw(0)), Errno::EPROTONOSUPPORT);
}

#[test]
fn bind_and_getsockname_follow_the_addrlen_rules() {
    let (kernel, task, memory) = networked();
    let caller = Caller::new(task, &memory);
    let fd = socket(&kernel, &caller, DGRAM);
    sockaddr(&memory, 0x200, 5000);
    let bind = |len| block_on(kernel.bind(&caller, fd, GuestPtr::new(0x200), len));

    assert_eq!(errno(bind(129)), Errno::EINVAL);
    assert_eq!(errno(bind(8)), Errno::EINVAL);
    sockaddr(&memory, 0x200, 80);
    assert_eq!(errno(bind(16)), Errno::EACCES);
    sockaddr(&memory, 0x200, 5000);
    assert_eq!(bind(16), Ok(0));

    // A short buffer gets a prefix; the length says how long the address really is.
    put(&memory, 0x300, &[0xee; 16]);
    put(&memory, 0x2f0, &4u32.to_le_bytes());
    let name = block_on(kernel.getsockname(
        &caller,
        fd,
        GuestPtrMut::new(0x300),
        GuestPtrMut::new(0x2f0),
    ));
    assert_eq!(name, Ok(0));
    assert_eq!(get(&memory, 0x2f0, 4), 16u32.to_le_bytes());
    assert_eq!(get(&memory, 0x300, 6), [2, 0, 0x13, 0x88, 0xee, 0xee]);
    let peer = block_on(kernel.getpeername(
        &caller,
        fd,
        GuestPtrMut::new(0x300),
        GuestPtrMut::new(0x2f0),
    ));
    assert_eq!(errno(peer), Errno::ENOTCONN);
}

#[test]
fn stream_connect_accept_and_transfer() {
    let (kernel, task, memory) = networked();
    let caller = Caller::new(task, &memory);
    let listener = socket(&kernel, &caller, STREAM);
    sockaddr(&memory, 0x200, 6000);
    assert_eq!(
        block_on(kernel.bind(&caller, listener, GuestPtr::new(0x200), 16)),
        Ok(0)
    );
    assert_eq!(block_on(kernel.listen(&caller, listener, -1)), Ok(0));

    // Non-blocking connect reports EINPROGRESS; a blocking one waits for completion.
    let nonblocking = block_on(kernel.socket(&caller, AddressFamily::AF_INET, STREAM | 0x800, 0));
    let nonblocking = nonblocking.unwrap();
    let connect = |fd| block_on(kernel.connect(&caller, fd, GuestPtr::new(0x200), 16));
    assert_eq!(errno(connect(nonblocking)), Errno::EINPROGRESS);
    let client = socket(&kernel, &caller, STREAM);
    assert_eq!(connect(client), Ok(0));

    put(&memory, 0x2f0, &16u32.to_le_bytes());
    let accept = |flags| {
        block_on(kernel.accept4(
            &caller,
            listener,
            GuestPtrMut::new(0x300),
            GuestPtrMut::new(0x2f0),
            flags,
        ))
    };
    let first = accept(SocketFlags(0)).unwrap();
    let server = accept(SocketFlags::SOCK_NONBLOCK).unwrap();
    assert_eq!(read_sockaddr(&memory, 0x300).ip().to_string(), "127.0.0.1");
    assert_eq!(errno(accept(SocketFlags(1))), Errno::EINVAL);
    let _ = first;

    put(&memory, 0x400, b"ping");
    let sent = block_on(kernel.sendto(
        &caller,
        client,
        UserSlice::new(0x400, 4),
        MsgFlags(0),
        GuestPtr::NULL,
        0,
    ));
    assert_eq!(sent, Ok(4));
    assert_eq!(
        block_on(kernel.read(&caller, server, UserSliceMut::new(0x500, 2))),
        Ok(2)
    );
    assert_eq!(
        block_on(kernel.read(&caller, server, UserSliceMut::new(0x502, 9))),
        Ok(2)
    );
    assert_eq!(get(&memory, 0x500, 4), b"ping");
    assert_eq!(
        errno(block_on(kernel.read(
            &caller,
            server,
            UserSliceMut::new(0x500, 9)
        ))),
        Errno::EAGAIN
    );
}

#[test]
fn datagrams_report_sender_truncation_and_control_messages() {
    let (kernel, task, memory) = networked();
    let caller = Caller::new(task, &memory);
    let receiver = socket(&kernel, &caller, DGRAM);
    let sender = socket(&kernel, &caller, DGRAM);
    sockaddr(&memory, 0x200, 7000);
    assert_eq!(
        block_on(kernel.bind(&caller, receiver, GuestPtr::new(0x200), 16)),
        Ok(0)
    );
    put(&memory, 0x400, b"hello world");
    let send = || {
        block_on(kernel.sendto(
            &caller,
            sender,
            UserSlice::new(0x400, 11),
            MsgFlags(0),
            GuestPtr::new(0x200),
            16,
        ))
    };
    assert_eq!(send(), Ok(11));
    assert_eq!(send(), Ok(11));

    // recvfrom with MSG_TRUNC returns the datagram's real length.
    put(&memory, 0x2f0, &16u32.to_le_bytes());
    let n = block_on(kernel.recvfrom(
        &caller,
        receiver,
        UserSliceMut::new(0x500, 5),
        MsgFlags::MSG_TRUNC,
        GuestPtrMut::new(0x300),
        GuestPtrMut::new(0x2f0),
    ));
    assert_eq!(n, Ok(11));
    assert_eq!(get(&memory, 0x500, 5), b"hello");
    assert_eq!(read_sockaddr(&memory, 0x300).port(), 40000);

    // recvmsg: one iovec of 5 bytes, a name buffer and room for 14 control bytes, so the
    // 16-byte control message is cut.
    let iov = [0x600u32, 5];
    put(&memory, 0x700, &iov[0].to_le_bytes());
    put(&memory, 0x704, &iov[1].to_le_bytes());
    let hdr = UserMsghdr {
        msg_name: GuestPtrMut::new(0x300),
        msg_namelen: 16,
        msg_iov: GuestPtr::new(0x700),
        msg_iovlen: 1,
        msg_control: GuestPtrMut::new(0x800),
        msg_controllen: 14,
        msg_flags: 0,
    };
    block_on(write_value(&memory, GuestPtrMut::new(0x900), &hdr)).unwrap();
    let n = block_on(kernel.recvmsg(&caller, receiver, GuestPtrMut::new(0x900), MsgFlags(0)));
    assert_eq!(n, Ok(5));
    let hdr: UserMsghdr = block_on(read_value(&memory, GuestPtr::new(0x900))).unwrap();
    assert_eq!(hdr.msg_namelen, 16);
    assert_eq!(
        MsgFlags(hdr.msg_flags),
        MsgFlags::MSG_TRUNC | MsgFlags::MSG_CTRUNC
    );
    assert_eq!(hdr.msg_controllen, 14);
    assert_eq!(get(&memory, 0x800, 4), 14u32.to_le_bytes());

    let ioctl_none = block_on(kernel.ioctl(&caller, receiver, IoctlRequest::FIONREAD, 0x980));
    assert_eq!(ioctl_none, Ok(0));
    assert_eq!(get(&memory, 0x980, 4), 0u32.to_le_bytes());
}

#[test]
fn a_blocking_error_queue_read_never_waits() {
    let (kernel, task, memory) = networked();
    let caller = Caller::new(task, &memory);
    let sock = socket(&kernel, &caller, DGRAM);
    sockaddr(&memory, 0x200, 7000);
    assert_eq!(
        block_on(kernel.bind(&caller, sock, GuestPtr::new(0x200), 16)),
        Ok(0)
    );
    // `udp_recvmsg` reads the error queue before it would wait for data.
    let mut read = Box::pin(kernel.recvfrom(
        &caller,
        sock,
        UserSliceMut::new(0x500, 16),
        MsgFlags::MSG_ERRQUEUE,
        GuestPtrMut::new(0),
        GuestPtrMut::new(0),
    ));
    assert_eq!(read.as_mut().now_or_never().map(errno), Some(Errno::EAGAIN));
}

#[test]
fn error_queue_entries_arrive_as_ip_recverr_control_messages() {
    let net = wasixv2_vnet::Net::new(wasixv2_vnet::NetConfig::default(), None);
    let (kernel, task, memory) = super::cred::machine_with(|kernel| kernel.with_network(net));
    let caller = Caller::new(task, &memory);
    let sock = socket(&kernel, &caller, DGRAM);
    put(&memory, 0x400, &1i32.to_le_bytes());
    let set = block_on(kernel.setsockopt(
        &caller,
        sock,
        SocketLevel::SOL_IP,
        wasixv2_abi::IpOption::IP_RECVERR.0,
        UserSlice::new(0x400, 4),
    ));
    assert_eq!(set, Ok(0));
    sockaddr(&memory, 0x200, 9);
    put(&memory, 0x410, b"ping");
    let sent = block_on(kernel.sendto(
        &caller,
        sock,
        UserSlice::new(0x410, 4),
        MsgFlags(0),
        GuestPtr::new(0x200),
        16,
    ));
    assert_eq!(sent, Ok(4));
    let iov = [0x600u32, 16];
    put(&memory, 0x700, &iov[0].to_le_bytes());
    put(&memory, 0x704, &iov[1].to_le_bytes());
    let hdr = UserMsghdr {
        msg_name: GuestPtrMut::new(0x300),
        msg_namelen: 16,
        msg_iov: GuestPtr::new(0x700),
        msg_iovlen: 1,
        msg_control: GuestPtrMut::new(0x800),
        msg_controllen: 64,
        msg_flags: 0,
    };
    block_on(write_value(&memory, GuestPtrMut::new(0x900), &hdr)).unwrap();
    let n = block_on(kernel.recvmsg(
        &caller,
        sock,
        GuestPtrMut::new(0x900),
        MsgFlags::MSG_ERRQUEUE,
    ));
    assert_eq!(n, Ok(4));
    assert_eq!(get(&memory, 0x600, 4), b"ping");
    let hdr: UserMsghdr = block_on(read_value(&memory, GuestPtr::new(0x900))).unwrap();
    assert_eq!(MsgFlags(hdr.msg_flags), MsgFlags::MSG_ERRQUEUE);
    // The address is the refused datagram's destination.
    assert_eq!(hdr.msg_namelen, 16);
    assert_eq!(
        read_sockaddr(&memory, 0x300),
        "127.0.0.1:9".parse().unwrap()
    );
    // One cmsghdr (12 bytes) with `struct sock_extended_err` and the offender's sockaddr_in.
    assert_eq!(hdr.msg_controllen, 12 + 16 + 16);
    let cmsg = get(&memory, 0x800, 44);
    assert_eq!(cmsg[..4], 44u32.to_le_bytes());
    assert_eq!(cmsg[4..8], 0i32.to_le_bytes());
    assert_eq!(cmsg[8..12], 11i32.to_le_bytes());
    assert_eq!(cmsg[12..16], (Errno::ECONNREFUSED.0 as u32).to_le_bytes());
    // SO_EE_ORIGIN_ICMP, ICMP_DEST_UNREACH, ICMP_PORT_UNREACH, pad, then ee_info and ee_data.
    assert_eq!(cmsg[16..28], [2, 3, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(cmsg[28..36], [2, 0, 0, 0, 127, 0, 0, 1]);
    // Read once; a blocking read of the empty queue fails at once.
    let again = Box::pin(kernel.recvmsg(
        &caller,
        sock,
        GuestPtrMut::new(0x900),
        MsgFlags::MSG_ERRQUEUE,
    ))
    .now_or_never();
    assert_eq!(again.map(errno), Some(Errno::EAGAIN));
}

#[test]
fn socket_level_options_come_from_the_kernel() {
    let (kernel, task, memory) = networked();
    let caller = Caller::new(task, &memory);
    let fd = socket(&kernel, &caller, DGRAM);
    let get_int = |name: SocketOption, len: u32| {
        put(&memory, 0x2f0, &len.to_le_bytes());
        put(&memory, 0x300, &[0xff; 8]);
        let result = block_on(kernel.getsockopt(
            &caller,
            fd,
            SocketLevel::SOL_SOCKET,
            name.0,
            GuestPtrMut::new(0x300),
            GuestPtrMut::new(0x2f0),
        ));
        (result, get(&memory, 0x300, 4), get(&memory, 0x2f0, 4))
    };

    let (result, value, len) = get_int(SocketOption::SO_TYPE, 4);
    assert_eq!(
        (result, value, len),
        (
            Ok(0),
            2u32.to_le_bytes().to_vec(),
            4u32.to_le_bytes().to_vec()
        )
    );
    // Truncated to the caller's length.
    let (_, value, len) = get_int(SocketOption::SO_PROTOCOL, 1);
    assert_eq!(
        (value, len),
        (vec![17, 0xff, 0xff, 0xff], 1u32.to_le_bytes().to_vec())
    );
    let (result, _, _) = get_int(SocketOption::SO_REUSEADDR, 4);
    assert_eq!(errno(result), Errno::ENOPROTOOPT);

    // SO_RCVTIMEO_NEW: 1.5 seconds; a bad microsecond count is EDOM.
    let set_timeout = |usec: i64| {
        put(&memory, 0x400, &1i64.to_le_bytes());
        put(&memory, 0x408, &usec.to_le_bytes());
        block_on(kernel.setsockopt(
            &caller,
            fd,
            SocketLevel::SOL_SOCKET,
            SocketOption::SO_RCVTIMEO_NEW.0,
            UserSlice::new(0x400, 16),
        ))
    };
    assert_eq!(errno(set_timeout(1_000_000)), Errno::EDOM);
    assert_eq!(set_timeout(500_000), Ok(0));
    // The test host's sleeps end at once, so the receive times out with EAGAIN.
    let read = block_on(kernel.read(&caller, fd, UserSliceMut::new(0x500, 4)));
    assert_eq!(errno(read), Errno::EAGAIN);
}

const SEQPACKET: i32 = 5;

fn unix_pair(kernel: &Kernel, caller: &Caller<'_>, memory: &FlatMemory, r#type: i32) -> (Fd, Fd) {
    let pair = wasixv2_abi::FdPair::new(0x100);
    let made = block_on(kernel.socketpair(caller, AddressFamily::AF_UNIX, r#type, 0, pair));
    assert_eq!(made, Ok(0));
    let raw = get(memory, 0x100, 8);
    let fd = |i: usize| Fd(i32::from_le_bytes(raw[i..i + 4].try_into().unwrap()));
    (fd(0), fd(4))
}

fn send_bytes(
    kernel: &Kernel,
    caller: &Caller<'_>,
    fd: Fd,
    at: u32,
    len: u32,
    flags: MsgFlags,
) -> SysResult<u32> {
    block_on(kernel.sendto(
        caller,
        fd,
        UserSlice::new(at, len),
        flags,
        GuestPtr::NULL,
        0,
    ))
}

/// Whether SIGPIPE is pending (and blocked, as the test keeps it).
pub(super) fn sigpipe_pending(kernel: &Kernel, caller: &Caller<'_>, memory: &FlatMemory) -> bool {
    assert_eq!(
        block_on(kernel.rt_sigpending(caller, GuestPtrMut::new(0x800), 8)),
        Ok(0)
    );
    let set = u64::from_le_bytes(get(memory, 0x800, 8).try_into().unwrap());
    set & (1 << 12) != 0
}

#[test]
fn stream_sends_raise_sigpipe_unless_msg_nosignal() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    kernel.task_signals(task).replace_blocked(1 << 12);
    let (a, _b) = unix_pair(&kernel, &caller, &memory, STREAM);
    let shut = block_on(kernel.shutdown(&caller, a, wasixv2_abi::ShutdownHow::SHUT_WR));
    assert_eq!(shut, Ok(0));
    let quiet = send_bytes(&kernel, &caller, a, 0x400, 4, MsgFlags::MSG_NOSIGNAL);
    assert_eq!(errno(quiet), Errno::EPIPE);
    assert!(!sigpipe_pending(&kernel, &caller, &memory));
    assert_eq!(
        errno(send_bytes(&kernel, &caller, a, 0x400, 4, MsgFlags(0))),
        Errno::EPIPE
    );
    assert!(sigpipe_pending(&kernel, &caller, &memory));

    // Datagram sockets never raise it.
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    kernel.task_signals(task).replace_blocked(1 << 12);
    let (a, _b) = unix_pair(&kernel, &caller, &memory, SEQPACKET);
    block_on(kernel.shutdown(&caller, a, wasixv2_abi::ShutdownHow::SHUT_WR)).unwrap();
    assert_eq!(
        errno(send_bytes(&kernel, &caller, a, 0x400, 4, MsgFlags(0))),
        Errno::EPIPE
    );
    assert!(!sigpipe_pending(&kernel, &caller, &memory));
}

#[test]
fn unix_records_are_not_capped_at_64k() {
    let (kernel, task, _) = machine();
    let memory = FlatMemory(RefCell::new(vec![0; 1 << 20]));
    let caller = Caller::new(task, &memory);
    let (a, b) = unix_pair(&kernel, &caller, &memory, SEQPACKET);
    let data: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
    put(&memory, 0x1000, &data);
    assert_eq!(
        send_bytes(&kernel, &caller, a, 0x1000, 100_000, MsgFlags(0)),
        Ok(100_000)
    );
    let got = block_on(kernel.recvfrom(
        &caller,
        b,
        UserSliceMut::new(0x40000, 200_000),
        MsgFlags(0),
        GuestPtrMut::NULL,
        GuestPtrMut::NULL,
    ));
    assert_eq!(got, Ok(100_000));
    assert_eq!(get(&memory, 0x40000, 100_000), data);
    // Past `SO_SNDBUF - 32`: EMSGSIZE, not a silent truncation.
    let too_big = send_bytes(&kernel, &caller, a, 0x1000, 212_992 - 31, MsgFlags(0));
    assert_eq!(errno(too_big), Errno::EMSGSIZE);
}

#[test]
fn unix_sockets_honour_so_rcvtimeo() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (a, _b) = unix_pair(&kernel, &caller, &memory, STREAM);
    put(&memory, 0x400, &2i64.to_le_bytes());
    put(&memory, 0x408, &0i64.to_le_bytes());
    let set = block_on(kernel.setsockopt(
        &caller,
        a,
        SocketLevel::SOL_SOCKET,
        SocketOption::SO_RCVTIMEO_NEW.0,
        UserSlice::new(0x400, 16),
    ));
    assert_eq!(set, Ok(0));
    put(&memory, 0x2f0, &16u32.to_le_bytes());
    let got = block_on(kernel.getsockopt(
        &caller,
        a,
        SocketLevel::SOL_SOCKET,
        SocketOption::SO_RCVTIMEO_NEW.0,
        GuestPtrMut::new(0x300),
        GuestPtrMut::new(0x2f0),
    ));
    assert_eq!(got, Ok(0));
    assert_eq!(get(&memory, 0x300, 8), 2i64.to_le_bytes());
    // The test host's sleeps end at once: the blocking read times out.
    let read = block_on(kernel.read(&caller, a, UserSliceMut::new(0x500, 4)));
    assert_eq!(errno(read), Errno::EAGAIN);
}

/// A listening abstract Unix socket named `name` and a client connected to it.
fn unix_listener(
    kernel: &Kernel,
    caller: &Caller<'_>,
    memory: &FlatMemory,
    name: &[u8],
) -> (Fd, Fd, u32) {
    let mut addr = (AddressFamily::AF_UNIX.0 as u16).to_le_bytes().to_vec();
    addr.push(0);
    addr.extend(name);
    put(memory, 0x200, &addr);
    let len = addr.len() as u32;
    let socket = |kernel: &Kernel| {
        block_on(kernel.socket(caller, AddressFamily::AF_UNIX, STREAM, 0)).unwrap()
    };
    let listener = socket(kernel);
    assert_eq!(
        block_on(kernel.bind(caller, listener, GuestPtr::new(0x200), len)),
        Ok(0)
    );
    assert_eq!(block_on(kernel.listen(caller, listener, 4)), Ok(0));
    let client = socket(kernel);
    assert_eq!(
        block_on(kernel.connect(caller, client, GuestPtr::new(0x200), len)),
        Ok(0)
    );
    (listener, client, len)
}

#[test]
fn accept_at_the_descriptor_limit_keeps_the_connection_queued() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (listener, client, _) = unix_listener(&kernel, &caller, &memory, b"emfile");
    let limit = kernel.files(task).lock_or_recover().end();
    kernel.files(task).lock_or_recover().set_limit(limit);
    let accept = || {
        block_on(kernel.accept4(
            &caller,
            listener,
            GuestPtrMut::NULL,
            GuestPtrMut::NULL,
            SocketFlags(0),
        ))
    };
    assert_eq!(errno(accept()), Errno::EMFILE);
    kernel.files(task).lock_or_recover().set_limit(limit + 1);
    let server = accept().unwrap();
    put(&memory, 0x400, b"ping");
    assert_eq!(
        send_bytes(&kernel, &caller, client, 0x400, 4, MsgFlags(0)),
        Ok(4)
    );
    let read = block_on(kernel.read(&caller, server, UserSliceMut::new(0x500, 8)));
    assert_eq!(read, Ok(4));
}

#[test]
fn accept_that_faults_on_the_address_closes_the_connection() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (listener, client, _) = unix_listener(&kernel, &caller, &memory, b"efault");
    // A negative address length: the accepted socket goes, and its client sees EOF.
    put(&memory, 0x2f0, &(-1i32).to_le_bytes());
    let accepted = block_on(kernel.accept4(
        &caller,
        listener,
        GuestPtrMut::new(0x300),
        GuestPtrMut::new(0x2f0),
        SocketFlags(0),
    ));
    assert_eq!(errno(accepted), Errno::EINVAL);
    let read = block_on(kernel.read(&caller, client, UserSliceMut::new(0x500, 8)));
    assert_eq!(read, Ok(0));
}

#[test]
fn ip_accept_at_the_descriptor_limit_keeps_the_connection_queued() {
    let (kernel, task, memory) = networked();
    let caller = Caller::new(task, &memory);
    let listener = socket(&kernel, &caller, STREAM);
    sockaddr(&memory, 0x200, 6100);
    assert_eq!(
        block_on(kernel.bind(&caller, listener, GuestPtr::new(0x200), 16)),
        Ok(0)
    );
    assert_eq!(block_on(kernel.listen(&caller, listener, 4)), Ok(0));
    let client = socket(&kernel, &caller, STREAM);
    assert_eq!(
        block_on(kernel.connect(&caller, client, GuestPtr::new(0x200), 16)),
        Ok(0)
    );
    let limit = kernel.files(task).lock_or_recover().end();
    kernel.files(task).lock_or_recover().set_limit(limit);
    let accept = || {
        block_on(kernel.accept4(
            &caller,
            listener,
            GuestPtrMut::NULL,
            GuestPtrMut::NULL,
            SocketFlags(0),
        ))
    };
    assert_eq!(errno(accept()), Errno::EMFILE);
    kernel.files(task).lock_or_recover().set_limit(limit + 1);
    assert!(accept().is_ok());
}

#[test]
fn blocking_connect_on_a_connecting_socket_waits_instead_of_ealready() {
    let net = wasixv2_vnet::Net::new(wasixv2_vnet::NetConfig::default(), None);
    let (kernel, task, memory) = super::cred::machine_with(|kernel| kernel.with_network(net));
    let caller = Caller::new(task, &memory);
    let listener = socket(&kernel, &caller, STREAM);
    sockaddr(&memory, 0x200, 6200);
    assert_eq!(
        block_on(kernel.bind(&caller, listener, GuestPtr::new(0x200), 16)),
        Ok(0)
    );
    assert_eq!(block_on(kernel.listen(&caller, listener, 0)), Ok(0));
    let first = socket(&kernel, &caller, STREAM);
    let connect = |fd| block_on(kernel.connect(&caller, fd, GuestPtr::new(0x200), 16));
    assert_eq!(connect(first), Ok(0));
    // The accept queue is full: the second connection waits in SYN_SENT.
    let second =
        block_on(kernel.socket(&caller, AddressFamily::AF_INET, STREAM | 0x800, 0)).unwrap();
    assert_eq!(errno(connect(second)), Errno::EINPROGRESS);
    assert_eq!(errno(connect(second)), Errno::EALREADY);
    // Blocking again, as a connect restarted after a signal is: it waits (here until its
    // SO_SNDTIMEO, which the test host ends at once) rather than failing with EALREADY.
    let setfl = block_on(kernel.fcntl64(&caller, second, wasixv2_abi::FcntlCmd::F_SETFL, 0));
    assert_eq!(setfl, Ok(0));
    put(&memory, 0x400, &1i64.to_le_bytes());
    put(&memory, 0x408, &0i64.to_le_bytes());
    let timeo = block_on(kernel.setsockopt(
        &caller,
        second,
        SocketLevel::SOL_SOCKET,
        SocketOption::SO_SNDTIMEO_NEW.0,
        UserSlice::new(0x400, 16),
    ));
    assert_eq!(timeo, Ok(0));
    assert_eq!(errno(connect(second)), Errno::EINPROGRESS);
    let accepted = block_on(kernel.accept4(
        &caller,
        listener,
        GuestPtrMut::NULL,
        GuestPtrMut::NULL,
        SocketFlags(0),
    ));
    assert!(accepted.is_ok());
    assert_eq!(errno(connect(second)), Errno::EISCONN);
}

/// `sendmsg` of one byte from 0x400 with the control messages `cmsgs` (level, type, data)
/// laid out at 0xa00.
fn send_with_controls(
    kernel: &Kernel,
    caller: &Caller<'_>,
    memory: &FlatMemory,
    fd: Fd,
    to: Option<(u32, u32)>,
    cmsgs: &[(i32, i32, Vec<u8>)],
) -> SysResult<i32> {
    let mut raw = Vec::new();
    for (level, kind, data) in cmsgs {
        raw.extend(((12 + data.len()) as u32).to_le_bytes());
        raw.extend(level.to_le_bytes());
        raw.extend(kind.to_le_bytes());
        raw.extend(data);
        raw.resize(raw.len().next_multiple_of(4), 0);
    }
    put(memory, 0xa00, &raw);
    put(memory, 0x700, &0x400u32.to_le_bytes());
    put(memory, 0x704, &1u32.to_le_bytes());
    let (name, namelen) = to.unwrap_or((0, 0));
    let hdr = UserMsghdr {
        msg_name: GuestPtrMut::new(name),
        msg_namelen: namelen as i32,
        msg_iov: GuestPtr::new(0x700),
        msg_iovlen: 1,
        msg_control: GuestPtrMut::new(if raw.is_empty() { 0 } else { 0xa00 }),
        msg_controllen: raw.len() as u32,
        msg_flags: 0,
    };
    block_on(write_value(memory, GuestPtrMut::new(0x900), &hdr)).unwrap();
    block_on(kernel.sendmsg(caller, fd, GuestPtr::new(0x900), MsgFlags(0)))
}

fn rights(fds: &[Fd]) -> (i32, i32, Vec<u8>) {
    let data = fds.iter().flat_map(|fd| fd.0.to_le_bytes()).collect();
    (SocketLevel::SOL_SOCKET.0, crate::unix::SCM_RIGHTS, data)
}

fn close(kernel: &Kernel, caller: &Caller<'_>, fd: Fd) {
    assert_eq!(block_on(kernel.close(caller, fd)), Ok(0));
}

#[test]
fn unix_sockets_only_their_own_queues_hold_are_collected() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let inflight = kernel.task_ns(task).net.unix_names.inflight.clone();
    // `b` in flight in its own queue, then both descriptors closed: a cycle.
    let (a, b) = unix_pair(&kernel, &caller, &memory, STREAM);
    let sent = send_with_controls(&kernel, &caller, &memory, a, None, &[rights(&[b])]);
    assert_eq!(sent, Ok(1));
    assert_eq!(inflight.user(1), 1);
    close(&kernel, &caller, a);
    close(&kernel, &caller, b);
    assert_eq!(inflight.user(1), 1, "nothing but its own queue holds b");
    // The next Unix socket released runs the collector (`unix_release_sock`).
    let (c, d) = unix_pair(&kernel, &caller, &memory, STREAM);
    close(&kernel, &caller, c);
    assert_eq!(inflight.user(1), 0);

    // A socket in flight to a socket someone still holds is not garbage.
    let (x, y) = unix_pair(&kernel, &caller, &memory, STREAM);
    let sent = send_with_controls(&kernel, &caller, &memory, d, None, &[rights(&[x])]);
    assert_eq!(sent, Err(SysErr::Errno(Errno::EPIPE)), "d's peer is gone");
    let (p, q) = unix_pair(&kernel, &caller, &memory, STREAM);
    let sent = send_with_controls(&kernel, &caller, &memory, p, None, &[rights(&[x])]);
    assert_eq!(sent, Ok(1));
    close(&kernel, &caller, x);
    close(&kernel, &caller, d);
    assert_eq!(inflight.user(1), 1);
    // Received from q, x still works: its peer y hears from it.
    let hdr = UserMsghdr {
        msg_name: GuestPtrMut::NULL,
        msg_namelen: 0,
        msg_iov: GuestPtr::new(0x700),
        msg_iovlen: 1,
        msg_control: GuestPtrMut::new(0xb00),
        msg_controllen: 64,
        msg_flags: 0,
    };
    block_on(write_value(&memory, GuestPtrMut::new(0x900), &hdr)).unwrap();
    let got = block_on(kernel.recvmsg(&caller, q, GuestPtrMut::new(0x900), MsgFlags(0)));
    assert_eq!(got, Ok(1));
    let x = Fd(i32::from_le_bytes(
        get(&memory, 0xb0c, 4).try_into().unwrap(),
    ));
    assert_eq!(inflight.user(1), 0);
    put(&memory, 0x400, b"z");
    assert_eq!(
        send_bytes(&kernel, &caller, x, 0x400, 1, MsgFlags(0)),
        Ok(1)
    );
    let read = block_on(kernel.read(&caller, y, UserSliceMut::new(0x500, 4)));
    assert_eq!(read, Ok(1));
}

#[test]
fn files_in_flight_count_against_their_sender_only() {
    let (kernel, first, memory) =
        super::cred::machine_with(|kernel| kernel.with_credentials(Credentials::new(0, 0)));
    let spawn = || {
        kernel
            .spawn(ProcessConfig {
                args: vec!["test".into()],
                env: Vec::new(),
                stdio: Stdio {
                    stdin: Arc::new(Input(Mutex::new(Vec::new()))),
                    stdout: Arc::new(Capture::default()),
                    stderr: Arc::new(Capture::default()),
                },
            })
            .unwrap()
    };
    let (alice, bob) = (spawn(), spawn());
    for (task, uid) in [(alice, 1), (bob, 2)] {
        let caller = Caller::new(task, &memory);
        assert_eq!(block_on(kernel.setresuid(&caller, uid, uid, uid)), Ok(0));
    }
    // Room for the open descriptors and four more.
    let fill = |task: TaskId| {
        let caller = Caller::new(task, &memory);
        let (a, _b) = unix_pair(&kernel, &caller, &memory, STREAM);
        let limit = kernel.files(task).lock_or_recover().end() + 4;
        kernel.files(task).lock_or_recover().set_limit(limit);
        (0..limit + 4)
            .map(|_| send_with_controls(&kernel, &caller, &memory, a, None, &[rights(&[a])]))
            .position(|sent| sent.is_err())
            .map(|at| (at, limit))
    };
    // `too_many_unix_fds`: more than RLIMIT_NOFILE in flight is ETOOMANYREFS.
    let (at, limit) = fill(alice).unwrap();
    assert_eq!(at, limit + 1);
    // Alice's files are Alice's: Bob has his whole allowance.
    let (at, limit) = fill(bob).unwrap();
    assert_eq!(at, limit + 1);
    // Root holds CAP_SYS_RESOURCE, which lifts the limit.
    assert_eq!(fill(first), None);
}

#[test]
fn unix_socket_options_read_back_what_was_set() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (a, _b) = unix_pair(&kernel, &caller, &memory, STREAM);
    let set = |fd: Fd, name: SocketOption, value: &[u8]| {
        put(&memory, 0x400, value);
        block_on(kernel.setsockopt(
            &caller,
            fd,
            SocketLevel::SOL_SOCKET,
            name.0,
            UserSlice::new(0x400, value.len() as u32),
        ))
    };
    let get = |fd: Fd, name: SocketOption, len: u32| {
        put(&memory, 0x2f0, &len.to_le_bytes());
        let result = block_on(kernel.getsockopt(
            &caller,
            fd,
            SocketLevel::SOL_SOCKET,
            name.0,
            GuestPtrMut::new(0x300),
            GuestPtrMut::new(0x2f0),
        ));
        let len = u32::from_le_bytes(super::ipc::get(&memory, 0x2f0, 4).try_into().unwrap());
        (result, super::ipc::get(&memory, 0x300, len as usize))
    };
    let linger = [1i32.to_le_bytes(), 5i32.to_le_bytes()].concat();
    assert_eq!(set(a, SocketOption::SO_LINGER, &linger), Ok(0));
    assert_eq!(get(a, SocketOption::SO_LINGER, 8), (Ok(0), linger));
    assert_eq!(
        set(a, SocketOption::SO_RCVLOWAT, &10i32.to_le_bytes()),
        Ok(0)
    );
    assert_eq!(
        get(a, SocketOption::SO_RCVLOWAT, 4),
        (Ok(0), 10i32.to_le_bytes().to_vec())
    );
    assert_eq!(
        set(a, SocketOption::SO_KEEPALIVE, &1i32.to_le_bytes()),
        Ok(0)
    );
    assert_eq!(
        get(a, SocketOption::SO_KEEPALIVE, 4),
        (Ok(0), 1i32.to_le_bytes().to_vec())
    );
    assert_eq!(
        set(a, SocketOption::SO_PASSCRED, &1i32.to_le_bytes()),
        Ok(0)
    );
    assert_eq!(
        get(a, SocketOption::SO_PASSCRED, 4),
        (Ok(0), 1i32.to_le_bytes().to_vec())
    );
    // Only IP sockets share ports.
    let reuseport = set(a, SocketOption::SO_REUSEPORT, &1i32.to_le_bytes());
    assert_eq!(errno(reuseport), Errno::EOPNOTSUPP);
    // An unknown option is ENOPROTOOPT both ways.
    assert_eq!(
        errno(set(a, SocketOption(9999), &[0; 4])),
        Errno::ENOPROTOOPT
    );
    assert_eq!(errno(get(a, SocketOption(9999), 4).0), Errno::ENOPROTOOPT);

    // The peer of a pair is the caller; groups come with it.
    let ucred = |pid: i32, uid: u32, gid: u32| {
        [pid.to_le_bytes(), uid.to_le_bytes(), gid.to_le_bytes()].concat()
    };
    let pid = kernel.process(task).pid.0 as i32;
    assert_eq!(
        get(a, SocketOption::SO_PEERCRED, 12),
        (Ok(0), ucred(pid, 1, 1))
    );
    assert_eq!(
        get(a, SocketOption::SO_PEERGROUPS, 4),
        (Ok(0), 1u32.to_le_bytes().to_vec())
    );
    let (short, _) = get(a, SocketOption::SO_PEERGROUPS, 0);
    assert_eq!(errno(short), Errno::ERANGE);
    assert_eq!(super::ipc::get(&memory, 0x2f0, 4), 4u32.to_le_bytes());

    // Without a peer: pid 0 and the invalid ids, and no groups.
    let lone = block_on(kernel.socket(&caller, AddressFamily::AF_UNIX, STREAM, 0)).unwrap();
    assert_eq!(
        get(lone, SocketOption::SO_PEERCRED, 12),
        (Ok(0), ucred(0, u32::MAX, u32::MAX))
    );
    assert_eq!(
        errno(get(lone, SocketOption::SO_PEERGROUPS, 64).0),
        Errno::ENODATA
    );
    // A listener reports whoever listened; a connected datagram socket has no peer.
    let (listener, _client, _) = unix_listener(&kernel, &caller, &memory, b"peercred");
    assert_eq!(
        get(listener, SocketOption::SO_PEERCRED, 12),
        (Ok(0), ucred(pid, 1, 1))
    );
    let dgram = block_on(kernel.socket(&caller, AddressFamily::AF_UNIX, DGRAM, 0)).unwrap();
    let target = block_on(kernel.socket(&caller, AddressFamily::AF_UNIX, DGRAM, 0)).unwrap();
    let mut addr = (AddressFamily::AF_UNIX.0 as u16).to_le_bytes().to_vec();
    addr.extend(b"\0dgram-peer");
    put(&memory, 0x200, &addr);
    let len = addr.len() as u32;
    assert_eq!(
        block_on(kernel.bind(&caller, target, GuestPtr::new(0x200), len)),
        Ok(0)
    );
    assert_eq!(
        block_on(kernel.connect(&caller, dgram, GuestPtr::new(0x200), len)),
        Ok(0)
    );
    assert_eq!(
        get(dgram, SocketOption::SO_PEERCRED, 12),
        (Ok(0), ucred(0, u32::MAX, u32::MAX))
    );
}

/// A peer's pid stays what `SO_PEERCRED` and `SCM_CREDENTIALS` report once it was reaped:
/// the socket and its queued message hold its `struct pid` (`sk_peer_pid`,
/// `UNIXCB(skb).pid`), which `pid_vnr` reads at the time of the call.
#[test]
fn a_reaped_peer_keeps_its_pid_in_its_credentials() {
    let (kernel, a, memory) = machine();
    let caller = Caller::new(a, &memory);
    let b = super::mapping::another_process(&kernel);
    let memory_b = FlatMemory(std::cell::RefCell::new(vec![0; 65536]));
    let caller_b = Caller::new(b, &memory_b);
    let mut addr = (AddressFamily::AF_UNIX.0 as u16).to_le_bytes().to_vec();
    addr.extend(b"\0reaped-peer");
    let len = addr.len() as u32;
    put(&memory, 0x200, &addr);
    put(&memory_b, 0x200, &addr);
    let listener = block_on(kernel.socket(&caller, AddressFamily::AF_UNIX, STREAM, 0)).unwrap();
    assert_eq!(
        block_on(kernel.bind(&caller, listener, GuestPtr::new(0x200), len)),
        Ok(0)
    );
    assert_eq!(block_on(kernel.listen(&caller, listener, 1)), Ok(0));
    let client = block_on(kernel.socket(&caller_b, AddressFamily::AF_UNIX, STREAM, 0)).unwrap();
    assert_eq!(
        block_on(kernel.connect(&caller_b, client, GuestPtr::new(0x200), len)),
        Ok(0)
    );
    let server = block_on(kernel.accept4(
        &caller,
        listener,
        GuestPtrMut::NULL,
        GuestPtrMut::NULL,
        SocketFlags(0),
    ))
    .unwrap();
    put(&memory, 0x400, &1i32.to_le_bytes());
    let passcred = SocketOption::SO_PASSCRED.0;
    let set = kernel.setsockopt(
        &caller,
        server,
        SocketLevel::SOL_SOCKET,
        passcred,
        UserSlice::new(0x400, 4),
    );
    assert_eq!(block_on(set), Ok(0));
    put(&memory_b, 0x400, b"x");
    assert_eq!(
        send_bytes(&kernel, &caller_b, client, 0x400, 1, MsgFlags(0)),
        Ok(1)
    );
    // B, without a parent, is reaped as it exits, and its pid is free.
    assert_eq!(block_on(kernel.exit_group(&caller_b, 0)), Ok(()));
    wasixv2_syscalls::Control::release(&kernel, b);
    assert_eq!(
        errno(block_on(kernel.sys_kill(&caller, b.0 as i32, Signal(0)))),
        Errno::ESRCH
    );

    put(&memory, 0x2f0, &12u32.to_le_bytes());
    let peercred = kernel.getsockopt(
        &caller,
        server,
        SocketLevel::SOL_SOCKET,
        SocketOption::SO_PEERCRED.0,
        GuestPtrMut::new(0x300),
        GuestPtrMut::new(0x2f0),
    );
    assert_eq!(block_on(peercred), Ok(0));
    assert_eq!(get_pid(&memory, 0x300), b.0 as i32);
    // recvmsg: one byte to 0x500, the control messages to 0xb00.
    put(&memory, 0x700, &0x500u32.to_le_bytes());
    put(&memory, 0x704, &1u32.to_le_bytes());
    let hdr = UserMsghdr {
        msg_name: GuestPtrMut::NULL,
        msg_namelen: 0,
        msg_iov: GuestPtr::new(0x700),
        msg_iovlen: 1,
        msg_control: GuestPtrMut::new(0xb00),
        msg_controllen: 64,
        msg_flags: 0,
    };
    block_on(write_value(&memory, GuestPtrMut::new(0x900), &hdr)).unwrap();
    let got = block_on(kernel.recvmsg(&caller, server, GuestPtrMut::new(0x900), MsgFlags(0)));
    assert_eq!(got, Ok(1));
    let kind = i32::from_le_bytes(get(&memory, 0xb08, 4).try_into().unwrap());
    assert_eq!(kind, crate::unix::SCM_CREDENTIALS);
    assert_eq!(get_pid(&memory, 0xb0c), b.0 as i32);
}

/// A stream send longer than one part carries the credentials it claims in every part, as
/// `unix_stream_sendmsg` attaches them to each skb (`unix_scm_to_skb`): a reader with
/// `SO_PASSCRED` sees one writer throughout.
#[test]
fn claimed_credentials_go_with_every_part_of_a_long_stream_send() {
    let (kernel, task, _) =
        super::cred::machine_with(|kernel| kernel.with_credentials(Credentials::new(0, 0)));
    let memory = FlatMemory(std::cell::RefCell::new(vec![0; 1 << 20]));
    let caller = Caller::new(task, &memory);
    let (a, b) = unix_pair(&kernel, &caller, &memory, STREAM);
    put(&memory, 0x400, &1i32.to_le_bytes());
    let passcred = SocketOption::SO_PASSCRED.0;
    let slice = UserSlice::new(0x400, 4);
    let set = kernel.setsockopt(&caller, b, SocketLevel::SOL_SOCKET, passcred, slice);
    assert_eq!(block_on(set), Ok(0));

    // Root claims uid and gid 5 for a send of one byte more than a part.
    let pid = kernel.process(task).pid.0 as i32;
    let mut cmsg = Vec::new();
    cmsg.extend(24u32.to_le_bytes());
    cmsg.extend(SocketLevel::SOL_SOCKET.0.to_le_bytes());
    cmsg.extend(crate::unix::SCM_CREDENTIALS.to_le_bytes());
    for word in [pid as u32, 5, 5] {
        cmsg.extend(word.to_le_bytes());
    }
    put(&memory, 0xa00, &cmsg);
    let len = crate::net::SOCK_CHUNK as u32 + 1;
    put(&memory, 0x700, &0x80000u32.to_le_bytes());
    put(&memory, 0x704, &len.to_le_bytes());
    let hdr = UserMsghdr {
        msg_name: GuestPtrMut::NULL,
        msg_namelen: 0,
        msg_iov: GuestPtr::new(0x700),
        msg_iovlen: 1,
        msg_control: GuestPtrMut::new(0xa00),
        msg_controllen: cmsg.len() as u32,
        msg_flags: 0,
    };
    block_on(write_value(&memory, GuestPtrMut::new(0x900), &hdr)).unwrap();
    let mut send = Box::pin(kernel.sendmsg(&caller, a, GuestPtr::new(0x900), MsgFlags(0)));

    // The send blocks as the queue fills; each read takes what came, with its writer's uid.
    let mut uids = Vec::new();
    let mut received = 0;
    let sent = loop {
        let done = send.as_mut().now_or_never();
        put(&memory, 0x708, &0x10000u32.to_le_bytes());
        put(&memory, 0x70c, &0x40000u32.to_le_bytes());
        let hdr = UserMsghdr {
            msg_iov: GuestPtr::new(0x708),
            msg_control: GuestPtrMut::new(0xb00),
            msg_controllen: 64,
            ..hdr
        };
        block_on(write_value(&memory, GuestPtrMut::new(0x980), &hdr)).unwrap();
        let flags = MsgFlags::MSG_DONTWAIT;
        let got = block_on(kernel.recvmsg(&caller, b, GuestPtrMut::new(0x980), flags));
        if let Ok(n) = got {
            received += n as u32;
            uids.push(u32::from_le_bytes(
                get(&memory, 0xb10, 4).try_into().unwrap(),
            ));
        }
        if let Some(sent) = done {
            break sent;
        }
    };
    while let Ok(n) =
        block_on(kernel.recvmsg(&caller, b, GuestPtrMut::new(0x980), MsgFlags::MSG_DONTWAIT))
    {
        received += n as u32;
        uids.push(u32::from_le_bytes(
            get(&memory, 0xb10, 4).try_into().unwrap(),
        ));
    }
    assert_eq!(sent, Ok(len as i32));
    assert_eq!(received, len);
    assert!(uids.iter().all(|&uid| uid == 5), "uids {uids:?}");
}

fn get_pid(memory: &FlatMemory, at: u32) -> i32 {
    i32::from_le_bytes(get(memory, at, 4).try_into().unwrap())
}

#[test]
fn ip_sends_check_socket_level_control_messages() {
    let (kernel, task, memory) = networked();
    let caller = Caller::new(task, &memory);
    let fd = socket(&kernel, &caller, DGRAM);
    sockaddr(&memory, 0x200, 7000);
    let to = Some((0x200, 16));
    let send = |cmsgs: &[(i32, i32, Vec<u8>)]| {
        send_with_controls(&kernel, &caller, &memory, fd, to, cmsgs)
    };
    let sol = SocketLevel::SOL_SOCKET.0;
    let word = |v: u32| v.to_le_bytes().to_vec();
    // Unix-only messages are ignored on IP sockets.
    assert_eq!(send(&[rights(&[fd])]), Ok(1));
    // SO_MARK needs CAP_NET_RAW or CAP_NET_ADMIN; low priorities are anyone's.
    assert_eq!(errno(send(&[(sol, 36, word(1))])), Errno::EPERM);
    assert_eq!(send(&[(sol, 12, word(6))]), Ok(1));
    assert_eq!(errno(send(&[(sol, 12, word(7))])), Errno::EPERM);
    assert_eq!(errno(send(&[(sol, 12, vec![1])])), Errno::EINVAL);
    // Timestamp requests for transmit only; unknown types fail.
    assert_eq!(send(&[(sol, 37, word(1 << 1))]), Ok(1));
    assert_eq!(errno(send(&[(sol, 37, word(1 << 3))])), Errno::EINVAL);
    assert_eq!(errno(send(&[(sol, 4242, Vec::new())])), Errno::EINVAL);
}

#[test]
fn netlink_connect_checks_its_address_and_shutdown_is_unsupported() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let fd = block_on(kernel.socket(&caller, AddressFamily::AF_NETLINK, 3, 0)).unwrap();
    let connect = |family: u16, pid: u32, groups: u32, len: u32| {
        let mut raw = family.to_le_bytes().to_vec();
        raw.extend([0, 0]);
        raw.extend(pid.to_le_bytes());
        raw.extend(groups.to_le_bytes());
        put(&memory, 0x200, &raw);
        block_on(kernel.connect(&caller, fd, GuestPtr::new(0x200), len))
    };
    let netlink = AddressFamily::AF_NETLINK.0 as u16;
    assert_eq!(connect(netlink, 0, 0, 12), Ok(0));
    assert_eq!(errno(connect(netlink, 0, 0, 8)), Errno::EINVAL);
    assert_eq!(
        errno(connect(AddressFamily::AF_INET.0 as u16, 0, 0, 16)),
        Errno::EINVAL
    );
    // Another process or a group needs CAP_NET_ADMIN.
    assert_eq!(errno(connect(netlink, 42, 0, 12)), Errno::EPERM);
    assert_eq!(errno(connect(netlink, 0, 1, 12)), Errno::EPERM);
    assert_eq!(connect(0, 0, 0, 2), Ok(0));
    let shut = block_on(kernel.shutdown(&caller, fd, wasixv2_abi::ShutdownHow::SHUT_RDWR));
    assert_eq!(errno(shut), Errno::EOPNOTSUPP);
}

/// `w` sends "abcdef" to the non-blocking stream `r`. A receive whose buffer faults, even
/// part-way, takes nothing, as `unix_stream_read_generic` and `tcp_recvmsg` consume an skb
/// only once all of it was copied.
fn stream_reads_that_fault_keep_the_data(
    kernel: &Kernel,
    caller: &Caller<'_>,
    memory: &FlatMemory,
    w: Fd,
    r: Fd,
) {
    put(memory, 0x400, b"abcdef");
    assert_eq!(send_bytes(kernel, caller, w, 0x400, 6, MsgFlags(0)), Ok(6));
    let recv = |at, len, flags| {
        block_on(kernel.recvfrom(
            caller,
            r,
            UserSliceMut::new(at, len),
            flags,
            GuestPtrMut::NULL,
            GuestPtrMut::NULL,
        ))
    };
    let outside = 1 << 30;
    assert_eq!(errno(recv(outside, 6, MsgFlags(0))), Errno::EFAULT);
    assert_eq!(
        errno(recv(outside, 6, MsgFlags::MSG_WAITALL)),
        Errno::EFAULT
    );
    let read = block_on(kernel.read(caller, r, UserSliceMut::new(outside, 6)));
    assert_eq!(errno(read), Errno::EFAULT);

    // Two bytes fit the first buffer; the second lies outside guest memory.
    put(memory, 0x80, &0x500u32.to_le_bytes());
    put(memory, 0x84, &2u32.to_le_bytes());
    put(memory, 0x88, &outside.to_le_bytes());
    put(memory, 0x8c, &4u32.to_le_bytes());
    let readv = block_on(kernel.readv(caller, r, GuestPtr::new(0x80), 2));
    assert_eq!(errno(readv), Errno::EFAULT);
    assert_eq!(recv(0x600, 9, MsgFlags(0)), Ok(6));
    assert_eq!(get(memory, 0x600, 6), b"abcdef");
}

#[test]
fn unix_stream_reads_that_fault_keep_the_data() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (a, b) = unix_pair(&kernel, &caller, &memory, STREAM | 0x800);
    stream_reads_that_fault_keep_the_data(&kernel, &caller, &memory, a, b);
}

#[test]
fn tcp_reads_that_fault_keep_the_data() {
    let (kernel, task, memory) = networked();
    let caller = Caller::new(task, &memory);
    let listener = socket(&kernel, &caller, STREAM);
    sockaddr(&memory, 0x200, 6001);
    assert_eq!(
        block_on(kernel.bind(&caller, listener, GuestPtr::new(0x200), 16)),
        Ok(0)
    );
    assert_eq!(block_on(kernel.listen(&caller, listener, 1)), Ok(0));
    let client = socket(&kernel, &caller, STREAM);
    let connect = block_on(kernel.connect(&caller, client, GuestPtr::new(0x200), 16));
    assert_eq!(connect, Ok(0));
    let accept = block_on(kernel.accept4(
        &caller,
        listener,
        GuestPtrMut::NULL,
        GuestPtrMut::NULL,
        SocketFlags::SOCK_NONBLOCK,
    ));
    let server = accept.unwrap();
    stream_reads_that_fault_keep_the_data(&kernel, &caller, &memory, client, server);
}
