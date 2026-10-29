//! SCTP through the socket syscalls: records, `SOL_SCTP` options read in and out, control
//! messages, and the options the kernel serves itself (bindx, connectx, peeloff).

use std::net::SocketAddr;

use wasixv2_abi::{
    AddressFamily, GuestLayout, MsgFlags, SctpCmsgType, SctpGetaddrsOld, SctpOption,
    SctpPeeloffArg, SctpPeeloffFlagsArg, SctpRcvinfo, SctpSndinfo, SctpStatus, SocketFlags,
    SocketLevel, SocketOption, UserMsghdr,
};
use wasixv2_syscalls::{read_value, write_value};
use wasixv2_vfs::MountFlags;

use super::{
    ipc::{errno, get, put},
    *,
};

const STREAM: i32 = 1;
const SEQPACKET: i32 = 5;
const NONBLOCK: i32 = 0x800;
const IPPROTO_SCTP: i32 = 132;

/// A machine with a loopback namespace and `/proc`.
fn machine() -> (Kernel, TaskId, FlatMemory) {
    let vnet = wasixv2_vnet::Net::new(wasixv2_vnet::NetConfig::default(), None);
    let (kernel, task, memory) = super::cred::machine_with(|kernel| kernel.with_network(vnet));
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(b"/proc", kernel.procfs(), MountFlags::default(), true)).unwrap();
    (kernel, task, memory)
}

fn socket(kernel: &Kernel, caller: &Caller<'_>, r#type: i32, protocol: i32) -> Fd {
    block_on(kernel.socket(caller, AddressFamily::AF_INET, r#type, protocol)).unwrap()
}

fn addr(port: u16) -> Vec<u8> {
    crate::socket::encode(SocketAddr::new([127, 0, 0, 1].into(), port))
}

fn bind(kernel: &Kernel, caller: &Caller<'_>, memory: &FlatMemory, fd: Fd, port: u16) {
    put(memory, 0x200, &addr(port));
    assert_eq!(
        block_on(kernel.bind(caller, fd, GuestPtr::new(0x200), 16)),
        Ok(0)
    );
}

fn connect(
    kernel: &Kernel,
    caller: &Caller<'_>,
    memory: &FlatMemory,
    fd: Fd,
    port: u16,
) -> SysResult<i32> {
    put(memory, 0x200, &addr(port));
    block_on(kernel.connect(caller, fd, GuestPtr::new(0x200), 16))
}

fn setsockopt(
    kernel: &Kernel,
    caller: &Caller<'_>,
    memory: &FlatMemory,
    fd: Fd,
    name: SctpOption,
    value: &[u8],
) -> SysResult<i32> {
    put(memory, 0x2000, value);
    block_on(kernel.setsockopt(
        caller,
        fd,
        SocketLevel::SOL_SCTP,
        name.0,
        UserSlice::new(0x2000, value.len() as u32),
    ))
}

/// A `SOL_SCTP` `getsockopt` whose value starts as `value`: the result, the value and length
/// written back.
fn getsockopt(
    kernel: &Kernel,
    caller: &Caller<'_>,
    memory: &FlatMemory,
    fd: Fd,
    name: SctpOption,
    value: &[u8],
) -> (SysResult<i32>, Vec<u8>) {
    put(memory, 0x2000, value);
    put(memory, 0x1ff0, &(value.len() as u32).to_le_bytes());
    let result = block_on(kernel.getsockopt(
        caller,
        fd,
        SocketLevel::SOL_SCTP,
        name.0,
        GuestPtrMut::new(0x2000),
        GuestPtrMut::new(0x1ff0),
    ));
    let len = u32::from_le_bytes(get(memory, 0x1ff0, 4).try_into().unwrap()) as usize;
    (result, get(memory, 0x2000, len))
}

fn bytes<T: GuestLayout>(value: &T) -> Vec<u8> {
    let mut raw = vec![0; T::SIZE];
    value.write_to(&mut raw);
    raw
}

/// A one-to-one listener on `port`, a client connected to it and the accepted socket.
fn pair(kernel: &Kernel, caller: &Caller<'_>, memory: &FlatMemory, port: u16) -> (Fd, Fd, Fd) {
    let l = socket(kernel, caller, STREAM, IPPROTO_SCTP);
    bind(kernel, caller, memory, l, port);
    assert_eq!(block_on(kernel.listen(caller, l, 5)), Ok(0));
    let c = socket(kernel, caller, STREAM, IPPROTO_SCTP);
    // A blocking connect returns once the association is up.
    assert_eq!(connect(kernel, caller, memory, c, port), Ok(0));
    let s = block_on(kernel.accept4(
        caller,
        l,
        GuestPtrMut::NULL,
        GuestPtrMut::NULL,
        SocketFlags(0),
    ));
    (l, c, s.unwrap())
}

fn send(
    kernel: &Kernel,
    caller: &Caller<'_>,
    memory: &FlatMemory,
    fd: Fd,
    data: &[u8],
) -> SysResult<u32> {
    put(memory, 0x400, data);
    block_on(kernel.sendto(
        caller,
        fd,
        UserSlice::new(0x400, data.len() as u32),
        MsgFlags(0),
        GuestPtr::NULL,
        0,
    ))
}

fn recv(
    kernel: &Kernel,
    caller: &Caller<'_>,
    memory: &FlatMemory,
    fd: Fd,
    len: u32,
    flags: MsgFlags,
) -> Vec<u8> {
    let n = block_on(kernel.recvfrom(
        caller,
        fd,
        UserSliceMut::new(0x600, len),
        flags,
        GuestPtrMut::NULL,
        GuestPtrMut::NULL,
    ));
    get(memory, 0x600, n.unwrap() as usize)
}

#[test]
fn sctp_sockets_of_both_styles() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let seq = socket(&kernel, &caller, SEQPACKET, 0);
    let one = socket(&kernel, &caller, STREAM, IPPROTO_SCTP);
    let int = |fd, name: SocketOption| {
        put(&memory, 0x1ff0, &4u32.to_le_bytes());
        let got = block_on(kernel.getsockopt(
            &caller,
            fd,
            SocketLevel::SOL_SOCKET,
            name.0,
            GuestPtrMut::new(0x2000),
            GuestPtrMut::new(0x1ff0),
        ));
        assert_eq!(got, Ok(0));
        i32::from_le_bytes(get(&memory, 0x2000, 4).try_into().unwrap())
    };
    assert_eq!(
        (
            int(seq, SocketOption::SO_TYPE),
            int(seq, SocketOption::SO_PROTOCOL)
        ),
        (SEQPACKET, IPPROTO_SCTP)
    );
    assert_eq!(
        (
            int(one, SocketOption::SO_TYPE),
            int(one, SocketOption::SO_PROTOCOL)
        ),
        (STREAM, IPPROTO_SCTP)
    );
    // `inetsw` has only SCTP for `SOCK_SEQPACKET`.
    let tcp_seq = block_on(kernel.socket(&caller, AddressFamily::AF_INET, SEQPACKET, 6));
    assert_eq!(errno(tcp_seq), Errno::EPROTONOSUPPORT);
}

#[test]
fn one_to_one_messages_keep_their_boundaries() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (_l, c, s) = pair(&kernel, &caller, &memory, 7000);
    assert_eq!(send(&kernel, &caller, &memory, c, b"first"), Ok(5));
    assert_eq!(send(&kernel, &caller, &memory, c, b"second"), Ok(6));
    // Each receive is one message, even with room and MSG_WAITALL for more.
    assert_eq!(
        recv(&kernel, &caller, &memory, s, 64, MsgFlags::MSG_WAITALL),
        b"first"
    );
    assert_eq!(
        recv(&kernel, &caller, &memory, s, 64, MsgFlags(0)),
        b"second"
    );
    // A read(2) is a receive too.
    send(&kernel, &caller, &memory, s, b"back").unwrap();
    let n = block_on(kernel.read(&caller, c, UserSliceMut::new(0x600, 64)));
    assert_eq!(n, Ok(4));
}

#[test]
fn sctp_options_are_read_in_and_out() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (_l, c, _s) = pair(&kernel, &caller, &memory, 7001);
    let (got, value) = getsockopt(
        &kernel,
        &caller,
        &memory,
        c,
        SctpOption::SCTP_STATUS,
        &bytes(&SctpStatus::ZERO),
    );
    assert_eq!(got, Ok(0));
    let status = SctpStatus::read_from(&value).unwrap();
    assert_eq!((status.sstat_state, status.sstat_outstrms), (4, 10));
    let (got, _) = getsockopt(
        &kernel,
        &caller,
        &memory,
        c,
        SctpOption::SCTP_STATUS,
        &[0; 8],
    );
    assert_eq!(errno(got), Errno::EINVAL);
    // GET_PEER_ADDRS writes past the header and reports how far.
    let (got, value) = getsockopt(
        &kernel,
        &caller,
        &memory,
        c,
        SctpOption::SCTP_GET_PEER_ADDRS,
        &[0; 64],
    );
    assert_eq!(got, Ok(0));
    assert_eq!(value.len(), 8 + 16);
    assert_eq!(&value[8..], addr(7001).as_slice());
}

#[test]
fn control_messages_pass_through() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (_l, c, s) = pair(&kernel, &caller, &memory, 7002);
    setsockopt(
        &kernel,
        &caller,
        &memory,
        s,
        SctpOption::SCTP_RECVRCVINFO,
        &1i32.to_le_bytes(),
    )
    .unwrap();
    // sendmsg with an SCTP_SNDINFO: stream 4, ppid 99.
    let info = SctpSndinfo {
        snd_sid: 4,
        snd_ppid: 99,
        ..SctpSndinfo::ZERO
    };
    let mut cmsg = Vec::new();
    cmsg.extend(((12 + SctpSndinfo::SIZE) as u32).to_le_bytes());
    cmsg.extend(IPPROTO_SCTP.to_le_bytes());
    cmsg.extend(SctpCmsgType::SCTP_SNDINFO.0.to_le_bytes());
    cmsg.extend(bytes(&info));
    put(&memory, 0x800, &cmsg);
    put(&memory, 0x400, b"data");
    put(&memory, 0x700, &0x400u32.to_le_bytes());
    put(&memory, 0x704, &4u32.to_le_bytes());
    let hdr = UserMsghdr {
        msg_name: GuestPtrMut::NULL,
        msg_namelen: 0,
        msg_iov: GuestPtr::new(0x700),
        msg_iovlen: 1,
        msg_control: GuestPtrMut::new(0x800),
        msg_controllen: cmsg.len() as u32,
        msg_flags: 0,
    };
    block_on(write_value(&memory, GuestPtrMut::new(0x900), &hdr)).unwrap();
    assert_eq!(
        block_on(kernel.sendmsg(&caller, c, GuestPtr::new(0x900), MsgFlags(0))),
        Ok(4)
    );
    // recvmsg: the data, MSG_EOR and an SCTP_RCVINFO.
    put(&memory, 0x704, &64u32.to_le_bytes());
    let hdr = UserMsghdr {
        msg_control: GuestPtrMut::new(0xa00),
        msg_controllen: 64,
        ..hdr
    };
    block_on(write_value(&memory, GuestPtrMut::new(0x900), &hdr)).unwrap();
    assert_eq!(
        block_on(kernel.recvmsg(&caller, s, GuestPtrMut::new(0x900), MsgFlags(0))),
        Ok(4)
    );
    let hdr: UserMsghdr = block_on(read_value(&memory, GuestPtr::new(0x900))).unwrap();
    assert!(MsgFlags(hdr.msg_flags).contains(MsgFlags::MSG_EOR));
    assert_eq!(hdr.msg_controllen as usize, 12 + SctpRcvinfo::SIZE);
    let ctl = get(&memory, 0xa00, 12 + SctpRcvinfo::SIZE);
    assert_eq!(
        i32::from_le_bytes(ctl[4..8].try_into().unwrap()),
        IPPROTO_SCTP
    );
    assert_eq!(
        i32::from_le_bytes(ctl[8..12].try_into().unwrap()),
        SctpCmsgType::SCTP_RCVINFO.0
    );
    let rcv = SctpRcvinfo::read_from(&ctl[12..]).unwrap();
    assert_eq!((rcv.rcv_sid, rcv.rcv_ppid), (4, 99));
}

#[test]
fn bindx_and_connectx_take_packed_addresses() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let l = socket(&kernel, &caller, SEQPACKET, 0);
    let mut packed = addr(7003);
    packed.extend(crate::socket::encode(SocketAddr::new(
        [127, 0, 0, 2].into(),
        7003,
    )));
    let bindx = |fd, raw: &[u8]| {
        setsockopt(
            &kernel,
            &caller,
            &memory,
            fd,
            SctpOption::SCTP_SOCKOPT_BINDX_ADD,
            raw,
        )
    };
    // A family with no SCTP address, an array cut short, an empty one.
    assert_eq!(errno(bindx(l, &[1, 0, 0, 0])), Errno::EINVAL);
    assert_eq!(errno(bindx(l, &packed[..20])), Errno::EINVAL);
    assert_eq!(errno(bindx(l, &[])), Errno::EINVAL);
    assert_eq!(bindx(l, &packed), Ok(0));
    assert_eq!(block_on(kernel.listen(&caller, l, 5)), Ok(0));
    // CONNECTX returns the association's id; a non-blocking caller hears EINPROGRESS.
    let c = socket(&kernel, &caller, SEQPACKET, 0);
    let id = setsockopt(
        &kernel,
        &caller,
        &memory,
        c,
        SctpOption::SCTP_SOCKOPT_CONNECTX,
        &packed,
    )
    .unwrap();
    assert!(id > 2);
    let nb = socket(&kernel, &caller, SEQPACKET | NONBLOCK, 0);
    let got = setsockopt(
        &kernel,
        &caller,
        &memory,
        nb,
        SctpOption::SCTP_SOCKOPT_CONNECTX,
        &packed,
    );
    assert_eq!(errno(got), Errno::EINPROGRESS);
    // CONNECTX3 reads the addresses through a pointer and writes the id over the value.
    let c3 = socket(&kernel, &caller, SEQPACKET, 0);
    put(&memory, 0x3000, &packed);
    let param = SctpGetaddrsOld {
        assoc_id: 0,
        addr_num: packed.len() as i32,
        addrs: GuestPtr::new(0x3000),
    };
    let (got, value) = getsockopt(
        &kernel,
        &caller,
        &memory,
        c3,
        SctpOption::SCTP_SOCKOPT_CONNECTX3,
        &bytes(&param),
    );
    assert_eq!(got, Ok(0));
    assert_eq!(value.len(), 4);
    assert!(i32::from_le_bytes(value.try_into().unwrap()) > id);
    // `__sctp_connect`: an association with the peer exists.
    assert_eq!(
        errno(connect(&kernel, &caller, &memory, c, 7003)),
        Errno::EISCONN
    );
}

#[test]
fn peeloff_makes_a_descriptor() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let l = socket(&kernel, &caller, SEQPACKET, 0);
    bind(&kernel, &caller, &memory, l, 7004);
    assert_eq!(block_on(kernel.listen(&caller, l, 5)), Ok(0));
    let c = socket(&kernel, &caller, SEQPACKET, 0);
    let id = setsockopt(
        &kernel,
        &caller,
        &memory,
        c,
        SctpOption::SCTP_SOCKOPT_CONNECTX,
        &addr(7004),
    )
    .unwrap();
    // A one-to-many send names its association, here by the peer's address.
    assert_eq!(
        errno(send(&kernel, &caller, &memory, c, b"x")),
        Errno::EPIPE
    );
    put(&memory, 0x200, &addr(7004));
    put(&memory, 0x400, b"to peel");
    let sent = block_on(kernel.sendto(
        &caller,
        c,
        UserSlice::new(0x400, 7),
        MsgFlags(0),
        GuestPtr::new(0x200),
        16,
    ));
    assert_eq!(sent, Ok(7));
    // The client's own association, peeled off its one-to-many socket.
    let arg = SctpPeeloffArg {
        associd: id,
        sd: -1,
    };
    let (got, value) = getsockopt(
        &kernel,
        &caller,
        &memory,
        c,
        SctpOption::SCTP_SOCKOPT_PEELOFF,
        &bytes(&arg),
    );
    let fd = got.unwrap();
    assert_eq!(SctpPeeloffArg::read_from(&value).unwrap().sd, fd);
    assert_eq!(send(&kernel, &caller, &memory, Fd(fd), b"again"), Ok(5));
    assert_eq!(
        recv(&kernel, &caller, &memory, l, 64, MsgFlags(0)),
        b"to peel"
    );
    assert_eq!(
        recv(&kernel, &caller, &memory, l, 64, MsgFlags(0)),
        b"again"
    );
    // The association is gone from the original socket; a short value is EINVAL.
    let (got, _) = getsockopt(
        &kernel,
        &caller,
        &memory,
        c,
        SctpOption::SCTP_SOCKOPT_PEELOFF,
        &bytes(&arg),
    );
    assert_eq!(errno(got), Errno::EINVAL);
    let (got, _) = getsockopt(
        &kernel,
        &caller,
        &memory,
        c,
        SctpOption::SCTP_SOCKOPT_PEELOFF,
        &[0; 4],
    );
    assert_eq!(errno(got), Errno::EINVAL);
    // PEELOFF_FLAGS: the server's association, non-blocking and close-on-exec.
    let mut ids = [0u8; 8];
    ids[0] = 0;
    let (got, list) = getsockopt(
        &kernel,
        &caller,
        &memory,
        l,
        SctpOption::SCTP_GET_ASSOC_ID_LIST,
        &ids,
    );
    assert_eq!(got, Ok(0));
    let server_id = i32::from_le_bytes(list[4..8].try_into().unwrap());
    let arg = SctpPeeloffFlagsArg {
        p_arg: SctpPeeloffArg {
            associd: server_id,
            sd: -1,
        },
        flags: (SocketFlags::SOCK_NONBLOCK | SocketFlags::SOCK_CLOEXEC).0,
    };
    let (got, _) = getsockopt(
        &kernel,
        &caller,
        &memory,
        l,
        SctpOption::SCTP_SOCKOPT_PEELOFF_FLAGS,
        &bytes(&arg),
    );
    let fd = Fd(got.unwrap());
    assert!(
        kernel
            .file(task, fd)
            .unwrap()
            .flags()
            .contains(OpenFlags::O_NONBLOCK)
    );
    assert_eq!(kernel.files(task).lock_or_recover().cloexec(fd), Some(true));
    let n = block_on(kernel.recvfrom(
        &caller,
        fd,
        UserSliceMut::new(0x600, 64),
        MsgFlags(0),
        GuestPtrMut::NULL,
        GuestPtrMut::NULL,
    ));
    assert_eq!(errno(n), Errno::EAGAIN);
}

#[test]
fn proc_net_sctp_lists_endpoints_and_associations() {
    let (kernel, task, memory) = machine();
    let caller = Caller::new(task, &memory);
    let (_l, _c, _s) = pair(&kernel, &caller, &memory, 7005);
    let eps = super::ns::read_text(&kernel, &caller, "/proc/net/sctp/eps");
    let mut lines = eps.lines();
    assert_eq!(
        lines.next(),
        Some(" ENDPT     SOCK   STY SST HBKT LPORT   UID INODE LADDRS")
    );
    let ep: Vec<&str> = lines.next().unwrap().split_whitespace().collect();
    // Style 0 (one-to-one), TCP_LISTEN, bucket 7005 & 63, the port, the address.
    assert_eq!(
        (ep[2], ep[3], ep[4], ep[5], ep[8]),
        ("0", "10", "29", "7005", "127.0.0.1")
    );
    let assocs = super::ns::read_text(&kernel, &caller, "/proc/net/sctp/assocs");
    // The client's association and the accepted one.
    assert_eq!(assocs.lines().count(), 3);
    assert!(assocs.lines().nth(1).unwrap().contains("<-> *127.0.0.1"));
    let snmp = super::ns::read_text(&kernel, &caller, "/proc/net/sctp/snmp");
    assert!(snmp.starts_with("SctpCurrEstab                   \t2\n"));
    assert_eq!(snmp.lines().count(), 32);
}
