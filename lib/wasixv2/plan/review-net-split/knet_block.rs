
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
    let inflight = kernel.unix_names.inflight.clone();
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
        kernel.spawn(ProcessConfig {
            args: vec!["test".into()],
            env: Vec::new(),
            stdio: Stdio {
                stdin: Arc::new(Input(Mutex::new(Vec::new()))),
                stdout: Arc::new(Capture::default()),
                stderr: Arc::new(Capture::default()),
            },
        })
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
        let limit = kernel.files(task).lock().unwrap().end() + 4;
        kernel.files(task).lock().unwrap().set_limit(limit);
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
