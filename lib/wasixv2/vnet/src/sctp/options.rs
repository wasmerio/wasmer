//! The `SOL_SCTP` options (`sctp_setsockopt`, `sctp_getsockopt`). A `getsockopt` value
//! arrives holding the caller's bytes, since most of them name an association or an address
//! in the value they return.

use std::{net::SocketAddr, sync::Arc};

use super::*;

/// `len` must be exactly `size` (most `setsockopt`s).
fn exact(value: &[u8], size: usize) -> NetResult<()> {
    match value.len() == size {
        true => Ok(()),
        false => Err(Errno::EINVAL),
    }
}

/// A record of exactly its size.
fn record<T: GuestLayout>(value: &[u8]) -> NetResult<T> {
    exact(value, T::SIZE)?;
    Ok(T::read_from(value).expect("of its size"))
}

/// An `int` option value: at least one `int`.
fn int(value: &[u8]) -> NetResult<i32> {
    opts::read_int(value)
}

/// The request a `getsockopt` value holds, which must be at least the record (`EINVAL`).
fn request<T: GuestLayout>(out: &[u8]) -> NetResult<T> {
    T::read_from(out).ok_or(Errno::EINVAL)
}

fn put<T: GuestLayout>(out: &mut [u8], value: &T) -> usize {
    value.write_to(out);
    T::SIZE
}

/// An `int` `getsockopt` value (`len < sizeof(int)` is `EINVAL`).
fn put_int(out: &mut [u8], value: i32) -> NetResult<usize> {
    if out.len() < 4 {
        return Err(Errno::EINVAL);
    }
    Ok(opts::write_int(out, value))
}

/// A `struct sockaddr_storage` holding `addr` in `family`'s layout.
pub(super) fn storage(family: Domain, addr: SocketAddr) -> SockaddrStorage {
    let mut raw = [0; SockaddrStorage::SIZE];
    let bytes = sockaddr_bytes(family, Some(addr));
    raw[..bytes.len()].copy_from_slice(&bytes);
    SockaddrStorage::read_from(&raw).expect("of its size")
}

fn storage_addr(storage: &SockaddrStorage) -> Option<SocketAddr> {
    let mut raw = [0; SockaddrStorage::SIZE];
    storage.write_to(&mut raw);
    parse_sockaddr(&raw)
}

impl SctpSocket {
    /// `sctp_addr_id2transport`: the association with the peer address `addr`, which must be
    /// the one `id` names if that names one.
    fn addr_assoc(
        &self,
        g: &Inner,
        addr: &SockaddrStorage,
        id: i32,
    ) -> Option<(Arc<Assoc>, SocketAddr)> {
        let addr = canonical(storage_addr(addr)?);
        self.family_ok(g, addr).ok()?;
        let asoc = self.lookup_assoc(g, addr)?;
        match self.id2assoc(g, id) {
            Some(by_id) if !Arc::ptr_eq(&by_id, &asoc) => None,
            _ => Some((asoc, addr)),
        }
    }

    /// Applies `f` to the socket or associations an option's id names
    /// (`SCTP_FUTURE_ASSOC`, `SCTP_CURRENT_ASSOC`, `SCTP_ALL_ASSOC` or one association): a
    /// one-to-one socket's is always the socket and its association.
    fn for_assocs(
        &self,
        g: &mut Inner,
        id: i32,
        sock: impl FnOnce(&mut SctpOpts),
        mut asoc: impl FnMut(&mut AssocInner),
    ) -> NetResult<()> {
        if let Some(a) = self.option_assoc(g, id, SCTP_ALL_ASSOC)? {
            asoc(&mut a.lock());
            return Ok(());
        }
        let id = match g.style {
            Style::Tcp => SCTP_FUTURE_ASSOC,
            _ => id,
        };
        if id == SCTP_FUTURE_ASSOC || id == SCTP_ALL_ASSOC {
            sock(&mut g.so);
        }
        if id == SCTP_CURRENT_ASSOC || id == SCTP_ALL_ASSOC {
            for a in &g.asocs {
                asoc(&mut a.lock());
            }
        }
        Ok(())
    }

    /// `sctp_setsockopt`.
    pub(super) fn set_sctp_option(&self, name: i32, value: &[u8]) -> NetResult<()> {
        let mut g = self.lock();
        let queued = self.set_locked(&mut g, name, value)?;
        drop(g);
        self.rearm_idle();
        if queued {
            self.wq.wake(PollMask::IN | PollMask::RDNORM);
        }
        Ok(())
    }

    /// Sets an option with the socket locked; returns whether it queued a notification.
    fn set_locked(&self, g: &mut Inner, name: i32, value: &[u8]) -> NetResult<bool> {
        let mut queued = false;
        match SctpOption(name) {
            SctpOption::SCTP_RTOINFO => {
                let r: SctpRtoinfo = record(value)?;
                let asoc = self.option_assoc(g, r.srto_assoc_id, SCTP_FUTURE_ASSOC)?;
                let current = match &asoc {
                    Some(a) => a.lock().rto,
                    None => g.so.rto,
                };
                let max = if r.srto_max != 0 {
                    r.srto_max
                } else {
                    current[1]
                };
                let min = if r.srto_min != 0 {
                    r.srto_min
                } else {
                    current[2]
                };
                if min > max {
                    return Err(Errno::EINVAL);
                }
                let initial = if r.srto_initial != 0 {
                    r.srto_initial
                } else {
                    current[0]
                };
                match asoc {
                    Some(a) => a.lock().rto = [initial, max, min],
                    None => g.so.rto = [initial, max, min],
                }
            }
            SctpOption::SCTP_ASSOCINFO => {
                let p: SctpAssocparams = record(value)?;
                match self.option_assoc(g, p.sasoc_assoc_id, SCTP_FUTURE_ASSOC)? {
                    Some(a) => {
                        let mut a = a.lock();
                        if p.sasoc_asocmaxrxt != 0 {
                            // With more than one path, at most the paths' retransmissions
                            // together (`pathmaxrxt`, 5 each).
                            let paths = a.paddrs.len();
                            if paths > 1
                                && usize::from(p.sasoc_asocmaxrxt)
                                    > a.paddrs
                                        .iter()
                                        .map(|addr| {
                                            usize::from(
                                                a.paths.get(addr).unwrap_or(&a.path).retrans,
                                            )
                                        })
                                        .sum()
                            {
                                return Err(Errno::EINVAL);
                            }
                            a.max_retrans = p.sasoc_asocmaxrxt;
                        }
                        if p.sasoc_cookie_life != 0 {
                            a.cookie_life = p.sasoc_cookie_life;
                        }
                    }
                    None => {
                        if p.sasoc_asocmaxrxt != 0 {
                            g.so.asocmaxrxt = p.sasoc_asocmaxrxt;
                        }
                        if p.sasoc_cookie_life != 0 {
                            g.so.cookie_life = p.sasoc_cookie_life;
                        }
                    }
                }
            }
            SctpOption::SCTP_INITMSG => {
                let i: SctpInitmsg = record(value)?;
                let m = &mut g.so.initmsg;
                if i.sinit_num_ostreams != 0 {
                    m.sinit_num_ostreams = i.sinit_num_ostreams;
                }
                if i.sinit_max_instreams != 0 {
                    m.sinit_max_instreams = i.sinit_max_instreams;
                }
                if i.sinit_max_attempts != 0 {
                    m.sinit_max_attempts = i.sinit_max_attempts;
                }
                if i.sinit_max_init_timeo != 0 {
                    m.sinit_max_init_timeo = i.sinit_max_init_timeo;
                }
            }
            SctpOption::SCTP_NODELAY => g.so.nodelay = int(value)? != 0,
            SctpOption::SCTP_DELAYED_ACK_TIME => {
                // net/sctp/socket.c: sctp_setsockopt_delayed_ack (including legacy layout).
                let p = if value.len() == SctpAssocValue::SIZE {
                    let v: SctpAssocValue = record(value)?;
                    SctpSackInfo {
                        sack_assoc_id: v.assoc_id,
                        sack_delay: v.assoc_value,
                        sack_freq: u32::from(v.assoc_value == 0),
                    }
                } else {
                    record::<SctpSackInfo>(value)?
                };
                if p.sack_delay > 500 {
                    return Err(Errno::EINVAL);
                }
                if p.sack_delay == 0 && p.sack_freq == 0 {
                    return Ok(false);
                }
                self.for_assocs(
                    g,
                    p.sack_assoc_id,
                    |so| so.path.sack(&p),
                    |a| {
                        a.path.sack(&p);
                        for path in a.paths.values_mut() {
                            path.sack(&p);
                        }
                    },
                )?;
            }
            SctpOption::SCTP_PEER_ADDR_PARAMS => {
                self.set_peer_params(g, value)?;
            }
            SctpOption::SCTP_AUTOCLOSE => {
                if g.style == Style::Tcp {
                    return Err(Errno::EOPNOTSUPP);
                }
                exact(value, 4)?;
                g.so.autoclose = (int(value)? as u32).min(MAX_AUTOCLOSE);
            }
            SctpOption::SCTP_PRIMARY_ADDR => {
                let p: SctpPrim = record(value)?;
                let (asoc, addr) = self
                    .addr_assoc(g, &p.ssp_addr, p.ssp_assoc_id)
                    .ok_or(Errno::EINVAL)?;
                asoc.lock().primary = addr;
                // net/sctp/associola.c: sctp_assoc_set_primary /
                // net/sctp/ulpevent.c: sctp_ulpevent_notify_peer_addr_change.
                if subscribed(asoc.lock().subscribe, SctpSnType::SCTP_PEER_ADDR_CHANGE.0) {
                    let data = guest_bytes(&SctpPaddrChange {
                        spc_type: SctpSnType::SCTP_PEER_ADDR_CHANGE.0,
                        spc_flags: 0,
                        spc_length: SctpPaddrChange::SIZE as u32,
                        spc_aaddr: storage(self.domain, addr),
                        spc_state: 4,
                        spc_error: 0,
                        spc_assoc_id: asoc.id,
                    });
                    g.rcvq.push_back(Event {
                        asoc,
                        body: Body::Notification,
                        data,
                        read: 0,
                        partial_delivery: false,
                    });
                    queued = true;
                }
            }
            // `sctp_setsockopt_peer_primary_addr` needs ASCONF, which is off
            // (`net.sctp.addip_enable`).
            SctpOption::SCTP_SET_PEER_PRIMARY_ADDR => return Err(Errno::EPERM),
            SctpOption::SCTP_ADAPTATION_LAYER => {
                exact(value, 4)?;
                g.so.adaptation_ind = int(value)? as u32;
            }
            SctpOption::SCTP_DISABLE_FRAGMENTS => g.so.disable_fragments = int(value)? != 0,
            SctpOption::SCTP_DEFAULT_SEND_PARAM => {
                let i: SctpSndrcvinfo = record(value)?;
                if i.sinfo_flags & !DEFAULT_FLAGS != 0 {
                    return Err(Errno::EINVAL);
                }
                let d = Defaults {
                    stream: i.sinfo_stream,
                    flags: i.sinfo_flags,
                    ppid: i.sinfo_ppid,
                    context: i.sinfo_context,
                    timetolive: i.sinfo_timetolive,
                };
                self.for_assocs(g, i.sinfo_assoc_id, |s| s.defaults = d, |a| a.defaults = d)?;
            }
            SctpOption::SCTP_DEFAULT_SNDINFO => {
                let i: SctpSndinfo = record(value)?;
                if i.snd_flags & !DEFAULT_FLAGS != 0 {
                    return Err(Errno::EINVAL);
                }
                let set = |d: &mut Defaults| {
                    d.stream = i.snd_sid;
                    d.flags = i.snd_flags;
                    d.ppid = i.snd_ppid;
                    d.context = i.snd_context;
                };
                self.for_assocs(
                    g,
                    i.snd_assoc_id,
                    |s| set(&mut s.defaults),
                    |a| set(&mut a.defaults),
                )?;
            }
            SctpOption::SCTP_EVENTS => {
                if value.len() > SctpEventSubscribe::SIZE {
                    return Err(Errno::EINVAL);
                }
                for (i, on) in value.iter().enumerate() {
                    subscribe_set(&mut g.so.subscribe, SN_TYPE_BASE + i as u16, *on != 0);
                }
                for a in &g.asocs {
                    a.lock().subscribe = g.so.subscribe;
                }
                let dry = SctpSnType::SCTP_SENDER_DRY_EVENT.0;
                if subscribed(g.so.subscribe, dry)
                    && let Some(asoc) = self.id2assoc(g, 0)
                {
                    queued |= Self::dry_now(g, &asoc);
                }
            }
            SctpOption::SCTP_EVENT => {
                if value.len() < SctpEvent::SIZE {
                    return Err(Errno::EINVAL);
                }
                let e = SctpEvent::read_from(value).expect("long enough");
                if !(SN_TYPE_BASE..=SN_TYPE_MAX).contains(&e.se_type) {
                    return Err(Errno::EINVAL);
                }
                let on = e.se_on != 0;
                let dry = e.se_type == SctpSnType::SCTP_SENDER_DRY_EVENT.0 && on;
                let mut touched = Vec::new();
                self.for_assocs(
                    g,
                    e.se_assoc_id,
                    |s| subscribe_set(&mut s.subscribe, e.se_type, on),
                    |a| subscribe_set(&mut a.subscribe, e.se_type, on),
                )?;
                // `sctp_assoc_ulpevent_type_set`: a dry sender says so at once.
                if dry {
                    match self.id2assoc(g, e.se_assoc_id) {
                        Some(a) => touched.push(a),
                        None if e.se_assoc_id == SCTP_CURRENT_ASSOC
                            || e.se_assoc_id == SCTP_ALL_ASSOC =>
                        {
                            touched.extend(g.asocs.iter().cloned())
                        }
                        None => {}
                    }
                }
                for asoc in touched {
                    queued |= Self::dry_now(g, &asoc);
                }
            }
            SctpOption::SCTP_I_WANT_MAPPED_V4_ADDR => g.so.v4mapped = int(value)? != 0,
            SctpOption::SCTP_MAXSEG => {
                let (id, val) = match value.len() {
                    4 => (SCTP_FUTURE_ASSOC, int(value)?),
                    8 => {
                        let v: SctpAssocValue = record(value)?;
                        (v.assoc_id, v.assoc_value as i32)
                    }
                    _ => return Err(Errno::EINVAL),
                };
                let asoc = self.option_assoc(g, id, SCTP_FUTURE_ASSOC)?;
                if val != 0 {
                    // `sctp_min_frag_point` up to `SCTP_MAX_CHUNK_LEN` less the DATA header.
                    let net_hdr = match self.domain {
                        Domain::Inet => 20,
                        Domain::Inet6 => 40,
                    };
                    let min = (MIN_SEGMENT - net_hdr - SCTP_HDR - DATA_CHUNK) as i32;
                    let max = (MAX_CHUNK_LEN - DATA_CHUNK) as i32;
                    if val < min || val > max {
                        return Err(Errno::EINVAL);
                    }
                }
                match asoc {
                    Some(a) => a.lock().user_frag = val as u32,
                    None => g.so.user_frag = val as u32,
                }
            }
            SctpOption::SCTP_CONTEXT => {
                let v: SctpAssocValue = record(value)?;
                let ctx = v.assoc_value;
                self.for_assocs(
                    g,
                    v.assoc_id,
                    |s| s.rcv_context = ctx,
                    |a| a.rcv_context = ctx,
                )?;
            }
            SctpOption::SCTP_FRAGMENT_INTERLEAVE => {
                exact(value, 4)?;
                g.so.frag_interleave = int(value)? != 0;
            }
            SctpOption::SCTP_PARTIAL_DELIVERY_POINT => {
                exact(value, 4)?;
                let val = int(value)? as u32;
                if val as usize > g.opts.rcvbuf >> 1 {
                    return Err(Errno::EINVAL);
                }
                g.so.pd_point = val;
            }
            SctpOption::SCTP_MAX_BURST => {
                let (id, val) = match value.len() {
                    4 => (SCTP_FUTURE_ASSOC, int(value)? as u32),
                    8 => {
                        let v: SctpAssocValue = record(value)?;
                        (v.assoc_id, v.assoc_value)
                    }
                    _ => return Err(Errno::EINVAL),
                };
                self.for_assocs(g, id, |s| s.max_burst = val, |a| a.max_burst = val)?;
            }
            SctpOption::SCTP_RECVRCVINFO => g.so.recvrcvinfo = int(value)? != 0,
            SctpOption::SCTP_RECVNXTINFO => g.so.recvnxtinfo = int(value)? != 0,
            SctpOption::SCTP_REUSE_PORT => {
                if g.style != Style::Tcp {
                    return Err(Errno::EOPNOTSUPP);
                }
                if g.port != 0 {
                    return Err(Errno::EFAULT);
                }
                g.so.reuse = int(value)? != 0;
            }
            _ => return Err(Errno::ENOPROTOOPT),
        }
        Ok(queued)
    }

    /// Queues `SCTP_SENDER_DRY_EVENT` for `asoc` if nothing it sent waits outside the peer's
    /// window, straight into the queue of the socket the caller holds locked. Returns whether
    /// it did, so the caller wakes readers once it lets go.
    fn dry_now(g: &mut Inner, asoc: &Arc<Assoc>) -> bool {
        let dry = asoc.peer().is_none_or(|b| b.lock().unacked() == 0);
        let wanted = subscribed(asoc.lock().subscribe, SctpSnType::SCTP_SENDER_DRY_EVENT.0);
        if !dry || !wanted || g.rd_shut {
            return false;
        }
        g.rcvq.push_back(Event {
            asoc: asoc.clone(),
            body: Body::Notification,
            data: sender_dry(asoc),
            read: 0,
            partial_delivery: false,
        });
        true
    }

    /// `sctp_getsockopt`.
    pub(super) fn get_sctp_option(&self, name: i32, out: &mut [u8]) -> NetResult<usize> {
        let g = self.lock();
        let len = out.len();
        match SctpOption(name) {
            SctpOption::SCTP_STATUS => {
                let req: SctpStatus = request(out)?;
                let asoc = self.id2assoc(&g, req.sstat_assoc_id).ok_or(Errno::EINVAL)?;
                let status = self.status(&g, &asoc);
                Ok(put(out, &status))
            }
            SctpOption::SCTP_GET_PEER_ADDR_INFO => {
                let req: SctpPaddrinfo = request(out)?;
                let (asoc, addr) = self
                    .addr_assoc(&g, &req.spinfo_address, req.spinfo_assoc_id)
                    .ok_or(Errno::EINVAL)?;
                let info = self.paddrinfo(&g, &asoc, addr);
                Ok(put(out, &info))
            }
            SctpOption::SCTP_DISABLE_FRAGMENTS => put_int(out, g.so.disable_fragments as i32),
            SctpOption::SCTP_EVENTS => {
                if len == 0 {
                    return Err(Errno::EINVAL);
                }
                let n = len.min(SctpEventSubscribe::SIZE);
                for (i, byte) in out[..n].iter_mut().enumerate() {
                    *byte = subscribed(g.so.subscribe, SN_TYPE_BASE + i as u16) as u8;
                }
                Ok(n)
            }
            SctpOption::SCTP_DELAYED_ACK_TIME => {
                let legacy = out.len() == SctpAssocValue::SIZE;
                let id = i32::from_le_bytes(out.get(..4).ok_or(Errno::EINVAL)?.try_into().unwrap());
                let path = match self.option_assoc(&g, id, SCTP_FUTURE_ASSOC)? {
                    Some(a) => a.lock().path,
                    None => g.so.path,
                };
                let enabled = path.flags & 32 != 0;
                if legacy {
                    Ok(put(
                        out,
                        &SctpAssocValue {
                            assoc_id: id,
                            assoc_value: if enabled { path.sack_delay } else { 0 },
                        },
                    ))
                } else {
                    if out.len() < SctpSackInfo::SIZE {
                        return Err(Errno::EINVAL);
                    }
                    Ok(put(
                        out,
                        &SctpSackInfo {
                            sack_assoc_id: id,
                            sack_delay: path.sack_delay,
                            sack_freq: if enabled { path.sack_freq } else { 1 },
                        },
                    ))
                }
            }
            SctpOption::SCTP_PEER_ADDR_PARAMS => self.get_peer_params(&g, out),
            SctpOption::SCTP_AUTOCLOSE => {
                if g.style == Style::Tcp {
                    return Err(Errno::EOPNOTSUPP);
                }
                put_int(out, g.so.autoclose as i32)
            }
            SctpOption::SCTP_INITMSG => {
                if len < SctpInitmsg::SIZE {
                    return Err(Errno::EINVAL);
                }
                Ok(put(out, &g.so.initmsg))
            }
            SctpOption::SCTP_GET_PEER_ADDRS => {
                let req: SctpGetaddrs = request(out)?;
                let asoc = self.id2assoc(&g, req.assoc_id).ok_or(Errno::EINVAL)?;
                let addrs = asoc.lock().paddrs.clone();
                self.put_addrs(&g, out, &addrs)
            }
            SctpOption::SCTP_GET_LOCAL_ADDRS => {
                let req: SctpGetaddrs = request(out)?;
                let addrs: Vec<SocketAddr> = match req.assoc_id {
                    0 => {
                        let wildcard = matches!(g.bound.as_slice(), [one] if one.is_unspecified());
                        let ips = match wildcard {
                            // `sctp_copy_laddrs`: every local address the socket's family
                            // takes.
                            true => local_addrs(&self.net)
                                .into_iter()
                                .filter(|a| match self.domain {
                                    Domain::Inet => a.is_ipv4(),
                                    Domain::Inet6 => !(g.opts.v6only && a.is_ipv4()),
                                })
                                .collect(),
                            false => g.bound.clone(),
                        };
                        ips.into_iter()
                            .map(|ip| SocketAddr::new(ip, g.port))
                            .collect()
                    }
                    id => {
                        let asoc = self.id2assoc(&g, id).ok_or(Errno::EINVAL)?;
                        let a = asoc.lock();
                        a.laddrs
                            .iter()
                            .map(|ip| SocketAddr::new(*ip, a.lport))
                            .collect()
                    }
                };
                self.put_addrs(&g, out, &addrs)
            }
            SctpOption::SCTP_PRIMARY_ADDR => {
                let req: SctpPrim = request(out)?;
                let asoc = self.id2assoc(&g, req.ssp_assoc_id).ok_or(Errno::EINVAL)?;
                let primary = asoc.lock().primary;
                let prim = SctpPrim {
                    ssp_assoc_id: req.ssp_assoc_id,
                    ssp_addr: storage(self.user_family(&g, primary), primary),
                };
                Ok(put(out, &prim))
            }
            SctpOption::SCTP_ADAPTATION_LAYER => put_int(out, g.so.adaptation_ind as i32),
            SctpOption::SCTP_DEFAULT_SEND_PARAM => {
                let req: SctpSndrcvinfo = request(out)?;
                let asoc = self.option_assoc(&g, req.sinfo_assoc_id, SCTP_FUTURE_ASSOC)?;
                let d = asoc.map_or(g.so.defaults, |a| a.lock().defaults);
                let info = SctpSndrcvinfo {
                    sinfo_stream: d.stream,
                    sinfo_flags: d.flags,
                    sinfo_ppid: d.ppid,
                    sinfo_context: d.context,
                    sinfo_timetolive: d.timetolive,
                    ..req
                };
                Ok(put(out, &info))
            }
            SctpOption::SCTP_DEFAULT_SNDINFO => {
                let req: SctpSndinfo = request(out)?;
                let asoc = self.option_assoc(&g, req.snd_assoc_id, SCTP_FUTURE_ASSOC)?;
                let d = asoc.map_or(g.so.defaults, |a| a.lock().defaults);
                let info = SctpSndinfo {
                    snd_sid: d.stream,
                    snd_flags: d.flags,
                    snd_ppid: d.ppid,
                    snd_context: d.context,
                    ..req
                };
                Ok(put(out, &info))
            }
            SctpOption::SCTP_NODELAY => put_int(out, g.so.nodelay as i32),
            SctpOption::SCTP_RTOINFO => {
                let req: SctpRtoinfo = request(out)?;
                let asoc = self.option_assoc(&g, req.srto_assoc_id, SCTP_FUTURE_ASSOC)?;
                let [initial, max, min] = asoc.map_or(g.so.rto, |a| a.lock().rto);
                let info = SctpRtoinfo {
                    srto_initial: initial,
                    srto_max: max,
                    srto_min: min,
                    ..req
                };
                Ok(put(out, &info))
            }
            SctpOption::SCTP_ASSOCINFO => {
                let req: SctpAssocparams = request(out)?;
                let asoc = self.option_assoc(&g, req.sasoc_assoc_id, SCTP_FUTURE_ASSOC)?;
                let info = match asoc {
                    Some(asoc) => {
                        let peer_rwnd = asoc.peer().map_or(0, |b| b.lock().rwnd());
                        let a = asoc.lock();
                        SctpAssocparams {
                            sasoc_asocmaxrxt: a.max_retrans,
                            sasoc_number_peer_destinations: a.paddrs.len() as u16,
                            sasoc_peer_rwnd: peer_rwnd as u32,
                            sasoc_local_rwnd: a.rwnd() as u32,
                            sasoc_cookie_life: a.cookie_life,
                            ..req
                        }
                    }
                    None => SctpAssocparams {
                        sasoc_asocmaxrxt: g.so.asocmaxrxt,
                        sasoc_number_peer_destinations: 0,
                        sasoc_peer_rwnd: 0,
                        sasoc_local_rwnd: 0,
                        sasoc_cookie_life: g.so.cookie_life,
                        ..req
                    },
                };
                Ok(put(out, &info))
            }
            SctpOption::SCTP_I_WANT_MAPPED_V4_ADDR => put_int(out, g.so.v4mapped as i32),
            SctpOption::SCTP_CONTEXT => {
                let req: SctpAssocValue = request(out)?;
                let asoc = self.option_assoc(&g, req.assoc_id, SCTP_FUTURE_ASSOC)?;
                let ctx = asoc.map_or(g.so.rcv_context, |a| a.lock().rcv_context);
                Ok(put(
                    out,
                    &SctpAssocValue {
                        assoc_value: ctx,
                        ..req
                    },
                ))
            }
            SctpOption::SCTP_MAXSEG => {
                let req = match len {
                    4 => SctpAssocValue {
                        assoc_id: SCTP_FUTURE_ASSOC,
                        assoc_value: 0,
                    },
                    _ => request(out)?,
                };
                let asoc = self.option_assoc(&g, req.assoc_id, SCTP_FUTURE_ASSOC)?;
                let value = asoc.map_or(g.so.user_frag, |a| a.lock().frag_point());
                match len {
                    4 => Ok(opts::write_int(out, value as i32)),
                    _ => Ok(put(
                        out,
                        &SctpAssocValue {
                            assoc_value: value,
                            ..req
                        },
                    )),
                }
            }
            SctpOption::SCTP_FRAGMENT_INTERLEAVE => put_int(out, g.so.frag_interleave as i32),
            SctpOption::SCTP_PARTIAL_DELIVERY_POINT => put_int(out, g.so.pd_point as i32),
            SctpOption::SCTP_MAX_BURST => {
                let req = match len {
                    4 => SctpAssocValue {
                        assoc_id: SCTP_FUTURE_ASSOC,
                        assoc_value: 0,
                    },
                    _ => request(out)?,
                };
                let asoc = self.option_assoc(&g, req.assoc_id, SCTP_FUTURE_ASSOC)?;
                let value = asoc.map_or(g.so.max_burst, |a| a.lock().max_burst);
                match len {
                    4 => Ok(opts::write_int(out, value as i32)),
                    _ => Ok(put(
                        out,
                        &SctpAssocValue {
                            assoc_value: value,
                            ..req
                        },
                    )),
                }
            }
            SctpOption::SCTP_GET_ASSOC_NUMBER => {
                if g.style == Style::Tcp {
                    return Err(Errno::EOPNOTSUPP);
                }
                put_int(out, g.asocs.len() as i32)
            }
            SctpOption::SCTP_GET_ASSOC_ID_LIST => {
                if g.style == Style::Tcp {
                    return Err(Errno::EOPNOTSUPP);
                }
                let need = 4 + 4 * g.asocs.len();
                if len < 4 || len < need {
                    return Err(Errno::EINVAL);
                }
                opts::write_int(out, g.asocs.len() as i32);
                for (i, a) in g.asocs.iter().enumerate() {
                    out[4 + 4 * i..8 + 4 * i].copy_from_slice(&a.id.to_le_bytes());
                }
                Ok(need)
            }
            SctpOption::SCTP_RECVRCVINFO => put_int(out, g.so.recvrcvinfo as i32),
            SctpOption::SCTP_RECVNXTINFO => put_int(out, g.so.recvnxtinfo as i32),
            SctpOption::SCTP_REUSE_PORT => put_int(out, g.so.reuse as i32),
            SctpOption::SCTP_EVENT => {
                let req: SctpEvent = request(out)?;
                if !(SN_TYPE_BASE..=SN_TYPE_MAX).contains(&req.se_type) {
                    return Err(Errno::EINVAL);
                }
                let asoc = self.option_assoc(&g, req.se_assoc_id, SCTP_FUTURE_ASSOC)?;
                let subscribe = asoc.map_or(g.so.subscribe, |a| a.lock().subscribe);
                let on = subscribed(subscribe, req.se_type) as u8;
                Ok(put(out, &SctpEvent { se_on: on, ..req }))
            }
            _ => Err(Errno::ENOPROTOOPT),
        }
    }

    /// `SCTP_GET_PEER_ADDRS`/`SCTP_GET_LOCAL_ADDRS`: the count over the header, the packed
    /// addresses after it, `ENOMEM` past the buffer; returns the bytes used.
    fn put_addrs(&self, g: &Inner, out: &mut [u8], addrs: &[SocketAddr]) -> NetResult<usize> {
        let mut at = SctpGetaddrs::SIZE;
        for addr in addrs {
            let raw = sockaddr_bytes(self.user_family(g, *addr), Some(*addr));
            let dst = out.get_mut(at..at + raw.len()).ok_or(Errno::ENOMEM)?;
            dst.copy_from_slice(&raw);
            at += raw.len();
        }
        out[4..8].copy_from_slice(&(addrs.len() as u32).to_le_bytes());
        Ok(at)
    }

    /// `struct sctp_paddrinfo` of a peer address: always active, as there are no heartbeats.
    fn paddrinfo(&self, g: &Inner, asoc: &Assoc, addr: SocketAddr) -> SctpPaddrinfo {
        let a = asoc.lock();
        // `sctp_transport_route`: the initial window of the path MTU (RFC 4960 7.2.1).
        let cwnd = (4 * a.pmtu).min((2 * a.pmtu).max(4380));
        SctpPaddrinfo {
            spinfo_assoc_id: asoc.id,
            spinfo_address: storage(self.user_family(g, addr), addr),
            spinfo_state: SctpSpinfoState::SCTP_ACTIVE.0,
            spinfo_cwnd: cwnd,
            spinfo_srtt: 0,
            spinfo_rto: a.rto[0],
            spinfo_mtu: a.pmtu,
        }
    }

    /// `sctp_getsockopt_sctp_status`.
    fn status(&self, g: &Inner, asoc: &Arc<Assoc>) -> SctpStatus {
        let peer_rwnd = asoc.peer().map_or(0, |b| b.lock().rwnd());
        let (state, instrms, outstrms, frag, primary) = {
            let a = asoc.lock();
            (a.state, a.incnt, a.outcnt, a.frag_point(), a.primary)
        };
        SctpStatus {
            sstat_assoc_id: asoc.id,
            // `sctp_assoc_to_state`: the UAPI numbering starts with `SCTP_EMPTY`.
            sstat_state: i32::from(state.internal()) + 1,
            sstat_rwnd: peer_rwnd as u32,
            sstat_unackdata: 0,
            sstat_penddata: 0,
            sstat_instrms: instrms,
            sstat_outstrms: outstrms,
            sstat_fragmentation_point: frag,
            sstat_primary: self.paddrinfo(g, asoc, primary),
        }
    }
}

impl PeerParams {
    fn sack(&mut self, p: &SctpSackInfo) {
        if p.sack_delay != 0 {
            self.sack_delay = p.sack_delay;
            self.flags = (self.flags & !96) | 32;
        }
        if p.sack_freq == 1 {
            self.flags = (self.flags & !96) | 64;
        } else if p.sack_freq > 1 {
            self.sack_freq = p.sack_freq;
            self.flags = (self.flags & !96) | 32;
        }
    }

    /// net/sctp/socket.c: sctp_apply_peer_addr_params. The message transport has no
    /// packet loss, but keeps the endpoint/association/path settings independently.
    fn apply(&mut self, p: &SctpPaddrparams, ipv6: bool) {
        let flags = u32::from_le_bytes(p.spp_flags);
        if flags & 1 != 0 && (p.spp_hbinterval != 0 || flags & 128 != 0) {
            self.hb = if flags & 128 != 0 {
                0
            } else {
                p.spp_hbinterval
            };
        }
        let mtu = u32::from_le_bytes(p.spp_pathmtu);
        if flags & 16 != 0 && mtu != 0 {
            self.mtu = mtu;
        }
        let delay = u32::from_le_bytes(p.spp_sackdelay);
        if flags & 32 != 0 && delay != 0 {
            self.sack_delay = delay;
        }
        for mask in [3, 24, 96] {
            if flags & mask != 0 {
                self.flags = (self.flags & !mask) | (flags & mask);
            }
        }
        if p.spp_pathmaxrxt != 0 {
            self.retrans = p.spp_pathmaxrxt;
        }
        if flags & 256 != 0 && ipv6 {
            self.flowlabel = u32::from_le_bytes(p.spp_ipv6_flowlabel) & 0xfffff;
            self.flags |= 256;
        }
        if flags & 512 != 0 {
            self.dscp = p.spp_dscp & 0xfc;
            self.flags |= 512;
        }
    }

    fn write(self, mut p: SctpPaddrparams) -> SctpPaddrparams {
        p.spp_hbinterval = self.hb;
        p.spp_pathmaxrxt = self.retrans;
        p.spp_pathmtu = self.mtu.to_le_bytes();
        p.spp_sackdelay = self.sack_delay.to_le_bytes();
        p.spp_flags = self.flags.to_le_bytes();
        p.spp_ipv6_flowlabel = self.flowlabel.to_le_bytes();
        p.spp_dscp = self.dscp;
        p
    }
}

type PeerParamsTarget = Option<(Arc<Assoc>, Option<SocketAddr>)>;

impl SctpSocket {
    fn peer_params_request(
        &self,
        g: &Inner,
        value: &[u8],
    ) -> NetResult<(SctpPaddrparams, PeerParamsTarget)> {
        let mut bytes = vec![0; SctpPaddrparams::SIZE];
        if value.len() < 152 {
            return Err(Errno::EINVAL);
        }
        let n = value.len().min(bytes.len());
        bytes[..n].copy_from_slice(&value[..n]);
        let p = SctpPaddrparams::read_from(&bytes).unwrap();
        let addr = storage_addr(&p.spp_address);
        let specific = addr.is_some_and(|a| !a.ip().is_unspecified());
        let path = if specific {
            let (a, addr) = self
                .addr_assoc(g, &p.spp_address, p.spp_assoc_id)
                .ok_or(Errno::EINVAL)?;
            Some((a, Some(addr)))
        } else {
            let a = self.id2assoc(g, p.spp_assoc_id);
            if a.is_none() && p.spp_assoc_id != SCTP_FUTURE_ASSOC && g.style == Style::Udp {
                return Err(Errno::EINVAL);
            }
            a.map(|a| (a, None))
        };
        Ok((p, path))
    }

    fn set_peer_params(&self, g: &mut Inner, value: &[u8]) -> NetResult<()> {
        if value.len() != 152 && value.len() != SctpPaddrparams::SIZE {
            return Err(Errno::EINVAL);
        }
        let (p, target) = self.peer_params_request(g, value)?;
        let flags = u32::from_le_bytes(p.spp_flags);
        if [3, 24, 96].iter().any(|mask| flags & mask == *mask)
            || u32::from_le_bytes(p.spp_sackdelay) > 500
            || (u32::from_le_bytes(p.spp_pathmtu) != 0 && u32::from_le_bytes(p.spp_pathmtu) < 512)
            || (value.len() == 152 && flags & (256 | 512) != 0)
            || (flags & 4 != 0 && target.is_none())
        {
            return Err(Errno::EINVAL);
        }
        if let Some((a, addr)) = target {
            let mut a = a.lock();
            let default = a.path;
            for addr in a.paddrs.clone() {
                a.paths.entry(addr).or_insert(default);
            }
            if let Some(addr) = addr {
                self.apply_peer_path(a.paths.get_mut(&addr).expect("peer path exists"), &p, addr);
            } else {
                a.path.apply(&p, self.domain == Domain::Inet6);
                for (addr, path) in &mut a.paths {
                    self.apply_peer_path(path, &p, *addr);
                }
            }
            let mtu = a
                .paddrs
                .iter()
                .map(|addr| a.paths.get(addr).unwrap_or(&a.path).mtu)
                .filter(|mtu| *mtu != 0)
                .min();
            if let Some(mtu) = mtu {
                a.pmtu = mtu;
                a.path.mtu = mtu;
            }
        } else {
            g.so.path.apply(&p, self.domain == Domain::Inet6);
        }
        Ok(())
    }

    /// sctp_apply_peer_addr_params re-probes a transport when PMTU discovery is
    /// re-enabled; a formerly fixed path MTU must not survive that transition.
    fn apply_peer_path(&self, path: &mut PeerParams, p: &SctpPaddrparams, addr: SocketAddr) {
        let discover = path.flags & 16 != 0 && u32::from_le_bytes(p.spp_flags) & 8 != 0;
        path.apply(p, addr.is_ipv6());
        if discover {
            path.mtu = self.net.mtu_to(addr.ip()).unwrap_or(1500);
        }
    }

    fn get_peer_params(&self, g: &Inner, out: &mut [u8]) -> NetResult<usize> {
        let (p, target) = self.peer_params_request(g, out)?;
        let path = match target {
            Some((a, addr)) => {
                let a = a.lock();
                addr.and_then(|addr| a.paths.get(&addr).copied())
                    .unwrap_or(a.path)
            }
            None => g.so.path,
        };
        let size = if out.len() >= SctpPaddrparams::SIZE {
            SctpPaddrparams::SIZE
        } else {
            152
        };
        let bytes = guest_bytes(&path.write(p));
        out[..size].copy_from_slice(&bytes[..size]);
        Ok(size)
    }
}
