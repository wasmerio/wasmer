//! The `NETLINK_SOCK_DIAG` requests (`net/core/sock_diag.c`): socket dumps for `ss`, from the
//! port tables of the socket's network namespace (`net/ipv4/inet_diag.c` with `tcp_diag.c`,
//! `udp_diag.c` and `raw_diag.c`) and its Unix sockets (`net/unix/diag.c`). Requests are
//! checked as Linux checks them, bytecode filters included (`inet_diag_bc_audit`,
//! `inet_diag_bc_run`); dumps come back whole, then `NLMSG_DONE`.
//!
//! What a namespace without packets cannot tell is reported as idle: no timers, no
//! retransmissions, no `INET_DIAG_INFO` (`tcp_info`); and `SOCK_DESTROY` is not supported, as
//! in a kernel without `CONFIG_INET_DIAG_DESTROY`. A TCP socket that is only bound is in no
//! hash `inet_diag` walks, so it is not listed.

use std::net::{IpAddr, SocketAddr};

use wasixv2_abi::{
    AddressFamily, GuestLayout, InetDiagMeminfo, InetDiagMsg, InetDiagReq, InetDiagReqV2,
    InetDiagSockid, IpProtocol, SockDiagReq, UnixDiagMsg, UnixDiagReq, UnixDiagRqlen, UnixDiagVfs,
};
use wasixv2_syscalls::Errno;
use wasixv2_vnet::{Domain, IcmpEntry, PortEntry, SocketKind};

use super::{NLM_F_DUMP, NLM_F_MULTI, attrs, nlmsg, rta, rtnl::Request};
use crate::ns::NetNs;

pub(super) const TCPDIAG_GETSOCK: u16 = 18;
pub(super) const SOCK_DIAG_BY_FAMILY: u16 = 20;
pub(super) const SOCK_DESTROY: u16 = 21;
/// `AF_MAX`.
const AF_MAX: u8 = 46;

const TCP_ESTABLISHED: u8 = 1;
const TCP_CLOSE: u8 = 7;
const TCP_LISTEN: u8 = 10;

/// `INET_DIAG_REQ_*`, the attributes of a request.
const INET_DIAG_REQ_BYTECODE: u16 = 1;
const INET_DIAG_REQ_PROTOCOL: u16 = 3;
/// `INET_DIAG_*`, the attributes of a message; the bits a request's `idiag_ext` asks for are
/// `1 << (attribute - 1)`.
const INET_DIAG_MEMINFO: u16 = 1;
const INET_DIAG_CONG: u16 = 4;
const INET_DIAG_TOS: u16 = 5;
const INET_DIAG_TCLASS: u16 = 6;
const INET_DIAG_SKMEMINFO: u16 = 7;
const INET_DIAG_SHUTDOWN: u16 = 8;
const INET_DIAG_PROTOCOL: u16 = 10;
const INET_DIAG_SKV6ONLY: u16 = 11;
const INET_DIAG_MARK: u16 = 15;
const INET_DIAG_CLASS_ID: u16 = 17;
const INET_DIAG_SOCKOPT: u16 = 22;
/// `INET_DIAG_NOCOOKIE`.
const NOCOOKIE: u32 = !0;

/// `UDIAG_SHOW_*` and `UNIX_DIAG_*`.
const UDIAG_SHOW_NAME: u32 = 1;
const UDIAG_SHOW_VFS: u32 = 2;
const UDIAG_SHOW_PEER: u32 = 4;
const UDIAG_SHOW_ICONS: u32 = 8;
const UDIAG_SHOW_RQLEN: u32 = 0x10;
const UDIAG_SHOW_MEMINFO: u32 = 0x20;
const UDIAG_SHOW_UID: u32 = 0x40;
const UNIX_DIAG_NAME: u16 = 0;
const UNIX_DIAG_VFS: u16 = 1;
const UNIX_DIAG_PEER: u16 = 2;
const UNIX_DIAG_ICONS: u16 = 3;
const UNIX_DIAG_RQLEN: u16 = 4;
const UNIX_DIAG_MEMINFO: u16 = 5;
const UNIX_DIAG_SHUTDOWN: u16 = 6;
const UNIX_DIAG_UID: u16 = 7;

/// `sock_diag_rcv_msg`: one request with its payload. `net_admin` is the sender's
/// `CAP_NET_ADMIN` (`netlink_net_capable`).
pub(super) fn request(
    r: &Request<'_>,
    kind: u16,
    payload: &[u8],
    net_admin: bool,
) -> Result<Vec<u8>, Errno> {
    match kind {
        TCPDIAG_GETSOCK => inet_compat(r, payload, net_admin),
        SOCK_DIAG_BY_FAMILY | SOCK_DESTROY => {
            let req = SockDiagReq::read_from(payload).ok_or(Errno::EINVAL)?;
            if req.sdiag_family >= AF_MAX {
                return Err(Errno::EINVAL);
            }
            match i32::from(req.sdiag_family) {
                f if f == AddressFamily::AF_INET.0 || f == AddressFamily::AF_INET6.0 => {
                    inet(r, kind, payload, net_admin)
                }
                f if f == AddressFamily::AF_UNIX.0 => match kind {
                    SOCK_DIAG_BY_FAMILY => unix(r, payload),
                    _ => Err(Errno::EOPNOTSUPP),
                },
                _ => Err(Errno::ENOENT),
            }
        }
        _ => Err(Errno::EINVAL),
    }
}

/// One socket as the inet handlers see it (`struct inet_diag_entry` and what fills a
/// message).
struct InetSock {
    family: u8,
    state: u8,
    protocol: u8,
    raw: bool,
    local: SocketAddr,
    peer: Option<SocketAddr>,
    rx_queue: usize,
    tx_queue: usize,
    uid: u32,
    inode: u64,
    v6only: bool,
    port_locked: bool,
    bound_dev: u32,
}

impl InetSock {
    fn from_port(p: PortEntry, protocol: u8) -> Self {
        let state = match (p.listening, p.peer.is_some()) {
            (true, _) => TCP_LISTEN,
            (false, true) => TCP_ESTABLISHED,
            (false, false) => TCP_CLOSE,
        };
        Self {
            family: family_of(p.local.ip()),
            state,
            protocol,
            raw: false,
            local: p.local,
            peer: p.peer,
            rx_queue: p.rx_queue,
            tx_queue: p.tx_queue,
            uid: p.uid,
            inode: p.inode,
            v6only: p.v6only,
            port_locked: p.port_locked,
            bound_dev: p.bound_dev,
        }
    }

    /// A raw socket: its port is its protocol (`inet_num`), its peer's port 0.
    fn from_raw(e: IcmpEntry) -> Self {
        let any = match e.domain {
            Domain::Inet => IpAddr::from([0u8; 4]),
            Domain::Inet6 => IpAddr::from([0u16; 8]),
        };
        Self {
            family: match e.domain {
                Domain::Inet => AddressFamily::AF_INET.0 as u8,
                Domain::Inet6 => AddressFamily::AF_INET6.0 as u8,
            },
            state: match e.peer {
                Some(_) => TCP_ESTABLISHED,
                None => TCP_CLOSE,
            },
            protocol: e.num as u8,
            raw: true,
            local: SocketAddr::new(e.local.unwrap_or(any), e.num),
            peer: e.peer.map(|(ip, _)| SocketAddr::new(ip, 0)),
            rx_queue: e.rx_queue,
            tx_queue: 0,
            uid: e.uid,
            inode: e.inode,
            v6only: false,
            port_locked: true,
            bound_dev: 0,
        }
    }

    fn sport(&self) -> u16 {
        self.local.port()
    }

    fn dport(&self) -> u16 {
        self.peer.map_or(0, |p| p.port())
    }

    fn daddr(&self) -> IpAddr {
        match self.peer {
            Some(p) => p.ip(),
            None if self.local.is_ipv4() => IpAddr::from([0u8; 4]),
            None => IpAddr::from([0u16; 8]),
        }
    }

    /// `sock_gen_cookie`: the inode stands in, unique while the socket has a file.
    fn cookie(&self) -> [u32; 2] {
        [self.inode as u32, (self.inode >> 32) as u32]
    }
}

fn family_of(ip: IpAddr) -> u8 {
    match ip {
        IpAddr::V4(_) => AddressFamily::AF_INET.0 as u8,
        IpAddr::V6(_) => AddressFamily::AF_INET6.0 as u8,
    }
}

/// An address as `inet_diag_sockid` holds it: network-order words (as the guest's
/// little-endian `u32`s carry them), IPv4 in the first.
fn words(ip: IpAddr) -> [u32; 4] {
    let mut out = [0; 4];
    match ip {
        IpAddr::V4(v4) => out[0] = u32::from_le_bytes(v4.octets()),
        IpAddr::V6(v6) => {
            for (w, c) in out.iter_mut().zip(v6.octets().chunks(4)) {
                *w = u32::from_le_bytes(c.try_into().unwrap());
            }
        }
    }
    out
}

/// A port as the guest's `__be16` field holds it, both ways.
fn be16(v: u16) -> u16 {
    u16::from_le_bytes(v.to_be_bytes())
}

/// The request a dump or lookup works from, whichever form it came in.
struct InetQuery<'a> {
    family: u8,
    protocol: u8,
    ext: u8,
    states: u32,
    id: InetDiagSockid,
    bytecode: Option<&'a [u8]>,
    /// The raw protocol `raw_diag` takes from the `pad` byte (`sdiag_raw_protocol`).
    raw_protocol: u8,
}

/// `inet_diag_rcv_msg_compat`: `TCPDIAG_GETSOCK` with the legacy `inet_diag_req`, which
/// dumps both families.
fn inet_compat(r: &Request<'_>, payload: &[u8], net_admin: bool) -> Result<Vec<u8>, Errno> {
    let req = InetDiagReq::read_from(payload).ok_or(Errno::EINVAL)?;
    let bytecode = request_attrs(&payload[InetDiagReq::SIZE..])?.0;
    let dump = r.flags & NLM_F_DUMP != 0;
    let q = InetQuery {
        family: match dump {
            true => AddressFamily::AF_UNSPEC.0 as u8,
            false => req.idiag_family,
        },
        protocol: IpProtocol::IPPROTO_TCP.0 as u8,
        ext: req.idiag_ext,
        states: req.idiag_states,
        id: req.id,
        bytecode,
        raw_protocol: 0,
    };
    audit(q.bytecode, net_admin)?;
    inet_answer(r, &q, net_admin, dump)
}

/// `inet_diag_handler_cmd`.
fn inet(r: &Request<'_>, kind: u16, payload: &[u8], net_admin: bool) -> Result<Vec<u8>, Errno> {
    let req = InetDiagReqV2::read_from(payload).ok_or(Errno::EINVAL)?;
    let (bytecode, protocol) = request_attrs(&payload[InetDiagReqV2::SIZE..])?;
    let q = InetQuery {
        family: req.sdiag_family,
        protocol: protocol.unwrap_or(req.sdiag_protocol),
        ext: req.idiag_ext,
        states: req.idiag_states,
        id: req.id,
        bytecode,
        raw_protocol: req.pad,
    };
    if kind == SOCK_DESTROY {
        // No protocol here has a `destroy`: `CONFIG_INET_DIAG_DESTROY` is off.
        sockets_of(r.net, q.protocol)?;
        return Err(Errno::EOPNOTSUPP);
    }
    let dump = r.flags & NLM_F_DUMP != 0;
    if dump {
        audit(q.bytecode, net_admin)?;
    }
    inet_answer(r, &q, net_admin, dump)
}

/// `inet_diag_parse_attrs`: the bytecode and the protocol, which must be a `u32`.
fn request_attrs(raw: &[u8]) -> Result<(Option<&[u8]>, Option<u8>), Errno> {
    let raw = &raw[..raw.len() - raw.len() % 4];
    let mut bytecode = None;
    let mut protocol = None;
    for (kind, data) in attrs(raw) {
        match kind & 0x3fff {
            INET_DIAG_REQ_BYTECODE => bytecode = Some(data),
            INET_DIAG_REQ_PROTOCOL => {
                let v: [u8; 4] = data.try_into().map_err(|_| Errno::EINVAL)?;
                protocol = Some(u32::from_le_bytes(v) as u8);
            }
            _ => {}
        }
    }
    Ok((bytecode, protocol))
}

/// The sockets of `protocol`'s handler (`inet_diag_lock_handler`): `ENOENT` for a protocol
/// without one.
fn sockets_of(net: &NetNs, protocol: u8) -> Result<Vec<InetSock>, Errno> {
    let network = net.network.as_ref();
    let ports = |kind: SocketKind| {
        network
            .map(|n| n.ports())
            .unwrap_or_default()
            .into_iter()
            .filter(move |p| p.kind == kind)
            .map(move |p| InetSock::from_port(p, protocol))
    };
    match IpProtocol(i32::from(protocol)) {
        // A bound TCP socket that neither listens nor connects is in no hash dumped.
        IpProtocol::IPPROTO_TCP => Ok(ports(SocketKind::Stream)
            .filter(|s| s.state != TCP_CLOSE)
            .collect()),
        IpProtocol::IPPROTO_UDP => Ok(ports(SocketKind::Datagram).collect()),
        IpProtocol::IPPROTO_RAW => Ok(network
            .map(|n| n.icmp_sockets())
            .unwrap_or_default()
            .into_iter()
            .filter(|e| e.raw)
            .map(InetSock::from_raw)
            .collect()),
        _ => Err(Errno::ENOENT),
    }
}

fn inet_answer(
    r: &Request<'_>,
    q: &InetQuery<'_>,
    net_admin: bool,
    dump: bool,
) -> Result<Vec<u8>, Errno> {
    let sockets = sockets_of(r.net, q.protocol)?;
    let raw = q.protocol == IpProtocol::IPPROTO_RAW.0 as u8;
    if !dump {
        let found = sockets
            .iter()
            .find(|s| exact(q, s, raw))
            .ok_or(Errno::ENOENT)?;
        let cookie = q.id.idiag_cookie;
        if cookie != [NOCOOKIE; 2] && cookie != found.cookie() {
            return Err(Errno::ESTALE);
        }
        return Ok(inet_msg(r, q, found, net_admin, 0));
    }
    let msgs = sockets
        .iter()
        .filter(|s| dumped(q, s, raw))
        .map(|s| inet_msg(r, q, s, net_admin, NLM_F_MULTI))
        .collect();
    Ok(r.dump(msgs))
}

/// Whether a dump lists `s`: its family (any for `AF_UNSPEC`, exactly for raw sockets), its
/// state among `idiag_states`, the ports asked for, the bytecode.
fn dumped(q: &InetQuery<'_>, s: &InetSock, raw: bool) -> bool {
    let any_family = q.family == AddressFamily::AF_UNSPEC.0 as u8 && !raw;
    if !any_family && s.family != q.family {
        return false;
    }
    if q.states & (1 << s.state) == 0 && !raw {
        return false;
    }
    if raw && q.raw_protocol != IpProtocol::IPPROTO_RAW.0 as u8 && s.protocol != q.raw_protocol {
        return false;
    }
    let (sport, dport) = (be16(q.id.idiag_sport), be16(q.id.idiag_dport));
    if (sport != 0 && sport != s.sport()) || (dport != 0 && dport != s.dport()) {
        return false;
    }
    q.bytecode.is_none_or(|bc| run(bc, s))
}

/// The socket a lookup names (`inet_diag_find_one_icsk`, `udp_dump_one`, `raw_lookup`): the
/// request's source is the socket's local end, its destination the peer; a listener or an
/// unconnected socket matches with the destination unspecified and its own address any.
fn exact(q: &InetQuery<'_>, s: &InetSock, raw: bool) -> bool {
    if s.family != q.family {
        return false;
    }
    let local = words(s.local.ip());
    let peer = words(s.daddr());
    let len = if s.local.is_ipv4() { 1 } else { 4 };
    let src = &q.id.idiag_src[..len];
    let dst = &q.id.idiag_dst[..len];
    let any = |w: &[u32]| w.iter().all(|w| *w == 0);
    let src_ok = src == &local[..len] || any(&local[..len]);
    let dst_ok = dst == &peer[..len] || (any(&peer[..len]) && any(dst));
    if raw {
        return s.protocol == q.raw_protocol && src_ok && dst_ok;
    }
    let (sport, dport) = (be16(q.id.idiag_sport), be16(q.id.idiag_dport));
    sport == s.sport() && dport == s.dport() && src_ok && dst_ok
}

/// `inet_sk_diag_fill`.
fn inet_msg(
    r: &Request<'_>,
    q: &InetQuery<'_>,
    s: &InetSock,
    net_admin: bool,
    flags: u16,
) -> Vec<u8> {
    let ext = |attr: u16| u32::from(q.ext) & (1 << (attr - 1)) != 0;
    let listening = s.state == TCP_LISTEN;
    let msg = InetDiagMsg {
        idiag_family: s.family,
        idiag_state: s.state,
        idiag_timer: 0,
        idiag_retrans: 0,
        id: InetDiagSockid {
            idiag_sport: be16(s.sport()),
            idiag_dport: be16(s.dport()),
            idiag_src: words(s.local.ip()),
            idiag_dst: words(s.daddr()),
            idiag_if: s.bound_dev,
            idiag_cookie: s.cookie(),
        },
        idiag_expires: 0,
        idiag_rqueue: s.rx_queue as u32,
        idiag_wqueue: s.tx_queue as u32,
        idiag_uid: s.uid,
        idiag_inode: s.inode as u32,
    };
    let mut p = vec![0; InetDiagMsg::SIZE];
    msg.write_to(&mut p);
    // `inet_diag_msg_attrs_fill`.
    rta(&mut p, INET_DIAG_SHUTDOWN, &[0]);
    if ext(INET_DIAG_TOS) {
        rta(&mut p, INET_DIAG_TOS, &[0]);
    }
    if s.family == AddressFamily::AF_INET6.0 as u8 {
        if ext(INET_DIAG_TCLASS) {
            rta(&mut p, INET_DIAG_TCLASS, &[0]);
        }
        if listening || s.state == TCP_CLOSE {
            rta(&mut p, INET_DIAG_SKV6ONLY, &[s.v6only as u8]);
        }
    }
    if net_admin {
        rta(&mut p, INET_DIAG_MARK, &0u32.to_le_bytes());
    }
    if ext(INET_DIAG_CLASS_ID) || ext(INET_DIAG_TCLASS) {
        rta(&mut p, INET_DIAG_CLASS_ID, &0u32.to_le_bytes());
    }
    // `inet_diag_sockopt`: `is_icsk` for TCP, and the defaults of `mc_loop` and `mc_all`.
    let tcp = s.protocol == IpProtocol::IPPROTO_TCP.0 as u8 && !s.raw;
    let sockopt = (u8::from(tcp) << 1) | (1 << 4) | (1 << 6);
    rta(&mut p, INET_DIAG_SOCKOPT, &[sockopt, 0]);
    if ext(INET_DIAG_MEMINFO) {
        let meminfo = InetDiagMeminfo {
            idiag_rmem: if listening { 0 } else { s.rx_queue as u32 },
            idiag_wmem: if listening { 0 } else { s.tx_queue as u32 },
            idiag_fmem: 0,
            idiag_tmem: 0,
        };
        let mut raw = vec![0; InetDiagMeminfo::SIZE];
        meminfo.write_to(&mut raw);
        rta(&mut p, INET_DIAG_MEMINFO, &raw);
    }
    if ext(INET_DIAG_SKMEMINFO) {
        let rmem = if listening { 0 } else { s.rx_queue as u32 };
        let mem = [rmem, 0, 0, 0, 0, 0, 0, 0, 0];
        let raw: Vec<u8> = mem.iter().flat_map(|v| v.to_le_bytes()).collect();
        rta(&mut p, INET_DIAG_SKMEMINFO, &raw);
    }
    if s.raw {
        rta(&mut p, INET_DIAG_PROTOCOL, &[s.protocol]);
    }
    if tcp && ext(INET_DIAG_CONG) {
        rta(&mut p, INET_DIAG_CONG, b"cubic\0");
    }
    nlmsg(SOCK_DIAG_BY_FAMILY, flags, r.seq, r.port, &p)
}

/// `inet_diag_bc_op` codes.
const BC_NOP: u8 = 0;
const BC_JMP: u8 = 1;
const BC_S_GE: u8 = 2;
const BC_S_LE: u8 = 3;
const BC_D_GE: u8 = 4;
const BC_D_LE: u8 = 5;
const BC_AUTO: u8 = 6;
const BC_S_COND: u8 = 7;
const BC_D_COND: u8 = 8;
const BC_DEV_COND: u8 = 9;
const BC_MARK_COND: u8 = 10;
const BC_S_EQ: u8 = 11;
const BC_D_EQ: u8 = 12;
/// `sizeof(struct inet_diag_bc_op)` and `sizeof(struct inet_diag_hostcond)`.
const OP: usize = 4;
const HOSTCOND: usize = 8;

/// One `inet_diag_bc_op` at `at`: code, yes, no.
fn op(bc: &[u8], at: usize) -> (u8, usize, usize) {
    (
        bc[at],
        usize::from(bc[at + 1]),
        usize::from(u16::from_le_bytes([bc[at + 2], bc[at + 3]])),
    )
}

/// `inet_diag_bc_audit`.
fn audit(bc: Option<&[u8]>, net_admin: bool) -> Result<(), Errno> {
    let Some(bc) = bc else { return Ok(()) };
    if bc.len() < OP {
        return Err(Errno::EINVAL);
    }
    let total = bc.len() as isize;
    let mut at = 0usize;
    let mut len = total;
    while len > 0 {
        if (len as usize) < OP {
            return Err(Errno::EINVAL);
        }
        let (code, yes, no) = op(bc, at);
        let mut min_len = OP;
        let rest = &bc[at..];
        match code {
            BC_S_COND | BC_D_COND => {
                min_len += HOSTCOND;
                if rest.len() < min_len {
                    return Err(Errno::EINVAL);
                }
                let addr_len = match i32::from(rest[OP]) {
                    f if f == AddressFamily::AF_UNSPEC.0 => 0,
                    f if f == AddressFamily::AF_INET.0 => 4,
                    f if f == AddressFamily::AF_INET6.0 => 16,
                    _ => return Err(Errno::EINVAL),
                };
                min_len += addr_len;
                if rest.len() < min_len || usize::from(rest[OP + 1]) > 8 * addr_len {
                    return Err(Errno::EINVAL);
                }
            }
            BC_DEV_COND | BC_S_EQ | BC_S_GE | BC_S_LE | BC_D_EQ | BC_D_GE | BC_D_LE => {
                min_len += 4;
                if rest.len() < min_len {
                    return Err(Errno::EINVAL);
                }
            }
            BC_MARK_COND => {
                if !net_admin {
                    return Err(Errno::EPERM);
                }
                min_len += 8;
                if rest.len() < min_len {
                    return Err(Errno::EINVAL);
                }
            }
            BC_AUTO | BC_JMP | BC_NOP => {}
            _ => return Err(Errno::EINVAL),
        }
        if code != BC_NOP {
            if no < min_len || no as isize > len + 4 || no & 3 != 0 {
                return Err(Errno::EINVAL);
            }
            if (no as isize) < len && !valid_cc(bc, len - no as isize) {
                return Err(Errno::EINVAL);
            }
        }
        if yes < min_len || yes as isize > len + 4 || yes & 3 != 0 {
            return Err(Errno::EINVAL);
        }
        at += yes;
        len -= yes as isize;
    }
    match len {
        0 => Ok(()),
        _ => Err(Errno::EINVAL),
    }
}

/// `valid_cc`: a jump lands on an instruction, `cc` bytes before the end.
fn valid_cc(bc: &[u8], cc: isize) -> bool {
    let mut len = bc.len() as isize;
    let mut at = 0usize;
    while len >= 0 {
        if cc > len {
            return false;
        }
        if cc == len {
            return true;
        }
        let Some(&yes) = bc.get(at + 1) else {
            return false;
        };
        if yes < 4 || yes & 3 != 0 {
            return false;
        }
        len -= isize::from(yes);
        at += usize::from(yes);
    }
    false
}

/// `inet_diag_bc_run`: whether the audited program accepts `s`.
fn run(bc: &[u8], s: &InetSock) -> bool {
    let mut len = bc.len() as isize;
    let mut at = 0usize;
    let port_at = |at: usize| u16::from_le_bytes([bc[at + OP + 2], bc[at + OP + 3]]);
    while len > 0 {
        let (code, yes_step, no_step) = op(bc, at);
        let yes = match code {
            BC_JMP => false,
            BC_S_EQ => s.sport() == port_at(at),
            BC_S_GE => s.sport() >= port_at(at),
            BC_S_LE => s.sport() <= port_at(at),
            BC_D_EQ => s.dport() == port_at(at),
            BC_D_GE => s.dport() >= port_at(at),
            BC_D_LE => s.dport() <= port_at(at),
            BC_AUTO => !s.port_locked,
            BC_S_COND | BC_D_COND => hostcond(&bc[at + OP..], s, code == BC_S_COND),
            BC_DEV_COND => {
                let index = u32::from_le_bytes(bc[at + OP..at + OP + 4].try_into().unwrap());
                index == s.bound_dev
            }
            // Every socket's mark is 0.
            BC_MARK_COND => u32::from_le_bytes(bc[at + OP..at + OP + 4].try_into().unwrap()) == 0,
            _ => true,
        };
        let step = if yes { yes_step } else { no_step };
        len -= step as isize;
        at += step;
    }
    len == 0
}

/// An `S_COND`/`D_COND` on the socket's local or peer end: the port unless -1, the family
/// (an IPv4 condition matches an IPv4-mapped address), the prefix.
fn hostcond(cond: &[u8], s: &InetSock, source: bool) -> bool {
    let family = cond[0];
    let prefix = usize::from(cond[1]);
    let port = i32::from_le_bytes(cond[4..8].try_into().unwrap());
    let (sock_port, ip) = match source {
        true => (s.sport(), s.local.ip()),
        false => (s.dport(), s.daddr()),
    };
    if port != -1 && port != i32::from(sock_port) {
        return false;
    }
    let addr = &cond[HOSTCOND..];
    let bytes = match ip {
        IpAddr::V4(v4) => v4.octets().to_vec(),
        IpAddr::V6(v6) => v6.octets().to_vec(),
    };
    let unspec = AddressFamily::AF_UNSPEC.0 as u8;
    let inet = AddressFamily::AF_INET.0 as u8;
    if family != unspec && family != s.family {
        let mapped = match ip {
            IpAddr::V6(v6) => v6.to_ipv4_mapped(),
            IpAddr::V4(_) => None,
        };
        return match (family == inet, mapped) {
            (true, Some(v4)) => prefix_match(&v4.octets(), addr, prefix),
            _ => false,
        };
    }
    prefix == 0 || prefix_match(&bytes, addr, prefix)
}

/// `bitstring_match`: the first `bits` bits agree.
fn prefix_match(a: &[u8], b: &[u8], bits: usize) -> bool {
    let (whole, rest) = (bits / 8, bits % 8);
    if a.len() < whole + usize::from(rest > 0) || b.len() < whole + usize::from(rest > 0) {
        return false;
    }
    if a[..whole] != b[..whole] {
        return false;
    }
    let mask = 0xffu8 << (8 - rest);
    rest == 0 || (a[whole] ^ b[whole]) & mask == 0
}

/// `unix_diag_handler_dump`: a dump of the namespace's Unix sockets in the states asked for,
/// or the one socket of an inode.
fn unix(r: &Request<'_>, payload: &[u8]) -> Result<Vec<u8>, Errno> {
    let req = UnixDiagReq::read_from(payload).ok_or(Errno::EINVAL)?;
    let entries = r
        .net
        .unix_names
        .sockets()
        .iter()
        .map(|sock| sock.diag_entry())
        // `sk_diag_dump`: a socket without a file is skipped.
        .filter(|e| e.ino != 0)
        .collect::<Vec<_>>();
    if r.flags & NLM_F_DUMP == 0 {
        // `unix_diag_get_exact`.
        if req.udiag_ino == 0 {
            return Err(Errno::ENOENT);
        }
        let found = entries
            .iter()
            .find(|e| e.ino as u32 == req.udiag_ino)
            .ok_or(Errno::ENOENT)?;
        let cookie = [found.cookie as u32, (found.cookie >> 32) as u32];
        if req.udiag_cookie != [NOCOOKIE; 2] && req.udiag_cookie != cookie {
            return Err(Errno::ESTALE);
        }
        return Ok(unix_msg(r, &req, found, 0));
    }
    let msgs = entries
        .iter()
        .filter(|e| req.udiag_states & (1 << e.state) != 0)
        .map(|e| unix_msg(r, &req, e, NLM_F_MULTI))
        .collect();
    Ok(r.dump(msgs))
}

/// `sk_diag_fill` for a Unix socket.
fn unix_msg(r: &Request<'_>, req: &UnixDiagReq, e: &crate::unix::DiagEntry, flags: u16) -> Vec<u8> {
    let show = |bit: u32| req.udiag_show & bit != 0;
    let msg = UnixDiagMsg {
        udiag_family: AddressFamily::AF_UNIX.0 as u8,
        udiag_type: e.socket_type.0 as u8,
        udiag_state: e.state,
        pad: 0,
        udiag_ino: e.ino as u32,
        udiag_cookie: [e.cookie as u32, (e.cookie >> 32) as u32],
    };
    let mut p = vec![0; UnixDiagMsg::SIZE];
    msg.write_to(&mut p);
    if show(UDIAG_SHOW_NAME)
        && let Some(addr) = &e.addr
    {
        rta(&mut p, UNIX_DIAG_NAME, &addr[2..]);
    }
    if show(UDIAG_SHOW_VFS)
        && let Some((ino, dev)) = e.vfs
    {
        let vfs = UnixDiagVfs {
            udiag_vfs_ino: ino as u32,
            udiag_vfs_dev: new_encode_dev(dev.major, dev.minor),
        };
        let mut raw = vec![0; UnixDiagVfs::SIZE];
        vfs.write_to(&mut raw);
        rta(&mut p, UNIX_DIAG_VFS, &raw);
    }
    if show(UDIAG_SHOW_PEER)
        && let Some(peer) = e.peer
    {
        rta(&mut p, UNIX_DIAG_PEER, &(peer as u32).to_le_bytes());
    }
    if show(UDIAG_SHOW_ICONS) && e.state == TCP_LISTEN {
        let icons: Vec<u8> = e
            .icons
            .iter()
            .flat_map(|ino| (*ino as u32).to_le_bytes())
            .collect();
        rta(&mut p, UNIX_DIAG_ICONS, &icons);
    }
    if show(UDIAG_SHOW_RQLEN) {
        let rqlen = UnixDiagRqlen {
            udiag_rqueue: e.rqlen.0,
            udiag_wqueue: e.rqlen.1,
        };
        let mut raw = vec![0; UnixDiagRqlen::SIZE];
        rqlen.write_to(&mut raw);
        rta(&mut p, UNIX_DIAG_RQLEN, &raw);
    }
    if show(UDIAG_SHOW_MEMINFO) {
        let raw: Vec<u8> = e.meminfo.iter().flat_map(|v| v.to_le_bytes()).collect();
        rta(&mut p, UNIX_DIAG_MEMINFO, &raw);
    }
    rta(&mut p, UNIX_DIAG_SHUTDOWN, &[e.shutdown]);
    if show(UDIAG_SHOW_UID) {
        rta(&mut p, UNIX_DIAG_UID, &e.uid.to_le_bytes());
    }
    nlmsg(SOCK_DIAG_BY_FAMILY, flags, r.seq, r.port, &p)
}

/// `new_encode_dev`.
fn new_encode_dev(major: u32, minor: u32) -> u32 {
    (minor & 0xff) | (major << 8) | ((minor & !0xff) << 12)
}
