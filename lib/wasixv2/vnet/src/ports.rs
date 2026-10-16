//! Port tables: which local addresses and ports the sockets of a namespace hold.
//!
//! One table per protocol covers both address families, as Linux's bind hash does, so an IPv6
//! wildcard socket that is not `IPV6_V6ONLY` collides with IPv4 sockets on the same port.
//! Conflict rules follow `inet_csk_bind_conflict`/`inet_bind_conflict` (TCP) and
//! `udp_lib_lport_inuse` (UDP); ephemeral selection follows `inet_csk_find_open_port`,
//! `__inet_hash_connect` and `udp_lib_get_port`.

use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr},
};

use wasixv2_abi::Errno;

use crate::{Domain, NetResult};

/// Identifies a socket in the tables of its namespace.
pub type SockId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Proto {
    Tcp,
    Udp,
}

/// The attributes of a socket that decide bind conflicts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BindSpec {
    pub family: Domain,
    /// Canonical local address (see [`crate::addr`]).
    pub addr: IpAddr,
    pub v6only: bool,
    /// `SO_REUSEADDR` (`sk_reuse`).
    pub reuseaddr: bool,
    /// `SO_REUSEPORT` (`sk_reuseport`).
    pub reuseport: bool,
    /// Owner uid; `SO_REUSEPORT` groups only form within one uid.
    pub uid: u32,
}

impl BindSpec {
    fn is_v4ish(&self) -> bool {
        self.addr.is_ipv4()
    }
}

/// `ip_local_port_range`, inclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortRange {
    pub low: u16,
    pub high: u16,
}

impl Default for PortRange {
    fn default() -> Self {
        Self {
            low: 32768,
            high: 60999,
        }
    }
}

/// Tiny deterministic generator for ephemeral port offsets (xorshift64*).
#[derive(Clone, Debug)]
pub struct PortRng(u64);

impl PortRng {
    pub fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    pub fn below(&mut self, n: u32) -> u32 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        let r = x.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 32;
        if n == 0 { 0 } else { (r % n as u64) as u32 }
    }
}

#[derive(Clone, Debug)]
struct Entry<T> {
    id: SockId,
    spec: BindSpec,
    listening: bool,
    /// Bound implicitly by `connect` (`__inet_hash_connect`), with the remote it connected to.
    remote: Option<SocketAddr>,
    connect_bound: bool,
    payload: T,
}

/// A port table for one protocol.
#[derive(Debug)]
pub struct PortTable<T> {
    proto: Proto,
    buckets: BTreeMap<u16, Vec<Entry<T>>>,
}

/// Whether two bound addresses overlap (`inet_rcv_saddr_equal` with wildcards matching).
pub fn addrs_overlap(a: &BindSpec, b: &BindSpec) -> bool {
    match (a.is_v4ish(), b.is_v4ish()) {
        (true, true) => a.addr == b.addr || a.addr.is_unspecified() || b.addr.is_unspecified(),
        (false, false) => a.addr.is_unspecified() || b.addr.is_unspecified() || a.addr == b.addr,
        // One IPv6 socket against an IPv4(-mapped) one: only a dual-stack IPv6 wildcard
        // covers the IPv4 space.
        (true, false) => b.addr.is_unspecified() && !b.v6only,
        (false, true) => a.addr.is_unspecified() && !a.v6only,
    }
}

impl<T: Clone> PortTable<T> {
    pub fn new(proto: Proto) -> Self {
        Self {
            proto,
            buckets: BTreeMap::new(),
        }
    }

    /// Does `new` conflict with `old` for an explicit bind (or `listen`, `relax == true`), or
    /// for ephemeral selection (`relax == false`, where any overlap is a conflict)?
    fn conflicts(&self, new: &BindSpec, old: &Entry<T>, relax: bool) -> bool {
        if !addrs_overlap(new, &old.spec) {
            return false;
        }
        let o = &old.spec;
        let reuseport_ok = new.reuseport && o.reuseport && new.uid == o.uid;
        match self.proto {
            Proto::Tcp => {
                if !relax {
                    return true;
                }
                // `inet_bind_conflict`: SO_REUSEADDR on both sides is enough unless the
                // existing socket listens; SO_REUSEPORT needs the same owner.
                let reuseaddr_ok = new.reuseaddr && o.reuseaddr && !old.listening;
                !(reuseaddr_ok || reuseport_ok)
            }
            Proto::Udp => !((new.reuseaddr && o.reuseaddr) || reuseport_ok),
        }
    }

    fn port_conflicts(&self, id: SockId, spec: &BindSpec, port: u16, relax: bool) -> bool {
        self.buckets.get(&port).is_some_and(|bucket| {
            bucket
                .iter()
                .any(|e| e.id != id && self.conflicts(spec, e, relax))
        })
    }

    fn insert(&mut self, id: SockId, spec: BindSpec, port: u16, payload: T) {
        self.buckets.entry(port).or_default().push(Entry {
            id,
            spec,
            listening: false,
            remote: None,
            connect_bound: false,
            payload,
        });
    }

    /// `bind`: claims `port` (or an ephemeral port when 0) for `spec`. Returns the port.
    /// Exhausting the ephemeral range is `EADDRINUSE`, as in `inet_csk_get_port`.
    pub fn bind(
        &mut self,
        id: SockId,
        spec: BindSpec,
        port: u16,
        range: PortRange,
        rng: &mut PortRng,
        payload: T,
    ) -> NetResult<u16> {
        let port = if port != 0 {
            if self.port_conflicts(id, &spec, port, true) {
                return Err(Errno::EADDRINUSE);
            }
            port
        } else {
            self.find_bind_port(id, &spec, range, rng)
                .ok_or(Errno::EADDRINUSE)?
        };
        self.insert(id, spec, port, payload);
        Ok(port)
    }

    fn find_bind_port(
        &self,
        id: SockId,
        spec: &BindSpec,
        range: PortRange,
        rng: &mut PortRng,
    ) -> Option<u16> {
        let free = |p: u16| !self.port_conflicts(id, spec, p, false);
        match self.proto {
            Proto::Udp => scan(range.low as u32, range.high as u32 + 1, rng, None, free),
            Proto::Tcp => {
                let (low, high) = (range.low as u32, range.high as u32 + 1);
                // `inet_csk_find_open_port`: SO_REUSEADDR sockets try the lower half first,
                // other sockets the upper half, and odd offsets come first.
                if high - low >= 4 {
                    let half = low + (((high - low) >> 2) << 1);
                    let (first, second) = if spec.reuseaddr {
                        ((low, half), (half, high))
                    } else {
                        ((half, high), (low, half))
                    };
                    scan(first.0, first.1, rng, Some(1), free)
                        .or_else(|| scan(second.0, second.1, rng, Some(1), free))
                } else {
                    scan(low, high, rng, Some(1), free)
                }
            }
        }
    }

    /// Implicit bind by `connect` (`__inet_hash_connect`): even offsets first, and a port may be
    /// shared with other connect-bound sockets as long as the remote differs. Exhaustion is
    /// `EADDRNOTAVAIL`.
    pub fn bind_connect(
        &mut self,
        id: SockId,
        spec: BindSpec,
        remote: SocketAddr,
        range: PortRange,
        rng: &mut PortRng,
        payload: T,
    ) -> NetResult<u16> {
        let usable = |p: u16| {
            self.buckets.get(&p).is_none_or(|bucket| {
                bucket.iter().all(|e| {
                    e.id == id
                        || !addrs_overlap(&spec, &e.spec)
                        || (e.connect_bound && e.remote != Some(remote))
                })
            })
        };
        let port = scan(
            range.low as u32,
            range.high as u32 + 1,
            rng,
            Some(0),
            usable,
        )
        .ok_or(Errno::EADDRNOTAVAIL)?;
        self.insert(id, spec, port, payload);
        let entry = self.entry_mut(id, port).expect("just inserted");
        entry.connect_bound = true;
        entry.remote = Some(remote);
        Ok(port)
    }

    fn entry_mut(&mut self, id: SockId, port: u16) -> Option<&mut Entry<T>> {
        self.buckets.get_mut(&port)?.iter_mut().find(|e| e.id == id)
    }

    /// Adds a socket that shares an existing socket's port without a conflict check, as an
    /// accepted connection inherits its listener's port (`inet_csk_clone_lock`/`__inet_inherit_port`).
    pub fn inherit(
        &mut self,
        id: SockId,
        spec: BindSpec,
        port: u16,
        remote: SocketAddr,
        payload: T,
    ) {
        self.insert(id, spec, port, payload);
        if let Some(e) = self.entry_mut(id, port) {
            e.remote = Some(remote);
        }
    }

    /// `listen` re-validates the port against listeners (`inet_csk_listen_start` calls
    /// `get_port` again): two `SO_REUSEADDR` sockets may share a port, but only one may listen.
    pub fn listen(&mut self, id: SockId, port: u16) -> NetResult<()> {
        let spec = self.entry_mut(id, port).ok_or(Errno::EINVAL)?.spec;
        if self.port_conflicts(id, &spec, port, true) {
            return Err(Errno::EADDRINUSE);
        }
        self.entry_mut(id, port).expect("checked").listening = true;
        Ok(())
    }

    pub fn set_listening(&mut self, id: SockId, port: u16, listening: bool) {
        if let Some(e) = self.entry_mut(id, port) {
            e.listening = listening;
        }
    }

    pub fn set_remote(&mut self, id: SockId, port: u16, remote: Option<SocketAddr>) {
        if let Some(e) = self.entry_mut(id, port) {
            e.remote = remote;
        }
    }

    /// Socket options that take part in conflicts changed after bind.
    pub fn set_reuse(&mut self, id: SockId, port: u16, reuseaddr: bool, reuseport: bool) {
        if let Some(e) = self.entry_mut(id, port) {
            e.spec.reuseaddr = reuseaddr;
            e.spec.reuseport = reuseport;
        }
    }

    pub fn unbind(&mut self, id: SockId, port: u16) {
        if let Some(bucket) = self.buckets.get_mut(&port) {
            bucket.retain(|e| e.id != id);
            if bucket.is_empty() {
                self.buckets.remove(&port);
            }
        }
    }

    pub fn is_bound(&self, port: u16) -> bool {
        self.buckets.contains_key(&port)
    }

    /// A wildcard-bound socket took a specific source address by connecting
    /// (`inet_bhash2_update_saddr`).
    pub fn rehash(&mut self, id: SockId, port: u16, addr: IpAddr) {
        if let Some(e) = self.entry_mut(id, port) {
            e.spec.addr = addr;
        }
    }

    /// Every entry, for `/proc/net/*`.
    pub fn entries(&self) -> impl Iterator<Item = (u16, &T)> {
        self.buckets
            .iter()
            .flat_map(|(port, b)| b.iter().map(move |e| (*port, &e.payload)))
    }

    /// Socket lookup for an incoming connection or datagram to `dst` from `src`, mirroring
    /// `compute_score`: IPv4 sockets beat dual-stack IPv6 ones, exact addresses beat wildcards,
    /// a connected (UDP) socket only matches its peer and beats unconnected ones. Returns the
    /// best-scoring payloads; more than one means a `SO_REUSEPORT` group.
    pub fn lookup(
        &self,
        dst: SocketAddr,
        src: Option<SocketAddr>,
        filter: impl Fn(&T, bool) -> bool,
    ) -> Vec<T> {
        let best = self.scored(dst, src, &filter).map(|(s, _)| s).max();
        self.scored(dst, src, &filter)
            .filter(|(s, _)| Some(*s) == best)
            .map(|(_, p)| p.clone())
            .collect()
    }

    /// `lookup` narrowed to one socket without allocating: a `SO_REUSEPORT` group is spread by
    /// `hash` (`reuseport_select_sock`).
    pub fn lookup_one(
        &self,
        dst: SocketAddr,
        src: Option<SocketAddr>,
        filter: impl Fn(&T, bool) -> bool,
        hash: usize,
    ) -> Option<T> {
        let (best, count) =
            self.scored(dst, src, &filter)
                .fold((0, 0), |(best, n), (s, _)| match s.cmp(&best) {
                    std::cmp::Ordering::Greater => (s, 1),
                    std::cmp::Ordering::Equal => (best, n + 1),
                    std::cmp::Ordering::Less => (best, n),
                });
        if count == 0 {
            return None;
        }
        self.scored(dst, src, &filter)
            .filter(|(s, _)| *s == best)
            .nth(hash % count)
            .map(|(_, p)| p.clone())
    }

    /// Every entry that can take traffic to `dst` from `src`, with its `compute_score`.
    fn scored<'a>(
        &'a self,
        dst: SocketAddr,
        src: Option<SocketAddr>,
        filter: &'a impl Fn(&T, bool) -> bool,
    ) -> impl Iterator<Item = (u32, &'a T)> + 'a {
        let probe = BindSpec {
            family: if dst.is_ipv4() {
                Domain::Inet
            } else {
                Domain::Inet6
            },
            addr: dst.ip(),
            v6only: false,
            reuseaddr: false,
            reuseport: false,
            uid: 0,
        };
        let bucket = self.buckets.get(&dst.port()).into_iter().flatten();
        bucket.filter_map(move |e| {
            if !filter(&e.payload, e.listening) || !addrs_overlap(&probe, &e.spec) {
                return None;
            }
            let mut score = if e.spec.family == Domain::Inet { 2 } else { 1 };
            if e.spec.addr == dst.ip() {
                score += 4;
            }
            if let Some(remote) = e.remote.filter(|_| !e.listening) {
                if Some(remote) != src {
                    return None;
                }
                score += 4;
            }
            Some((score, &e.payload))
        })
    }
}

/// Scans `[low, high)` from a random offset, trying `parity` offsets first when given.
fn scan(
    low: u32,
    high: u32,
    rng: &mut PortRng,
    parity: Option<u32>,
    mut free: impl FnMut(u16) -> bool,
) -> Option<u16> {
    let remaining = high.checked_sub(low).filter(|r| *r > 0)?;
    let offset = rng.below(remaining);
    let passes: &[u32] = match parity {
        Some(p) => &[p, p ^ 1],
        None => &[2],
    };
    for &pass in passes {
        for i in 0..remaining {
            let off = (offset + i) % remaining;
            if pass < 2 && off % 2 != pass {
                continue;
            }
            let port = (low + off) as u16;
            if port != 0 && free(port) {
                return Some(port);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use super::*;

    fn v4(a: [u8; 4]) -> BindSpec {
        BindSpec {
            family: Domain::Inet,
            addr: Ipv4Addr::from(a).into(),
            v6only: false,
            reuseaddr: false,
            reuseport: false,
            uid: 1,
        }
    }

    fn v6(a: Ipv6Addr, v6only: bool) -> BindSpec {
        BindSpec {
            family: Domain::Inet6,
            addr: a.into(),
            v6only,
            reuseaddr: false,
            reuseport: false,
            uid: 1,
        }
    }

    fn table(proto: Proto) -> (PortTable<()>, PortRng) {
        (PortTable::new(proto), PortRng::new(7))
    }

    const R: PortRange = PortRange {
        low: 32768,
        high: 60999,
    };

    #[test]
    fn same_port_conflicts_unless_addresses_differ() {
        let (mut t, mut rng) = table(Proto::Tcp);
        t.bind(1, v4([127, 0, 0, 1]), 80, R, &mut rng, ()).unwrap();
        assert_eq!(
            t.bind(2, v4([127, 0, 0, 1]), 80, R, &mut rng, ()),
            Err(Errno::EADDRINUSE)
        );
        assert_eq!(
            t.bind(3, v4([0, 0, 0, 0]), 80, R, &mut rng, ()),
            Err(Errno::EADDRINUSE)
        );
        t.bind(4, v4([10, 0, 0, 1]), 80, R, &mut rng, ()).unwrap();
    }

    #[test]
    fn dual_stack_wildcard_covers_ipv4_unless_v6only() {
        let (mut t, mut rng) = table(Proto::Tcp);
        t.bind(1, v6(Ipv6Addr::UNSPECIFIED, false), 80, R, &mut rng, ())
            .unwrap();
        assert_eq!(
            t.bind(2, v4([127, 0, 0, 1]), 80, R, &mut rng, ()),
            Err(Errno::EADDRINUSE)
        );
        t.bind(3, v6(Ipv6Addr::UNSPECIFIED, true), 81, R, &mut rng, ())
            .unwrap();
        t.bind(4, v4([0, 0, 0, 0]), 81, R, &mut rng, ()).unwrap();
        // A specific IPv6 address never collides with IPv4.
        t.bind(5, v6(Ipv6Addr::LOCALHOST, false), 82, R, &mut rng, ())
            .unwrap();
        t.bind(6, v4([0, 0, 0, 0]), 82, R, &mut rng, ()).unwrap();
        // ...but a dual-stack wildcard collides with the IPv4 wildcard it would cover.
        assert_eq!(
            t.bind(7, v6(Ipv6Addr::UNSPECIFIED, false), 82, R, &mut rng, ()),
            Err(Errno::EADDRINUSE)
        );
    }

    #[test]
    fn reuseaddr_shares_until_one_listens() {
        let (mut t, mut rng) = table(Proto::Tcp);
        let s = BindSpec {
            reuseaddr: true,
            ..v4([0, 0, 0, 0])
        };
        t.bind(1, s, 80, R, &mut rng, ()).unwrap();
        t.bind(2, s, 80, R, &mut rng, ()).unwrap();
        t.listen(1, 80).unwrap();
        assert_eq!(t.listen(2, 80), Err(Errno::EADDRINUSE));
        assert_eq!(t.bind(3, s, 80, R, &mut rng, ()), Err(Errno::EADDRINUSE));
        // Without SO_REUSEADDR on the newcomer there is no sharing at all.
        t.unbind(1, 80);
        assert_eq!(
            t.bind(4, v4([0, 0, 0, 0]), 80, R, &mut rng, ()),
            Err(Errno::EADDRINUSE)
        );
    }

    #[test]
    fn reuseport_needs_both_and_same_uid() {
        let (mut t, mut rng) = table(Proto::Tcp);
        let s = BindSpec {
            reuseport: true,
            ..v4([0, 0, 0, 0])
        };
        t.bind(1, s, 80, R, &mut rng, ()).unwrap();
        t.listen(1, 80).unwrap();
        t.bind(2, s, 80, R, &mut rng, ()).unwrap();
        t.listen(2, 80).unwrap();
        assert_eq!(
            t.bind(3, BindSpec { uid: 2, ..s }, 80, R, &mut rng, ()),
            Err(Errno::EADDRINUSE)
        );
        assert_eq!(
            t.bind(4, v4([0, 0, 0, 0]), 80, R, &mut rng, ()),
            Err(Errno::EADDRINUSE)
        );
    }

    #[test]
    fn udp_reuseaddr_shares_freely() {
        let (mut t, mut rng) = table(Proto::Udp);
        let s = BindSpec {
            reuseaddr: true,
            ..v4([0, 0, 0, 0])
        };
        t.bind(1, s, 53, R, &mut rng, ()).unwrap();
        t.bind(2, s, 53, R, &mut rng, ()).unwrap();
        assert_eq!(
            t.bind(3, v4([127, 0, 0, 1]), 53, R, &mut rng, ()),
            Err(Errno::EADDRINUSE)
        );
    }

    #[test]
    fn ephemeral_ports_stay_in_range_and_exhaust() {
        let (mut t, mut rng) = table(Proto::Tcp);
        let range = PortRange {
            low: 40000,
            high: 40003,
        };
        let mut seen = Vec::new();
        for id in 0..4 {
            let p = t
                .bind(id, v4([0, 0, 0, 0]), 0, range, &mut rng, ())
                .unwrap();
            assert!((40000..=40003).contains(&p));
            seen.push(p);
        }
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 4);
        assert_eq!(
            t.bind(9, v4([0, 0, 0, 0]), 0, range, &mut rng, ()),
            Err(Errno::EADDRINUSE)
        );
    }

    #[test]
    fn bind_autobind_prefers_odd_connect_prefers_even() {
        let (mut t, mut rng) = table(Proto::Tcp);
        for id in 0..20 {
            let p = t.bind(id, v4([0, 0, 0, 0]), 0, R, &mut rng, ()).unwrap();
            assert_eq!(p % 2, 1, "bind autobind uses odd ports first");
        }
        let remote: SocketAddr = "127.0.0.1:80".parse().unwrap();
        for id in 100..120 {
            let p = t
                .bind_connect(id, v4([127, 0, 0, 1]), remote, R, &mut rng, ())
                .unwrap();
            assert_eq!(p % 2, 0, "connect autobind uses even ports first");
        }
    }

    #[test]
    fn connect_bound_ports_are_shared_across_remotes() {
        let (mut t, mut rng) = table(Proto::Tcp);
        let range = PortRange {
            low: 40000,
            high: 40000,
        };
        let a: SocketAddr = "127.0.0.1:80".parse().unwrap();
        let b: SocketAddr = "127.0.0.1:81".parse().unwrap();
        let s = v4([127, 0, 0, 1]);
        assert_eq!(t.bind_connect(1, s, a, range, &mut rng, ()), Ok(40000));
        assert_eq!(t.bind_connect(2, s, b, range, &mut rng, ()), Ok(40000));
        assert_eq!(
            t.bind_connect(3, s, a, range, &mut rng, ()),
            Err(Errno::EADDRNOTAVAIL)
        );
    }

    #[test]
    fn lookup_prefers_exact_and_connected() {
        let (mut t, mut rng) = table(Proto::Udp);
        let mut tt: PortTable<u32> = PortTable::new(Proto::Udp);
        let _ = &mut t;
        let wild = BindSpec {
            reuseaddr: true,
            ..v4([0, 0, 0, 0])
        };
        let exact = BindSpec {
            reuseaddr: true,
            ..v4([127, 0, 0, 1])
        };
        tt.bind(1, wild, 53, R, &mut rng, 1).unwrap();
        tt.bind(2, exact, 53, R, &mut rng, 2).unwrap();
        let dst: SocketAddr = "127.0.0.1:53".parse().unwrap();
        let peer: SocketAddr = "127.0.0.1:9999".parse().unwrap();
        assert_eq!(tt.lookup(dst, Some(peer), |_, _| true), vec![2]);
        tt.bind(3, exact, 53, R, &mut rng, 3).unwrap();
        tt.set_remote(3, 53, Some(peer));
        assert_eq!(tt.lookup(dst, Some(peer), |_, _| true), vec![3]);
        let other: SocketAddr = "127.0.0.1:1".parse().unwrap();
        assert_eq!(tt.lookup(dst, Some(other), |_, _| true), vec![2]);
    }
}
