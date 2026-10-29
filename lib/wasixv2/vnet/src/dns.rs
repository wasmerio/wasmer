//! The namespace's DNS server ([`NetConfig::dns_server`]), slirp's 10.0.2.3: a host on the
//! uplink that answers DNS queries over UDP and TCP port 53 (RFC 1035 4.2, RFC 7766) from the
//! backend's resolver ([`Backend::lookup`]).
//!
//! - Questions of class IN for A, AAAA and CNAME, and PTR of a whole address (`in-addr.arpa`,
//!   `ip6.arpa`), are answered from the host's lookups; `localhost` and the names under it
//!   from RFC 6761 6.3 without asking. Every other query goes to the host's name servers as it
//!   is ([`Lookup::Forward`]).
//! - A name whose canonical name differs is answered with the CNAME and the records of the
//!   canonical name. A name the host does not know is `NXDOMAIN`, one without addresses of the
//!   family asked `NOERROR` with no answer, a lookup the host cannot finish `SERVFAIL`.
//! - A UDP answer longer than 512 bytes, or than the payload size of the query's EDNS record
//!   (RFC 6891), keeps the whole records that fit and sets TC, so the client asks again over
//!   TCP, as musl's `res_msend` does.
//! - The host also answers echo requests, as slirp's does, and refuses other ports: a port
//!   unreachable for UDP, a refused connection for TCP.
//!
//! A query never blocks the sender: the lookup completes on the backend's thread, and the
//! answer is delivered then, to whichever socket holds the client's address by that time.
//!
//! [`NetConfig::dns_server`]: crate::NetConfig::dns_server

use std::{
    collections::VecDeque,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

use wasixv2_abi::Errno;
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue};

use crate::{
    Answer, Backend, BackendStream, Found, Lookup, LookupError, NetResult, Shutdown, StreamState,
    bpf::{Arrival, PACKET_HOST},
    net::{Net, udp_receiver},
    sync::MutexExt,
};

/// The DNS port, UDP and TCP.
pub(crate) const PORT: u16 = 53;
const HEADER: usize = 12;
/// The longest UDP message without EDNS (RFC 1035 4.2.1).
const UDP_LIMIT: usize = 512;
/// The longest message over TCP, whose length prefix is 16 bits (RFC 1035 4.2.2).
const TCP_LIMIT: usize = 65535;
/// The payload size the server's OPT record announces (the DNS flag day 2020 default).
const EDNS_SIZE: u16 = 1232;
/// The TTL of `localhost` records, which never change.
const LOCALHOST_TTL: u32 = 86400;

const TYPE_A: u16 = 1;
const TYPE_CNAME: u16 = 5;
const TYPE_PTR: u16 = 12;
const TYPE_AAAA: u16 = 28;
const TYPE_OPT: u16 = 41;
const CLASS_IN: u16 = 1;

const FLAG_QR: u16 = 0x8000;
const FLAG_TC: u16 = 0x0200;
const FLAG_RD: u16 = 0x0100;
const FLAG_RA: u16 = 0x0080;

const NOERROR: u8 = 0;
const FORMERR: u8 = 1;
const SERVFAIL: u8 = 2;
const NXDOMAIN: u8 = 3;
const NOTIMP: u8 = 4;
/// `BADVERS` (16): 1 in the OPT record's extended RCODE, 0 in the header (RFC 6891 6.1.3).
const BADVERS_EXT: u8 = 1;

/// The longest name on the wire, its terminating root label included (RFC 1035 3.1).
const NAME_MAX: usize = 255;
const LABEL_MAX: usize = 63;

/// The transport a query came over, which bounds its answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Transport {
    Udp,
    Tcp,
}

/// A query the server answers: its question and what the answer echoes.
#[derive(Clone, Debug)]
struct Query {
    id: u16,
    /// The RD bit, which the answer copies.
    rd: bool,
    /// The question section as received, the name's case kept (RFC 1035 7.3, and the 0x20
    /// bit some resolvers check).
    question: Vec<u8>,
    labels: Vec<Vec<u8>>,
    qtype: u16,
    qclass: u16,
    /// The query's OPT record: (UDP payload size, EDNS version).
    edns: Option<(u16, u8)>,
}

impl Query {
    /// The most the answer may take over `transport`.
    fn limit(&self, transport: Transport) -> usize {
        match (transport, self.edns) {
            (Transport::Tcp, _) => TCP_LIMIT,
            (Transport::Udp, None) => UDP_LIMIT,
            (Transport::Udp, Some((size, _))) => usize::from(size).max(UDP_LIMIT),
        }
    }
}

/// What a message to the server asks for.
#[derive(Debug)]
enum Parsed {
    Query(Query),
    /// A message the server answers with this header only: `FORMERR`, `NOTIMP`.
    Refuse {
        id: u16,
        opcode: u8,
        rd: bool,
        rcode: u8,
    },
    /// A response, or too short for a header: nothing to answer.
    Ignore,
}

/// Reads the name at `pos` of `msg`, following compression pointers (RFC 1035 4.1.4), each of
/// which must point back: its labels and where it ends in place.
fn read_name(msg: &[u8], pos: usize) -> Option<(Vec<Vec<u8>>, usize)> {
    let mut labels = Vec::new();
    let mut at = pos;
    let mut end = None;
    let mut len = 1;
    loop {
        let b = *msg.get(at)?;
        match b & 0xc0 {
            0x00 if b == 0 => return Some((labels, end.unwrap_or(at + 1))),
            0x00 => {
                let n = usize::from(b);
                len += n + 1;
                if len > NAME_MAX {
                    return None;
                }
                labels.push(msg.get(at + 1..at + 1 + n)?.to_vec());
                at += 1 + n;
            }
            0xc0 => {
                let target = usize::from(b & 0x3f) << 8 | usize::from(*msg.get(at + 1)?);
                if target >= at {
                    return None;
                }
                end.get_or_insert(at + 2);
                at = target;
            }
            _ => return None,
        }
    }
}

fn read_u16(msg: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(msg.get(at..at + 2)?.try_into().ok()?))
}

/// A resource record's fields at `pos`: (type, class, TTL, end of the record).
fn read_rr(msg: &[u8], pos: usize) -> Option<(u16, u16, u32, usize)> {
    let (_, at) = read_name(msg, pos)?;
    let kind = read_u16(msg, at)?;
    let class = read_u16(msg, at + 2)?;
    let ttl = u32::from_be_bytes(msg.get(at + 4..at + 8)?.try_into().ok()?);
    let rdlen = usize::from(read_u16(msg, at + 8)?);
    let end = at + 10 + rdlen;
    (end <= msg.len()).then_some((kind, class, ttl, end))
}

/// `msg`'s header counts: question, answer, authority, additional.
fn counts(msg: &[u8]) -> [u16; 4] {
    [4, 6, 8, 10].map(|at| read_u16(msg, at).unwrap_or(0))
}

/// What a message to the server asks for (RFC 1035 4.1, RFC 6891 6.1.1).
fn parse(msg: &[u8]) -> Parsed {
    let (Some(id), Some(flags)) = (read_u16(msg, 0), read_u16(msg, 2)) else {
        return Parsed::Ignore;
    };
    if msg.len() < HEADER || flags & FLAG_QR != 0 {
        return Parsed::Ignore;
    }
    let opcode = ((flags >> 11) & 0xf) as u8;
    let rd = flags & FLAG_RD != 0;
    let refuse = |rcode| Parsed::Refuse {
        id,
        opcode,
        rd,
        rcode,
    };
    if opcode != 0 {
        return refuse(NOTIMP);
    }
    let [qd, an, ns, ar] = counts(msg);
    if qd != 1 {
        return refuse(FORMERR);
    }
    let Some((labels, at)) = read_name(msg, HEADER) else {
        return refuse(FORMERR);
    };
    let (Some(qtype), Some(qclass)) = (read_u16(msg, at), read_u16(msg, at + 2)) else {
        return refuse(FORMERR);
    };
    let question = msg[HEADER..at + 4].to_vec();
    let mut pos = at + 4;
    let mut edns = None;
    for i in 0..usize::from(an) + usize::from(ns) + usize::from(ar) {
        let Some((kind, class, ttl, end)) = read_rr(msg, pos) else {
            return refuse(FORMERR);
        };
        if i >= usize::from(an) + usize::from(ns) && kind == TYPE_OPT {
            // One OPT record, owned by the root (RFC 6891 6.1.1).
            if edns.is_some() || msg[pos] != 0 {
                return refuse(FORMERR);
            }
            edns = Some((class, (ttl >> 16) as u8));
        }
        pos = end;
    }
    Parsed::Query(Query {
        id,
        rd,
        question,
        labels,
        qtype,
        qclass,
        edns,
    })
}

/// What the server answers with: at once, or once the backend's resolver has looked.
enum Plan {
    Ready(Result<Found, LookupError>, Want),
    Ask(Lookup, Want),
}

/// The records an answer is made of.
#[derive(Clone, Copy, Debug)]
enum Want {
    /// The addresses of this type (A or AAAA), after the CNAME if the name has one.
    Addrs(u16),
    /// The CNAME, if the name has one.
    Cname,
    Ptr,
    /// Nothing: the name exists without records of the type.
    Nothing,
    /// The name servers' response as it is.
    Forward,
}

/// RFC 6761 6.3: `localhost.` and the names under it are the loopback addresses.
fn is_localhost(labels: &[Vec<u8>]) -> bool {
    labels
        .last()
        .is_some_and(|l| l.eq_ignore_ascii_case(b"localhost"))
}

/// The name as the host's resolver takes it, lower case without the trailing dot: `None` for
/// the root and for labels a host name cannot spell (dots, backslashes, bytes outside
/// printable ASCII), which go to the name servers as they are.
fn host_name(labels: &[Vec<u8>]) -> Option<String> {
    let printable = |l: &Vec<u8>| {
        l.iter()
            .all(|&b| b.is_ascii_graphic() && b != b'.' && b != b'\\')
    };
    if labels.is_empty() || !labels.iter().all(printable) {
        return None;
    }
    let labels: Vec<String> = labels
        .iter()
        .map(|l| String::from_utf8_lossy(l).to_ascii_lowercase())
        .collect();
    Some(labels.join("."))
}

/// The address a whole reverse name stands for (RFC 1035 3.5, RFC 3596 2.5); `None` for a
/// name above it (a reverse zone, a classless delegation), which only name servers know.
fn reverse_addr(labels: &[Vec<u8>]) -> Option<IpAddr> {
    let lower: Vec<String> = labels
        .iter()
        .map(|l| String::from_utf8_lossy(l).to_ascii_lowercase())
        .collect();
    match lower.as_slice() {
        [d, c, b, a, arpa, top] if arpa == "in-addr" && top == "arpa" => {
            let octet = |s: &String| {
                (!s.is_empty() && s.len() <= 3 && s.bytes().all(|b| b.is_ascii_digit()))
                    .then(|| s.parse::<u8>().ok())
                    .flatten()
            };
            Some(IpAddr::V4(Ipv4Addr::new(
                octet(a)?,
                octet(b)?,
                octet(c)?,
                octet(d)?,
            )))
        }
        [nibbles @ .., ip6, top] if nibbles.len() == 32 && ip6 == "ip6" && top == "arpa" => {
            let mut v = 0u128;
            for n in nibbles.iter().rev() {
                let [c] = n.as_bytes() else {
                    return None;
                };
                v = v << 4 | u128::from(char::from(*c).to_digit(16)?);
            }
            Some(IpAddr::V6(Ipv6Addr::from(v)))
        }
        _ => None,
    }
}

/// Decides how to answer `q`, received as `msg`.
fn plan(q: &Query, msg: &[u8]) -> Plan {
    let forward = || Plan::Ask(Lookup::Forward(msg.to_vec()), Want::Forward);
    if q.qclass != CLASS_IN {
        return forward();
    }
    let want = match q.qtype {
        TYPE_A | TYPE_AAAA => Want::Addrs(q.qtype),
        TYPE_CNAME => Want::Cname,
        TYPE_PTR => match reverse_addr(&q.labels) {
            Some(addr) => return Plan::Ask(Lookup::Addr(addr), Want::Ptr),
            None => return forward(),
        },
        _ if is_localhost(&q.labels) => Want::Nothing,
        _ => return forward(),
    };
    let Some(name) = host_name(&q.labels) else {
        return forward();
    };
    if is_localhost(&q.labels) {
        let found = Found {
            answer: Answer::Addrs {
                canonical: name,
                addrs: vec![Ipv4Addr::LOCALHOST.into(), Ipv6Addr::LOCALHOST.into()],
            },
            ttl: LOCALHOST_TTL,
        };
        return Plan::Ready(Ok(found), want);
    }
    Plan::Ask(Lookup::Name(name), want)
}

/// The labels of a name the host's resolver gave, `None` if it cannot be one.
fn labels_of(name: &str) -> Option<Vec<Vec<u8>>> {
    let labels: Vec<Vec<u8>> = name
        .trim_end_matches('.')
        .split('.')
        .map(|l| l.as_bytes().to_vec())
        .collect();
    let len: usize = labels.iter().map(|l| l.len() + 1).sum::<usize>() + 1;
    let ok = labels.iter().all(|l| !l.is_empty() && l.len() <= LABEL_MAX) && len <= NAME_MAX;
    ok.then_some(labels)
}

/// Where a record's owner name is: a compression pointer to an earlier name.
type NameAt = u16;
/// The question's name, right after the header.
const QNAME_AT: NameAt = HEADER as NameAt;

/// An answer under construction: the header, the question and the answer records.
struct Builder {
    msg: Vec<u8>,
    answers: u16,
}

impl Builder {
    fn new(q: &Query, rcode: u8) -> Self {
        let mut flags = FLAG_QR | FLAG_RA | u16::from(rcode);
        if q.rd {
            flags |= FLAG_RD;
        }
        let mut msg = header(q.id, flags, 1);
        msg.extend_from_slice(&q.question);
        Self { msg, answers: 0 }
    }

    /// Appends a record owned by the name at `owner`, of `kind` and `ttl`, whose data is
    /// written by `rdata`; it returns where in the message the data starts.
    fn record(&mut self, owner: NameAt, kind: u16, ttl: u32, rdata: impl FnOnce(&mut Vec<u8>)) {
        self.msg.extend_from_slice(&(0xc000 | owner).to_be_bytes());
        self.msg.extend_from_slice(&kind.to_be_bytes());
        self.msg.extend_from_slice(&CLASS_IN.to_be_bytes());
        self.msg.extend_from_slice(&ttl.to_be_bytes());
        let len_at = self.msg.len();
        self.msg.extend_from_slice(&[0, 0]);
        rdata(&mut self.msg);
        let len = (self.msg.len() - len_at - 2) as u16;
        self.msg[len_at..len_at + 2].copy_from_slice(&len.to_be_bytes());
        self.answers += 1;
    }

    /// Appends a record whose data is the name `labels`, written in full: where that name
    /// starts, for later records it owns.
    fn name_record(&mut self, owner: NameAt, kind: u16, ttl: u32, labels: &[Vec<u8>]) -> NameAt {
        // Owner pointer (2), type, class, TTL, length (10).
        let at = (self.msg.len() + 12) as NameAt;
        self.record(owner, kind, ttl, |out| write_name(out, labels));
        at
    }

    /// The finished message for `q` over `transport`: the answers that fit, TC if some did
    /// not, and the OPT record if the query had one, even when truncated (RFC 6891 7).
    fn finish(mut self, q: &Query, transport: Transport, ext_rcode: u8) -> Vec<u8> {
        self.msg[6..8].copy_from_slice(&self.answers.to_be_bytes());
        let opt = q.edns.map(|_| opt_record(ext_rcode));
        let room = q.limit(transport) - opt.as_ref().map_or(0, Vec::len);
        let mut msg = fit(self.msg, room);
        if let Some(opt) = opt {
            msg.extend_from_slice(&opt);
            msg[10..12].copy_from_slice(&1u16.to_be_bytes());
        }
        msg
    }
}

fn header(id: u16, flags: u16, qdcount: u16) -> Vec<u8> {
    let mut msg = Vec::with_capacity(UDP_LIMIT);
    msg.extend_from_slice(&id.to_be_bytes());
    msg.extend_from_slice(&flags.to_be_bytes());
    msg.extend_from_slice(&qdcount.to_be_bytes());
    msg.extend_from_slice(&[0; 6]);
    msg
}

fn write_name(out: &mut Vec<u8>, labels: &[Vec<u8>]) {
    for l in labels {
        out.push(l.len() as u8);
        out.extend_from_slice(l);
    }
    out.push(0);
}

/// The server's OPT record: its payload size, `ext_rcode`, version 0 (RFC 6891 6.1.2).
fn opt_record(ext_rcode: u8) -> Vec<u8> {
    let mut rr = vec![0];
    rr.extend_from_slice(&TYPE_OPT.to_be_bytes());
    rr.extend_from_slice(&EDNS_SIZE.to_be_bytes());
    rr.extend_from_slice(&[ext_rcode, 0, 0, 0, 0, 0]);
    rr
}

/// `msg` cut to at most `room` bytes at a record boundary, with TC set and the counts of
/// what is left, as RFC 2181 9 has a truncated answer; `msg` itself when it fits. Records
/// only point back (RFC 1035 4.1.4), so the ones left stay whole. A message whose records do
/// not parse loses all of them.
fn fit(mut msg: Vec<u8>, room: usize) -> Vec<u8> {
    if msg.len() <= room {
        return msg;
    }
    let [qd, an, ns, ar] = counts(&msg);
    let mut pos = HEADER;
    for _ in 0..qd {
        match read_name(&msg, pos) {
            Some((_, end)) if end + 4 <= msg.len() => pos = end + 4,
            _ => {
                pos = HEADER;
                break;
            }
        }
    }
    let mut kept = [0u16; 3];
    let mut cut = pos;
    'sections: for (section, count) in [an, ns, ar].into_iter().enumerate() {
        for _ in 0..count {
            match read_rr(&msg, pos) {
                Some((.., end)) if end <= room => {
                    pos = end;
                    cut = end;
                    kept[section] += 1;
                }
                _ => break 'sections,
            }
        }
    }
    if cut > room {
        // Not even the question fits: the header alone.
        cut = HEADER;
        msg[4..6].fill(0);
    }
    msg.truncate(cut);
    for (i, n) in kept.into_iter().enumerate() {
        msg[6 + 2 * i..8 + 2 * i].copy_from_slice(&n.to_be_bytes());
    }
    let flags = read_u16(&msg, 2).unwrap_or(0) | FLAG_TC;
    msg[2..4].copy_from_slice(&flags.to_be_bytes());
    msg
}

/// The answer to `q` over `transport`, from what the resolver found.
fn respond(
    q: &Query,
    transport: Transport,
    found: Result<Found, LookupError>,
    want: Want,
) -> Vec<u8> {
    let (found, ttl) = match found {
        Ok(Found { answer, ttl }) => (answer, ttl),
        Err(LookupError::NotFound { .. }) => {
            return Builder::new(q, NXDOMAIN).finish(q, transport, 0);
        }
        Err(LookupError::Failed) => return Builder::new(q, SERVFAIL).finish(q, transport, 0),
    };
    let mut b = Builder::new(q, NOERROR);
    match (want, found) {
        (Want::Forward, Answer::Message(mut msg)) if msg.len() >= HEADER => {
            msg[..2].copy_from_slice(&q.id.to_be_bytes());
            return fit(msg, q.limit(transport));
        }
        (Want::Addrs(_) | Want::Cname | Want::Nothing, Answer::Addrs { canonical, addrs }) => {
            let canonical = labels_of(&canonical).filter(|c| !same_name(c, &q.labels));
            let mut owner = QNAME_AT;
            if let Some(c) = &canonical
                && !matches!(want, Want::Nothing)
            {
                owner = b.name_record(QNAME_AT, TYPE_CNAME, ttl, c);
            }
            if let Want::Addrs(kind) = want {
                for addr in addrs {
                    match (kind, addr) {
                        (TYPE_A, IpAddr::V4(v4)) => {
                            b.record(owner, TYPE_A, ttl, |o| o.extend_from_slice(&v4.octets()))
                        }
                        (TYPE_AAAA, IpAddr::V6(v6)) => {
                            b.record(owner, TYPE_AAAA, ttl, |o| o.extend_from_slice(&v6.octets()))
                        }
                        _ => {}
                    }
                }
            }
        }
        (Want::Ptr, Answer::Name(name)) => match labels_of(&name) {
            Some(labels) => {
                b.name_record(QNAME_AT, TYPE_PTR, ttl, &labels);
            }
            None => return Builder::new(q, SERVFAIL).finish(q, transport, 0),
        },
        _ => return Builder::new(q, SERVFAIL).finish(q, transport, 0),
    }
    b.finish(q, transport, 0)
}

/// Whether two names are the same, label by label, ignoring ASCII case (RFC 4343).
fn same_name(a: &[Vec<u8>], b: &[Vec<u8>]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.eq_ignore_ascii_case(b))
}

/// The answer to a message that names no query the server takes (`FORMERR`, `NOTIMP`): the
/// header alone, with the query's identifier, opcode and RD bit.
fn refusal(id: u16, opcode: u8, rd: bool, rcode: u8) -> Vec<u8> {
    let mut flags = FLAG_QR | FLAG_RA | u16::from(opcode) << 11 | u16::from(rcode);
    if rd {
        flags |= FLAG_RD;
    }
    header(id, flags, 0)
}

/// Answers the message `msg` that came over `transport`: `reply` gets the answer, or `None`
/// for a message that gets none, at once or once the backend's resolver has looked.
fn serve(
    backend: Option<&Arc<dyn Backend>>,
    msg: &[u8],
    transport: Transport,
    reply: impl FnOnce(Option<Vec<u8>>) + Send + 'static,
) {
    let q = match parse(msg) {
        Parsed::Query(q) => q,
        Parsed::Refuse {
            id,
            opcode,
            rd,
            rcode,
        } => return reply(Some(refusal(id, opcode, rd, rcode))),
        Parsed::Ignore => return reply(None),
    };
    if q.edns.is_some_and(|(_, version)| version != 0) {
        return reply(Some(Builder::new(&q, NOERROR).finish(
            &q,
            transport,
            BADVERS_EXT,
        )));
    }
    match (plan(&q, msg), backend) {
        (Plan::Ready(found, want), _) => reply(Some(respond(&q, transport, found, want))),
        (Plan::Ask(lookup, want), Some(backend)) => backend.lookup(
            lookup,
            Box::new(move |found| reply(Some(respond(&q, transport, found, want)))),
        ),
        (Plan::Ask(_, want), None) => {
            reply(Some(respond(&q, transport, Err(LookupError::Failed), want)))
        }
    }
}

impl Net {
    /// Whether `ip` (canonical) is the namespace's DNS server.
    pub(crate) fn is_dns_server(&self, ip: IpAddr) -> bool {
        self.config.dns_server.contains(&ip)
    }

    /// A UDP datagram from `from` to the DNS server at `to`, sent at `now`: a query to port
    /// 53 is answered back to `from`; another port is unreachable.
    pub(crate) fn dns_datagram(
        &self,
        from: SocketAddr,
        to: SocketAddr,
        data: &[u8],
        now: Duration,
    ) {
        if to.port() != PORT {
            self.dns_port_unreachable(from, to, data, now);
            return;
        }
        let net = Arc::downgrade(&self.arc());
        serve(self.backend.as_ref(), data, Transport::Udp, move |answer| {
            if let Some(answer) = answer {
                deliver(&net, to, from, &answer);
            }
        });
    }

    /// A TCP connection to the DNS server at `to`: a DNS stream for port 53, refused
    /// elsewhere. It wakes `wq`, the guest socket's queue.
    pub(crate) fn dns_connect(&self, to: SocketAddr, wq: WaitQueue) -> Arc<dyn BackendStream> {
        let refused = to.port() != PORT;
        Arc::new(DnsStream(Arc::new(Conn {
            peer: to,
            backend: self.backend.clone(),
            wq,
            st: Mutex::new(ConnState {
                err: refused.then_some(Errno::ECONNREFUSED),
                refused,
                ..ConnState::default()
            }),
        })))
    }
}

/// The answer `data` from the server at `server` to the client at `client`: it arrives on the
/// uplink at the socket that holds the client's address now, if any, as a datagram from
/// outside does.
fn deliver(net: &Weak<Net>, server: SocketAddr, client: SocketAddr, data: &[u8]) {
    let Some(net) = net.upgrade() else {
        return;
    };
    let to = udp_receiver(&net.state(), server.ip(), server, client);
    let Some(sock) = to.and_then(|w| w.upgrade()) else {
        return;
    };
    let arrival = Arrival {
        src: server.ip(),
        dst: client.ip(),
        ttl: 64,
        iif: net.uplink_index(),
        pkttype: PACKET_HOST,
    };
    sock.deliver(data, server, &arrival);
}

/// Queries one connection may have unanswered before the server stops reading it.
const MAX_PENDING: usize = 16;
/// A whole message with its length prefix: as much of a query as the server buffers.
const FRAME_MAX: usize = 2 + TCP_LIMIT;

/// A TCP connection to the DNS server (RFC 7766): queries and answers each with a 16-bit
/// length prefix, answered as their lookups finish, possibly out of order (6.2.1.1). The
/// server closes its side once the client closed its own and every query is answered.
/// Connections to other ports are refused.
struct DnsStream(Arc<Conn>);

struct Conn {
    peer: SocketAddr,
    backend: Option<Arc<dyn Backend>>,
    wq: WaitQueue,
    st: Mutex<ConnState>,
}

#[derive(Default)]
struct ConnState {
    /// The connection was refused (`err` until taken).
    refused: bool,
    err: Option<Errno>,
    /// What the client sent of a query that is not whole yet.
    query: Vec<u8>,
    /// Prefix delivered by TCP; bytes after it are still held by MSG_MORE/TCP_CORK.
    ready: usize,
    /// SO_LINGER zero requests reset rather than an orderly close.
    abort: bool,
    /// Answers for the client, length prefixes included.
    out: VecDeque<u8>,
    pending: usize,
    /// The client shut down its sending side.
    fin: bool,
    /// The client shut down its receiving side.
    rd_shut: bool,
}

impl ConnState {
    /// The server has closed its side: the client's FIN came and every query is answered.
    fn eof(&self) -> bool {
        self.fin && self.pending == 0
    }
}

impl Conn {
    /// A query's answer (`None` for none) is ready.
    fn answered(&self, answer: Option<Vec<u8>>) {
        let mut st = self.st.lock_or_recover();
        st.pending -= 1;
        if let Some(answer) = answer.filter(|_| !st.rd_shut) {
            st.out.extend((answer.len() as u16).to_be_bytes());
            st.out.extend(answer);
        }
        drop(st);
        self.wq.wake(PollMask::empty());
    }
}

impl Pollable for DnsStream {
    /// What `tcp_poll` reports for the guest socket the connection backs.
    fn poll(&self, _: Option<&mut PollTable<'_>>) -> PollMask {
        let st = self.0.st.lock_or_recover();
        let mut m = PollMask::empty();
        if st.err.is_some() {
            m |= PollMask::ERR;
        }
        if st.refused {
            return m
                | PollMask::HUP
                | PollMask::IN
                | PollMask::RDNORM
                | PollMask::RDHUP
                | PollMask::OUT
                | PollMask::WRNORM;
        }
        let rcv_shut = st.eof() || st.rd_shut;
        if rcv_shut && st.fin {
            m |= PollMask::HUP;
        }
        if rcv_shut {
            m |= PollMask::IN | PollMask::RDNORM | PollMask::RDHUP;
        }
        if !st.out.is_empty() {
            m |= PollMask::IN | PollMask::RDNORM;
        }
        if st.fin || st.pending < MAX_PENDING {
            m |= PollMask::OUT | PollMask::WRNORM;
        }
        m
    }
}

impl BackendStream for DnsStream {
    fn tcp_established(&self) -> bool {
        let st = self.0.st.lock_or_recover();
        !st.refused && !st.fin
    }

    fn state(&self) -> StreamState {
        match self.0.st.lock_or_recover().refused {
            true => StreamState::Closed,
            false => StreamState::Established,
        }
    }

    fn read(&self, buf: &mut [u8], peek: bool) -> NetResult<usize> {
        let mut st = self.0.st.lock_or_recover();
        if !st.out.is_empty() {
            let n = buf.len().min(st.out.len());
            for (d, s) in buf.iter_mut().zip(st.out.iter()) {
                *d = *s;
            }
            if !peek {
                st.out.drain(..n);
            }
            return Ok(n);
        }
        if let Some(e) = st.err.take() {
            return Err(e);
        }
        if st.refused || st.eof() || st.rd_shut {
            return Ok(0);
        }
        Err(Errno::EAGAIN)
    }

    /// Takes what fits of a query and answers each query it completes.
    fn write(&self, data: &[u8]) -> NetResult<usize> {
        self.write_more(data, false, 1460)
    }

    fn push(&self) {
        let _ = self.write_more(&[], false, 1);
    }

    fn corked_len(&self) -> usize {
        let st = self.0.st.lock_or_recover();
        st.query.len() - st.ready
    }

    fn output_len(&self) -> usize {
        self.corked_len()
    }

    /// tcp_push applies to this internal TCP peer too: parse only released bytes.
    fn write_more(&self, data: &[u8], more: bool, mss: usize) -> NetResult<usize> {
        let (n, queries) = {
            let mut st = self.0.st.lock_or_recover();
            if st.refused || st.fin {
                return Err(st.err.take().unwrap_or(Errno::EPIPE));
            }
            if st.pending >= MAX_PENDING && !data.is_empty() {
                return Err(Errno::EAGAIN);
            }
            let n = data.len().min(FRAME_MAX - st.query.len());
            st.query.extend_from_slice(&data[..n]);
            let held = st.query.len() - st.ready;
            st.ready += if more {
                held / mss.max(1) * mss.max(1)
            } else {
                held
            };
            let mut queries = Vec::new();
            while let Some(len) = read_u16(&st.query, 0).map(usize::from)
                && st.ready >= 2 + len
            {
                queries.push(st.query[2..2 + len].to_vec());
                st.query.drain(..2 + len);
                st.ready -= 2 + len;
            }
            st.pending += queries.len();
            (n, queries)
        };
        for query in queries {
            let conn = Arc::downgrade(&self.0);
            serve(
                self.0.backend.as_ref(),
                &query,
                Transport::Tcp,
                move |answer| {
                    if let Some(conn) = conn.upgrade() {
                        conn.answered(answer);
                    }
                },
            );
        }
        Ok(n)
    }

    /// The server reads without `SO_OOBINLINE`, so an urgent byte the client sends never
    /// reaches it (`tcp_urg` takes it out of band); the bytes before it are a query's. When not
    /// all of those fit, the urgent byte is not taken.
    fn write_urgent(&self, data: &[u8]) -> NetResult<usize> {
        let Some((_, inband)) = data.split_last() else {
            return Ok(0);
        };
        let n = match inband.is_empty() {
            true => {
                self.push();
                0
            }
            false => self.write(inband)?,
        };
        Ok(if n == inband.len() { n + 1 } else { n })
    }

    /// The server sends no urgent data (`tcp_recv_urg` without `urg_data`).
    fn read_urgent(&self, _: bool) -> NetResult<u8> {
        Err(Errno::EINVAL)
    }

    fn at_mark(&self) -> bool {
        false
    }

    fn shutdown(&self, how: Shutdown) -> NetResult<()> {
        if how != Shutdown::Read {
            self.push();
        }
        let mut st = self.0.st.lock_or_recover();
        if how != Shutdown::Write {
            st.rd_shut = true;
            st.out.clear();
        }
        if how != Shutdown::Read {
            st.fin = true;
        }
        drop(st);
        self.0.wq.wake(PollMask::empty());
        Ok(())
    }

    fn take_error(&self) -> Option<Errno> {
        self.0.st.lock_or_recover().err.take()
    }

    fn peer_addr(&self) -> SocketAddr {
        self.0.peer
    }

    fn set_option(&self, level: i32, name: i32, value: &[u8]) -> NetResult<()> {
        if (level, name)
            == (
                wasixv2_abi::SocketLevel::SOL_SOCKET.0,
                wasixv2_abi::SocketOption::SO_LINGER.0,
            )
        {
            if value.len() < 8 {
                return Err(Errno::EINVAL);
            }
            let on = crate::opts::read_int(value)?;
            let secs = crate::opts::read_int(&value[4..])?;
            self.0.st.lock_or_recover().abort = on != 0 && secs == 0;
        }
        Ok(())
    }

    fn input_len(&self) -> usize {
        self.0.st.lock_or_recover().out.len()
    }
}

impl Drop for DnsStream {
    fn drop(&mut self) {
        let st = self.0.st.lock_or_recover();
        let abort = st.abort || !st.out.is_empty();
        drop(st);
        if !abort {
            self.push();
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A query for `name` of `qtype`, with an OPT record of `edns` payload size if given.
    pub(crate) fn query(id: u16, name: &str, qtype: u16, edns: Option<u16>) -> Vec<u8> {
        let mut msg = header(id, FLAG_RD, 1);
        let labels = labels_of(name).unwrap();
        write_name(&mut msg, &labels);
        msg.extend_from_slice(&qtype.to_be_bytes());
        msg.extend_from_slice(&CLASS_IN.to_be_bytes());
        if let Some(size) = edns {
            msg.extend_from_slice(&[0, 0, 41]);
            msg.extend_from_slice(&size.to_be_bytes());
            msg.extend_from_slice(&[0; 6]);
            msg[10..12].copy_from_slice(&1u16.to_be_bytes());
        }
        msg
    }

    fn query_of(msg: &[u8]) -> Query {
        match parse(msg) {
            Parsed::Query(q) => q,
            other => panic!("not a query: {other:?}"),
        }
    }

    #[test]
    fn names_follow_pointers_back_only() {
        // "a" then a pointer to itself: a loop.
        let msg = [0u8; 12]
            .iter()
            .copied()
            .chain([1, b'a', 0xc0, 12])
            .collect::<Vec<_>>();
        assert!(read_name(&msg, 12).is_none());
        let msg = [0u8; 12]
            .iter()
            .copied()
            .chain([1, b'a', 0, 1, b'b', 0xc0, 12])
            .collect::<Vec<_>>();
        let (labels, end) = read_name(&msg, 15).unwrap();
        assert_eq!(labels, vec![b"b".to_vec(), b"a".to_vec()]);
        assert_eq!(end, 19);
    }

    #[test]
    fn reverse_names() {
        let l = |s: &str| labels_of(s).unwrap();
        assert_eq!(
            reverse_addr(&l("4.3.2.1.in-addr.arpa")),
            Some("1.2.3.4".parse().unwrap())
        );
        assert_eq!(reverse_addr(&l("3.2.1.in-addr.arpa")), None);
        assert_eq!(reverse_addr(&l("256.3.2.1.in-addr.arpa")), None);
        let v6 = "1.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.ip6.arpa";
        assert_eq!(reverse_addr(&l(v6)), Some("::1".parse().unwrap()));
    }

    #[test]
    fn malformed_queries() {
        assert!(matches!(parse(&[0; 4]), Parsed::Ignore));
        let mut q = query(7, "example.com", TYPE_A, None);
        q[2] |= 0x80;
        assert!(matches!(parse(&q), Parsed::Ignore), "a response");
        let mut q = query(7, "example.com", TYPE_A, None);
        q[2] |= 2 << 3;
        assert!(matches!(
            parse(&q),
            Parsed::Refuse {
                id: 7,
                opcode: 2,
                rcode: NOTIMP,
                ..
            }
        ));
        let mut q = query(7, "example.com", TYPE_A, None);
        q[5] = 2;
        assert!(matches!(parse(&q), Parsed::Refuse { rcode: FORMERR, .. }));
        let q = query(7, "example.com", TYPE_A, None);
        assert!(matches!(
            parse(&q[..q.len() - 1]),
            Parsed::Refuse { rcode: FORMERR, .. }
        ));
    }

    #[test]
    fn truncation_keeps_whole_records_and_the_opt_record() {
        let msg = query(1, "example.com", TYPE_AAAA, None);
        let q = query_of(&msg);
        let addrs: Vec<IpAddr> = (0..40u16)
            .map(|i| Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, i).into())
            .collect();
        let found = Found {
            answer: Answer::Addrs {
                canonical: "example.com".into(),
                addrs: addrs.clone(),
            },
            ttl: 30,
        };
        let answer = respond(
            &q,
            Transport::Udp,
            Ok(found.clone()),
            Want::Addrs(TYPE_AAAA),
        );
        assert!(answer.len() <= UDP_LIMIT);
        let flags = read_u16(&answer, 2).unwrap();
        assert_ne!(flags & FLAG_TC, 0);
        // 12 + 17 of question, 28 per record.
        assert_eq!(counts(&answer), [1, 17, 0, 0]);
        assert_eq!(answer.len(), 12 + 17 + 17 * 28);
        let answer = respond(
            &q,
            Transport::Tcp,
            Ok(found.clone()),
            Want::Addrs(TYPE_AAAA),
        );
        assert_eq!(read_u16(&answer, 2).unwrap() & FLAG_TC, 0);
        assert_eq!(counts(&answer), [1, 40, 0, 0]);

        let msg = query(1, "example.com", TYPE_AAAA, Some(1000));
        let q = query_of(&msg);
        let answer = respond(&q, Transport::Udp, Ok(found), Want::Addrs(TYPE_AAAA));
        assert!(answer.len() <= 1000);
        assert_ne!(read_u16(&answer, 2).unwrap() & FLAG_TC, 0);
        let [_, an, _, ar] = counts(&answer);
        assert_eq!((an, ar), ((1000 - 12 - 17 - 11) as u16 / 28, 1));
        assert_eq!(&answer[answer.len() - 11..answer.len() - 8], &[0, 0, 41]);
    }

    #[test]
    fn canonical_names_answer_with_a_cname() {
        let msg = query(9, "WWW.Example.com", TYPE_A, None);
        let q = query_of(&msg);
        let found = Found {
            answer: Answer::Addrs {
                canonical: "web.example.net".into(),
                addrs: vec!["192.0.2.1".parse().unwrap(), "2001:db8::1".parse().unwrap()],
            },
            ttl: 30,
        };
        let answer = respond(&q, Transport::Udp, Ok(found.clone()), Want::Addrs(TYPE_A));
        assert_eq!(read_u16(&answer, 0), Some(9));
        assert_eq!(counts(&answer), [1, 2, 0, 0]);
        // The question keeps the case it was asked in.
        assert_eq!(&answer[12..12 + q.question.len()], &q.question[..]);
        let mut pos = 12 + q.question.len();
        let (kind, _, _, end) = read_rr(&answer, pos).unwrap();
        assert_eq!(kind, TYPE_CNAME);
        let (target, _) = read_name(&answer, pos + 12).unwrap();
        assert_eq!(target, labels_of("web.example.net").unwrap());
        pos = end;
        let (owner, at) = read_name(&answer, pos).unwrap();
        assert_eq!(owner, labels_of("web.example.net").unwrap());
        assert_eq!(read_u16(&answer, at), Some(TYPE_A));
        assert_eq!(&answer[at + 10..at + 14], &[192, 0, 2, 1]);

        // The same name, in another case, is no CNAME.
        let msg = query(9, "Web.Example.NET", TYPE_CNAME, None);
        let q = query_of(&msg);
        let answer = respond(&q, Transport::Udp, Ok(found), Want::Cname);
        assert_eq!(counts(&answer), [1, 0, 0, 0]);
        assert_eq!(answer[3] & 0xf, NOERROR);
    }

    #[test]
    fn localhost_needs_no_resolver() {
        for (qtype, rdata) in [
            (TYPE_A, &[127, 0, 0, 1][..]),
            (TYPE_AAAA, &Ipv6Addr::LOCALHOST.octets()[..]),
        ] {
            let msg = query(3, "db.localhost", qtype, None);
            let (tx, rx) = std::sync::mpsc::channel();
            serve(None, &msg, Transport::Udp, move |a| tx.send(a).unwrap());
            let answer = rx.recv().unwrap().unwrap();
            assert_eq!(answer[3] & 0xf, NOERROR);
            assert_eq!(counts(&answer), [1, 1, 0, 0]);
            assert!(answer.ends_with(rdata));
        }
        // Without a resolver, other names fail.
        let msg = query(3, "example.com", TYPE_A, None);
        let (tx, rx) = std::sync::mpsc::channel();
        serve(None, &msg, Transport::Udp, move |a| tx.send(a).unwrap());
        assert_eq!(rx.recv().unwrap().unwrap()[3] & 0xf, SERVFAIL);
    }

    #[test]
    fn edns_versions_past_0_are_badvers() {
        let mut msg = query(3, "example.com", TYPE_A, Some(4096));
        let n = msg.len();
        // The OPT record's version byte.
        msg[n - 5] = 1;
        let (tx, rx) = std::sync::mpsc::channel();
        serve(None, &msg, Transport::Udp, move |a| tx.send(a).unwrap());
        let answer = rx.recv().unwrap().unwrap();
        assert_eq!(answer[3] & 0xf, 0);
        assert_eq!(counts(&answer), [1, 0, 0, 1]);
        assert_eq!(answer[answer.len() - 6], BADVERS_EXT);
    }
}
