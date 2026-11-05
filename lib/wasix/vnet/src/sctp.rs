//! SCTP sockets (`net/sctp/socket.c`) whose associations stay inside the machine.
//!
//! - **Styles:** one-to-one (`SOCK_STREAM`, `SCTP_SOCKET_TCP`) with `listen`/`accept`, and
//!   one-to-many (`SOCK_SEQPACKET`, `SCTP_SOCKET_UDP`) whose associations are told apart by
//!   their ids and can be peeled off into sockets of their own (`SCTP_SOCKET_UDP_HIGH_BANDWIDTH`).
//! - **Associations** are in-memory pairs between two endpoints of this namespace or the one
//!   across a veth. The four-way handshake and the three-way shutdown need nothing from the
//!   applications, so each completes at once: `connect` sets up an association that is up
//!   when it returns (a non-blocking caller still hears `EINPROGRESS`, as
//!   `sctp_wait_for_connect` says), and a graceful shutdown ends both ends with their
//!   notifications. No SCTP leaves the machine: an INIT through the uplink is answered with an
//!   ABORT (`ECONNREFUSED`) and one out an interface where nothing answers times out
//!   (`ETIMEDOUT`).
//! - **Messages** keep their boundaries, stream, payload protocol id and order; an unordered
//!   message goes in the order it was sent, as nothing is lost. A short buffer reads a message
//!   in parts, `MSG_EOR` marking the last. Notifications (`MSG_NOTIFICATION`) share the
//!   receive queue as `sctp_ulpq_tail_event` puts them there, for the events the association
//!   subscribed to (`SCTP_EVENTS`, `SCTP_EVENT`).
//! - **Flow:** a message goes to the peer's receive queue at once; what is past the peer's
//!   window (half its `SO_RCVBUF`, as `sctp_association_init` sets `rwnd`) stays charged to
//!   the sender's `SO_SNDBUF`, and a send waits while that is used up (`sctp_wspace`).
//!
//! Not modelled: heartbeats and path failover (the primary path is always active), PR-SCTP
//! abandonment (nothing is lost), stream reconfiguration, AUTH and ASCONF (bound addresses
//! added later stay out of existing associations, as with `addip_enable` off). The kernel
//! supplies the timer hook driving idle association shutdown.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
    sync::{
        Arc, Mutex, MutexGuard, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

// net/sctp/sm_statefuns.c:sctp_sf_autoclose_timer_expire measures idle elapsed time.
// std::time::Instant panics on unknown-unknown; web-time uses Performance and, in
// threaded artifacts, timeOrigin to compare association timestamps across workers.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use std::time::Instant;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use web_time::Instant;

use wasixv2_abi::{
    Errno, GuestLayout, MsgFlags, SctpAdaptationEvent, SctpAssocChange, SctpAssocValue,
    SctpAssocparams, SctpCmsgType, SctpEvent, SctpEventSubscribe, SctpGetaddrs, SctpInfo,
    SctpInitmsg, SctpNxtinfo, SctpOption, SctpPaddrChange, SctpPaddrinfo, SctpPaddrparams,
    SctpPdapiEvent, SctpPrim, SctpRcvinfo, SctpRtoinfo, SctpSacState, SctpSackInfo,
    SctpSenderDryEvent, SctpShutdownEvent, SctpSinfoFlags, SctpSnType, SctpSndinfo, SctpSndrcvinfo,
    SctpSpinfoState, SctpStatus, SockaddrStorage, SocketLevel, SocketOption as So,
};
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, poll_wait};

use crate::{
    Accepted, Control, Domain, NetResult, RecvMsg, SctpAssocEntry, SctpEndpoint, SctpOps, SendMsg,
    Shutdown, Socket, SocketKind,
    addr::{canonical, canonical_ip, present, sockaddr_bytes},
    cmsg::guest_bytes,
    iface::flags::IFF_UP,
    net::{Net, Path},
    opts::{self, Changed, SockOpts},
    ports::{BindSpec, PortRange, PortRng, SockId, addrs_overlap},
};

/// `IPPROTO_SCTP`, which is also `SOL_SCTP`.
pub(crate) const IPPROTO_SCTP: i32 = 132;
const SOL_SCTP: i32 = SocketLevel::SOL_SCTP.0;

/// The association ids with a meaning of their own (`SCTP_FUTURE_ASSOC` and friends).
const SCTP_FUTURE_ASSOC: i32 = 0;
const SCTP_CURRENT_ASSOC: i32 = 1;
const SCTP_ALL_ASSOC: i32 = 2;

/// `SCTP_SN_TYPE_BASE` and `SCTP_SN_TYPE_MAX`.
const SN_TYPE_BASE: u16 = SctpSnType::SCTP_DATA_IO_EVENT.0;
const SN_TYPE_MAX: u16 = SctpSnType::SCTP_SEND_FAILED_EVENT.0;

const UNORDERED: u16 = SctpSinfoFlags::SCTP_UNORDERED.0;
const ADDR_OVER: u16 = SctpSinfoFlags::SCTP_ADDR_OVER.0;
const ABORT: u16 = SctpSinfoFlags::SCTP_ABORT.0;
const SACK_IMMEDIATELY: u16 = SctpSinfoFlags::SCTP_SACK_IMMEDIATELY.0;
const SENDALL: u16 = SctpSinfoFlags::SCTP_SENDALL.0;
const EOF: u16 = SctpSinfoFlags::SCTP_EOF.0;
const NOTIFICATION: u16 = SctpSinfoFlags::SCTP_NOTIFICATION.0;
/// `MSG_NOTIFICATION`, the `msg_flags` bit SCTP shares with `SCTP_NOTIFICATION`.
const MSG_NOTIFICATION: MsgFlags = MsgFlags(NOTIFICATION as _);
/// `SCTP_PR_SCTP_MASK`: the PR-SCTP policy bits of `sinfo_flags`.
const PR_SCTP_MASK: u16 = 0x30;
/// What a send's `SCTP_SNDRCV`/`SCTP_SNDINFO` may set (`sctp_msghdr_parse`).
const SEND_FLAGS: u16 =
    UNORDERED | ADDR_OVER | SACK_IMMEDIATELY | SENDALL | PR_SCTP_MASK | ABORT | EOF;
/// What `SCTP_DEFAULT_SEND_PARAM`/`SCTP_DEFAULT_SNDINFO` may set.
const DEFAULT_FLAGS: u16 = UNORDERED | ADDR_OVER | ABORT | EOF;

/// `sizeof(struct sctp_prinfo)` and `struct sctp_authinfo`, which a send may carry.
const PRINFO_LEN: usize = 8;
const AUTHINFO_LEN: usize = 2;

/// `SCTP_ERROR_USER_ABORT`, the cause of an ABORT the application asked for.
const ERROR_USER_ABORT: u16 = 12;

/// `sctp_max_outstreams` (`SCTP_DEFAULT_OUTSTREAMS`) and `sctp_max_instreams`.
const DEFAULT_OUTSTREAMS: u16 = 10;
const DEFAULT_INSTREAMS: u16 = 65535;
/// `net.sctp` defaults: `max_init_retransmits`, `rto_*`, `association_max_retrans`,
/// `valid_cookie_life`, `max_burst`, `hb_interval`, `sack_timeout`.
const MAX_RETRANS_INIT: u16 = 8;
const RTO_INITIAL: u32 = 3000;
const RTO_MIN: u32 = 1000;
const RTO_MAX: u32 = 60000;
const MAX_RETRANS_ASSOC: u16 = 10;
const COOKIE_LIFE: u32 = 60000;
const MAX_BURST: u32 = 4;
const HB_INTERVAL: u32 = 30000;
/// `net.sctp.max_autoclose`: `INT_MAX / HZ` with `HZ` 1000.
const MAX_AUTOCLOSE: u32 = i32::MAX as u32 / 1000;
/// `SCTP_DEFAULT_MINSEGMENT`, `SCTP_MAX_CHUNK_LEN` and the headers a DATA chunk's payload
/// sits behind (`sizeof(struct sctphdr)`, `sizeof(struct sctp_data_chunk)`).
const MIN_SEGMENT: u32 = 512;
const MAX_CHUNK_LEN: u32 = 65532;
const SCTP_HDR: u32 = 12;
const DATA_CHUNK: u32 = 16;
/// The largest message a send can take: `SO_SNDBUF` at its largest (`sk_sndbuf` caps a
/// message, `sctp_sendmsg_parse`).
const MAX_MESSAGE: usize = 2 * opts::WMEM_MAX as usize;
/// The TCP states SCTP's socket states share (`SCTP_SS_*`).
const TCP_ESTABLISHED: u8 = 1;
const TCP_CLOSE: u8 = 7;
const TCP_CLOSE_WAIT: u8 = 8;
const TCP_LISTEN: u8 = 10;

/// `sctp_sock.type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Style {
    /// One-to-one: `SOCK_STREAM`, and what `accept` returns.
    Tcp,
    /// One-to-many: `SOCK_SEQPACKET`.
    Udp,
    /// An association peeled off a one-to-many socket.
    UdpHighBandwidth,
}

impl Style {
    fn raw(self) -> u8 {
        match self {
            Style::Tcp => 0,
            Style::Udp => 1,
            Style::UdpHighBandwidth => 2,
        }
    }
}

/// `sk_state` (`SCTP_SS_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SkState {
    Closed,
    Listening,
    Established,
    /// `SCTP_SS_CLOSING`: shut down for sending, or the peer shut down.
    Closing,
}

impl SkState {
    fn raw(self) -> u8 {
        match self {
            SkState::Closed => TCP_CLOSE,
            SkState::Listening => TCP_LISTEN,
            SkState::Established => TCP_ESTABLISHED,
            SkState::Closing => TCP_CLOSE_WAIT,
        }
    }
}

/// An association's state, as far as anything waits on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AState {
    /// The INIT is out (`SCTP_STATE_COOKIE_WAIT`).
    CookieWait,
    Established,
    ShutdownPending,
    Closed,
}

impl AState {
    /// `enum sctp_state` (`SCTP_STATE_CLOSED` is 0).
    fn internal(self) -> u8 {
        match self {
            AState::Closed => 0,
            AState::CookieWait => 1,
            AState::Established => 3,
            AState::ShutdownPending => 4,
        }
    }
}

/// The send parameters a socket gives new associations (`default_stream` and friends).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Defaults {
    stream: u16,
    flags: u16,
    ppid: u32,
    context: u32,
    timetolive: u32,
}

/// The `SOL_SCTP` options of a socket (`struct sctp_sock`).
#[derive(Clone, Debug)]
struct SctpOpts {
    path: PeerParams,
    nodelay: bool,
    initmsg: SctpInitmsg,
    /// `rtoinfo`: initial, max, min, in milliseconds.
    rto: [u32; 3],
    asocmaxrxt: u16,
    cookie_life: u32,
    defaults: Defaults,
    rcv_context: u32,
    subscribe: u16,
    autoclose: u32,
    disable_fragments: bool,
    user_frag: u32,
    max_burst: u32,
    frag_interleave: bool,
    pd_point: u32,
    recvrcvinfo: bool,
    recvnxtinfo: bool,
    v4mapped: bool,
    adaptation_ind: u32,
    /// `SCTP_REUSE_PORT` (`sp->reuse`).
    reuse: bool,
}

impl Default for SctpOpts {
    /// `sctp_init_sock`.
    fn default() -> Self {
        Self {
            path: PeerParams::default(),
            nodelay: false,
            initmsg: SctpInitmsg {
                sinit_num_ostreams: DEFAULT_OUTSTREAMS,
                sinit_max_instreams: DEFAULT_INSTREAMS,
                sinit_max_attempts: MAX_RETRANS_INIT,
                sinit_max_init_timeo: RTO_MAX as u16,
            },
            rto: [RTO_INITIAL, RTO_MAX, RTO_MIN],
            asocmaxrxt: MAX_RETRANS_ASSOC,
            cookie_life: COOKIE_LIFE,
            defaults: Defaults::default(),
            rcv_context: 0,
            subscribe: 0,
            autoclose: 0,
            disable_fragments: false,
            user_frag: 0,
            max_burst: MAX_BURST,
            frag_interleave: false,
            pd_point: 0,
            recvrcvinfo: false,
            recvnxtinfo: false,
            v4mapped: true,
            adaptation_ind: 0,
            reuse: false,
        }
    }
}

/// `sctp_ulpevent_type_set`.
fn subscribe_set(subscribe: &mut u16, sn_type: u16, on: bool) {
    if !(SN_TYPE_BASE..=SN_TYPE_MAX).contains(&sn_type) {
        return;
    }
    let bit = 1 << (sn_type - SN_TYPE_BASE);
    match on {
        true => *subscribe |= bit,
        false => *subscribe &= !bit,
    }
}

/// `sctp_ulpevent_type_enabled`.
fn subscribed(subscribe: u16, sn_type: u16) -> bool {
    (SN_TYPE_BASE..=SN_TYPE_MAX).contains(&sn_type)
        && subscribe & (1 << (sn_type - SN_TYPE_BASE)) != 0
}

mod options;
#[cfg(test)]
mod tests;

// ==== Association ids ====

/// `sctp_assocs_id`: ids are machine-wide, given cyclically from `SCTP_ALL_ASSOC + 1`
/// (`sctp_assoc_set_id`).
struct AssocIds {
    next: i32,
    live: BTreeSet<i32>,
}

static ASSOC_IDS: Mutex<AssocIds> = Mutex::new(AssocIds {
    next: SCTP_ALL_ASSOC + 1,
    live: BTreeSet::new(),
});

fn alloc_assoc_id() -> i32 {
    let mut ids = ASSOC_IDS.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        let id = ids.next;
        ids.next = id.checked_add(1).unwrap_or(SCTP_ALL_ASSOC + 1);
        if ids.live.insert(id) {
            return id;
        }
    }
}

fn free_assoc_id(id: i32) {
    let mut ids = ASSOC_IDS.lock().unwrap_or_else(|e| e.into_inner());
    ids.live.remove(&id);
}

// ==== The port table ====

/// An endpoint in its namespace's SCTP port table (an owner of a `sctp_bind_bucket`), with its
/// whole bind address list.
#[derive(Clone, Debug)]
struct Bound {
    id: SockId,
    port: u16,
    family: Domain,
    /// Canonical; a wildcard is the unspecified address it was bound as.
    addrs: Vec<IpAddr>,
    v6only: bool,
    /// `sk_reuse || sp->reuse`.
    reuse: bool,
    reuseport: bool,
    uid: u32,
    /// In the endpoint hash: listening, so an INIT finds it.
    listening: bool,
    sock: Weak<SctpSocket>,
}

impl Bound {
    fn spec(&self, addr: IpAddr) -> BindSpec {
        BindSpec {
            family: self.family,
            addr,
            v6only: self.v6only,
            reuseaddr: self.reuse,
            reuseport: self.reuseport,
            uid: self.uid,
        }
    }

    /// `sctp_bind_addr_match` for an incoming association.
    fn serves(&self, dst: IpAddr) -> bool {
        let probe = BindSpec {
            family: if dst.is_ipv4() {
                Domain::Inet
            } else {
                Domain::Inet6
            },
            addr: dst,
            v6only: false,
            reuseaddr: false,
            reuseport: false,
            uid: 0,
        };
        self.addrs
            .iter()
            .any(|a| addrs_overlap(&self.spec(*a), &probe))
    }
}

/// The SCTP endpoints of a namespace by port, and its SCTP counters.
#[derive(Debug, Default)]
pub(crate) struct SctpPorts {
    bound: Vec<Bound>,
    stats: Stats,
}

/// The `/proc/net/sctp/snmp` counters that move without packets.
#[derive(Clone, Copy, Debug, Default)]
struct Stats {
    active_estabs: u64,
    passive_estabs: u64,
    aborteds: u64,
    shutdowns: u64,
    out_of_blues: u64,
    out_order: u64,
    out_unorder: u64,
    in_order: u64,
    in_unorder: u64,
    autoclose_expireds: u64,
}

impl SctpPorts {
    fn entry_mut(&mut self, id: SockId) -> Option<&mut Bound> {
        self.bound.iter_mut().find(|b| b.id == id)
    }

    /// `sctp_get_port_local` for one more address of `new` on `port`: another endpoint on the
    /// port conflicts when one of its addresses overlaps, unless both reuse the address and it
    /// does not listen, or both reuse the port under one owner.
    fn conflicts(&self, new: &Bound, port: u16, addr: IpAddr) -> bool {
        let spec = new.spec(addr);
        self.bound.iter().any(|old| {
            if old.id == new.id || old.port != port {
                return false;
            }
            if (new.reuse && old.reuse && !old.listening)
                || (new.reuseport && old.reuseport && new.uid == old.uid)
            {
                return false;
            }
            old.addrs
                .iter()
                .any(|a| addrs_overlap(&spec, &old.spec(*a)))
        })
    }

    /// An ephemeral port no endpoint uses, from a random point of the range on.
    fn free_port(&self, range: PortRange, rng: &mut PortRng) -> Option<u16> {
        let span = u32::from(range.high) - u32::from(range.low) + 1;
        let start = rng.below(span);
        (0..span)
            .map(|i| (u32::from(range.low) + (start + i + 1) % span) as u16)
            .find(|p| !self.bound.iter().any(|b| b.port == *p))
    }

    /// The listening endpoint an INIT to `dst` reaches, an exact address before a wildcard.
    fn listener(&self, dst: SocketAddr) -> Option<Arc<SctpSocket>> {
        let ip = canonical_ip(dst.ip());
        let candidates = || {
            self.bound
                .iter()
                .filter(|b| b.listening && b.port == dst.port() && b.serves(ip))
        };
        candidates()
            .find(|b| b.addrs.contains(&ip))
            .or_else(|| candidates().next())
            .and_then(|b| b.sock.upgrade())
    }

    fn remove(&mut self, id: SockId) {
        self.bound.retain(|b| b.id != id);
    }
}

// ==== Associations ====

/// One end of an association (`struct sctp_association`).
pub(crate) struct Assoc {
    id: i32,
    /// Held across a send, so messages reach the peer in the order their SSNs were given.
    tx: Mutex<()>,
    inner: Mutex<AssocInner>,
}

#[derive(Clone, Copy, Debug)]
struct PeerParams {
    hb: u32,
    retrans: u16,
    mtu: u32,
    sack_delay: u32,
    sack_freq: u32,
    flags: u32,
    flowlabel: u32,
    dscp: u8,
}

impl Default for PeerParams {
    fn default() -> Self {
        Self {
            hb: HB_INTERVAL,
            retrans: 5,
            mtu: 0,
            sack_delay: 200,
            sack_freq: 2,
            flags: 1 | 8 | 32,
            flowlabel: 0,
            dscp: 0,
        }
    }
}

struct AssocInner {
    adaptation: u32,
    path: PeerParams,
    paths: BTreeMap<SocketAddr, PeerParams>,
    /// The socket the association belongs to; accept and peeloff move it.
    ep: Weak<SctpSocket>,
    peer: Weak<Assoc>,
    state: AState,
    lport: u16,
    /// The association's local addresses (`base.bind_addr`) and the primary path's source.
    laddrs: Vec<IpAddr>,
    saddr: IpAddr,
    /// The peer's addresses (`transport_addr_list`), canonical, and the primary path.
    paddrs: Vec<SocketAddr>,
    primary: SocketAddr,
    outcnt: u16,
    incnt: u16,
    /// The next stream sequence number of each outbound stream.
    ssn: Vec<u16>,
    next_tsn: u32,
    /// The last TSN received, which is also the cumulative one: nothing is lost.
    cum_tsn: u32,
    defaults: Defaults,
    rcv_context: u32,
    subscribe: u16,
    rto: [u32; 3],
    max_retrans: u16,
    cookie_life: u32,
    user_frag: u32,
    max_burst: u32,
    /// The path MTU and the IP header in front of each packet.
    pmtu: u32,
    net_hdr: u32,
    /// The owner's `SO_RCVBUF`, which sets the window (`rwnd` is half of it).
    rcvbuf: usize,
    /// `rmem_alloc`: bytes of this association's messages in the owner's receive queue.
    rmem: usize,
    /// When the association last sent or received data, for `SCTP_AUTOCLOSE`.
    last_data: Instant,
}

impl AssocInner {
    /// `sctp_assoc_update_frag_point`: the path MTU's payload, or less with `SCTP_MAXSEG`.
    fn frag_point(&self) -> u32 {
        let frag = (self.pmtu - self.net_hdr - SCTP_HDR - DATA_CHUNK) & !3;
        match self.user_frag {
            0 => frag,
            user => frag.min(user),
        }
    }

    /// What the window does not take of the queued bytes (`sctp_wspace`'s share).
    fn unacked(&self) -> usize {
        self.rmem.saturating_sub(self.rcvbuf / 2)
    }

    /// `a_rwnd`: what the window still takes.
    fn rwnd(&self) -> usize {
        (self.rcvbuf / 2).saturating_sub(self.rmem)
    }
}

impl Assoc {
    fn lock(&self) -> MutexGuard<'_, AssocInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn owner(&self) -> Option<Arc<SctpSocket>> {
        self.lock().ep.upgrade()
    }

    fn peer(&self) -> Option<Arc<Assoc>> {
        self.lock().peer.upgrade()
    }

    fn established(&self) -> bool {
        self.lock().state == AState::Established
    }
}

impl Drop for Assoc {
    fn drop(&mut self) {
        free_assoc_id(self.id);
    }
}

/// What a socket's receive queue holds: a message or a notification of an association.
struct Event {
    asoc: Arc<Assoc>,
    body: Body,
    data: Vec<u8>,
    /// How much of `data` a partial read already took.
    read: usize,
    /// The message exceeds the receive window: its remaining fragments are still
    /// charged to the sender until the receiver reopens its window.
    partial_delivery: bool,
}

enum Body {
    Data {
        stream: u16,
        ssn: u16,
        ppid: u32,
        flags: u16,
        tsn: u32,
    },
    Notification,
}

impl Event {
    fn is_notification(&self) -> bool {
        matches!(self.body, Body::Notification)
    }

    fn left(&self) -> usize {
        self.data.len() - self.read
    }
}

/// `sctp_ulpq_tail_event` into the receive queue of whichever socket owns `asoc` now: a
/// notification only if the association subscribed to it, nothing data once the socket shut
/// down for reading (and nothing at all once for both). Returns whether it was queued.
fn enqueue(asoc: &Arc<Assoc>, body: Body, data: Vec<u8>) -> bool {
    loop {
        let Some(ep) = asoc.owner() else {
            return false;
        };
        let mut g = ep.lock();
        // Accept and peeloff move the association under the old owner's lock.
        if !Weak::ptr_eq(&asoc.lock().ep, &ep.me) {
            continue;
        }
        let notification = matches!(body, Body::Notification);
        if g.rd_shut && (g.wr_shut || !notification) {
            return false;
        }
        if notification {
            let sn_type = u16::from_le_bytes([data[0], data[1]]);
            if !subscribed(asoc.lock().subscribe, sn_type) {
                return false;
            }
        } else {
            asoc.lock().rmem += data.len();
        }
        let partial_delivery = !notification && data.len() > g.opts.rcvbuf / 2;
        g.rcvq.push_back(Event {
            partial_delivery,
            asoc: asoc.clone(),
            body,
            data,
            read: 0,
        });
        drop(g);
        ep.wq.wake(PollMask::IN | PollMask::RDNORM);
        return true;
    }
}

/// `sctp_ulpevent_make_assoc_change`, with `info` as `sac_info` (an ABORT's causes).
fn assoc_change(asoc: &Assoc, state: SctpSacState, error: u16, info: &[u8]) -> Vec<u8> {
    let (outbound, inbound) = match state {
        SctpSacState::SCTP_COMM_UP | SctpSacState::SCTP_RESTART => {
            let a = asoc.lock();
            (a.outcnt, a.incnt)
        }
        _ => (0, 0),
    };
    let mut out = guest_bytes(&SctpAssocChange {
        sac_type: SctpSnType::SCTP_ASSOC_CHANGE.0,
        sac_flags: 0,
        sac_length: (SctpAssocChange::SIZE + info.len()) as u32,
        sac_state: state.0,
        sac_error: error,
        sac_outbound_streams: outbound,
        sac_inbound_streams: inbound,
        sac_assoc_id: asoc.id,
    });
    out.extend_from_slice(info);
    out
}

fn notify(asoc: &Arc<Assoc>, event: Vec<u8>) {
    enqueue(asoc, Body::Notification, event);
}

/// `sctp_ulpevent_make_sender_dry_event`.
fn sender_dry(asoc: &Assoc) -> Vec<u8> {
    guest_bytes(&SctpSenderDryEvent {
        sender_dry_type: SctpSnType::SCTP_SENDER_DRY_EVENT.0,
        sender_dry_flags: 0,
        sender_dry_length: SctpSenderDryEvent::SIZE as u32,
        sender_dry_assoc_id: asoc.id,
    })
}

/// How an association ends.
enum End {
    /// SHUTDOWN, SHUTDOWN ACK, SHUTDOWN COMPLETE.
    Shutdown,
    /// An ABORT with the user abort cause and its reason, from this end.
    Abort(Vec<u8>),
}

/// Ends the association `a` and its peer. Takes no lock of either socket on entry.
fn end_assoc(a: &Arc<Assoc>, how: End) {
    let (b, a_net) = {
        let mut ai = a.lock();
        if ai.state == AState::Closed {
            return;
        }
        ai.state = AState::Closed;
        (ai.peer.upgrade(), ai.ep.upgrade().map(|ep| ep.net.clone()))
    };
    let b = b.filter(|b| {
        let mut bi = b.lock();
        let open = bi.state != AState::Closed;
        bi.state = AState::Closed;
        open
    });
    let b_ep = b.as_ref().and_then(|b| b.owner());
    match how {
        End::Shutdown => {
            if let (Some(b), Some(b_ep)) = (&b, &b_ep) {
                // `sctp_sf_do_9_2_shutdown`: SHUTDOWN_RECEIVED, then the ACK and COMPLETE.
                let event = guest_bytes(&SctpShutdownEvent {
                    sse_type: SctpSnType::SCTP_SHUTDOWN_EVENT.0,
                    sse_flags: 0,
                    sse_length: SctpShutdownEvent::SIZE as u32,
                    sse_assoc_id: b.id,
                });
                notify(b, event);
                b_ep.peer_shutdown();
                notify(b, assoc_change(b, SctpSacState::SCTP_SHUTDOWN_COMP, 0, &[]));
                b_ep.net.state().sctp.stats.shutdowns += 1;
            }
            notify(a, assoc_change(a, SctpSacState::SCTP_SHUTDOWN_COMP, 0, &[]));
        }
        End::Abort(reason) => {
            // The ABORT chunk's cause (`sctp_make_abort_user`), which `sac_info` quotes.
            let mut cause = Vec::with_capacity(4 + reason.len());
            cause.extend_from_slice(&ERROR_USER_ABORT.to_be_bytes());
            cause.extend_from_slice(&((4 + reason.len()) as u16).to_be_bytes());
            cause.extend_from_slice(&reason);
            if let (Some(b), Some(b_ep)) = (&b, &b_ep) {
                // net/sctp/ulpqueue.c: sctp_ulpq_abort_pd.
                b_ep.abort_partial(b);
                // `sctp_sf_do_9_1_abort`.
                let event = assoc_change(b, SctpSacState::SCTP_COMM_LOST, ERROR_USER_ABORT, &cause);
                notify(b, event);
                b_ep.set_err(Errno::ECONNRESET);
                b_ep.net.state().sctp.stats.aborteds += 1;
            }
            // `sctp_sf_do_9_1_prm_abort`.
            notify(
                a,
                assoc_change(a, SctpSacState::SCTP_COMM_LOST, ERROR_USER_ABORT, &[]),
            );
            if let Some(net) = a_net {
                net.state().sctp.stats.aborteds += 1;
            }
        }
    }
    detach(a);
    if let Some(b) = &b {
        detach(b);
    }
}

/// `sctp_cmd_delete_tcb`: the association leaves its socket, unless it waits in a one-to-one
/// listener's accept queue, where `accept` still finds it.
fn detach(asoc: &Arc<Assoc>) {
    let Some(ep) = asoc.owner() else {
        return;
    };
    let mut g = ep.lock();
    let keep = g.style == Style::Tcp && g.state == SkState::Listening && !(g.rd_shut && g.wr_shut);
    if !keep {
        g.asocs.retain(|x| !Arc::ptr_eq(x, asoc));
    }
    drop(g);
    ep.wq.wake(PollMask::empty());
    // The peer may wait for room this association's queue took.
    if let Some(peer_ep) = asoc.peer().and_then(|p| p.owner()) {
        peer_ep.wq.wake(PollMask::OUT | PollMask::WRNORM);
    }
}

// ==== Sockets ====

type IdleTimer = Box<dyn Fn(Option<Duration>) + Send + Sync>;

/// An SCTP socket of a [`Net`] (`struct sctp_sock` and its endpoint).
pub struct SctpSocket {
    idle_timer: Mutex<Option<IdleTimer>>,
    net: Arc<Net>,
    id: SockId,
    domain: Domain,
    kind: SocketKind,
    /// `sk_uid`.
    uid: u32,
    ino: AtomicU64,
    wq: WaitQueue,
    me: Weak<SctpSocket>,
    inner: Mutex<Inner>,
}

struct Inner {
    style: Style,
    state: SkState,
    opts: SockOpts,
    so: SctpOpts,
    /// The bind address list (canonical) and its port, 0 while unbound.
    port: u16,
    bound: Vec<IpAddr>,
    /// `inet_rcv_saddr`: the address the last bind gave, for `getsockname`.
    saddr: Option<IpAddr>,
    /// `inet_daddr`/`inet_dport`, which `connect` and `accept` set.
    daddr: Option<SocketAddr>,
    /// `ep->asocs`; a one-to-one listener's accept queue.
    asocs: Vec<Arc<Assoc>>,
    backlog: u32,
    rcvq: VecDeque<Event>,
    /// `sk_err`.
    err: Option<Errno>,
    /// `sk_shutdown`.
    rd_shut: bool,
    wr_shut: bool,
}

/// What a send of one message carries (`struct sctp_sndrcvinfo` once filled in).
#[derive(Clone, Copy, Debug, Default)]
struct SendInfo {
    stream: u16,
    flags: u16,
    ppid: u32,
    context: u32,
    assoc_id: i32,
}

/// The SCTP control messages of one send (`struct sctp_cmsgs`).
#[derive(Default)]
struct Cmsgs {
    init: Option<SctpInitmsg>,
    srinfo: Option<SctpSndrcvinfo>,
    sinfo: Option<SctpSndinfo>,
    prinfo: bool,
}

/// `sctp_msghdr_parse`: the `SOL_SCTP` control messages, each of its exact size; others are
/// ignored.
fn parse_cmsgs(controls: &[Control]) -> NetResult<Cmsgs> {
    let mut c = Cmsgs::default();
    for control in controls.iter().filter(|c| c.level == SOL_SCTP) {
        let exact = |size: usize| match control.data.len() == size {
            true => Ok(()),
            false => Err(Errno::EINVAL),
        };
        match SctpCmsgType(control.kind) {
            SctpCmsgType::SCTP_INIT => {
                exact(SctpInitmsg::SIZE)?;
                c.init = SctpInitmsg::read_from(&control.data);
            }
            SctpCmsgType::SCTP_SNDRCV => {
                exact(SctpSndrcvinfo::SIZE)?;
                let info = SctpSndrcvinfo::read_from(&control.data).expect("of its size");
                if info.sinfo_flags & !SEND_FLAGS != 0 {
                    return Err(Errno::EINVAL);
                }
                c.srinfo = Some(info);
            }
            SctpCmsgType::SCTP_SNDINFO => {
                exact(SctpSndinfo::SIZE)?;
                let info = SctpSndinfo::read_from(&control.data).expect("of its size");
                if info.snd_flags & !SEND_FLAGS != 0 {
                    return Err(Errno::EINVAL);
                }
                c.sinfo = Some(info);
            }
            SctpCmsgType::SCTP_PRINFO => {
                exact(PRINFO_LEN)?;
                let policy = u16::from_le_bytes([control.data[0], control.data[1]]);
                if policy & !PR_SCTP_MASK != 0 {
                    return Err(Errno::EINVAL);
                }
                c.prinfo = true;
            }
            SctpCmsgType::SCTP_AUTHINFO => exact(AUTHINFO_LEN)?,
            // Further addresses of a new association's peer.
            SctpCmsgType::SCTP_DSTADDRV4 | SctpCmsgType::SCTP_DSTADDRV6 => {}
            _ => return Err(Errno::EINVAL),
        }
    }
    Ok(c)
}

/// A `struct sockaddr_in`/`sockaddr_in6` in guest layout (`sctp_get_af_specific` and the
/// family's length).
pub(crate) fn parse_sockaddr(raw: &[u8]) -> Option<SocketAddr> {
    let family = u16::from_le_bytes([*raw.first()?, *raw.get(1)?]);
    let port = u16::from_be_bytes([*raw.get(2)?, *raw.get(3)?]);
    match i32::from(family) {
        f if f == Domain::Inet.raw() && raw.len() >= 16 => {
            let ip = Ipv4Addr::new(raw[4], raw[5], raw[6], raw[7]);
            Some(SocketAddr::V4(SocketAddrV4::new(ip, port)))
        }
        f if f == Domain::Inet6.raw() && raw.len() >= 28 => {
            let flow = u32::from_be_bytes(raw[4..8].try_into().unwrap());
            let ip: [u8; 16] = raw[8..24].try_into().unwrap();
            let scope = u32::from_le_bytes(raw[24..28].try_into().unwrap());
            Some(SocketAddr::V6(SocketAddrV6::new(
                ip.into(),
                port,
                flow,
                scope,
            )))
        }
        _ => None,
    }
}

/// `IS_IPV4_UNUSABLE_ADDRESS` and `sctp_v6_addr_valid`: an address no association can use.
fn unusable(ip: IpAddr) -> bool {
    match canonical_ip(ip) {
        IpAddr::V4(v4) => v4.is_broadcast() || v4.is_multicast() || v4.octets()[0] == 0,
        IpAddr::V6(v6) => v6.is_unspecified() || v6.is_multicast(),
    }
}

/// `sctp_in_scope`: which local addresses an association with `peer` lists.
fn in_scope(addr: IpAddr, peer: IpAddr) -> bool {
    if peer.is_loopback() {
        return true;
    }
    if addr.is_loopback() {
        return false;
    }
    match (addr, peer) {
        (IpAddr::V6(a), IpAddr::V6(p)) => !a.is_unicast_link_local() || p.is_unicast_link_local(),
        _ => true,
    }
}

/// The addresses of the namespace's up interfaces (`net->sctp.local_addr_list`).
fn local_addrs(net: &Net) -> Vec<IpAddr> {
    let st = net.state();
    st.ifaces
        .links()
        .iter()
        .filter(|l| l.flags & IFF_UP != 0)
        .flat_map(|l| l.addrs.iter().map(|a| a.addr))
        .collect()
}

impl SctpSocket {
    pub(crate) fn new(net: Arc<Net>, domain: Domain, kind: SocketKind, uid: u32) -> Arc<Self> {
        let style = match kind {
            SocketKind::Stream => Style::Tcp,
            _ => Style::Udp,
        };
        let mut opts = SockOpts::new(kind, domain);
        // `sock_init_data`: SCTP keeps `wmem_default` and `rmem_default`.
        opts.sndbuf = opts::UDP_BUF_DEFAULT;
        opts.rcvbuf = opts::UDP_BUF_DEFAULT;
        Self::build(
            net,
            domain,
            kind,
            uid,
            Inner {
                style,
                state: SkState::Closed,
                opts,
                so: SctpOpts::default(),
                port: 0,
                bound: Vec::new(),
                saddr: None,
                daddr: None,
                asocs: Vec::new(),
                backlog: 0,
                rcvq: VecDeque::new(),
                err: None,
                rd_shut: false,
                wr_shut: false,
            },
        )
    }

    fn build(net: Arc<Net>, domain: Domain, kind: SocketKind, uid: u32, inner: Inner) -> Arc<Self> {
        let id = net.next_id();
        Arc::new_cyclic(|me| Self {
            idle_timer: Mutex::new(None),
            net,
            id,
            domain,
            kind,
            uid,
            ino: AtomicU64::new(0),
            wq: WaitQueue::new(),
            me: me.clone(),
            inner: Mutex::new(inner),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn set_err(&self, err: Errno) {
        let mut g = self.lock();
        // `sctp_cmd_set_sk_err`: one-to-many sockets learn of failures by notification.
        if g.style != Style::Udp {
            g.err = Some(err);
        }
        drop(g);
        self.wq.wake(PollMask::empty());
    }

    /// `sctp_cmd_new_state` to `SHUTDOWN_RECEIVED`: a one-to-one socket can read no more.
    fn peer_shutdown(&self) {
        let mut g = self.lock();
        if g.style == Style::Tcp && g.state == SkState::Established {
            g.state = SkState::Closing;
            g.rd_shut = true;
        }
        drop(g);
        self.wq.wake(PollMask::empty());
    }

    /// The table entry this socket's binds go in, for conflict checks.
    fn bound_entry(&self, g: &Inner, addrs: Vec<IpAddr>, port: u16) -> Bound {
        Bound {
            id: self.id,
            port,
            family: self.domain,
            addrs,
            v6only: g.opts.v6only,
            reuse: g.opts.reuseaddr || g.so.reuse,
            reuseport: g.opts.reuseport,
            uid: self.uid,
            listening: g.state == SkState::Listening,
            sock: self.me.clone(),
        }
    }

    /// Puts the socket's bind list in the table.
    fn rehash(&self, g: &Inner) {
        let entry = self.bound_entry(g, g.bound.clone(), g.port);
        let mut st = self.net.state();
        match st.sctp.entry_mut(self.id) {
            Some(e) => *e = entry,
            None => st.sctp.bound.push(entry),
        }
    }

    /// `sctp_sockaddr_af` and `af_supported`: an IPv4 socket takes IPv4 addresses, an IPv6
    /// one both unless `IPV6_V6ONLY`. Returns the canonical address.
    fn family_ok(&self, g: &Inner, addr: SocketAddr) -> NetResult<SocketAddr> {
        let addr = canonical(addr);
        match (self.domain, addr) {
            (Domain::Inet, SocketAddr::V6(_)) => Err(Errno::EINVAL),
            (Domain::Inet6, SocketAddr::V4(_)) if g.opts.v6only => Err(Errno::EINVAL),
            _ => Ok(addr),
        }
    }

    /// `sctp_do_bind` of one address.
    fn do_bind(&self, g: &mut Inner, addr: SocketAddr, privileged: bool) -> NetResult<()> {
        let addr = self.family_ok(g, addr)?;
        let ip = addr.ip();
        let freebind = g.opts.flag(0, wasixv2_abi::IpOption::IP_FREEBIND.0);
        if !ip.is_unspecified() && !freebind && !self.net.state().ifaces.is_local(ip) {
            return Err(Errno::EADDRNOTAVAIL);
        }
        let mut port = addr.port();
        if g.port != 0 {
            if port == 0 {
                port = g.port;
            } else if port != g.port {
                return Err(Errno::EINVAL);
            }
        }
        if port != 0 && port < self.net.config.unprivileged_port_start && !privileged {
            return Err(Errno::EACCES);
        }
        if g.bound.contains(&ip) {
            return Err(Errno::EINVAL);
        }
        let mut st = self.net.state();
        let st = &mut *st;
        let mut addrs = g.bound.clone();
        addrs.push(ip);
        let port = match port {
            0 => st
                .sctp
                .free_port(self.net.config.port_range, &mut st.rng)
                .ok_or(Errno::EADDRINUSE)?,
            port => port,
        };
        let entry = self.bound_entry(g, addrs, port);
        if st.sctp.conflicts(&entry, port, ip) {
            return Err(Errno::EADDRINUSE);
        }
        match st.sctp.entry_mut(self.id) {
            Some(e) => *e = entry,
            None => st.sctp.bound.push(entry),
        }
        g.port = port;
        g.bound.push(ip);
        g.saddr = Some(ip);
        Ok(())
    }

    /// `sctp_autobind`: the wildcard of the socket's family, an ephemeral port.
    fn autobind(&self, g: &mut Inner) -> NetResult<()> {
        if g.port != 0 {
            return Ok(());
        }
        let any = match self.domain {
            Domain::Inet => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            Domain::Inet6 => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
        };
        // `sctp_autobind` fails with `EAGAIN` when no port is free.
        self.do_bind(g, SocketAddr::new(any, 0), true)
            .map_err(|_| Errno::EAGAIN)
    }

    /// `sctp_id2assoc`.
    fn id2assoc(&self, g: &Inner, id: i32) -> Option<Arc<Assoc>> {
        if g.style != Style::Udp {
            if g.state != SkState::Established && g.state != SkState::Closing {
                return None;
            }
            return g.asocs.first().cloned();
        }
        if id <= SCTP_ALL_ASSOC {
            return None;
        }
        g.asocs.iter().find(|a| a.id == id).cloned()
    }

    /// `sctp_id2assoc` for an option: `EINVAL` for an id a one-to-many socket does not have,
    /// and `None` for the socket itself.
    fn option_assoc(&self, g: &Inner, id: i32, allow: i32) -> NetResult<Option<Arc<Assoc>>> {
        let asoc = self.id2assoc(g, id);
        if asoc.is_none() && id > allow && g.style == Style::Udp {
            return Err(Errno::EINVAL);
        }
        Ok(asoc)
    }

    /// `sctp_endpoint_lookup_assoc`: the association with `peer` among its addresses.
    fn lookup_assoc(&self, g: &Inner, peer: SocketAddr) -> Option<Arc<Assoc>> {
        let peer = canonical(peer);
        g.asocs
            .iter()
            .find(|a| a.lock().paddrs.contains(&peer))
            .cloned()
    }

    /// How an address is shown to the application (`addr_to_user`): an IPv6 socket maps
    /// IPv4 addresses with `SCTP_I_WANT_MAPPED_V4_ADDR` (the default), else shows them as
    /// IPv4.
    fn user_addr(&self, g: &Inner, addr: SocketAddr) -> SocketAddr {
        match g.so.v4mapped {
            true => present(self.domain, addr),
            false => canonical(addr),
        }
    }

    fn user_family(&self, g: &Inner, addr: SocketAddr) -> Domain {
        match self.user_addr(g, addr) {
            SocketAddr::V4(_) => Domain::Inet,
            SocketAddr::V6(_) => Domain::Inet6,
        }
    }

    /// `sctp_wspace` over the socket: `SO_SNDBUF` less what the peers' windows do not take.
    fn wspace(&self, g: &Inner) -> isize {
        let used: usize = g
            .asocs
            .iter()
            .filter_map(|a| a.peer())
            .map(|b| b.lock().unacked())
            .sum();
        g.opts.sndbuf as isize - used as isize
    }

    /// The local addresses an association with `peer` lists: the bound ones, or for a
    /// wildcard those of the namespace in the socket's families and the peer's scope.
    fn asoc_laddrs(&self, g: &Inner, peer: IpAddr) -> Vec<IpAddr> {
        let any = g.bound.iter().any(|a| a.is_unspecified());
        let addrs = match any {
            true => local_addrs(&self.net),
            false => g.bound.clone(),
        };
        addrs
            .into_iter()
            .filter(|a| a.is_ipv4() == peer.is_ipv4())
            .filter(|a| in_scope(*a, peer))
            .collect()
    }

    fn new_assoc(
        &self,
        g: &Inner,
        state: AState,
        laddrs: Vec<IpAddr>,
        saddr: IpAddr,
    ) -> AssocInner {
        let pmtu = if g.so.path.flags & 16 != 0 && g.so.path.mtu != 0 {
            g.so.path.mtu
        } else {
            self.net.mtu_to(saddr).unwrap_or(1500)
        };
        AssocInner {
            adaptation: g.so.adaptation_ind,
            path: PeerParams {
                mtu: pmtu,
                ..g.so.path
            },
            paths: BTreeMap::new(),
            ep: self.me.clone(),
            peer: Weak::new(),
            state,
            lport: g.port,
            laddrs,
            saddr,
            paddrs: Vec::new(),
            primary: SocketAddr::new(saddr, 0),
            outcnt: g.so.initmsg.sinit_num_ostreams,
            incnt: g.so.initmsg.sinit_max_instreams,
            ssn: Vec::new(),
            next_tsn: 0,
            cum_tsn: 0,
            defaults: g.so.defaults,
            rcv_context: g.so.rcv_context,
            subscribe: g.so.subscribe,
            rto: g.so.rto,
            max_retrans: g.so.asocmaxrxt,
            cookie_life: g.so.cookie_life,
            user_frag: g.so.user_frag,
            max_burst: g.so.max_burst,
            pmtu,
            net_hdr: match self.domain {
                Domain::Inet => 20,
                Domain::Inet6 => 40,
            },
            rcvbuf: g.opts.rcvbuf,
            rmem: 0,
            last_data: Instant::now(),
        }
    }

    /// `sctp_verify_addr` of a destination.
    fn verify_dst(&self, g: &Inner, addr: SocketAddr) -> NetResult<SocketAddr> {
        let addr = self.family_ok(g, addr)?;
        if unusable(addr.ip()) {
            return Err(Errno::EINVAL);
        }
        Ok(addr)
    }

    /// Sets up an association with the endpoint at `dsts` (the first is the primary path) and
    /// returns it, up: `sctp_connect_new_asoc`, `sctp_primitive_ASSOCIATE` and what the peer's
    /// `sctp_sf_do_5_1B_init` and `sctp_sf_do_5_1D_ce` do. The caller checked the socket's
    /// state and that no association with `dsts[0]` exists. `init` is an `SCTP_INIT` control
    /// message's.
    fn associate(&self, dsts: &[SocketAddr], init: Option<SctpInitmsg>) -> NetResult<Arc<Assoc>> {
        let dst = dsts[0];
        // INIT out: the association waits in COOKIE_WAIT on this socket.
        let (a, net, src) = {
            let mut g = self.lock();
            self.autobind(&mut g)?;
            let bound = match g.bound.as_slice() {
                [one] if !one.is_unspecified() => Some(*one),
                _ => None,
            };
            let path = self.net.path(dst, bound)?;
            let src = match g.bound.iter().any(|a| a.is_unspecified()) {
                true => path.src(),
                false if g.bound.contains(&path.src()) => path.src(),
                false => g.bound[0],
            };
            let net = match path {
                Path::Local { net, .. } => net.unwrap_or_else(|| self.net.clone()),
                // An endpoint beyond answers the INIT with an ABORT: nothing there speaks
                // SCTP to the machine.
                Path::Backend { .. } | Path::Dns { .. } => {
                    self.net.state().sctp.stats.out_of_blues += 1;
                    return Err(Errno::ECONNREFUSED);
                }
                // `sctp_wait_for_connect` once the INIT retransmissions run out.
                Path::Nowhere { .. } => return Err(Errno::ETIMEDOUT),
            };
            let laddrs = self.asoc_laddrs(&g, dst.ip());
            let mut inner = self.new_assoc(&g, AState::CookieWait, laddrs, src);
            if let Some(init) = init {
                // `sctp_connect_new_asoc` with an `SCTP_INIT`: what it sets wins.
                if init.sinit_num_ostreams != 0 {
                    inner.outcnt = init.sinit_num_ostreams;
                }
                if init.sinit_max_instreams != 0 {
                    inner.incnt = init.sinit_max_instreams;
                }
            }
            inner.paddrs = dsts.iter().map(|d| canonical(*d)).collect();
            inner.primary = canonical(dst);
            let a = Arc::new(Assoc {
                id: alloc_assoc_id(),
                tx: Mutex::new(()),
                inner: Mutex::new(inner),
            });
            g.asocs.push(a.clone());
            (a, net, src)
        };
        match self.answer_init(&a, &net, dst, src) {
            Ok(b) => {
                self.established(&a, &b);
                Ok(a)
            }
            Err(err) => {
                // `sctp_cmd_init_failed`.
                notify(
                    &a,
                    assoc_change(&a, SctpSacState::SCTP_CANT_STR_ASSOC, 0, &[]),
                );
                a.lock().state = AState::Closed;
                self.lock().asocs.retain(|x| !Arc::ptr_eq(x, &a));
                self.wq.wake(PollMask::empty());
                Err(err)
            }
        }
    }

    /// The peer's side of the handshake: the listening endpoint at `dst` in `net` makes its
    /// end of the association, or refuses.
    fn answer_init(
        &self,
        a: &Arc<Assoc>,
        net: &Arc<Net>,
        dst: SocketAddr,
        src: IpAddr,
    ) -> NetResult<Arc<Assoc>> {
        let Some(listener) = net.state().sctp.listener(dst) else {
            // An out-of-the-blue INIT is answered with an ABORT.
            net.state().sctp.stats.out_of_blues += 1;
            return Err(Errno::ECONNREFUSED);
        };
        let (a_laddrs, a_out, a_in, a_port) = {
            let ai = a.lock();
            (ai.laddrs.clone(), ai.outcnt, ai.incnt, ai.lport)
        };
        let b = {
            let mut l = listener.lock();
            // `sctp_sf_do_5_1B_init`: only a listening endpoint takes an association, and a
            // one-to-one one only while its accept queue has room.
            let full = l.style == Style::Tcp && l.asocs.len() > l.backlog as usize;
            if l.state != SkState::Listening || full {
                return Err(Errno::ECONNREFUSED);
            }
            let mut laddrs = listener.asoc_laddrs(&l, src);
            let dst_ip = canonical_ip(dst.ip());
            if !laddrs.contains(&dst_ip) {
                laddrs.insert(0, dst_ip);
            }
            let mut inner = listener.new_assoc(&l, AState::Established, laddrs, dst_ip);
            let src_port = SocketAddr::new(src, a_port);
            let mut paddrs: Vec<SocketAddr> = a_laddrs
                .iter()
                .map(|ip| SocketAddr::new(*ip, a_port))
                .collect();
            if !paddrs.contains(&src_port) {
                paddrs.insert(0, src_port);
            }
            inner.paddrs = paddrs;
            inner.primary = src_port;
            // The streams each side may use: the fewer of what one offers and the other takes.
            inner.outcnt = inner.outcnt.min(a_in);
            inner.incnt = inner.incnt.min(a_out);
            inner.ssn = vec![0; usize::from(inner.outcnt)];
            inner.peer = Arc::downgrade(a);
            let b = Arc::new(Assoc {
                id: alloc_assoc_id(),
                tx: Mutex::new(()),
                inner: Mutex::new(inner),
            });
            l.asocs.push(b.clone());
            b
        };
        net.state().sctp.stats.passive_estabs += 1;
        listener.wq.wake(PollMask::IN | PollMask::RDNORM);
        notify(&b, assoc_change(&b, SctpSacState::SCTP_COMM_UP, 0, &[]));
        listener.rearm_idle();
        Ok(b)
    }

    /// COOKIE ACK in: the association `a` with its peer end `b` is up.
    fn established(&self, a: &Arc<Assoc>, b: &Arc<Assoc>) {
        let (b_laddrs, b_lport, b_out, b_in, b_saddr) = {
            let bi = b.lock();
            (bi.laddrs.clone(), bi.lport, bi.outcnt, bi.incnt, bi.saddr)
        };
        {
            let mut g = self.lock();
            let mut ai = a.lock();
            if ai.state != AState::CookieWait {
                return;
            }
            ai.state = AState::Established;
            ai.peer = Arc::downgrade(b);
            let primary = SocketAddr::new(b_saddr, b_lport);
            let mut paddrs: Vec<SocketAddr> = b_laddrs
                .iter()
                .map(|ip| SocketAddr::new(*ip, b_lport))
                .collect();
            if !paddrs.contains(&primary) {
                paddrs.insert(0, primary);
            }
            ai.paddrs = paddrs;
            ai.primary = primary;
            ai.outcnt = b_in;
            ai.incnt = b_out;
            ai.ssn = vec![0; usize::from(b_in)];
            drop(ai);
            // `sctp_cmd_new_state`: a one-to-one socket is connected.
            if g.style == Style::Tcp && g.state == SkState::Closed {
                g.state = SkState::Established;
            }
            if g.daddr.is_none() {
                g.daddr = Some(primary);
            }
        }
        self.net.state().sctp.stats.active_estabs += 1;
        self.wq.wake(PollMask::OUT | PollMask::WRNORM);
        notify(a, assoc_change(a, SctpSacState::SCTP_COMM_UP, 0, &[]));
        self.rearm_idle();
        // net/sctp/sm_statefuns.c: sctp_sf_do_5_1E_ca / sctp_sf_do_5_1D_ce.
        for (receiver, peer) in [(a, b), (b, a)] {
            let adaptation = peer.lock().adaptation;
            if adaptation != 0 {
                notify(
                    receiver,
                    guest_bytes(&SctpAdaptationEvent {
                        sai_type: SctpSnType::SCTP_ADAPTATION_INDICATION.0,
                        sai_flags: 0,
                        sai_length: SctpAdaptationEvent::SIZE as u32,
                        sai_adaptation_ind: adaptation,
                        sai_assoc_id: receiver.id,
                    }),
                );
            }
        }
    }

    /// `__sctp_connect` for `connect` and `sctp_connectx`.
    fn connect_addrs(&self, addrs: &[SocketAddr]) -> NetResult<Arc<Assoc>> {
        let dsts = {
            let g = self.lock();
            if matches!(g.state, SkState::Established | SkState::Closing)
                || (g.style == Style::Tcp && g.state == SkState::Listening)
            {
                return Err(Errno::EISCONN);
            }
            let first = self.verify_dst(&g, *addrs.first().ok_or(Errno::EINVAL)?)?;
            if let Some(asoc) = self.lookup_assoc(&g, first) {
                return Err(match asoc.established() {
                    true => Errno::EISCONN,
                    false => Errno::EALREADY,
                });
            }
            let mut dsts = vec![first];
            for addr in &addrs[1..] {
                if addr.port() != first.port() {
                    return Err(Errno::EINVAL);
                }
                let addr = self.verify_dst(&g, *addr)?;
                // `sctp_connect_add_peer`: an address of another association is in use.
                if self.lookup_assoc(&g, addr).is_some() {
                    return Err(Errno::EADDRNOTAVAIL);
                }
                if !dsts.contains(&addr) {
                    dsts.push(addr);
                }
            }
            dsts
        };
        let a = self.associate(&dsts, None)?;
        self.lock().err = None;
        Ok(a)
    }

    /// The `sk_err` a failed send reports in place of `EPIPE` (`sctp_error`).
    fn send_error(&self, err: Errno) -> Errno {
        match err {
            Errno::EPIPE => self.lock().err.take().unwrap_or(Errno::EPIPE),
            err => err,
        }
    }

    /// `sctp_sendmsg_check_sflags`: `None` to send, `Some(0)` when the flags were the request.
    fn check_sflags(
        &self,
        style: Style,
        asoc: &Arc<Assoc>,
        flags: u16,
        data: &[u8],
    ) -> NetResult<Option<usize>> {
        let state = asoc.lock().state;
        if state == AState::Closed && style == Style::Tcp {
            return Err(Errno::EPIPE);
        }
        if flags & SENDALL != 0 && style == Style::Udp && state != AState::Established {
            return Ok(Some(0));
        }
        if flags & EOF != 0 {
            end_assoc(asoc, End::Shutdown);
            return Ok(Some(0));
        }
        if flags & ABORT != 0 {
            end_assoc(asoc, End::Abort(data.to_vec()));
            return Ok(Some(0));
        }
        Ok(None)
    }

    /// `sctp_sendmsg_to_asoc`: one message to the association's peer.
    fn send_to_asoc(&self, asoc: &Arc<Assoc>, data: &[u8], info: SendInfo) -> NetResult<usize> {
        let g = self.lock();
        let (disable_fragments, wspace, subscribed_dry) = (
            g.so.disable_fragments,
            self.wspace(&g),
            subscribed(asoc.lock().subscribe, SctpSnType::SCTP_SENDER_DRY_EVENT.0),
        );
        drop(g);
        let _tx = asoc.tx.lock().unwrap_or_else(|e| e.into_inner());
        let (peer, ssn, tsn) = {
            let mut a = asoc.lock();
            if info.stream >= a.outcnt {
                return Err(Errno::EINVAL);
            }
            if disable_fragments && data.len() > a.frag_point() as usize {
                return Err(Errno::EMSGSIZE);
            }
            if wspace <= 0 {
                return Err(Errno::EAGAIN);
            }
            let peer = match (a.state, a.peer.upgrade()) {
                (AState::Established, Some(peer)) => peer,
                (AState::Closed, _) => return Err(Errno::EPIPE),
                // `sctp_sf_error_shutdown`.
                _ => return Err(Errno::ESHUTDOWN),
            };
            let unordered = info.flags & UNORDERED != 0;
            let ssn = match unordered {
                true => 0,
                false => {
                    let next = &mut a.ssn[usize::from(info.stream)];
                    let ssn = *next;
                    *next = next.wrapping_add(1);
                    ssn
                }
            };
            let tsn = a.next_tsn;
            a.next_tsn = tsn.wrapping_add(1);
            a.last_data = Instant::now();
            (peer, ssn, tsn)
        };
        {
            let mut b = peer.lock();
            b.cum_tsn = tsn;
            b.last_data = Instant::now();
        }
        let unordered = info.flags & UNORDERED != 0;
        let body = Body::Data {
            stream: info.stream,
            ssn,
            ppid: info.ppid,
            flags: if unordered { UNORDERED } else { 0 },
            tsn,
        };
        enqueue(&peer, body, data.to_vec());
        {
            let mut st = self.net.state();
            match unordered {
                true => st.sctp.stats.out_unorder += 1,
                false => st.sctp.stats.out_order += 1,
            }
        }
        if let Some(b_ep) = peer.owner() {
            let mut st = b_ep.net.state();
            match unordered {
                true => st.sctp.stats.in_unorder += 1,
                false => st.sctp.stats.in_order += 1,
            }
        }
        // `sctp_outq_sack`: once all that was sent is in the peer's window, the sender is dry.
        if subscribed_dry && peer.lock().unacked() == 0 {
            notify(asoc, sender_dry(asoc));
        }
        Ok(data.len())
    }

    /// `sctp_sendmsg`.
    fn sendmsg(&self, msg: &SendMsg<'_>) -> NetResult<usize> {
        let data = msg.data;
        let style = {
            let g = self.lock();
            if g.state == SkState::Listening && g.style == Style::Tcp {
                return Err(Errno::EPIPE);
            }
            if data.len() > g.opts.sndbuf {
                return Err(Errno::EMSGSIZE);
            }
            g.style
        };
        let cmsgs = parse_cmsgs(msg.control)?;
        let mut info = SendInfo::default();
        if let Some(sr) = cmsgs.srinfo {
            info = SendInfo {
                stream: sr.sinfo_stream,
                flags: sr.sinfo_flags,
                ppid: sr.sinfo_ppid,
                context: sr.sinfo_context,
                assoc_id: sr.sinfo_assoc_id,
            };
        }
        if let Some(s) = cmsgs.sinfo {
            info = SendInfo {
                stream: s.snd_sid,
                flags: s.snd_flags,
                ppid: s.snd_ppid,
                context: s.snd_context,
                assoc_id: s.snd_assoc_id,
            };
        }
        let sflags = info.flags;
        if sflags != 0 || data.is_empty() {
            if style == Style::Tcp && sflags & (EOF | ABORT) != 0 {
                return Err(Errno::EINVAL);
            }
            if (sflags & EOF != 0 && !data.is_empty())
                || (sflags & (EOF | ABORT) == 0 && data.is_empty())
            {
                return Err(Errno::EINVAL);
            }
            if sflags & ADDR_OVER != 0 && msg.to.is_none() {
                return Err(Errno::EINVAL);
            }
        }
        // `sctp_sendmsg_get_daddr`: a peeled-off socket ignores the address.
        let daddr = match (style, msg.to) {
            (Style::UdpHighBandwidth, _) | (_, None) => None,
            (_, Some(to)) => Some(self.verify_dst(&self.lock(), to)?),
        };
        if sflags & SENDALL != 0 && style == Style::Udp {
            let asocs = self.lock().asocs.clone();
            let mut sent = 0;
            for asoc in asocs {
                if self.check_sflags(style, &asoc, sflags, data)?.is_some() {
                    continue;
                }
                let info = self.fill_info(&asoc, info, &cmsgs);
                sent = self.send_to_asoc(&asoc, data, info)?;
            }
            return Ok(sent);
        }
        let asoc = match daddr {
            Some(dst) => {
                let found = self.lookup_assoc(&self.lock(), dst);
                match found {
                    Some(asoc) => {
                        if let Some(done) = self.check_sflags(style, &asoc, sflags, data)? {
                            return Ok(done);
                        }
                        asoc
                    }
                    None => {
                        // `sctp_sendmsg_new_asoc`.
                        if sflags & (EOF | ABORT) != 0 {
                            return Err(Errno::EINVAL);
                        }
                        {
                            let g = self.lock();
                            if style == Style::Tcp
                                && matches!(g.state, SkState::Established | SkState::Closing)
                            {
                                return Err(Errno::EADDRNOTAVAIL);
                            }
                        }
                        match self.associate(&[dst], cmsgs.init) {
                            Ok(asoc) => asoc,
                            // The message went out with the INIT's COOKIE ECHO, lost with
                            // the association; the failure is the socket's.
                            Err(err @ (Errno::ECONNREFUSED | Errno::ETIMEDOUT)) => {
                                self.set_err(err);
                                return Ok(data.len());
                            }
                            Err(err) => return Err(err),
                        }
                    }
                }
            }
            None => {
                let asoc = {
                    let g = self.lock();
                    self.id2assoc(&g, info.assoc_id)
                };
                let asoc = asoc.ok_or(Errno::EPIPE)?;
                if let Some(done) = self.check_sflags(style, &asoc, sflags, data)? {
                    return Ok(done);
                }
                asoc
            }
        };
        let info = self.fill_info(&asoc, info, &cmsgs);
        self.send_to_asoc(&asoc, data, info)
    }

    /// `sctp_sendmsg_update_sinfo`: without a send info, the association's defaults.
    fn fill_info(&self, asoc: &Assoc, mut info: SendInfo, cmsgs: &Cmsgs) -> SendInfo {
        if cmsgs.srinfo.is_none() && cmsgs.sinfo.is_none() {
            let a = asoc.lock();
            info.stream = a.defaults.stream;
            info.ppid = a.defaults.ppid;
            info.context = a.defaults.context;
            info.assoc_id = asoc.id;
            if !cmsgs.prinfo {
                info.flags = a.defaults.flags;
            }
        }
        info
    }

    fn abort_partial(&self, asoc: &Arc<Assoc>) {
        let aborted = {
            let mut g = self.lock();
            let mut aborted = false;
            g.rcvq.retain(|event| {
                let partial = Arc::ptr_eq(&event.asoc, asoc)
                    && !event.is_notification()
                    && event.read != 0
                    && event.partial_delivery;
                if partial {
                    let mut a = asoc.lock();
                    a.rmem = a.rmem.saturating_sub(event.left());
                    aborted = true;
                }
                !partial
            });
            aborted
        };
        if aborted {
            notify(
                asoc,
                guest_bytes(&SctpPdapiEvent {
                    pdapi_type: SctpSnType::SCTP_PARTIAL_DELIVERY_EVENT.0,
                    pdapi_flags: 0,
                    pdapi_length: SctpPdapiEvent::SIZE as u32,
                    pdapi_indication: 0,
                    pdapi_assoc_id: asoc.id,
                    pdapi_stream: 0,
                    pdapi_seq: 0,
                }),
            );
        }
    }

    /// Earliest association idle deadline (net/sctp/sm_statefuns.c:
    /// sctp_sf_autoclose_timer_expire). Outstanding data delays orderly shutdown.
    fn idle_delay(&self) -> Option<Duration> {
        let g = self.lock();
        if g.style != Style::Udp || g.so.autoclose == 0 {
            return None;
        }
        let limit = Duration::from_secs(u64::from(g.so.autoclose));
        g.asocs
            .iter()
            .filter_map(|a| {
                let a = a.lock();
                (a.state == AState::Established)
                    .then(|| limit.saturating_sub(a.last_data.elapsed()))
            })
            .min()
    }

    fn rearm_idle(&self) {
        let delay = self.idle_delay();
        if let Some(arm) = &*self.idle_timer.lock().unwrap() {
            arm(delay);
        }
    }

    /// `SCTP_AUTOCLOSE` for associations idle that long.
    fn expire_idle(&self) {
        let idle: Vec<Arc<Assoc>> = {
            let g = self.lock();
            if g.style != Style::Udp || g.so.autoclose == 0 {
                return;
            }
            let limit = Duration::from_secs(u64::from(g.so.autoclose));
            g.asocs
                .iter()
                .filter(|a| {
                    let a = a.lock();
                    a.state == AState::Established && a.last_data.elapsed() >= limit
                })
                .cloned()
                .collect()
        };
        for asoc in idle {
            self.net.state().sctp.stats.autoclose_expireds += 1;
            // sctp_sf_autoclose_timer_expire waits for the outgoing queue's ACKs.
            // Bytes outside the peer window are still on that outgoing queue.
            asoc.lock().state = AState::ShutdownPending;
            if asoc.peer().is_none_or(|peer| peer.lock().unacked() == 0) {
                end_assoc(&asoc, End::Shutdown);
            }
        }
    }

    /// The receive-side control messages of `event` (`sctp_recvmsg`): `SCTP_NXTINFO` for what
    /// follows it, `SCTP_RCVINFO` and `SCTP_SNDRCV` for a message.
    fn recv_controls(&self, g: &Inner, event: &Event, next: Option<&Event>) -> Vec<Control> {
        let mut out = Vec::new();
        if let (true, Some(next)) = (g.so.recvnxtinfo, next) {
            let (sid, ppid, mut flags) = match next.body {
                Body::Data {
                    stream,
                    ppid,
                    flags,
                    ..
                } => (stream, ppid, flags),
                Body::Notification => (0, 0, 0),
            };
            if next.is_notification() {
                flags |= NOTIFICATION;
            }
            let info = SctpNxtinfo {
                nxt_sid: sid,
                nxt_flags: flags,
                nxt_ppid: ppid,
                nxt_length: next.left() as u32,
                nxt_assoc_id: next.asoc.id,
            };
            out.push(Control {
                level: SOL_SCTP,
                kind: SctpCmsgType::SCTP_NXTINFO.0,
                data: guest_bytes(&info),
            });
        }
        let Body::Data {
            stream,
            ssn,
            ppid,
            flags,
            tsn,
        } = event.body
        else {
            return out;
        };
        let (cum_tsn, context) = {
            let a = event.asoc.lock();
            (a.cum_tsn, a.rcv_context)
        };
        if g.so.recvrcvinfo {
            let info = SctpRcvinfo {
                rcv_sid: stream,
                rcv_ssn: ssn,
                rcv_flags: flags,
                rcv_ppid: ppid,
                rcv_tsn: tsn,
                rcv_cumtsn: cum_tsn,
                rcv_context: context,
                rcv_assoc_id: event.asoc.id,
            };
            out.push(Control {
                level: SOL_SCTP,
                kind: SctpCmsgType::SCTP_RCVINFO.0,
                data: guest_bytes(&info),
            });
        }
        if subscribed(g.so.subscribe, SN_TYPE_BASE) {
            let info = SctpSndrcvinfo {
                sinfo_stream: stream,
                sinfo_ssn: ssn,
                sinfo_flags: flags,
                sinfo_ppid: ppid,
                sinfo_context: context,
                sinfo_timetolive: 0,
                sinfo_tsn: tsn,
                sinfo_cumtsn: cum_tsn,
                sinfo_assoc_id: event.asoc.id,
            };
            out.push(Control {
                level: SOL_SCTP,
                kind: SctpCmsgType::SCTP_SNDRCV.0,
                data: guest_bytes(&info),
            });
        }
        out
    }

    /// `sctp_recvmsg`.
    fn recvmsg(&self, buf: &mut [u8], flags: MsgFlags) -> NetResult<RecvMsg> {
        let peek = flags.contains(MsgFlags::MSG_PEEK);
        let mut g = self.lock();
        if g.style == Style::Tcp && g.state == SkState::Listening {
            return Err(Errno::ENOTCONN);
        }
        // `sctp_skb_recv_datagram` and `sctp_wait_for_packet`.
        if let Some(err) = g.err.take() {
            return Err(err);
        }
        if g.rcvq.is_empty() {
            if g.rd_shut {
                return Ok(RecvMsg::default());
            }
            // net/sctp/socket.c: sctp_wait_for_packet checks !timeo first;
            // nonblocking one-to-many sockets without associations return EAGAIN.
            if g.asocs.is_empty()
                && g.state != SkState::Listening
                && (g.style != Style::Udp || !flags.contains(MsgFlags::MSG_DONTWAIT))
            {
                return Err(Errno::ENOTCONN);
            }
            return Err(Errno::EAGAIN);
        }
        let event = match peek {
            true => None,
            false => g.rcvq.pop_front(),
        };
        let event_ref = event.as_ref().unwrap_or_else(|| &g.rcvq[0]);
        // net/sctp/ulpqueue.c: sctp_ulpq_partial_delivery. A message larger than
        // the receive window arrives in fragments, and can be aborted before EOR.
        let available = if event_ref.partial_delivery {
            event_ref.left().min(g.opts.rcvbuf / 2)
        } else {
            event_ref.left()
        };
        let n = available.min(buf.len());
        buf[..n].copy_from_slice(&event_ref.data[event_ref.read..event_ref.read + n]);
        let next = match peek {
            true => g.rcvq.front(),
            false => g.rcvq.front(),
        };
        let control = self.recv_controls(&g, event_ref, next);
        let notification = event_ref.is_notification();
        let primary = event_ref.asoc.lock().primary;
        let from = Some(self.user_addr(&g, primary));
        let mut out_flags = MsgFlags(0);
        if notification {
            out_flags |= MSG_NOTIFICATION;
        }
        let partial = event_ref.left() > n;
        if !partial {
            out_flags |= MsgFlags::MSG_EOR;
        }
        let mut freed = None;
        if let Some(mut event) = event {
            if !notification {
                let mut a = event.asoc.lock();
                a.rmem = a.rmem.saturating_sub(n);
                drop(a);
                freed = Some(event.asoc.clone());
            }
            if partial {
                event.read += n;
                g.rcvq.push_front(event);
            }
        }
        drop(g);
        // The window opened: the sender may have waited for room.
        if let Some(receiver) = freed
            && let Some(sender) = receiver.peer()
        {
            if sender.lock().state == AState::ShutdownPending && receiver.lock().unacked() == 0 {
                end_assoc(&sender, End::Shutdown);
            }
            if let Some(owner) = sender.owner() {
                owner.wq.wake(PollMask::OUT | PollMask::WRNORM);
            }
        }
        Ok(RecvMsg {
            len: n,
            full_len: n,
            from,
            flags: out_flags,
            control,
        })
    }

    /// `sctp_sock_migrate`: a new socket of `style` for `asoc`, with the listener's or the
    /// one-to-many socket's options, port and addresses, and the association's queued events.
    fn migrate(&self, g: &mut Inner, asoc: &Arc<Assoc>, style: Style) -> Arc<SctpSocket> {
        g.asocs.retain(|x| !Arc::ptr_eq(x, asoc));
        let (mine, rest): (VecDeque<Event>, VecDeque<Event>) = std::mem::take(&mut g.rcvq)
            .into_iter()
            .partition(|e| Arc::ptr_eq(&e.asoc, asoc));
        g.rcvq = rest;
        let closed = asoc.lock().state == AState::Closed;
        let (state, rd_shut) = match (closed, style) {
            (true, Style::Tcp) => (SkState::Closed, true),
            _ => (SkState::Established, false),
        };
        let kind = match style {
            Style::Tcp => SocketKind::Stream,
            _ => SocketKind::SeqPacket,
        };
        let primary = asoc.lock().primary;
        let child = Self::build(
            self.net.clone(),
            self.domain,
            kind,
            self.uid,
            Inner {
                style,
                state,
                opts: g.opts.inherit(),
                so: g.so.clone(),
                port: g.port,
                bound: g.bound.clone(),
                saddr: g.saddr,
                daddr: Some(primary),
                asocs: vec![asoc.clone()],
                backlog: 0,
                rcvq: mine,
                err: None,
                rd_shut,
                wr_shut: false,
            },
        );
        {
            let c = child.lock();
            child.rehash(&c);
        }
        asoc.lock().ep = child.me.clone();
        child
    }

    /// `sctp_close`: unread data or `SO_LINGER` 0 aborts each association, else it shuts down.
    fn release(&self) {
        let (asocs, abort) = {
            let mut g = self.lock();
            g.rd_shut = true;
            g.wr_shut = true;
            let unread = g.rcvq.iter().any(|e| !e.is_notification());
            g.rcvq.clear();
            let abort = unread || g.opts.linger == Some(0);
            (std::mem::take(&mut g.asocs), abort)
        };
        for asoc in asocs {
            match abort {
                true => end_assoc(&asoc, End::Abort(Vec::new())),
                false => end_assoc(&asoc, End::Shutdown),
            }
        }
        self.net.state().sctp.remove(self.id);
        let mut g = self.lock();
        g.opts.mc.drop_all(&self.net);
    }

    /// net/sctp/socket.c: sctp_get_sctp_info. Counters for packets/loss are zero in
    /// the message transport; path and socket configuration come from their actual state.
    fn diag_info(&self, g: &Inner, asoc: Option<&AssocInner>) -> SctpInfo {
        let mut info = if asoc.is_some() {
            SctpInfo::default()
        } else {
            SctpInfo {
                sctpi_s_autoclose: g.so.autoclose,
                sctpi_s_adaptation_ind: g.so.adaptation_ind,
                sctpi_s_pd_point: g.so.pd_point,
                sctpi_s_nodelay: g.so.nodelay as u8,
                sctpi_s_disable_fragments: g.so.disable_fragments as u8,
                sctpi_s_v4mapped: g.so.v4mapped as u8,
                sctpi_s_frag_interleave: g.so.frag_interleave as u8,
                sctpi_s_type: u32::from(g.style.raw()),
                ..SctpInfo::default()
            }
        };
        if let Some(a) = asoc {
            let path = a.paths.get(&a.primary).unwrap_or(&a.path);
            info.sctpi_state = u32::from(a.state.internal());
            info.sctpi_rwnd = a.rwnd() as u32;
            info.sctpi_instrms = a.incnt;
            info.sctpi_outstrms = a.outcnt;
            info.sctpi_max_burst = a.max_burst;
            info.sctpi_maxseg = a.frag_point();
            info.sctpi_p_address = options::storage(self.domain, a.primary);
            info.sctpi_p_state = SctpSpinfoState::SCTP_ACTIVE.0;
            info.sctpi_p_cwnd = a.pmtu * 4;
            info.sctpi_p_rto = a.rto[0];
            info.sctpi_p_hbinterval = path.hb / 4;
            info.sctpi_p_pathmaxrxt = u32::from(path.retrans);
            info.sctpi_p_sackdelay = path.sack_delay;
        }
        info
    }

    /// `/proc/net/sctp/eps`.
    fn endpoint(&self) -> Option<SctpEndpoint> {
        let g = self.lock();
        (g.state == SkState::Listening).then(|| SctpEndpoint {
            domain: self.domain,
            rx_queue: g.asocs.len(),
            backlog: g.backlog,
            info: self.diag_info(&g, None),
            style: g.style.raw(),
            state: g.state.raw(),
            port: g.port,
            addrs: g.bound.clone(),
            uid: self.uid,
            inode: self.ino.load(Ordering::Relaxed),
        })
    }

    /// `/proc/net/sctp/assocs`.
    fn assoc_entries(&self) -> Vec<SctpAssocEntry> {
        let g = self.lock();
        g.asocs
            .iter()
            .map(|asoc| {
                // The peer first: an association's lock is never held while taking another's.
                let tx = asoc.peer().map_or(0, |b| b.lock().unacked());
                let a = asoc.lock();
                SctpAssocEntry {
                    domain: self.domain,
                    info: self.diag_info(&g, Some(&a)),
                    style: g.style.raw(),
                    sk_state: g.state.raw(),
                    state: a.state.internal(),
                    id: asoc.id,
                    tx_queue: tx,
                    rx_queue: a.rmem,
                    uid: self.uid,
                    inode: self.ino.load(Ordering::Relaxed),
                    lport: a.lport,
                    rport: a.primary.port(),
                    laddrs: a.laddrs.clone(),
                    primary_src: Some(a.saddr),
                    paddrs: a.paddrs.iter().map(|p| p.ip()).collect(),
                    primary: Some(a.primary.ip()),
                    hbinterval: a.path.hb,
                    incnt: a.incnt,
                    outcnt: a.outcnt,
                    max_retrans: a.max_retrans,
                    sndbuf: g.opts.sndbuf,
                    rcvbuf: g.opts.rcvbuf,
                }
            })
            .collect()
    }
}

/// The namespace's listening endpoints.
pub(crate) fn endpoints(net: &Net) -> Vec<SctpEndpoint> {
    let socks = sockets(net);
    socks.iter().filter_map(|s| s.endpoint()).collect()
}

pub(crate) fn assocs(net: &Net) -> Vec<SctpAssocEntry> {
    let socks = sockets(net);
    socks.iter().flat_map(|s| s.assoc_entries()).collect()
}

/// `sctp_snmp_list` in order.
pub(crate) fn snmp(net: &Net) -> Vec<(&'static str, u64)> {
    let stats = net.state().sctp.stats;
    let curr: u64 = assocs(net)
        .iter()
        .filter(|a| a.state == AState::Established.internal())
        .count() as u64;
    let mut out = vec![
        ("SctpCurrEstab", curr),
        ("SctpActiveEstabs", stats.active_estabs),
        ("SctpPassiveEstabs", stats.passive_estabs),
        ("SctpAborteds", stats.aborteds),
        ("SctpShutdowns", stats.shutdowns),
        ("SctpOutOfBlues", stats.out_of_blues),
        ("SctpChecksumErrors", 0),
        ("SctpOutCtrlChunks", 0),
        ("SctpOutOrderChunks", stats.out_order),
        ("SctpOutUnorderChunks", stats.out_unorder),
        ("SctpInCtrlChunks", 0),
        ("SctpInOrderChunks", stats.in_order),
        ("SctpInUnorderChunks", stats.in_unorder),
    ];
    let zero = [
        "SctpFragUsrMsgs",
        "SctpReasmUsrMsgs",
        "SctpOutSCTPPacks",
        "SctpInSCTPPacks",
        "SctpT1InitExpireds",
        "SctpT1CookieExpireds",
        "SctpT2ShutdownExpireds",
        "SctpT3RtxExpireds",
        "SctpT4RtoExpireds",
        "SctpT5ShutdownGuardExpireds",
        "SctpDelaySackExpireds",
    ];
    out.extend(zero.iter().map(|n| (*n, 0)));
    out.push(("SctpAutocloseExpireds", stats.autoclose_expireds));
    let zero = [
        "SctpT3Retransmits",
        "SctpPmtudRetransmits",
        "SctpFastRetransmits",
        "SctpInPktSoftirq",
        "SctpInPktBacklog",
        "SctpInPktDiscards",
        "SctpInDataChunkDiscards",
    ];
    out.extend(zero.iter().map(|n| (*n, 0)));
    out
}

/// The live sockets of the table, read outside the namespace's lock (sockets lock before it).
fn sockets(net: &Net) -> Vec<Arc<SctpSocket>> {
    let weak: Vec<Weak<SctpSocket>> = net
        .state()
        .sctp
        .bound
        .iter()
        .map(|b| b.sock.clone())
        .collect();
    weak.iter().filter_map(Weak::upgrade).collect()
}

impl Pollable for SctpSocket {
    /// `sctp_poll`.
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, &self.wq);
        self.expire_idle();
        let g = self.lock();
        if g.style == Style::Tcp && g.state == SkState::Listening {
            return match g.asocs.is_empty() {
                true => PollMask::empty(),
                false => PollMask::IN | PollMask::RDNORM,
            };
        }
        let mut mask = PollMask::empty();
        if g.err.is_some() {
            mask |= PollMask::ERR;
        }
        if g.rd_shut {
            mask |= PollMask::RDHUP | PollMask::IN | PollMask::RDNORM;
        }
        if g.rd_shut && g.wr_shut {
            mask |= PollMask::HUP;
        }
        if !g.rcvq.is_empty() {
            mask |= PollMask::IN | PollMask::RDNORM;
        }
        if g.style != Style::Udp && g.state == SkState::Closed {
            return mask;
        }
        if self.wspace(&g) > 0 {
            mask |= PollMask::OUT | PollMask::WRNORM;
        }
        mask
    }
}

impl Socket for SctpSocket {
    fn domain(&self) -> Domain {
        self.domain
    }

    fn kind(&self) -> SocketKind {
        self.kind
    }

    fn protocol(&self) -> i32 {
        IPPROTO_SCTP
    }

    /// `sctp_bind`.
    fn bind(&self, addr: SocketAddr, privileged: bool) -> NetResult<()> {
        let mut g = self.lock();
        if g.port != 0 {
            return Err(Errno::EINVAL);
        }
        self.do_bind(&mut g, addr, privileged)
    }

    /// `sctp_inet_listen`.
    fn listen(&self, backlog: u32) -> NetResult<()> {
        let mut g = self.lock();
        if g.style == Style::UdpHighBandwidth
            || !matches!(g.state, SkState::Listening | SkState::Closed)
            || (g.style == Style::Tcp && g.daddr.is_some())
        {
            return Err(Errno::EINVAL);
        }
        if backlog == 0 {
            if g.state == SkState::Closed {
                return Err(Errno::EINVAL);
            }
            g.state = SkState::Closed;
            self.rehash(&g);
            return Ok(());
        }
        if g.state == SkState::Listening {
            g.backlog = backlog;
            return Ok(());
        }
        // `sctp_listen_start`: an unbound socket binds, a bound one checks its port again
        // now that it listens.
        g.state = SkState::Listening;
        if g.port == 0 {
            if let Err(err) = self.autobind(&mut g) {
                g.state = SkState::Closed;
                return Err(err);
            }
        } else {
            let entry = self.bound_entry(&g, g.bound.clone(), g.port);
            let st = self.net.state();
            if g.bound
                .iter()
                .any(|a| st.sctp.conflicts(&entry, g.port, *a))
            {
                drop(st);
                g.state = SkState::Closed;
                return Err(Errno::EADDRINUSE);
            }
        }
        g.backlog = backlog;
        self.rehash(&g);
        Ok(())
    }

    /// `sctp_accept`.
    fn accept(&self) -> NetResult<Accepted> {
        let mut g = self.lock();
        if g.style != Style::Tcp {
            return Err(Errno::EOPNOTSUPP);
        }
        if g.state != SkState::Listening || g.rd_shut {
            return Err(Errno::EINVAL);
        }
        let asoc = g.asocs.first().cloned().ok_or(Errno::EAGAIN)?;
        let child = self.migrate(&mut g, &asoc, Style::Tcp);
        let peer = asoc.lock().primary;
        let peer = self.user_addr(&g, peer);
        Ok(Accepted {
            socket: child,
            peer,
        })
    }

    /// `sctp_inet_connect` and `__sctp_connect` with one address.
    fn connect(&self, addr: SocketAddr) -> NetResult<()> {
        self.expire_idle();
        self.connect_addrs(&[addr])?;
        Err(Errno::EINPROGRESS)
    }

    /// `connect` with `AF_UNSPEC` (`sctp_inet_connect`).
    fn disconnect(&self) -> NetResult<()> {
        Err(Errno::EOPNOTSUPP)
    }

    /// `inet_shutdown` and `sctp_shutdown`.
    fn shutdown(&self, how: Shutdown) -> NetResult<()> {
        let rd = how != Shutdown::Write;
        let wr = how != Shutdown::Read;
        let mut g = self.lock();
        let result = match g.state {
            SkState::Listening => {
                if rd {
                    // `sctp_disconnect`.
                    match g.style {
                        Style::Tcp => {
                            g.rd_shut = true;
                            Ok(())
                        }
                        _ => Err(Errno::EOPNOTSUPP),
                    }
                } else {
                    Ok(())
                }
            }
            state => {
                g.rd_shut |= rd;
                g.wr_shut |= wr;
                let asoc = match (g.style, wr) {
                    (Style::Tcp, true) => g.asocs.first().cloned(),
                    _ => None,
                };
                if asoc.is_some() {
                    g.state = SkState::Closing;
                }
                drop(g);
                if let Some(asoc) = asoc {
                    end_assoc(&asoc, End::Shutdown);
                }
                g = self.lock();
                match state {
                    SkState::Closed => Err(Errno::ENOTCONN),
                    _ => Ok(()),
                }
            }
        };
        drop(g);
        self.wq.wake(PollMask::empty());
        result
    }

    fn send(&self, msg: SendMsg<'_>) -> NetResult<usize> {
        self.expire_idle();
        self.sendmsg(&msg).map_err(|err| self.send_error(err))
    }

    fn recv(&self, buf: &mut [u8], flags: MsgFlags) -> NetResult<RecvMsg> {
        // `inet_recv_error` with nothing queued.
        if flags.contains(MsgFlags::MSG_ERRQUEUE) {
            return Err(Errno::EAGAIN);
        }
        self.expire_idle();
        self.recvmsg(buf, flags)
    }

    fn local_addr(&self) -> NetResult<SocketAddr> {
        let g = self.lock();
        let any = match self.domain {
            Domain::Inet => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            Domain::Inet6 => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
        };
        let addr = SocketAddr::new(g.saddr.unwrap_or(any), g.port);
        Ok(self.user_addr(&g, addr))
    }

    /// `inet_getname` of the peer: a one-to-one socket's, while connected.
    fn peer_addr(&self) -> NetResult<SocketAddr> {
        let g = self.lock();
        match (g.daddr, g.state) {
            (Some(peer), SkState::Established | SkState::Closing | SkState::Listening) => {
                Ok(self.user_addr(&g, peer))
            }
            _ => Err(Errno::ENOTCONN),
        }
    }

    fn take_error(&self) -> Option<Errno> {
        self.lock().err.take()
    }

    /// `sctp_ioctl` `SIOCINQ`: what is left of the next message or notification.
    fn input_len(&self) -> NetResult<usize> {
        let g = self.lock();
        if g.state == SkState::Listening {
            return Err(Errno::EINVAL);
        }
        Ok(g.rcvq.front().map_or(0, Event::left))
    }

    fn set_option(&self, level: i32, name: i32, value: &[u8]) -> NetResult<()> {
        match level {
            SOL_SCTP => self.set_sctp_option(name, value),
            // `ip_setsockopt` and `ipv6_setsockopt` know no TCP options.
            l if l == SocketLevel::SOL_TCP.0 => Err(Errno::ENOPROTOOPT),
            _ => {
                let mut g = self.lock();
                if crate::mcast::handles(level, name) {
                    return g.opts.set_mcast(&self.net, level, name, value);
                }
                let bound = g.port != 0;
                match g.opts.set(level, name, value, bound)? {
                    Changed::Reuse if bound => self.rehash(&g),
                    Changed::Buffers => {
                        let rcvbuf = g.opts.rcvbuf;
                        for asoc in &g.asocs {
                            asoc.lock().rcvbuf = rcvbuf;
                        }
                        drop(g);
                        self.wq.wake(PollMask::OUT | PollMask::WRNORM);
                    }
                    _ => {}
                }
                Ok(())
            }
        }
    }

    fn get_option(&self, level: i32, name: i32, out: &mut [u8]) -> NetResult<usize> {
        match level {
            SOL_SCTP => self.get_sctp_option(name, out),
            l if l == SocketLevel::SOL_TCP.0 => Err(Errno::ENOPROTOOPT),
            _ => {
                let g = self.lock();
                if crate::mcast::handles(level, name) {
                    return g.opts.get_mcast(&self.net, level, name, out);
                }
                if level == SocketLevel::SOL_SOCKET.0 && name == So::SO_ACCEPTCONN.0 {
                    return Ok(opts::write_int(out, (g.state == SkState::Listening) as i32));
                }
                if name != wasixv2_abi::Ipv6Option::IPV6_ADDRFORM.0 {
                    let peer = match g.state {
                        SkState::Established | SkState::Closing => g.daddr.map(|p| p.ip()),
                        _ => None,
                    };
                    if let Some(result) = g.opts.get_mtu(&self.net, peer, level, name, out) {
                        return result;
                    }
                }
                g.opts.get(level, name, out)
            }
        }
    }

    fn graft(&self, ino: u64) {
        self.ino.store(ino, Ordering::Relaxed);
    }

    fn max_record(&self) -> Option<usize> {
        Some(MAX_MESSAGE)
    }

    fn sctp(&self) -> Option<&dyn SctpOps> {
        Some(self)
    }
}

impl SctpOps for SctpSocket {
    fn set_autoclose_timer(&self, arm: Box<dyn Fn(Option<Duration>) + Send + Sync>) {
        *self.idle_timer.lock().unwrap() = Some(arm);
        self.rearm_idle();
    }

    fn autoclose_expire(&self) -> Option<Duration> {
        self.expire_idle();
        self.idle_delay()
    }

    /// `sctp_bindx_add` and `sctp_bindx_rem`.
    fn bindx(&self, addrs: &[SocketAddr], add: bool, privileged: bool) -> NetResult<()> {
        let mut g = self.lock();
        let before = (g.bound.clone(), g.port, g.saddr);
        let result = match add {
            true => addrs
                .iter()
                .try_for_each(|addr| self.do_bind(&mut g, *addr, privileged)),
            false => addrs.iter().try_for_each(|addr| {
                if g.bound.len() <= 1 {
                    return Err(Errno::EBUSY);
                }
                let addr = self.family_ok(&g, *addr)?;
                if unusable(addr.ip()) && !addr.ip().is_unspecified() {
                    return Err(Errno::EADDRNOTAVAIL);
                }
                if addr.port() != 0 && addr.port() != g.port {
                    return Err(Errno::EINVAL);
                }
                // `sctp_del_bind_addr`.
                let at = g
                    .bound
                    .iter()
                    .position(|a| *a == addr.ip())
                    .ok_or(Errno::EINVAL)?;
                g.bound.remove(at);
                Ok(())
            }),
        };
        if result.is_err() {
            (g.bound, g.port, g.saddr) = before;
        }
        if g.port != 0 {
            self.rehash(&g);
        }
        result
    }

    fn connectx(&self, addrs: &[SocketAddr]) -> NetResult<i32> {
        self.expire_idle();
        self.connect_addrs(addrs).map(|a| a.id)
    }

    /// `sctp_do_peeloff`.
    fn peeloff(&self, id: i32) -> NetResult<Arc<dyn Socket>> {
        let mut g = self.lock();
        let asoc = self.id2assoc(&g, id).ok_or(Errno::EINVAL)?;
        if g.style != Style::Udp {
            return Err(Errno::EINVAL);
        }
        Ok(self.migrate(&mut g, &asoc, Style::UdpHighBandwidth))
    }
}

impl Drop for SctpSocket {
    fn drop(&mut self) {
        self.release();
    }
}
