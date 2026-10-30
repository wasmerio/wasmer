//! SCTP sockets in one namespace and across a veth.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use wasixv2_abi::{
    Errno, GuestLayout, MsgFlags, SctpAssocChange, SctpAssocValue, SctpCmsgType, SctpGetaddrs,
    SctpInitmsg, SctpOption, SctpPrim, SctpRcvinfo, SctpSacState, SctpShutdownEvent, SctpSnType,
    SctpSndinfo, SctpSndrcvinfo, SctpStatus, SockaddrStorage,
};
use wasixv2_wait::PollMask;

use super::{IPPROTO_SCTP, SOL_SCTP};
use crate::{Control, Domain, Net, NetConfig, Network, SendMsg, Shutdown, Socket, SocketKind};

fn net() -> Arc<Net> {
    Net::new(NetConfig::default(), None)
}

fn a(s: &str) -> SocketAddr {
    s.parse().unwrap()
}

fn stream(n: &Net) -> Arc<dyn Socket> {
    n.socket(Domain::Inet, SocketKind::Stream, IPPROTO_SCTP, 1)
        .unwrap()
}

fn seqpacket(n: &Net) -> Arc<dyn Socket> {
    n.socket(Domain::Inet, SocketKind::SeqPacket, 0, 1).unwrap()
}

fn send_ctl(
    s: &dyn Socket,
    data: &[u8],
    to: Option<SocketAddr>,
    control: &[Control],
) -> Result<usize, Errno> {
    s.send(SendMsg {
        data,
        to,
        flags: MsgFlags(0),
        control,
        net_raw: false,
        now: Duration::ZERO,
    })
}

fn send(s: &dyn Socket, data: &[u8]) -> Result<usize, Errno> {
    send_ctl(s, data, None, &[])
}

/// One receive: the bytes, the flags and the control messages.
fn recv(s: &dyn Socket, n: usize) -> Result<(Vec<u8>, MsgFlags, Vec<Control>), Errno> {
    let mut buf = vec![0; n];
    let r = s.recv(&mut buf, MsgFlags(0))?;
    buf.truncate(r.len);
    Ok((buf, r.flags, r.control))
}

fn set<T: GuestLayout>(s: &dyn Socket, name: SctpOption, value: &T) -> Result<(), Errno> {
    let mut raw = vec![0; T::SIZE];
    value.write_to(&mut raw);
    s.set_option(SOL_SCTP, name.0, &raw)
}

fn set_int(s: &dyn Socket, name: SctpOption, value: i32) -> Result<(), Errno> {
    s.set_option(SOL_SCTP, name.0, &value.to_le_bytes())
}

/// A `getsockopt` whose value starts as `req`.
fn get<T: GuestLayout>(s: &dyn Socket, name: SctpOption, req: &T) -> Result<T, Errno> {
    let mut raw = vec![0; T::SIZE];
    req.write_to(&mut raw);
    let n = s.get_option(SOL_SCTP, name.0, &mut raw)?;
    assert_eq!(n, T::SIZE);
    Ok(T::read_from(&raw).unwrap())
}

fn get_int(s: &dyn Socket, name: SctpOption) -> Result<i32, Errno> {
    let mut raw = [0; 4];
    s.get_option(SOL_SCTP, name.0, &mut raw)?;
    Ok(i32::from_le_bytes(raw))
}

/// Subscribes to association change and shutdown notifications.
fn subscribe(s: &dyn Socket) {
    // sctp_event_subscribe: data_io, association, address, send_failure, peer_error, shutdown.
    s.set_option(SOL_SCTP, SctpOption::SCTP_EVENTS.0, &[0, 1, 0, 0, 0, 1])
        .unwrap();
}

fn assoc_change(raw: &[u8]) -> SctpAssocChange {
    SctpAssocChange::read_from(raw).unwrap()
}

/// A listener on 127.0.0.1:`port` and a connected client with its accepted end.
fn connected(n: &Net, port: u16) -> (Arc<dyn Socket>, Arc<dyn Socket>, Arc<dyn Socket>) {
    let l = stream(n);
    l.bind(a(&format!("127.0.0.1:{port}")), true).unwrap();
    l.listen(5).unwrap();
    let c = stream(n);
    assert_eq!(
        c.connect(a(&format!("127.0.0.1:{port}"))),
        Err(Errno::EINPROGRESS)
    );
    assert_eq!(
        c.connect(a(&format!("127.0.0.1:{port}"))),
        Err(Errno::EISCONN)
    );
    let s = l.accept().unwrap().socket;
    (l, c, s)
}

#[test]
fn one_to_one_keeps_message_boundaries() {
    let n = net();
    let (l, c, s) = connected(&n, 5000);
    assert_eq!(l.poll(None), PollMask::empty());
    assert_eq!(c.peer_addr().unwrap(), a("127.0.0.1:5000"));
    assert_eq!(
        s.peer_addr().unwrap(),
        c.local_addr()
            .map(|l| a(&format!("127.0.0.1:{}", l.port())))
            .unwrap()
    );
    assert!(c.poll(None).contains(PollMask::OUT));
    send(&*c, b"hello").unwrap();
    send(&*c, b"world!").unwrap();
    assert_eq!(s.input_len().unwrap(), 5);
    let (data, flags, _) = recv(&*s, 64).unwrap();
    assert_eq!(data, b"hello");
    assert!(flags.contains(MsgFlags::MSG_EOR));
    // A short buffer reads a message in parts; only the last part ends the record.
    let (data, flags, _) = recv(&*s, 4).unwrap();
    assert_eq!(
        (data.as_slice(), flags.contains(MsgFlags::MSG_EOR)),
        (&b"worl"[..], false)
    );
    let (data, flags, _) = recv(&*s, 4).unwrap();
    assert_eq!(
        (data.as_slice(), flags.contains(MsgFlags::MSG_EOR)),
        (&b"d!"[..], true)
    );
    assert_eq!(recv(&*s, 4).unwrap_err(), Errno::EAGAIN);
    send(&*s, b"back").unwrap();
    assert_eq!(recv(&*c, 64).unwrap().0, b"back");
    assert_eq!(c.max_record(), Some(super::MAX_MESSAGE));
}

#[test]
fn refused_without_a_listener_or_room() {
    let n = net();
    let c = stream(&n);
    assert_eq!(c.connect(a("127.0.0.1:5001")), Err(Errno::ECONNREFUSED));
    // An address no association can use.
    assert_eq!(c.connect(a("0.0.0.0:5001")), Err(Errno::EINVAL));
    let l = stream(&n);
    l.bind(a("127.0.0.1:5001"), true).unwrap();
    l.listen(1).unwrap();
    for _ in 0..2 {
        assert_eq!(
            stream(&n).connect(a("127.0.0.1:5001")),
            Err(Errno::EINPROGRESS)
        );
    }
    // `sk_acceptq_is_full`: past the backlog an INIT gets an ABORT.
    assert_eq!(
        stream(&n).connect(a("127.0.0.1:5001")),
        Err(Errno::ECONNREFUSED)
    );
    assert!(l.poll(None).contains(PollMask::IN));
}

#[test]
fn notifications_and_graceful_shutdown() {
    let n = net();
    let l = stream(&n);
    l.bind(a("127.0.0.1:5002"), true).unwrap();
    subscribe(&*l);
    l.listen(5).unwrap();
    let c = stream(&n);
    subscribe(&*c);
    c.connect(a("127.0.0.1:5002")).unwrap_err();
    let s = l.accept().unwrap().socket;
    for sock in [&c, &s] {
        let (raw, flags, _) = recv(&**sock, 64).unwrap();
        assert!(flags.contains(super::MSG_NOTIFICATION) && flags.contains(MsgFlags::MSG_EOR));
        let change = assoc_change(&raw);
        assert_eq!(change.sac_type, SctpSnType::SCTP_ASSOC_CHANGE.0);
        assert_eq!(change.sac_state, SctpSacState::SCTP_COMM_UP.0);
        assert_eq!(
            (change.sac_outbound_streams, change.sac_inbound_streams),
            (10, 10)
        );
    }
    send(&*c, b"last").unwrap();
    c.shutdown(Shutdown::Write).unwrap();
    // The peer reads what came first, then the shutdown, then the end.
    assert_eq!(recv(&*s, 64).unwrap().0, b"last");
    let raw = recv(&*s, 64).unwrap().0;
    let shut = SctpShutdownEvent::read_from(&raw).unwrap();
    assert_eq!(shut.sse_type, SctpSnType::SCTP_SHUTDOWN_EVENT.0);
    let raw = recv(&*s, 64).unwrap().0;
    assert_eq!(
        assoc_change(&raw).sac_state,
        SctpSacState::SCTP_SHUTDOWN_COMP.0
    );
    assert!(s.poll(None).contains(PollMask::RDHUP));
    assert_eq!(recv(&*s, 64).unwrap().0, b"");
    assert_eq!(send(&*s, b"x"), Err(Errno::EPIPE));
    let raw = recv(&*c, 64).unwrap().0;
    assert_eq!(
        assoc_change(&raw).sac_state,
        SctpSacState::SCTP_SHUTDOWN_COMP.0
    );
    // The association is gone: nothing more to read on the initiator's side.
    assert_eq!(recv(&*c, 64).unwrap_err(), Errno::ENOTCONN);
}

#[test]
fn close_with_unread_data_aborts() {
    let n = net();
    let (_l, c, s) = connected(&n, 5003);
    subscribe(&*c);
    send(&*c, b"unread").unwrap();
    drop(s);
    assert!(c.poll(None).contains(PollMask::ERR | PollMask::IN));
    // `sctp_skb_recv_datagram` reports the socket's error before what is queued.
    assert_eq!(recv(&*c, 64).unwrap_err(), Errno::ECONNRESET);
    let raw = recv(&*c, 64).unwrap().0;
    let change = assoc_change(&raw);
    assert_eq!(change.sac_state, SctpSacState::SCTP_COMM_LOST.0);
    assert_eq!(change.sac_error, super::ERROR_USER_ABORT);
    assert_eq!(change.sac_length as usize, SctpAssocChange::SIZE + 4);
    assert_eq!(send(&*c, b"x"), Err(Errno::EPIPE));
}

#[test]
fn send_and_receive_info() {
    let n = net();
    let (_l, c, s) = connected(&n, 5004);
    set_int(&*s, SctpOption::SCTP_RECVRCVINFO, 1).unwrap();
    // sctp_event_subscribe's first byte: SCTP_SNDRCV with each message.
    s.set_option(SOL_SCTP, SctpOption::SCTP_EVENTS.0, &[1])
        .unwrap();
    set(
        &*s,
        SctpOption::SCTP_CONTEXT,
        &SctpAssocValue {
            assoc_id: 0,
            assoc_value: 77,
        },
    )
    .unwrap();
    let info = SctpSndinfo {
        snd_sid: 3,
        snd_flags: super::UNORDERED,
        snd_ppid: 0x1234,
        snd_context: 9,
        snd_assoc_id: 0,
    };
    let ctl = Control {
        level: SOL_SCTP,
        kind: SctpCmsgType::SCTP_SNDINFO.0,
        data: crate::cmsg::guest_bytes(&info),
    };
    send_ctl(&*c, b"one", None, std::slice::from_ref(&ctl)).unwrap();
    let (_, _, controls) = recv(&*s, 64).unwrap();
    assert_eq!(controls.len(), 2);
    let rcv = SctpRcvinfo::read_from(&controls[0].data).unwrap();
    assert_eq!(
        (rcv.rcv_sid, rcv.rcv_flags, rcv.rcv_ppid, rcv.rcv_context),
        (3, super::UNORDERED, 0x1234, 77)
    );
    let sr = SctpSndrcvinfo::read_from(&controls[1].data).unwrap();
    assert_eq!((sr.sinfo_stream, sr.sinfo_ssn), (3, 0));
    // Ordered messages on a stream count up.
    set(
        &*c,
        SctpOption::SCTP_DEFAULT_SEND_PARAM,
        &SctpSndrcvinfo {
            sinfo_stream: 2,
            sinfo_ppid: 5,
            ..SctpSndrcvinfo::ZERO
        },
    )
    .unwrap();
    send(&*c, b"a").unwrap();
    send(&*c, b"b").unwrap();
    for ssn in 0..2 {
        let (_, _, controls) = recv(&*s, 64).unwrap();
        let rcv = SctpRcvinfo::read_from(&controls[0].data).unwrap();
        assert_eq!((rcv.rcv_sid, rcv.rcv_ssn, rcv.rcv_ppid), (2, ssn, 5));
    }
    // A stream past the association's outbound streams, a bad control message length.
    let bad = SctpSndinfo {
        snd_sid: 10,
        ..info
    };
    let ctl = Control {
        data: crate::cmsg::guest_bytes(&bad),
        ..ctl
    };
    assert_eq!(
        send_ctl(&*c, b"x", None, std::slice::from_ref(&ctl)),
        Err(Errno::EINVAL)
    );
    let short = Control {
        data: vec![0; 4],
        ..ctl
    };
    assert_eq!(send_ctl(&*c, b"x", None, &[short]), Err(Errno::EINVAL));
    // One-to-one sockets take neither SCTP_EOF nor SCTP_ABORT.
    let eof = SctpSndinfo {
        snd_flags: super::EOF,
        snd_sid: 0,
        ..info
    };
    let ctl = Control {
        level: SOL_SCTP,
        kind: SctpCmsgType::SCTP_SNDINFO.0,
        data: crate::cmsg::guest_bytes(&eof),
    };
    assert_eq!(send_ctl(&*c, b"", None, &[ctl]), Err(Errno::EINVAL));
}

#[test]
fn one_to_many_ids_peeloff_and_eof() {
    let n = net();
    let server = seqpacket(&n);
    server.bind(a("127.0.0.1:5005"), true).unwrap();
    // Without listen, a one-to-many socket takes no associations.
    let client = seqpacket(&n);
    assert_eq!(
        send_ctl(&*client, b"x", Some(a("127.0.0.1:5005")), &[]),
        Ok(1)
    );
    assert_eq!(get_int(&*client, SctpOption::SCTP_GET_ASSOC_NUMBER), Ok(0));
    server.listen(5).unwrap();
    subscribe(&*server);
    let c1 = seqpacket(&n);
    let c2 = seqpacket(&n);
    send_ctl(&*c1, b"from c1", Some(a("127.0.0.1:5005")), &[]).unwrap();
    send_ctl(&*c2, b"from c2", Some(a("127.0.0.1:5005")), &[]).unwrap();
    assert_eq!(get_int(&*server, SctpOption::SCTP_GET_ASSOC_NUMBER), Ok(2));
    let mut ids = [0u8; 12];
    let used = server
        .get_option(SOL_SCTP, SctpOption::SCTP_GET_ASSOC_ID_LIST.0, &mut ids)
        .unwrap();
    assert_eq!(used, 12);
    let id = |i: usize| i32::from_le_bytes(ids[4 + 4 * i..8 + 4 * i].try_into().unwrap());
    let (id1, id2) = (id(0), id(1));
    assert!(id1 > 2 && id2 > 2 && id1 != id2);
    // COMM_UP for each, then the data, each with its association.
    let raw = recv(&*server, 64).unwrap().0;
    assert_eq!(assoc_change(&raw).sac_assoc_id, id1);
    s_eq(recv(&*server, 64).unwrap().0, b"from c1");
    // Peel the second association off: its queued events go with it.
    let peeled = server.sctp().unwrap().peeloff(id2).unwrap();
    assert_eq!(peeled.kind(), SocketKind::SeqPacket);
    assert_eq!(
        assoc_change(&recv(&*peeled, 64).unwrap().0).sac_assoc_id,
        id2
    );
    s_eq(recv(&*peeled, 64).unwrap().0, b"from c2");
    assert_eq!(
        server.sctp().unwrap().peeloff(id2).err(),
        Some(Errno::EINVAL)
    );
    assert_eq!(
        peeled.sctp().unwrap().peeloff(id2).err(),
        Some(Errno::EINVAL)
    );
    assert_eq!(get_int(&*server, SctpOption::SCTP_GET_ASSOC_NUMBER), Ok(1));
    // A reply to an association names it.
    let info = SctpSndinfo {
        snd_assoc_id: id1,
        ..SctpSndinfo::ZERO
    };
    let ctl = Control {
        level: SOL_SCTP,
        kind: SctpCmsgType::SCTP_SNDINFO.0,
        data: crate::cmsg::guest_bytes(&info),
    };
    send_ctl(&*server, b"reply", None, std::slice::from_ref(&ctl)).unwrap();
    s_eq(recv(&*c1, 64).unwrap().0, b"reply");
    // An unknown association: EPIPE.
    let info = SctpSndinfo {
        snd_assoc_id: 9999,
        ..info
    };
    let ctl = Control {
        data: crate::cmsg::guest_bytes(&info),
        ..ctl
    };
    assert_eq!(
        send_ctl(&*server, b"x", None, std::slice::from_ref(&ctl)),
        Err(Errno::EPIPE)
    );
    // SCTP_EOF ends the association.
    let info = SctpSndinfo {
        snd_assoc_id: id1,
        snd_flags: super::EOF,
        ..info
    };
    let ctl = Control {
        data: crate::cmsg::guest_bytes(&info),
        ..ctl
    };
    assert_eq!(send_ctl(&*server, b"", None, &[ctl]), Ok(0));
    assert_eq!(get_int(&*server, SctpOption::SCTP_GET_ASSOC_NUMBER), Ok(0));
    assert_eq!(get_int(&*c1, SctpOption::SCTP_GET_ASSOC_NUMBER), Ok(0));
    // One-to-many sockets neither accept nor take SCTP_REUSE_PORT; one-to-one ones have no
    // association count.
    assert_eq!(server.accept().err(), Some(Errno::EOPNOTSUPP));
    assert_eq!(
        set_int(&*server, SctpOption::SCTP_REUSE_PORT, 1),
        Err(Errno::EOPNOTSUPP)
    );
    assert_eq!(
        get_int(&*stream(&n), SctpOption::SCTP_GET_ASSOC_NUMBER),
        Err(Errno::EOPNOTSUPP)
    );
}

fn s_eq(got: Vec<u8>, want: &[u8]) {
    assert_eq!(got, want);
}

#[test]
fn status_and_options() {
    let n = net();
    let (_l, c, _s) = connected(&n, 5006);
    let init = get(&*c, SctpOption::SCTP_INITMSG, &SctpInitmsg::ZERO).unwrap();
    assert_eq!(
        (
            init.sinit_num_ostreams,
            init.sinit_max_instreams,
            init.sinit_max_attempts
        ),
        (10, 65535, 8)
    );
    let status = get(&*c, SctpOption::SCTP_STATUS, &SctpStatus::ZERO).unwrap();
    assert_eq!(status.sstat_state, 4);
    assert_eq!((status.sstat_instrms, status.sstat_outstrms), (10, 10));
    // Loopback's MTU less the IPv4, SCTP and DATA headers.
    assert_eq!(status.sstat_fragmentation_point, 65536 - 20 - 12 - 16);
    assert_eq!(status.sstat_primary.spinfo_state, 2);
    assert_eq!(status.sstat_rwnd as usize, crate::opts::UDP_BUF_DEFAULT / 2);
    let mut short = [0; 8];
    assert_eq!(
        c.get_option(SOL_SCTP, SctpOption::SCTP_STATUS.0, &mut short),
        Err(Errno::EINVAL)
    );
    // The primary path is the address connected to.
    let prim = get(&*c, SctpOption::SCTP_PRIMARY_ADDR, &SctpPrim::ZERO).unwrap();
    let mut raw = [0; SockaddrStorage::SIZE];
    prim.ssp_addr.write_to(&mut raw);
    assert_eq!(super::parse_sockaddr(&raw), Some(a("127.0.0.1:5006")));
    set(&*c, SctpOption::SCTP_PRIMARY_ADDR, &prim).unwrap();
    assert_eq!(get_int(&*c, SctpOption::SCTP_NODELAY), Ok(0));
    set_int(&*c, SctpOption::SCTP_NODELAY, 1).unwrap();
    assert_eq!(get_int(&*c, SctpOption::SCTP_NODELAY), Ok(1));
    assert_eq!(
        set_int(&*c, SctpOption::SCTP_AUTOCLOSE, 5),
        Err(Errno::EOPNOTSUPP)
    );
    // SCTP_MAXSEG: the association's fragmentation point, bounded settings.
    let seg = get(&*c, SctpOption::SCTP_MAXSEG, &SctpAssocValue::ZERO).unwrap();
    assert_eq!(seg.assoc_value, 65488);
    assert_eq!(
        set(
            &*c,
            SctpOption::SCTP_MAXSEG,
            &SctpAssocValue {
                assoc_id: 0,
                assoc_value: 100
            }
        ),
        Err(Errno::EINVAL)
    );
    set(
        &*c,
        SctpOption::SCTP_MAXSEG,
        &SctpAssocValue {
            assoc_id: 0,
            assoc_value: 1000,
        },
    )
    .unwrap();
    assert_eq!(
        get(&*c, SctpOption::SCTP_MAXSEG, &SctpAssocValue::ZERO)
            .unwrap()
            .assoc_value,
        1000
    );
    set_int(&*c, SctpOption::SCTP_DISABLE_FRAGMENTS, 1).unwrap();
    assert_eq!(send(&*c, &[0; 1001]), Err(Errno::EMSGSIZE));
    assert_eq!(send(&*c, &[0; 1000]), Ok(1000));
    // RTO bounds must stay ordered.
    let rto = get(
        &*c,
        SctpOption::SCTP_RTOINFO,
        &wasixv2_abi::SctpRtoinfo::ZERO,
    )
    .unwrap();
    assert_eq!(
        (rto.srto_initial, rto.srto_max, rto.srto_min),
        (3000, 60000, 1000)
    );
    let bad = wasixv2_abi::SctpRtoinfo {
        srto_min: 70000,
        ..rto
    };
    assert_eq!(set(&*c, SctpOption::SCTP_RTOINFO, &bad), Err(Errno::EINVAL));
    // SCTP_EVENTS: at most the whole record, read back as far as asked.
    assert_eq!(
        c.set_option(SOL_SCTP, SctpOption::SCTP_EVENTS.0, &[0; 15]),
        Err(Errno::EINVAL)
    );
    let mut ev = [9u8; 3];
    assert_eq!(
        c.get_option(SOL_SCTP, SctpOption::SCTP_EVENTS.0, &mut ev),
        Ok(3)
    );
    assert_eq!(ev, [0, 0, 0]);
    assert_eq!(
        c.set_option(SOL_SCTP, 999, &[0; 4]),
        Err(Errno::ENOPROTOOPT)
    );
    assert_eq!(
        c.set_option(wasixv2_abi::SocketLevel::SOL_TCP.0, 1, &[0; 4]),
        Err(Errno::ENOPROTOOPT)
    );
}

#[test]
fn bindx_and_address_lists() {
    let n = net();
    let s = stream(&n);
    let ops = s.sctp().unwrap();
    ops.bindx(&[a("127.0.0.1:5007"), a("127.0.0.2:0")], true, true)
        .unwrap();
    assert_eq!(s.local_addr().unwrap(), a("127.0.0.2:5007"));
    // Another port, an address bound already, one not local.
    assert_eq!(
        ops.bindx(&[a("127.0.0.3:5008")], true, true),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        ops.bindx(&[a("127.0.0.1:0")], true, true),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        ops.bindx(&[a("10.9.9.9:0")], true, true),
        Err(Errno::EADDRNOTAVAIL)
    );
    let other = stream(&n);
    assert_eq!(
        other.bind(a("127.0.0.2:5007"), true),
        Err(Errno::EADDRINUSE)
    );
    other.bind(a("127.0.0.3:5007"), true).unwrap();
    let mut raw = [0u8; 64];
    let used = s
        .get_option(SOL_SCTP, SctpOption::SCTP_GET_LOCAL_ADDRS.0, &mut raw)
        .unwrap();
    assert_eq!(used, SctpGetaddrs::SIZE + 32);
    assert_eq!(u32::from_le_bytes(raw[4..8].try_into().unwrap()), 2);
    assert_eq!(super::parse_sockaddr(&raw[8..]), Some(a("127.0.0.1:5007")));
    // Too little room for the addresses.
    let mut small = [0u8; 20];
    assert_eq!(
        s.get_option(SOL_SCTP, SctpOption::SCTP_GET_LOCAL_ADDRS.0, &mut small),
        Err(Errno::ENOMEM)
    );
    ops.bindx(&[a("127.0.0.1:0")], false, true).unwrap();
    assert_eq!(
        ops.bindx(&[a("127.0.0.2:0")], false, true),
        Err(Errno::EBUSY)
    );
    // A privileged port needs the capability.
    let low = stream(&n);
    assert_eq!(
        low.sctp().unwrap().bindx(&[a("127.0.0.1:80")], true, false),
        Err(Errno::EACCES)
    );
    // Peers see every address of a multi-homed endpoint.
    s.listen(1).unwrap();
    let c = stream(&n);
    let id = c.sctp().unwrap().connectx(&[a("127.0.0.2:5007")]).unwrap();
    assert!(id > 2);
    let mut raw = [0u8; 64];
    let used = c
        .get_option(SOL_SCTP, SctpOption::SCTP_GET_PEER_ADDRS.0, &mut raw)
        .unwrap();
    assert_eq!(used, SctpGetaddrs::SIZE + 16);
    assert_eq!(super::parse_sockaddr(&raw[8..]), Some(a("127.0.0.2:5007")));
}

#[test]
fn reuse_and_listen_conflicts() {
    let n = net();
    let one = stream(&n);
    let two = stream(&n);
    for s in [&one, &two] {
        s.set_option(
            wasixv2_abi::SocketLevel::SOL_SOCKET.0,
            wasixv2_abi::SocketOption::SO_REUSEADDR.0,
            &1i32.to_le_bytes(),
        )
        .unwrap();
        s.bind(a("127.0.0.1:5009"), true).unwrap();
    }
    one.listen(1).unwrap();
    assert_eq!(two.listen(1), Err(Errno::EADDRINUSE));
    // backlog 0 stops listening.
    one.listen(0).unwrap();
    two.listen(1).unwrap();
    assert_eq!(two.bind(a("127.0.0.1:5010"), true), Err(Errno::EINVAL));
}

#[test]
fn flow_control_waits_for_the_reader() {
    let n = net();
    let (_l, c, s) = connected(&n, 5011);
    let small: i32 = 4608;
    s.set_option(
        wasixv2_abi::SocketLevel::SOL_SOCKET.0,
        wasixv2_abi::SocketOption::SO_RCVBUF.0,
        &(small / 2).to_le_bytes(),
    )
    .unwrap();
    c.set_option(
        wasixv2_abi::SocketLevel::SOL_SOCKET.0,
        wasixv2_abi::SocketOption::SO_SNDBUF.0,
        &(small / 2).to_le_bytes(),
    )
    .unwrap();
    assert_eq!(send(&*c, &[0; 5000]), Err(Errno::EMSGSIZE));
    let mut sent = 0;
    while send(&*c, &[1; 1000]).is_ok() {
        sent += 1;
    }
    assert_eq!(send(&*c, &[1; 1000]), Err(Errno::EAGAIN));
    assert!(!c.poll(None).contains(PollMask::OUT));
    assert!(sent > 2);
    recv(&*s, 2000).unwrap();
    assert!(c.poll(None).contains(PollMask::OUT));
    assert_eq!(send(&*c, &[1; 1000]), Ok(1000));
}

#[test]
fn across_a_veth_and_in_proc() {
    let (na, nb) = crate::tests::veth_pair(("10.1.0.1", "10.1.0.2"), ("fd00::1", "fd00::2"));
    let l = nb
        .socket(Domain::Inet6, SocketKind::SeqPacket, IPPROTO_SCTP, 1)
        .unwrap();
    l.bind(a("[::]:5012"), true).unwrap();
    l.listen(5).unwrap();
    let c = stream(&na);
    c.connect(a("10.1.0.2:5012")).unwrap_err();
    send(&*c, b"over").unwrap();
    let mut buf = [0; 16];
    let got = l.recv(&mut buf, MsgFlags(0)).unwrap();
    assert_eq!(&buf[..got.len], b"over");
    // An IPv6 socket sees IPv4 peers mapped.
    let mapped: std::net::IpAddr = "::ffff:10.1.0.1".parse().unwrap();
    let port = c.local_addr().unwrap().port();
    assert_eq!(got.from, Some(SocketAddr::new(mapped, port)));
    let eps = nb.sctp_endpoints();
    assert_eq!(eps.len(), 1);
    assert_eq!((eps[0].style, eps[0].state, eps[0].port), (1, 10, 5012));
    let assocs = na.sctp_assocs();
    assert_eq!(assocs.len(), 1);
    assert_eq!((assocs[0].state, assocs[0].rport), (3, 5012));
    assert_eq!(assocs[0].primary, Some("10.1.0.2".parse().unwrap()));
    let snmp = na.sctp_snmp();
    assert_eq!(snmp[0], ("SctpCurrEstab", 1));
    assert_eq!(snmp[1], ("SctpActiveEstabs", 1));
    assert_eq!(nb.sctp_snmp()[2], ("SctpPassiveEstabs", 1));
    assert_eq!(snmp.len(), 32);
    // Nothing beyond the machine speaks SCTP; nobody answers past a dummy route.
    assert_eq!(
        stream(&na).connect(a("10.1.0.3:5012")),
        Err(Errno::ETIMEDOUT)
    );
}

#[test]
fn delayed_sack_and_packed_peer_parameters() {
    use wasixv2_abi::{SctpPaddrparams, SctpSackInfo};
    let n = net();
    let c = seqpacket(&n);
    let p = SctpSackInfo {
        sack_assoc_id: 0,
        sack_delay: 300,
        sack_freq: 3,
    };
    set(&*c, SctpOption::SCTP_DELAYED_ACK_TIME, &p).unwrap();
    assert_eq!(get(&*c, SctpOption::SCTP_DELAYED_ACK_TIME, &p).unwrap(), p);
    assert_eq!(
        set(
            &*c,
            SctpOption::SCTP_DELAYED_ACK_TIME,
            &SctpSackInfo {
                sack_delay: 501,
                ..p
            }
        ),
        Err(Errno::EINVAL)
    );
    let legacy = SctpAssocValue {
        assoc_id: 0,
        assoc_value: 0,
    };
    set(&*c, SctpOption::SCTP_DELAYED_ACK_TIME, &legacy).unwrap();
    assert_eq!(
        get(&*c, SctpOption::SCTP_DELAYED_ACK_TIME, &p)
            .unwrap()
            .sack_freq,
        1
    );
    let p = SctpPaddrparams {
        spp_hbinterval: 1234,
        spp_pathmaxrxt: 7,
        spp_pathmtu: 1400u32.to_le_bytes(),
        spp_sackdelay: 250u32.to_le_bytes(),
        spp_flags: (1u32 | 16 | 32 | 512).to_le_bytes(),
        spp_dscp: 0xff,
        ..SctpPaddrparams::ZERO
    };
    let bytes = crate::cmsg::guest_bytes(&p);
    assert_eq!(
        &bytes[138..142],
        &1400u32.to_le_bytes(),
        "packed unaligned pathmtu"
    );
    set(&*c, SctpOption::SCTP_PEER_ADDR_PARAMS, &p).unwrap();
    let got = get(&*c, SctpOption::SCTP_PEER_ADDR_PARAMS, &p).unwrap();
    assert_eq!(
        (got.spp_hbinterval, got.spp_pathmaxrxt, got.spp_dscp),
        (1234, 7, 0xfc)
    );
    assert_eq!(got.spp_pathmtu, 1400u32.to_le_bytes());
    assert_eq!(
        set(
            &*c,
            SctpOption::SCTP_PEER_ADDR_PARAMS,
            &SctpPaddrparams {
                spp_flags: 3u32.to_le_bytes(),
                ..p
            }
        ),
        Err(Errno::EINVAL)
    );
    let l = seqpacket(&n);
    l.bind(a("127.0.0.1:5060"), true).unwrap();
    l.listen(4).unwrap();
    let id = c.sctp().unwrap().connectx(&[a("127.0.0.1:5060")]).unwrap();
    let status = get(
        &*c,
        SctpOption::SCTP_STATUS,
        &SctpStatus {
            sstat_assoc_id: id,
            ..SctpStatus::ZERO
        },
    )
    .unwrap();
    let by_addr = SctpPaddrparams {
        spp_assoc_id: status.sstat_assoc_id,
        spp_address: status.sstat_primary.spinfo_address,
        spp_hbinterval: 2345,
        spp_flags: 1u32.to_le_bytes(),
        ..SctpPaddrparams::ZERO
    };
    set(&*c, SctpOption::SCTP_PEER_ADDR_PARAMS, &by_addr).unwrap();
    assert_eq!(
        get(&*c, SctpOption::SCTP_PEER_ADDR_PARAMS, &by_addr)
            .unwrap()
            .spp_hbinterval,
        2345
    );
    assert_eq!(
        get(
            &*c,
            SctpOption::SCTP_PEER_ADDR_PARAMS,
            &SctpPaddrparams {
                spp_assoc_id: status.sstat_assoc_id,
                ..SctpPaddrparams::ZERO
            }
        )
        .unwrap()
        .spp_hbinterval,
        1234
    );
    set(
        &*c,
        SctpOption::SCTP_PEER_ADDR_PARAMS,
        &SctpPaddrparams {
            spp_flags: 8u32.to_le_bytes(),
            ..by_addr
        },
    )
    .unwrap();
    let discovered = get(&*c, SctpOption::SCTP_PEER_ADDR_PARAMS, &by_addr).unwrap();
    assert_eq!(
        u32::from_le_bytes(discovered.spp_pathmtu),
        n.mtu_to(a("127.0.0.1:5060").ip()).unwrap()
    );
}

#[test]
fn autoclose_arms_a_timer_and_expires_without_socket_io() {
    let n = net();
    let c = super::SctpSocket::new(n.clone(), Domain::Inet, SocketKind::SeqPacket, 1);
    let l = seqpacket(&n);
    l.bind(a("127.0.0.1:5061"), true).unwrap();
    l.listen(4).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    c.sctp()
        .unwrap()
        .set_autoclose_timer(Box::new(move |after| {
            tx.send(after).unwrap();
        }));
    set_int(&*c, SctpOption::SCTP_AUTOCLOSE, 10).unwrap();
    c.sctp().unwrap().connectx(&[a("127.0.0.1:5061")]).unwrap();
    let arms: Vec<_> = rx.try_iter().flatten().collect();
    assert!(arms.iter().any(|delay| *delay <= Duration::from_secs(10)));
    c.lock().asocs[0].lock().last_data -= Duration::from_secs(11);
    assert_eq!(c.sctp().unwrap().autoclose_expire(), None);
    assert_eq!(get_int(&*c, SctpOption::SCTP_GET_ASSOC_NUMBER), Ok(0));
    assert_eq!(get_int(&*l, SctpOption::SCTP_GET_ASSOC_NUMBER), Ok(0));
}

#[test]
fn one_to_many_nonblocking_receive_without_associations_is_eagain() {
    let s = seqpacket(&net());
    assert_eq!(
        s.recv(&mut [0; 1], MsgFlags::MSG_DONTWAIT),
        Err(Errno::EAGAIN)
    );
    assert_eq!(s.recv(&mut [0; 1], MsgFlags(0)), Err(Errno::ENOTCONN));
    assert_eq!(
        stream(&net()).recv(&mut [0; 1], MsgFlags::MSG_DONTWAIT),
        Err(Errno::ENOTCONN)
    );
}

#[test]
fn adaptation_primary_and_partial_delivery_notifications() {
    use wasixv2_abi::{SctpAdaptationEvent, SctpPaddrChange, SctpPdapiEvent};
    let n = net();
    let l = stream(&n);
    l.bind(a("127.0.0.1:5062"), true).unwrap();
    l.set_option(
        SOL_SCTP,
        SctpOption::SCTP_EVENTS.0,
        &[0, 0, 1, 0, 0, 0, 1, 1],
    )
    .unwrap();
    l.listen(4).unwrap();
    let c = stream(&n);
    set_int(&*c, SctpOption::SCTP_ADAPTATION_LAYER, 0x12345678).unwrap();
    assert_eq!(c.connect(a("127.0.0.1:5062")), Err(Errno::EINPROGRESS));
    let s = l.accept().unwrap().socket;
    let raw = recv(&*s, 64).unwrap().0;
    assert_eq!(
        SctpAdaptationEvent::read_from(&raw)
            .unwrap()
            .sai_adaptation_ind,
        0x12345678
    );
    let status = get(&*s, SctpOption::SCTP_STATUS, &SctpStatus::ZERO).unwrap();
    set(
        &*s,
        SctpOption::SCTP_PRIMARY_ADDR,
        &SctpPrim {
            ssp_assoc_id: status.sstat_assoc_id,
            ssp_addr: status.sstat_primary.spinfo_address,
        },
    )
    .unwrap();
    assert_eq!(
        SctpPaddrChange::read_from(&recv(&*s, 256).unwrap().0)
            .unwrap()
            .spc_state,
        4
    );
    set_int(&*s, SctpOption::SCTP_PARTIAL_DELIVERY_POINT, 2).unwrap();
    send(&*c, &vec![b'p'; 200_000]).unwrap();
    assert_eq!(recv(&*s, 3).unwrap().0, b"ppp");
    c.set_option(
        1,
        wasixv2_abi::SocketOption::SO_LINGER.0,
        &[1i32.to_le_bytes(), 0i32.to_le_bytes()].concat(),
    )
    .unwrap();
    drop(c);
    assert_eq!(recv(&*s, 64).unwrap_err(), Errno::ECONNRESET);
    let raw = recv(&*s, 64).unwrap().0;
    assert_eq!(SctpPdapiEvent::read_from(&raw).unwrap().pdapi_indication, 0);
}

#[test]
fn a_short_read_of_a_complete_message_does_not_enter_partial_delivery() {
    let n = net();
    let (_l, c, s) = connected(&n, 5063);
    s.set_option(SOL_SCTP, SctpOption::SCTP_EVENTS.0, &[0, 0, 0, 0, 0, 0, 1])
        .unwrap();
    send(&*c, b"complete").unwrap();
    assert_eq!(recv(&*s, 3).unwrap().0, b"com");
    c.set_option(
        1,
        wasixv2_abi::SocketOption::SO_LINGER.0,
        &[1i32.to_le_bytes(), 0i32.to_le_bytes()].concat(),
    )
    .unwrap();
    drop(c);
    assert_eq!(recv(&*s, 64).unwrap_err(), Errno::ECONNRESET);
    assert_eq!(recv(&*s, 64).unwrap().0, b"plete");
}

#[test]
fn autoclose_waits_for_bytes_outside_the_peer_window() {
    let n = net();
    let c = super::SctpSocket::new(n.clone(), Domain::Inet, SocketKind::SeqPacket, 1);
    let l = seqpacket(&n);
    l.bind(a("127.0.0.1:5064"), true).unwrap();
    l.listen(4).unwrap();
    set_int(&*c, SctpOption::SCTP_AUTOCLOSE, 1).unwrap();
    let id = c.sctp().unwrap().connectx(&[a("127.0.0.1:5064")]).unwrap();
    let info = SctpSndinfo {
        snd_assoc_id: id,
        ..SctpSndinfo::ZERO
    };
    let ctl = Control {
        level: SOL_SCTP,
        kind: SctpCmsgType::SCTP_SNDINFO.0,
        data: crate::cmsg::guest_bytes(&info),
    };
    send_ctl(&*c, &vec![0; 200_000], None, &[ctl]).unwrap();
    c.lock().asocs[0].lock().last_data -= Duration::from_secs(2);
    assert_eq!(c.sctp().unwrap().autoclose_expire(), None);
    assert_eq!(
        get_int(&*c, SctpOption::SCTP_GET_ASSOC_NUMBER),
        Ok(1),
        "shutdown waits for outstanding data"
    );
    recv(&*l, 106_496).unwrap();
    assert_eq!(get_int(&*c, SctpOption::SCTP_GET_ASSOC_NUMBER), Ok(0));
}

#[test]
fn association_clock_data_refresh_does_not_autoclose_early() {
    let n = net();
    let listener = seqpacket(&n);
    listener.bind(a("127.0.0.1:5065"), true).unwrap();
    listener.listen(4).unwrap();
    let client = super::SctpSocket::new(n.clone(), Domain::Inet, SocketKind::SeqPacket, 1);
    set_int(&*client, SctpOption::SCTP_AUTOCLOSE, 10).unwrap();
    client
        .sctp()
        .unwrap()
        .connectx(&[a("127.0.0.1:5065")])
        .unwrap();
    let original = client.lock().asocs[0].lock().last_data;
    send_ctl(&*client, b"clock", Some(a("127.0.0.1:5065")), &[]).unwrap();
    assert!(client.lock().asocs[0].lock().last_data >= original);
    let left = client.sctp().unwrap().autoclose_expire().unwrap();
    assert!(!left.is_zero() && left <= Duration::from_secs(10));
    assert_eq!(get_int(&*client, SctpOption::SCTP_GET_ASSOC_NUMBER), Ok(1));
}
