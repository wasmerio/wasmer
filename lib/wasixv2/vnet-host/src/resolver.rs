//! The host's resolver behind the namespace's DNS server ([`Backend::lookup`]).
//!
//! Lookups run on a few worker threads, made when needed and gone after a while idle, so the
//! blocking host calls never hold up a guest or the reactor:
//!
//! - A name is looked up in the host's hosts file first, then with `getaddrinfo` as an
//!   absolute name (a trailing dot), so the host's search list never applies to a name the
//!   guest's resolver has already made whole.
//! - An address is looked up with `getnameinfo` (`NI_NAMEREQD`).
//! - A query the DNS server does not answer itself goes as it is to the name servers of the
//!   host's `resolv.conf`, which the host reaches even on a loopback address (a local stub
//!   such as systemd-resolved's): over UDP with a fresh identifier, again over TCP when the
//!   answer is truncated.
//!
//! Names and addresses are cached briefly, since the host's resolver tells no TTL: what a
//! lookup found for [`FOUND_TTL`], that nothing exists for [`NOT_FOUND_TTL`], a failure not at
//! all. Lookups of one name share one host call, as the A and AAAA queries a guest sends
//! together do.
//!
//! [`Backend::lookup`]: wasixv2_vnet::Backend::lookup

use std::{
    collections::{HashMap, VecDeque},
    ffi::{CStr, CString},
    hash::{BuildHasher, Hasher},
    io::{Read, Write},
    mem,
    net::{IpAddr, SocketAddr, TcpStream, UdpSocket},
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
    thread,
    time::{Duration, Instant},
};

use wasixv2_vnet::{Answer, Found, Lookup, LookupDone, LookupError};

/// How long what a lookup found holds.
pub const FOUND_TTL: Duration = Duration::from_secs(30);
/// How long a name or address that does not exist stays so.
pub const NOT_FOUND_TTL: Duration = Duration::from_secs(5);
/// Worker threads at most, which is how many host calls run at once.
const MAX_WORKERS: usize = 4;
/// How long a worker waits for work before it goes.
const IDLE: Duration = Duration::from_secs(10);
/// Names and addresses cached at most.
const MAX_CACHED: usize = 512;
/// Lookups waiting or running at most; more fail at once.
const MAX_PENDING: usize = 256;
/// Askers of one name at most, beyond which they fail at once.
const MAX_WAITERS: usize = 64;
/// How long a name server has to answer one attempt, and how many rounds of every server
/// there are: `resolv.conf`'s `timeout` and `attempts`, shortened, since the guest's resolver
/// waits on this one.
const SERVER_TIMEOUT: Duration = Duration::from_secs(2);
const ATTEMPTS: usize = 2;
/// `MAXNS`: the name servers of `resolv.conf` that count.
const MAX_SERVERS: usize = 3;
const DNS_HEADER: usize = 12;

/// Where the resolver finds the host's configuration.
#[derive(Clone, Debug)]
pub struct ResolverConfig {
    /// The hosts file, `/etc/hosts`.
    pub hosts: PathBuf,
    /// The file that names the host's name servers, `/etc/resolv.conf`.
    pub resolv_conf: PathBuf,
    /// The port the name servers listen on, 53.
    pub port: u16,
}

impl Default for ResolverConfig {
    fn default() -> Self {
        Self {
            hosts: "/etc/hosts".into(),
            resolv_conf: "/etc/resolv.conf".into(),
            port: 53,
        }
    }
}

/// What a lookup is cached under.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Key {
    /// Lower case.
    Name(String),
    Addr(IpAddr),
}

enum Job {
    Lookup(Key),
    Forward(Vec<u8>, LookupDone),
}

struct Cached {
    result: Result<Answer, ()>,
    until: Instant,
}

impl Cached {
    /// What the entry says at `now`, with the seconds it still holds; `None` once it expired.
    fn at(&self, now: Instant) -> Option<Result<Found, LookupError>> {
        let left = self.until.checked_duration_since(now)?;
        let ttl = u32::try_from(left.as_secs()).unwrap_or(u32::MAX).max(1);
        Some(match &self.result {
            Ok(answer) => Ok(Found {
                answer: answer.clone(),
                ttl,
            }),
            Err(()) => Err(LookupError::NotFound { ttl }),
        })
    }
}

#[derive(Default)]
struct State {
    cache: HashMap<Key, Cached>,
    /// Lookups queued or running, with who waits for each.
    waiting: HashMap<Key, Vec<LookupDone>>,
    queue: VecDeque<Job>,
    workers: usize,
    idle: usize,
    stop: bool,
}

struct Shared {
    config: ResolverConfig,
    st: Mutex<State>,
    work: Condvar,
}

/// The resolver of a [`crate::HostBackend`]; dropping it lets its workers go once their host
/// calls return.
pub(crate) struct Resolver(Arc<Shared>);

impl Resolver {
    pub(crate) fn new(config: ResolverConfig) -> Self {
        Self(Arc::new(Shared {
            config,
            st: Mutex::default(),
            work: Condvar::new(),
        }))
    }

    pub(crate) fn lookup(&self, lookup: Lookup, done: LookupDone) {
        let key = match lookup {
            Lookup::Name(name) => Key::Name(name.to_ascii_lowercase()),
            Lookup::Addr(addr) => Key::Addr(addr),
            Lookup::Forward(query) => return self.enqueue(Job::Forward(query, done)),
        };
        let mut st = self.0.st.lock().unwrap();
        if let Some(found) = st.cache.get(&key).and_then(|c| c.at(Instant::now())) {
            drop(st);
            return done(found);
        }
        let full = st.waiting.len() + st.queue.len() >= MAX_PENDING;
        match st.waiting.get_mut(&key) {
            Some(waiters) if waiters.len() < MAX_WAITERS => return waiters.push(done),
            None if !full => {}
            _ => {
                drop(st);
                return done(Err(LookupError::Failed));
            }
        }
        st.waiting.insert(key.clone(), vec![done]);
        st.queue.push_back(Job::Lookup(key));
        self.wake_worker(st);
    }

    fn enqueue(&self, job: Job) {
        let mut st = self.0.st.lock().unwrap();
        if st.waiting.len() + st.queue.len() >= MAX_PENDING {
            drop(st);
            if let Job::Forward(_, done) = job {
                done(Err(LookupError::Failed));
            }
            return;
        }
        st.queue.push_back(job);
        self.wake_worker(st);
    }

    /// Hands the queue's newest job to an idle worker, or a new one while there may be more;
    /// with no worker at all, every queued lookup fails.
    fn wake_worker(&self, mut st: std::sync::MutexGuard<'_, State>) {
        if st.idle > 0 || st.workers >= MAX_WORKERS {
            drop(st);
            self.0.work.notify_one();
            return;
        }
        let shared = self.0.clone();
        let spawned = thread::Builder::new()
            .name("wasixv2-vnet-resolver".into())
            .spawn(move || work(&shared));
        match spawned {
            Ok(_) => st.workers += 1,
            Err(err) if st.workers == 0 => {
                tracing::warn!(%err, "no resolver thread; DNS lookups fail");
                let failed = fail_all(&mut st);
                drop(st);
                failed
                    .into_iter()
                    .for_each(|done| done(Err(LookupError::Failed)));
            }
            Err(err) => tracing::debug!(%err, "no further resolver thread"),
        }
    }
}

impl Drop for Resolver {
    fn drop(&mut self) {
        self.0.st.lock().unwrap().stop = true;
        self.0.work.notify_all();
    }
}

/// Takes every queued job and waiter out of `st`, to be failed.
fn fail_all(st: &mut State) -> Vec<LookupDone> {
    let mut out: Vec<LookupDone> = st.waiting.drain().flat_map(|(_, w)| w).collect();
    for job in st.queue.drain(..) {
        if let Job::Forward(_, done) = job {
            out.push(done);
        }
    }
    out
}

/// A worker: takes jobs until it was idle for [`IDLE`] or the resolver went.
fn work(shared: &Shared) {
    loop {
        let job = {
            let mut st = shared.st.lock().unwrap();
            loop {
                if st.stop {
                    st.workers -= 1;
                    return;
                }
                if let Some(job) = st.queue.pop_front() {
                    break job;
                }
                st.idle += 1;
                let (guard, timeout) = shared.work.wait_timeout(st, IDLE).unwrap();
                st = guard;
                st.idle -= 1;
                if timeout.timed_out() && st.queue.is_empty() {
                    st.workers -= 1;
                    return;
                }
            }
        };
        match job {
            Job::Forward(query, done) => done(forward(&shared.config, &query)),
            Job::Lookup(key) => {
                let found = match &key {
                    Key::Name(name) => lookup_name(&shared.config, name),
                    Key::Addr(addr) => lookup_addr(*addr),
                };
                let waiters = {
                    let mut st = shared.st.lock().unwrap();
                    remember(&mut st.cache, &key, &found);
                    st.waiting.remove(&key).unwrap_or_default()
                };
                for done in waiters {
                    done(answer_now(&found));
                }
            }
        }
    }
}

/// A host lookup's outcome: what it found, that nothing exists, or `None` when it failed.
type Outcome = Option<Result<Answer, ()>>;

fn answer_now(found: &Outcome) -> Result<Found, LookupError> {
    match found {
        Some(Ok(answer)) => Ok(Found {
            answer: answer.clone(),
            ttl: FOUND_TTL.as_secs() as u32,
        }),
        Some(Err(())) => Err(LookupError::NotFound {
            ttl: NOT_FOUND_TTL.as_secs() as u32,
        }),
        None => Err(LookupError::Failed),
    }
}

/// Caches `found` for `key`, making room by dropping expired entries, then the one that
/// expires first.
fn remember(cache: &mut HashMap<Key, Cached>, key: &Key, found: &Outcome) {
    let Some(result) = found.clone() else {
        return;
    };
    let now = Instant::now();
    if cache.len() >= MAX_CACHED {
        cache.retain(|_, c| c.until > now);
    }
    if cache.len() >= MAX_CACHED
        && let Some(first) = cache
            .iter()
            .min_by_key(|(_, c)| c.until)
            .map(|(k, _)| k.clone())
    {
        cache.remove(&first);
    }
    let ttl = match result {
        Ok(_) => FOUND_TTL,
        Err(()) => NOT_FOUND_TTL,
    };
    cache.insert(
        key.clone(),
        Cached {
            result,
            until: now + ttl,
        },
    );
}

/// The addresses `name` has in the hosts file `hosts`, and its canonical name, the first
/// name of the first line that has it, as glibc's `files` service gives them; `None` if no
/// line names it.
fn hosts_lookup(hosts: &str, name: &str) -> Option<Answer> {
    let mut canonical = None;
    let mut addrs = Vec::new();
    for line in hosts.lines() {
        let line = line.split('#').next().unwrap_or_default();
        let mut words = line.split_whitespace();
        let Some(Ok(ip)) = words
            .next()
            .map(|w| w.split('%').next().unwrap_or(w).parse::<IpAddr>())
        else {
            continue;
        };
        let names: Vec<&str> = words.collect();
        if !names.iter().any(|n| n.eq_ignore_ascii_case(name)) {
            continue;
        }
        canonical.get_or_insert_with(|| names[0].to_string());
        if !addrs.contains(&ip) {
            addrs.push(ip);
        }
    }
    Some(Answer::Addrs {
        canonical: canonical?,
        addrs,
    })
}

fn lookup_name(config: &ResolverConfig, name: &str) -> Outcome {
    let hosts = std::fs::read_to_string(&config.hosts).unwrap_or_default();
    if let Some(found) = hosts_lookup(&hosts, name) {
        return Some(Ok(found));
    }
    getaddrinfo(name)
}

/// `getaddrinfo` of the absolute `name`, both families, with its canonical name.
fn getaddrinfo(name: &str) -> Outcome {
    let Ok(node) = CString::new(format!("{name}.")) else {
        return Some(Err(()));
    };
    // SAFETY: zeroed `addrinfo` hints are valid; the list is freed below.
    let mut hints: libc::addrinfo = unsafe { mem::zeroed() };
    hints.ai_family = libc::AF_UNSPEC;
    hints.ai_socktype = libc::SOCK_STREAM;
    hints.ai_flags = libc::AI_CANONNAME;
    let mut list: *mut libc::addrinfo = std::ptr::null_mut();
    // SAFETY: `node` is a C string, `hints` and `list` live across the call.
    let rc = unsafe { libc::getaddrinfo(node.as_ptr(), std::ptr::null(), &hints, &mut list) };
    if rc != 0 {
        return gai_outcome(rc);
    }
    let mut canonical = String::new();
    let mut addrs = Vec::new();
    let mut at = list;
    while !at.is_null() {
        // SAFETY: `at` is an entry of the list getaddrinfo returned, not yet freed.
        let ai = unsafe { &*at };
        if canonical.is_empty() && !ai.ai_canonname.is_null() {
            // SAFETY: `ai_canonname` is a C string of the entry.
            let c = unsafe { CStr::from_ptr(ai.ai_canonname) };
            canonical = c.to_string_lossy().trim_end_matches('.').to_string();
        }
        // SAFETY: `ai_addr` holds `ai_addrlen` bytes of a socket address of the entry.
        if let Some(addr) = unsafe { sockaddr_ip(ai.ai_addr, ai.ai_addrlen) }
            && !addrs.contains(&addr)
        {
            addrs.push(addr);
        }
        at = ai.ai_next;
    }
    // SAFETY: `list` came from getaddrinfo and is freed once.
    unsafe { libc::freeaddrinfo(list) };
    if canonical.is_empty() {
        canonical = name.to_string();
    }
    Some(Ok(Answer::Addrs { canonical, addrs }))
}

/// What a `getaddrinfo` or `getnameinfo` error says: no such name (`EAI_NONAME`), a name
/// without addresses (`EAI_NODATA`, glibc), or a failure.
fn gai_outcome(rc: libc::c_int) -> Outcome {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    if rc == libc::EAI_NODATA {
        return Some(Ok(Answer::Addrs {
            canonical: String::new(),
            addrs: Vec::new(),
        }));
    }
    match rc {
        libc::EAI_NONAME => Some(Err(())),
        _ => None,
    }
}

/// The IP address of a socket address.
///
/// # Safety
/// `sa` points to `len` readable bytes.
unsafe fn sockaddr_ip(sa: *const libc::sockaddr, len: libc::socklen_t) -> Option<IpAddr> {
    if sa.is_null() {
        return None;
    }
    let len = len as usize;
    // SAFETY: the family is the first field of every socket address, and the length is
    // checked against the structure read.
    unsafe {
        match i32::from((*sa).sa_family) {
            libc::AF_INET if len >= mem::size_of::<libc::sockaddr_in>() => {
                let sin = &*(sa as *const libc::sockaddr_in);
                Some(IpAddr::from(
                    u32::from_be(sin.sin_addr.s_addr).to_be_bytes(),
                ))
            }
            libc::AF_INET6 if len >= mem::size_of::<libc::sockaddr_in6>() => {
                let sin6 = &*(sa as *const libc::sockaddr_in6);
                Some(IpAddr::from(sin6.sin6_addr.s6_addr))
            }
            _ => None,
        }
    }
}

/// `getnameinfo` of `addr` with `NI_NAMEREQD`.
fn lookup_addr(addr: IpAddr) -> Outcome {
    let sa = socket2::SockAddr::from(SocketAddr::new(addr, 0));
    let mut host = [0 as libc::c_char; libc::NI_MAXHOST as usize];
    // SAFETY: `sa` holds a socket address of its length; `host` is writable for its length.
    let rc = unsafe {
        libc::getnameinfo(
            sa.as_ptr().cast(),
            sa.len(),
            host.as_mut_ptr(),
            host.len() as libc::socklen_t,
            std::ptr::null_mut(),
            0,
            libc::NI_NAMEREQD,
        )
    };
    if rc != 0 {
        return (rc == libc::EAI_NONAME).then_some(Err(()));
    }
    // SAFETY: getnameinfo wrote a C string into `host`.
    let name = unsafe { CStr::from_ptr(host.as_ptr()) };
    let name = name.to_string_lossy().trim_end_matches('.').to_string();
    Some(Ok(Answer::Name(name)))
}

/// The name servers `resolv.conf` lists, loopback ones included, as the host reaches them.
fn nameservers(resolv_conf: &str) -> Vec<IpAddr> {
    let mut out = Vec::new();
    for line in resolv_conf.lines() {
        let mut words = line.split_whitespace();
        if words.next() != Some("nameserver") {
            continue;
        }
        if let Some(Ok(ip)) = words
            .next()
            .map(|w| w.split('%').next().unwrap_or(w).parse::<IpAddr>())
            && !out.contains(&ip)
            && out.len() < MAX_SERVERS
        {
            out.push(ip);
        }
    }
    out
}

/// The end of the question section of a message with one question, `None` if it has none.
fn question_end(msg: &[u8]) -> Option<usize> {
    let mut at = DNS_HEADER;
    loop {
        let b = *msg.get(at)?;
        match b {
            0 => break Some(at + 1 + 4).filter(|&end| end <= msg.len()),
            b if b & 0xc0 == 0xc0 => break Some(at + 2 + 4).filter(|&end| end <= msg.len()),
            b if b & 0xc0 == 0 => at += 1 + usize::from(b),
            _ => break None,
        }
    }
}

/// A random query identifier.
fn random_id() -> u16 {
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    h.write_u128(now.unwrap_or_default().as_nanos());
    h.write_u64(std::process::id().into());
    h.finish() as u16
}

/// Whether `reply` answers `query`: a response with its identifier and question (RFC 5452
/// 4.3).
fn answers(query: &[u8], reply: &[u8]) -> bool {
    let (Some(q), Some(r)) = (question_end(query), question_end(reply)) else {
        return false;
    };
    reply.len() >= DNS_HEADER
        && reply[..2] == query[..2]
        && reply[2] & 0x80 != 0
        && reply[4..6] == query[4..6]
        && reply[DNS_HEADER..r].eq_ignore_ascii_case(&query[DNS_HEADER..q])
}

/// Forwards `query` to the host's name servers, each in turn for [`ATTEMPTS`] rounds, as the
/// stub resolver of libc does: the first answer that is no server failure (`SERVFAIL`,
/// `NOTIMP`, `REFUSED`) counts, else the last of those.
fn forward(config: &ResolverConfig, query: &[u8]) -> Result<Found, LookupError> {
    if question_end(query).is_none() {
        return Err(LookupError::Failed);
    }
    let conf = std::fs::read_to_string(&config.resolv_conf).unwrap_or_default();
    let servers = nameservers(&conf);
    let mut q = query.to_vec();
    q[..2].copy_from_slice(&random_id().to_be_bytes());
    let mut fallback = None;
    for _ in 0..ATTEMPTS {
        for &ip in &servers {
            let server = SocketAddr::new(ip, config.port);
            let Some(mut reply) = exchange(&q, server) else {
                continue;
            };
            reply[..2].copy_from_slice(&query[..2]);
            if matches!(reply[3] & 0xf, 2 | 4 | 5) {
                fallback = Some(reply);
                continue;
            }
            return Ok(Found {
                answer: Answer::Message(reply),
                ttl: 0,
            });
        }
    }
    match fallback {
        Some(reply) => Ok(Found {
            answer: Answer::Message(reply),
            ttl: 0,
        }),
        None => Err(LookupError::Failed),
    }
}

/// One query to `server`: over UDP, and again over TCP if the answer is truncated.
fn exchange(query: &[u8], server: SocketAddr) -> Option<Vec<u8>> {
    let reply = udp_exchange(query, server)?;
    if reply[2] & 0x02 == 0 {
        return Some(reply);
    }
    tcp_exchange(query, server).or(Some(reply))
}

fn udp_exchange(query: &[u8], server: SocketAddr) -> Option<Vec<u8>> {
    let any: SocketAddr = match server {
        SocketAddr::V4(_) => ([0, 0, 0, 0], 0).into(),
        SocketAddr::V6(_) => ([0u16; 8], 0).into(),
    };
    let sock = UdpSocket::bind(any).ok()?;
    sock.connect(server).ok()?;
    sock.send(query).ok()?;
    let deadline = Instant::now() + SERVER_TIMEOUT;
    let mut buf = vec![0; 65535];
    loop {
        let left = deadline.checked_duration_since(Instant::now())?;
        sock.set_read_timeout(Some(left.max(Duration::from_millis(1))))
            .ok()?;
        let n = sock.recv(&mut buf).ok()?;
        if answers(query, &buf[..n]) {
            buf.truncate(n);
            return Some(buf);
        }
    }
}

fn tcp_exchange(query: &[u8], server: SocketAddr) -> Option<Vec<u8>> {
    let mut sock = TcpStream::connect_timeout(&server, SERVER_TIMEOUT).ok()?;
    sock.set_read_timeout(Some(SERVER_TIMEOUT)).ok()?;
    sock.set_write_timeout(Some(SERVER_TIMEOUT)).ok()?;
    let mut framed = (query.len() as u16).to_be_bytes().to_vec();
    framed.extend_from_slice(query);
    sock.write_all(&framed).ok()?;
    let mut len = [0; 2];
    sock.read_exact(&mut len).ok()?;
    let mut reply = vec![0; usize::from(u16::from_be_bytes(len))];
    sock.read_exact(&mut reply).ok()?;
    answers(query, &reply).then_some(reply)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ips(list: &[&str]) -> Vec<IpAddr> {
        list.iter().map(|ip| ip.parse().unwrap()).collect()
    }

    #[test]
    fn hosts_file_lines() {
        let hosts = "127.0.0.1 localhost\n\
            # 10.0.0.1 commented\n\
            192.0.2.1 web.example web  # a comment\n\
            2001:db8::1 Web\n\
            fe80::1%eth0 web\n\
            bad web\n";
        assert_eq!(
            hosts_lookup(hosts, "WEB"),
            Some(Answer::Addrs {
                canonical: "web.example".into(),
                addrs: ips(&["192.0.2.1", "2001:db8::1", "fe80::1"]),
            })
        );
        assert_eq!(hosts_lookup(hosts, "commented"), None);
        assert_eq!(hosts_lookup(hosts, "example"), None);
    }

    #[test]
    fn nameservers_keep_loopback_and_stop_at_maxns() {
        let conf = "nameserver 127.0.0.53\nnameserver ::1\nnameserver 127.0.0.53\n\
            nameserver fe80::1%eth0\nnameserver 9.9.9.9\noptions edns0\n";
        assert_eq!(nameservers(conf), ips(&["127.0.0.53", "::1", "fe80::1"]));
    }

    #[test]
    fn the_cache_holds_found_and_not_found_and_makes_room() {
        let mut cache = HashMap::new();
        let found = Some(Ok(Answer::Name("a".into())));
        remember(&mut cache, &Key::Name("a".into()), &found);
        remember(&mut cache, &Key::Name("b".into()), &Some(Err(())));
        remember(&mut cache, &Key::Name("c".into()), &None);
        assert_eq!(cache.len(), 2);
        let now = Instant::now();
        assert!(matches!(
            cache[&Key::Name("a".into())].at(now),
            Some(Ok(Found { ttl: 29 | 30, .. }))
        ));
        assert!(matches!(
            cache[&Key::Name("b".into())].at(now),
            Some(Err(LookupError::NotFound { ttl: 4 | 5 }))
        ));
        assert!(
            cache[&Key::Name("b".into())]
                .at(now + NOT_FOUND_TTL)
                .is_none()
        );
        for i in 0..MAX_CACHED {
            remember(&mut cache, &Key::Name(format!("n{i}")), &found);
        }
        assert_eq!(cache.len(), MAX_CACHED);
        assert!(!cache.contains_key(&Key::Name("b".into())), "expires first");
    }

    #[test]
    fn replies_must_match_the_query() {
        let mut q = vec![0x12, 0x34, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        q.extend([3, b'f', b'o', b'o', 0, 0, 1, 0, 1]);
        let mut r = q.clone();
        r[2] |= 0x80;
        assert!(answers(&q, &r));
        r[13] = b'F';
        assert!(answers(&q, &r), "case is free");
        r[1] = 0x35;
        assert!(!answers(&q, &r), "another identifier");
        assert!(!answers(&q, &q), "a query");
    }
}
