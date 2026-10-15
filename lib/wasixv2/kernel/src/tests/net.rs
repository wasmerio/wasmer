//! The socket syscalls against a small in-test network implementing the vnet traits.

use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    sync::Weak,
};

use wasixv2_abi::{AddressFamily, MsgFlags, SocketFlags, SocketLevel, SocketOption, UserMsghdr};
use wasixv2_vnet::{
    Accepted, Control, Domain, Network, RecvMsg, SendMsg, Shutdown, Socket, SocketKind,
};
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, poll_wait};

use super::{
    ipc::{errno, get, machine, put},
    *,
};

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
        let bound = self.bound.lock().unwrap();
        let any = SocketAddr::new([0, 0, 0, 0].into(), addr.port());
        bound
            .get(&addr)
            .and_then(Weak::upgrade)
            .or_else(|| bound.get(&any).and_then(Weak::upgrade))
    }
}

impl Network for FakeNet {
    fn socket(&self, _: Domain, kind: SocketKind, protocol: i32) -> Result<Arc<dyn Socket>, Errno> {
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
            let mut next = net.next_port.lock().unwrap();
            addr.set_port(*next);
            *next += 1;
        }
        let mut bound = net.bound.lock().unwrap();
        if bound.get(&addr).is_some_and(|s| s.strong_count() > 0) {
            return Err(Errno::EADDRINUSE);
        }
        bound.insert(addr, self.me.clone());
        self.state.lock().unwrap().local = Some(addr);
        Ok(addr)
    }

    fn autobind(&self) -> SocketAddr {
        let local = self.state.lock().unwrap().local;
        local.unwrap_or_else(|| self.register("127.0.0.1:0".parse().unwrap()).unwrap())
    }

    fn deliver(&self, data: &[u8], from: SocketAddr) {
        self.state
            .lock()
            .unwrap()
            .rx
            .push_back((data.to_vec(), from));
        self.queue.wake(PollMask::READABLE);
    }
}

impl Pollable for FakeSocket {
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, &self.queue);
        let state = self.state.lock().unwrap();
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
        }
    }

    fn bind(&self, addr: SocketAddr, privileged: bool) -> Result<(), Errno> {
        if (1..1024).contains(&addr.port()) && !privileged {
            return Err(Errno::EACCES);
        }
        if self.state.lock().unwrap().local.is_some() {
            return Err(Errno::EINVAL);
        }
        self.register(addr).map(drop)
    }

    fn listen(&self, _: u32) -> Result<(), Errno> {
        self.autobind();
        Ok(())
    }

    fn accept(&self) -> Result<Accepted, Errno> {
        let mut state = self.state.lock().unwrap();
        state.backlog.pop_front().ok_or(Errno::EAGAIN)
    }

    fn connect(&self, addr: SocketAddr) -> Result<(), Errno> {
        let local = self.autobind();
        if self.kind == SocketKind::Datagram {
            self.state.lock().unwrap().peer = Some(addr);
            return Ok(());
        }
        if self.state.lock().unwrap().peer.is_some() {
            return Err(Errno::EISCONN);
        }
        let listener = self.net().lookup(addr).ok_or(Errno::ECONNREFUSED)?;
        let server = self.net().new_socket(SocketKind::Stream);
        {
            let mut state = server.state.lock().unwrap();
            state.local = Some(addr);
            state.peer = Some(local);
            state.peer_socket = self.me.clone();
        }
        {
            let mut state = self.state.lock().unwrap();
            state.peer = Some(addr);
            state.peer_socket = Arc::downgrade(&server);
        }
        listener.state.lock().unwrap().backlog.push_back(Accepted {
            socket: server,
            peer: local,
        });
        listener.queue.wake(PollMask::READABLE);
        Err(Errno::EINPROGRESS)
    }

    fn disconnect(&self) -> Result<(), Errno> {
        self.state.lock().unwrap().peer = None;
        Ok(())
    }

    fn shutdown(&self, _: Shutdown) -> Result<(), Errno> {
        Ok(())
    }

    fn send(&self, msg: SendMsg<'_>) -> Result<usize, Errno> {
        let from = self.autobind();
        let (peer, peer_socket) = {
            let state = self.state.lock().unwrap();
            (state.peer, state.peer_socket.upgrade())
        };
        let target = match (self.kind, msg.to.or(peer)) {
            (SocketKind::Stream, _) => peer_socket.ok_or(Errno::ENOTCONN)?,
            (SocketKind::Datagram, Some(to)) => match self.net().lookup(to) {
                Some(target) => target,
                None => return Ok(msg.data.len()),
            },
            (SocketKind::Datagram, None) => return Err(Errno::EDESTADDRREQ),
        };
        target.deliver(msg.data, from);
        Ok(msg.data.len())
    }

    fn recv(&self, buf: &mut [u8], flags: MsgFlags) -> Result<RecvMsg, Errno> {
        let mut state = self.state.lock().unwrap();
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
        let local = self.state.lock().unwrap().local;
        Ok(local.unwrap_or("0.0.0.0:0".parse().unwrap()))
    }

    fn peer_addr(&self) -> Result<SocketAddr, Errno> {
        self.state.lock().unwrap().peer.ok_or(Errno::ENOTCONN)
    }

    fn take_error(&self) -> Option<Errno> {
        None
    }

    fn input_len(&self) -> Result<usize, Errno> {
        let state = self.state.lock().unwrap();
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
    let (kernel, task, memory) = machine();
    (kernel.with_network(FakeNet::new()), task, memory)
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
    assert_eq!(create(AddressFamily::AF_INET, 3), Errno::EPERM);
    assert_eq!(create(AddressFamily::AF_INET, 5), Errno::ESOCKTNOSUPPORT);
    assert_eq!(create(AddressFamily(99), STREAM), Errno::EAFNOSUPPORT);
    let bad_protocol = block_on(kernel.socket(&caller, AddressFamily::AF_INET, STREAM, 99));
    assert_eq!(errno(bad_protocol), Errno::EPROTONOSUPPORT);

    let nonblocking = 0x800 | 0x80000;
    let fd = block_on(kernel.socket(&caller, AddressFamily::AF_INET, STREAM | nonblocking, 0));
    let fd = fd.unwrap();
    let file = kernel.file(task, fd).unwrap();
    assert!(file.flags().contains(OpenFlags::O_NONBLOCK));
    assert_eq!(kernel.files(task).lock().unwrap().cloexec(fd), Some(true));

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
