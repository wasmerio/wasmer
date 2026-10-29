//! The rate limits of the ICMP messages a namespace sends (`net/ipv4/icmp.c`,
//! `net/ipv6/icmp.c`, `net/ipv4/inetpeer.c`): a token bucket of `icmp_msgs_per_sec` up to
//! `icmp_msgs_burst` shared by both families (`icmp_global_allow`, `icmp_global_consume`),
//! and one per destination refilled one token a jiffy, of which a message takes
//! `icmp_ratelimit` (IPv4) or `icmp/ratelimit` (IPv6) (`inet_peer_xrlim_allow`). Only the
//! types `icmp_ratemask` and `icmp/ratemask` name are limited.
//!
//! Time is the kernel's monotonic clock in jiffies at its `HZ` of 1000, so the sysctls'
//! milliseconds are jiffies (`proc_dointvec_ms_jiffies`).

use std::{collections::HashMap, net::IpAddr, time::Duration};

use crate::ports::PortRng;

/// `HZ`: jiffies are milliseconds.
const HZ: u64 = 1000;
/// `NR_ICMP_TYPES`, `ICMPV6_MSG_MAX`: types past these are never limited.
const NR_ICMP_TYPES: u8 = 18;
const ICMP_DEST_UNREACH: u8 = 3;
const ICMP_FRAG_NEEDED: u8 = 4;
const ICMPV6_ERRMSG_MAX: usize = 127;
const ICMPV6_PKT_TOOBIG: usize = 2;
/// `XRLIM_BURST_FACTOR`: a destination's bucket holds this many messages' tokens.
const XRLIM_BURST_FACTOR: u64 = 6;
/// `inet_peer_maxttl`, `inet_peer_minttl` and `inet_peer_threshold` at its largest: how long
/// an idle destination is remembered, less the more there are (`inet_peer_gc`).
const PEER_MAXTTL: u64 = 10 * 60 * HZ;
const PEER_MINTTL: u64 = 120 * HZ;
const PEER_THRESHOLD: u64 = 65536 + 128;

/// One of the rate-limit sysctls of a namespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IcmpLimit {
    /// `net.ipv4.icmp_ratelimit`, in milliseconds.
    Ratelimit(i32),
    /// `net.ipv4.icmp_ratemask`: bit `n` limits type `n`.
    Ratemask(i32),
    /// `net.ipv4.icmp_msgs_per_sec`, `net.ipv4.icmp_msgs_burst` (both families'); not
    /// negative.
    MsgsPerSec(i32),
    MsgsBurst(i32),
    /// `net.ipv6.icmp.ratelimit`, in milliseconds.
    Ratelimit6(i32),
    /// `net.ipv6.icmp.ratemask`: bit `n` (of word `n / 64`) limits type `n`.
    Ratemask6([u64; 4]),
}

/// Why a message is sent, which decides the checks it passes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cause {
    /// An error about a packet (`__icmp_send`, `icmp6_send`).
    Error,
    /// An echo reply (`icmp_reply`, `icmpv6_echo_reply`).
    Echo,
}

/// The ICMP message about to be sent, for [`Limits::allow`].
pub(crate) struct Outgoing {
    pub v6: bool,
    pub type_: u8,
    pub code: u8,
    pub cause: Cause,
    /// The destination.
    pub dst: IpAddr,
    /// The packet answered arrived on loopback.
    pub in_loopback: bool,
    /// The message leaves through loopback.
    pub out_loopback: bool,
}

/// A destination's bucket (`inet_peer`'s `rate_tokens` and `rate_last`).
#[derive(Clone, Copy, Debug)]
struct Peer {
    tokens: u64,
    last: u64,
    /// When it was made (`dtime`), for forgetting it.
    made: u64,
}

/// A namespace's ICMP rate limits and their state.
#[derive(Debug)]
pub(crate) struct Limits {
    ratelimit: i32,
    ratemask: i32,
    msgs_per_sec: i32,
    msgs_burst: i32,
    ratelimit6: i32,
    ratemask6: [u64; 4],
    /// `icmp_global_credit` and `icmp_global_stamp`, never refilled before the first message.
    credit: i32,
    stamp: Option<u64>,
    /// `net->ipv4.peers`, `net->ipv6.peers`: the families' addresses stay apart.
    peers: HashMap<IpAddr, Peer>,
    rng: PortRng,
}

impl Default for Limits {
    /// `icmp_sk_init`'s and `inet6_net_init`'s values.
    fn default() -> Self {
        let mut ratemask6 = [0; 4];
        for bit in (0..=ICMPV6_ERRMSG_MAX).filter(|&b| b != ICMPV6_PKT_TOOBIG) {
            ratemask6[bit / 64] |= 1 << (bit % 64);
        }
        Self {
            ratelimit: HZ as i32,
            ratemask: 0x1818,
            msgs_per_sec: 10000,
            msgs_burst: 10000,
            ratelimit6: (HZ / 10) as i32,
            ratemask6,
            credit: 0,
            stamp: None,
            peers: HashMap::new(),
            rng: PortRng::new(0x1c3f),
        }
    }
}

/// `jiffies` at `now`.
fn jiffies(now: Duration) -> u64 {
    now.as_millis() as u64
}

impl Limits {
    pub(crate) fn set(&mut self, limit: IcmpLimit) {
        match limit {
            IcmpLimit::Ratelimit(v) => self.ratelimit = v,
            IcmpLimit::Ratemask(v) => self.ratemask = v,
            IcmpLimit::MsgsPerSec(v) => self.msgs_per_sec = v,
            IcmpLimit::MsgsBurst(v) => self.msgs_burst = v,
            IcmpLimit::Ratelimit6(v) => self.ratelimit6 = v,
            IcmpLimit::Ratemask6(v) => self.ratemask6 = v,
        }
    }

    /// Whether `msg` may be sent at `now`, taking its share of the buckets if so: the global
    /// check (`icmpv4_global_allow`, `icmpv6_global_allow`), which IPv4 echo replies pass
    /// even from loopback and the rest skip there, then the destination's
    /// (`icmpv4_xrlim_allow`, `icmpv6_xrlim_allow`).
    pub(crate) fn allow(&mut self, msg: &Outgoing, now: Duration) -> bool {
        let now = jiffies(now);
        let global = !msg.in_loopback || (!msg.v6 && msg.cause == Cause::Echo);
        let mut apply = false;
        if global && !self.mask_allow(msg) {
            if !self.global_allow(now) {
                return false;
            }
            apply = true;
        }
        if !apply {
            return true;
        }
        let timeout = match msg.v6 {
            false => self.ratelimit,
            true => self.ratelimit6,
        };
        let ok = timeout == 0 || msg.out_loopback || self.peer_allow(msg.dst, timeout, now);
        if ok {
            self.global_consume();
        }
        ok
    }

    /// `icmpv4_mask_allow`, `icmpv6_mask_allow`: types the mask leaves out, and IPv4's
    /// fragmentation needed, are never limited.
    fn mask_allow(&self, msg: &Outgoing) -> bool {
        match msg.v6 {
            false => {
                msg.type_ > NR_ICMP_TYPES
                    || (msg.type_ == ICMP_DEST_UNREACH && msg.code == ICMP_FRAG_NEEDED)
                    || (1i32 << msg.type_) & self.ratemask == 0
            }
            true => {
                let bit = usize::from(msg.type_);
                self.ratemask6[bit / 64] & (1 << (bit % 64)) == 0
            }
        }
    }

    /// `icmp_global_allow`: credit left, or at least `HZ / 50` since the last refill, which
    /// adds `icmp_msgs_per_sec` a second of it up to `icmp_msgs_burst`.
    fn global_allow(&mut self, now: u64) -> bool {
        if self.credit > 0 {
            return true;
        }
        let delta = self.stamp.map_or(HZ, |s| now.wrapping_sub(s).min(HZ));
        if delta < HZ / 50 {
            return false;
        }
        let incr = (self.msgs_per_sec.max(0) as u64 * delta / HZ) as i32;
        if incr == 0 {
            return false;
        }
        self.stamp = Some(now);
        self.credit = self.credit.saturating_add(incr).min(self.msgs_burst);
        true
    }

    /// `icmp_global_consume`: a message takes 0 to 2 credits, at random.
    fn global_consume(&mut self) {
        self.credit = self.credit.saturating_sub(self.rng.below(3) as i32);
    }

    /// `inet_getpeer` and `inet_peer_xrlim_allow`: the destination's bucket gains a token a
    /// jiffy up to `XRLIM_BURST_FACTOR` messages' worth, and a message takes `timeout`. A new
    /// destination starts as one last seen a minute ago.
    fn peer_allow(&mut self, dst: IpAddr, timeout: i32, now: u64) -> bool {
        self.forget_peers(now);
        let peer = self.peers.entry(dst).or_insert(Peer {
            tokens: 0,
            last: now.wrapping_sub(60 * HZ),
            made: now,
        });
        // C's int arithmetic, then `unsigned long`: a negative timeout never lets one through.
        let timeout = i64::from(timeout) as u64;
        let max = XRLIM_BURST_FACTOR.wrapping_mul(timeout);
        let delta = now.wrapping_sub(peer.last);
        if delta != 0 {
            peer.last = now;
            peer.tokens = peer.tokens.wrapping_add(delta);
            if peer.tokens > max {
                peer.tokens = max;
            }
        }
        if peer.tokens >= timeout {
            peer.tokens -= timeout;
            return true;
        }
        false
    }

    /// `inet_peer_gc`: destinations older than the time to live, which shrinks as their
    /// number grows, are forgotten.
    fn forget_peers(&mut self, now: u64) {
        let total = self.peers.len() as u64;
        let ttl = match total >= PEER_THRESHOLD {
            true => 0,
            false => PEER_MAXTTL - (PEER_MAXTTL - PEER_MINTTL) / HZ * total / PEER_THRESHOLD * HZ,
        };
        self.peers.retain(|_, p| now.wrapping_sub(p.made) < ttl);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error(dst: &str, in_loopback: bool) -> Outgoing {
        Outgoing {
            v6: false,
            type_: 3,
            code: 3,
            cause: Cause::Error,
            dst: dst.parse().unwrap(),
            in_loopback,
            out_loopback: in_loopback,
        }
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// A destination gets six messages at once, then one per `icmp_ratelimit`; another has a
    /// bucket of its own, loopback none, and a type outside the mask is never limited.
    #[test]
    fn destinations_get_a_burst_then_one_per_ratelimit() {
        let mut l = Limits::default();
        let t = ms(5_000_000);
        let sent = (0..10)
            .filter(|_| l.allow(&error("10.0.0.2", false), t))
            .count();
        assert_eq!(sent, 6);
        assert!(!l.allow(&error("10.0.0.2", false), t + ms(999)));
        assert!(l.allow(&error("10.0.0.2", false), t + ms(1000)));
        assert!(l.allow(&error("10.0.0.3", false), t));
        assert!((0..10).all(|_| l.allow(&error("127.0.0.1", true), t)));
        let echo = Outgoing {
            type_: 0,
            code: 0,
            cause: Cause::Echo,
            ..error("10.0.0.2", false)
        };
        assert!((0..10).all(|_| l.allow(&echo, t)));
        l.set(IcmpLimit::Ratelimit(0));
        assert!((0..10).all(|_| l.allow(&error("10.0.0.2", false), t)));
    }

    /// The global bucket: `icmp_msgs_burst` at most, refilled `icmp_msgs_per_sec` a second
    /// no sooner than `HZ / 50` after the last refill; IPv4 echo replies pass it even on
    /// loopback once the mask names them.
    #[test]
    fn the_global_bucket_refills_per_second_up_to_the_burst() {
        let mut l = Limits::default();
        l.set(IcmpLimit::Ratelimit(0));
        l.set(IcmpLimit::MsgsPerSec(100));
        l.set(IcmpLimit::MsgsBurst(4));
        let t = ms(5_000_000);
        let sent = (0..20)
            .filter(|_| l.allow(&error("10.0.0.2", false), t))
            .count();
        // Four credits, each message taking 0 to 2 of them; once spent, nothing for 20 ms.
        assert!((2..=20).contains(&sent), "{sent}");
        while l.allow(&error("10.0.0.2", false), t) {}
        assert!(!l.allow(&error("10.0.0.2", false), t + ms(19)));
        assert!(l.allow(&error("10.0.0.2", false), t + ms(20)));
        l.set(IcmpLimit::MsgsPerSec(0));
        while l.allow(&error("10.0.0.2", false), t + ms(20)) {}
        assert!(!l.allow(&error("10.0.0.2", false), t + ms(5000)));

        l.set(IcmpLimit::Ratemask(1));
        let echo = Outgoing {
            type_: 0,
            code: 0,
            cause: Cause::Echo,
            ..error("127.0.0.1", true)
        };
        assert!(!l.allow(&echo, t + ms(5000)));
        let echo6 = Outgoing {
            v6: true,
            type_: 129,
            dst: "::1".parse().unwrap(),
            ..echo
        };
        l.set(IcmpLimit::Ratemask6([u64::MAX; 4]));
        assert!(l.allow(&echo6, t + ms(5000)));
    }
}
