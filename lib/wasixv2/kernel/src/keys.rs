//! The key retention service (`security/keys`): keys of the `user`, `logon` and `keyring`
//! types, the thread, process and session keyrings of the credentials, each user's keyrings,
//! `add_key`, `request_key` (including negative caching when `/sbin/request-key` is unavailable),
//! `keyctl`, and `/proc/keys` and
//! `/proc/key-users`.
//!
//! A key lives while something holds it: a keyring that links it, credentials, or a caller;
//! the serial table only finds it. Keyrings never link in a cycle (`EDEADLK`), so nothing
//! keeps itself alive.

use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

use wasixv2_abi::{Errno, KeyPerm, KeySpec, KeyctlOp, Ulong};
use wasixv2_syscalls::{Caller, SysResult, read_cstr};
use wasixv2_vfs::Cap;

use crate::{
    Kernel,
    cred::Cred,
    ns::UserNs,
    sync::MutexExt,
    uidgid::{Kgid, Kuid},
};

/// `key_quota_maxkeys`, `key_quota_maxbytes`, and root's (`key_quota_root_*`).
const MAXKEYS: u32 = 200;
const MAXBYTES: u32 = 20000;
const ROOT_MAXKEYS: u32 = 1_000_000;
const ROOT_MAXBYTES: u32 = 25_000_000;

/// The configured security/keys/sysctl.c entries (persistent keyrings are unconfigured).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeySysctl {
    MaxKeys,
    MaxBytes,
    RootMaxKeys,
    RootMaxBytes,
    GcDelay,
}

struct KeyLimits([AtomicU32; 5]);

impl Default for KeyLimits {
    fn default() -> Self {
        Self([MAXKEYS, MAXBYTES, ROOT_MAXKEYS, ROOT_MAXBYTES, 300].map(AtomicU32::new))
    }
}

impl KeyLimits {
    fn get(&self, entry: KeySysctl) -> u32 {
        self.0[entry as usize].load(Ordering::Relaxed)
    }
}

/// `KEY_MAX_DESC_SIZE`, the longest type name, and the largest `user` payload.
const MAX_DESC: usize = 4096;
const MAX_TYPE: usize = 32;
const MAX_USER_PAYLOAD: usize = 32767;

/// `KEYRING_SEARCH_MAX_DEPTH`.
const MAX_DEPTH: usize = 6;

const POS_ALL: u32 = KeyPerm::KEY_POS_ALL.0;
const USR_ALL: u32 = KeyPerm::KEY_USR_ALL.0;

/// `KEY_NEED_*`: the permission bits an operation needs, as the low byte of each group.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Need {
    View = 0x01,
    Read = 0x02,
    Write = 0x04,
    Search = 0x08,
    Link = 0x10,
    Setattr = 0x20,
    /// `KEY_NEED_UNLINK`: no check of the key itself.
    Unlink = 0,
}

/// A key type (`struct key_type`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Kind {
    Keyring,
    User,
    /// `user` keys that only the kernel reads.
    Logon,
}

impl Kind {
    fn lookup(name: &[u8]) -> Result<Self, Errno> {
        match name {
            b"keyring" => Ok(Self::Keyring),
            b"user" => Ok(Self::User),
            b"logon" => Ok(Self::Logon),
            // `key_type_lookup`: internal types start with a dot.
            [b'.', ..] => Err(Errno::EPERM),
            _ => Err(Errno::ENODEV),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Keyring => "keyring",
            Self::User => "user",
            Self::Logon => "logon",
        }
    }

    /// `->read`: logon keys cannot be read.
    fn readable(self) -> bool {
        self != Self::Logon
    }

    /// `->vet_description`: a logon key's names its service, `service:rest`.
    fn vet_description(self, desc: &[u8]) -> Result<(), Errno> {
        match self {
            Self::Logon => match desc.iter().position(|&b| b == b':') {
                Some(0) | None => Err(Errno::EINVAL),
                Some(_) => Ok(()),
            },
            _ => Ok(()),
        }
    }

    /// `->preparse`: a keyring takes no payload; `user` and `logon` one of 1 to 32767 bytes.
    fn check_payload(self, payload: &[u8]) -> Result<(), Errno> {
        match self {
            Self::Keyring if !payload.is_empty() => Err(Errno::EINVAL),
            Self::Keyring => Ok(()),
            _ if payload.is_empty() || payload.len() > MAX_USER_PAYLOAD => Err(Errno::EINVAL),
            _ => Ok(()),
        }
    }
}

/// One user's keys and quota (`struct key_user`), and their keyrings.
pub(crate) struct KeyUser {
    uid: Kuid,
    limits: Arc<KeyLimits>,
    state: Mutex<UserState>,
}

#[derive(Default)]
struct UserState {
    /// `nkeys`, `nikeys`, `qnkeys`, `qnbytes`.
    nkeys: u32,
    nikeys: u32,
    qnbytes: u32,
    /// `_uid.<uid>` and `_uid_ses.<uid>` (`look_up_user_keyrings`).
    keyring: Option<Arc<Key>>,
    session: Option<Arc<Key>>,
}

impl KeyUser {
    fn quota(&self) -> (u32, u32) {
        match self.uid.0 {
            0 => (
                self.limits.get(KeySysctl::RootMaxKeys),
                self.limits.get(KeySysctl::RootMaxBytes),
            ),
            _ => (
                self.limits.get(KeySysctl::MaxKeys),
                self.limits.get(KeySysctl::MaxBytes),
            ),
        }
    }
}

/// A key (`struct key`).
pub(crate) struct Key {
    serial: i32,
    kind: Kind,
    description: Vec<u8>,
    /// Whose quota the key counts against: its owner's.
    user: Mutex<Arc<KeyUser>>,
    state: Mutex<KeyState>,
}

struct KeyState {
    uid: Kuid,
    gid: Kgid,
    perm: u32,
    payload: Vec<u8>,
    /// A negatively instantiated key's error (`key->state`), separate from key_validate.
    negative: Option<Errno>,
    /// A keyring's keys, in the order they were linked.
    links: Vec<Arc<Key>>,
    revoked: bool,
    revoked_at: Option<u64>,
    invalidated: bool,
    /// When it expires, in seconds of the realtime clock; `None` for never.
    expiry: Option<u64>,
    /// What the key counts against its owner's byte quota.
    quotalen: u32,
}

impl Drop for Key {
    fn drop(&mut self) {
        let quotalen = self.state.get_mut().map_or(0, |s| s.quotalen);
        let user = self.user.lock_or_recover().clone();
        let mut user = user.state.lock_or_recover();
        user.nkeys -= 1;
        user.nikeys -= 1;
        user.qnbytes -= quotalen;
    }
}

impl Key {
    fn st(&self) -> std::sync::MutexGuard<'_, KeyState> {
        self.state.lock_or_recover()
    }

    /// `key_validate`: revoked or expired keys are not used.
    fn validate(&self, now: u64) -> Result<(), Errno> {
        let st = self.st();
        if st.revoked {
            return Err(Errno::EKEYREVOKED);
        }
        if st.expiry.is_some_and(|at| now >= at) {
            return Err(Errno::EKEYEXPIRED);
        }
        Ok(())
    }

    /// `key_read_state`: a negative key has no payload its type's read method may see.
    fn read_state(&self) -> Result<(), Errno> {
        match self.st().negative {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Whether the key is gone for every lookup (`KEY_FLAG_INVALIDATED`, or garbage).
    fn is_dead(&self) -> bool {
        self.st().invalidated
    }

    /// A keyring's live keys.
    fn keys(&self) -> Vec<Arc<Key>> {
        self.st()
            .links
            .iter()
            .filter(|k| !k.is_dead())
            .cloned()
            .collect()
    }

    /// `key_permission` for `cred`, with the possessor's rights if `possessed`.
    fn permits(&self, cred: &Cred, possessed: bool, need: Need) -> bool {
        if need == Need::Unlink {
            return true;
        }
        let st = self.st();
        let mut kperm = if st.uid == cred.user.fs {
            st.perm >> 16
        } else if st.perm & KeyPerm::KEY_GRP_ALL.0 != 0 && cred.in_group(st.gid) {
            st.perm >> 8
        } else {
            st.perm
        };
        if possessed {
            kperm |= st.perm >> 24;
        }
        kperm & need as u32 != 0
    }

    /// `key_check_quota` and accounting for a payload of `len` bytes replacing the current one.
    fn charge(&self, len: u32) -> Result<(), Errno> {
        let mut st = self.st();
        let new = self.description.len() as u32 + len;
        let owner = self.user.lock_or_recover().clone();
        let mut user = owner.state.lock_or_recover();
        let (_, maxbytes) = owner.quota();
        let bytes = user.qnbytes - st.quotalen + new;
        if new > st.quotalen && bytes > maxbytes {
            return Err(Errno::EDQUOT);
        }
        user.qnbytes = bytes;
        st.quotalen = new;
        Ok(())
    }
}

/// The machine's keys: the serial table and each user's.
pub(crate) struct Keys {
    serials: Mutex<KeyTable>,
    users: Mutex<HashMap<u32, Arc<KeyUser>>>,
    /// `key_construction_mutex`: the final search, allocation and link are indivisible.
    construction: Mutex<()>,
    limits: Arc<KeyLimits>,
    host: Arc<dyn wasixv2_host::Host>,
    gc: crate::timer::Timer,
    gc_lock: Mutex<()>,
}

#[derive(Default)]
struct KeyTable {
    last: i32,
    keys: BTreeMap<i32, Weak<Key>>,
}

/// What a lookup works with: the caller's credentials and keyrings (`struct cred`'s keys).
pub(crate) struct Ctx {
    pub(crate) cred: Arc<Cred>,
    /// The realtime clock, in seconds.
    pub(crate) now: u64,
}

impl Ctx {
    /// The keyrings a process searches (`search_cred_keyrings_rcu`): thread, process, and the
    /// session keyring, or the user's session keyring without one.
    fn search_roots(&self, keys: &Keys) -> Vec<Arc<Key>> {
        let mut roots: Vec<Arc<Key>> = self.cred.keys.thread.iter().cloned().collect();
        roots.extend(self.cred.keys.process.iter().cloned());
        match &self.cred.keys.session {
            Some(session) => roots.push(session.clone()),
            None => roots.push(keys.user_keyrings(&self.cred).1),
        }
        roots
    }

    /// `lookup_user_key_possessed`: whether `key` is one of the process's keyrings or reachable
    /// from them through keyrings it may search.
    fn possesses(&self, keys: &Keys, key: &Arc<Key>) -> bool {
        let mut queue: VecDeque<(Arc<Key>, usize)> = self
            .search_roots(keys)
            .into_iter()
            .map(|k| (k, 0))
            .collect();
        let mut seen = HashSet::new();
        while let Some((ring, depth)) = queue.pop_front() {
            if Arc::ptr_eq(&ring, key) {
                return true;
            }
            if ring.kind != Kind::Keyring || depth >= MAX_DEPTH || !seen.insert(ring.serial) {
                continue;
            }
            if !ring.permits(&self.cred, true, Need::Search) {
                continue;
            }
            queue.extend(ring.keys().into_iter().map(|k| (k, depth + 1)));
        }
        false
    }
}

impl Keys {
    /// security/keys/process_keys.c `install_thread_keyring_to_cred`.
    fn new_thread_keyring(&self, cred: &Cred) -> Result<Arc<Key>, Errno> {
        self.alloc(
            Kind::Keyring,
            b"_tid",
            (cred.user.real, cred.group.real),
            POS_ALL | KeyPerm::KEY_USR_VIEW.0,
            Vec::new(),
            true,
        )
    }

    /// kernel/cred.c `copy_creds`: share unchanged credentials unless a thread
    /// keyring exists, in which case the child receives a fresh, empty one.
    pub(crate) fn copy_thread_creds(&self, cred: &Arc<Cred>) -> Result<Arc<Cred>, Errno> {
        if cred.keys.thread.is_none() {
            return Ok(cred.clone());
        }
        let mut new = (**cred).clone();
        new.keys.thread = Some(self.new_thread_keyring(cred)?);
        Ok(Arc::new(new))
    }

    pub(crate) fn new(
        host: Arc<dyn wasixv2_host::Host>,
        timers: &crate::timer::Timers,
    ) -> Arc<Self> {
        Arc::new_cyclic(|me: &Weak<Self>| {
            let weak = me.clone();
            let gc = timers.realtime_timer(Box::new(move || {
                let keys = weak.upgrade()?;
                let _guard = keys.gc_lock.lock_or_recover();
                keys.collect()
            }));
            Self {
                serials: Mutex::default(),
                users: Mutex::default(),
                construction: Mutex::default(),
                limits: Arc::default(),
                host,
                gc,
                gc_lock: Mutex::default(),
            }
        })
    }

    pub(crate) fn sysctl(&self, entry: KeySysctl) -> u32 {
        self.limits.get(entry)
    }

    pub(crate) fn set_sysctl(&self, entry: KeySysctl, value: u32) -> Result<(), Errno> {
        // security/keys/sysctl.c: proc_dointvec_minmax, [1, INT_MAX], or [0, INT_MAX] for gc_delay.
        if value > i32::MAX as u32 || (value == 0 && entry != KeySysctl::GcDelay) {
            return Err(Errno::EINVAL);
        }
        self.limits.0[entry as usize].store(value, Ordering::Relaxed);
        if entry == KeySysctl::GcDelay {
            self.rearm_gc();
        }
        Ok(())
    }

    fn rearm_gc(&self) {
        let _guard = self.gc_lock.lock_or_recover();
        match self.collect() {
            Some(at) => self.gc.start(at),
            None => self.gc.cancel(),
        }
    }

    /// security/keys/gc.c key_garbage_collector and keyring.c keyring_gc: collect links,
    /// not keys held by credentials, after expiry/revocation plus the grace period. Drop
    /// removed links outside key locks, and only hold the serial table while taking a snapshot.
    fn collect(&self) -> Option<Duration> {
        let keys: Vec<Arc<Key>> = self
            .serials
            .lock_or_recover()
            .keys
            .values()
            .filter_map(Weak::upgrade)
            .collect();
        let now = self.host.realtime().as_secs();
        let delay = u64::from(self.sysctl(KeySysctl::GcDelay));
        let mut dead = HashSet::new();
        let mut next: Option<u64> = None;
        for key in &keys {
            let st = key.st();
            let expiry = st.expiry.into_iter().chain(st.revoked_at).min();
            let at = match st.invalidated {
                true => Some(now),
                false => expiry.map(|at| at.saturating_add(delay)),
            };
            if let Some(at) = at {
                if at <= now {
                    dead.insert(key.serial);
                } else {
                    next = Some(next.map_or(at, |old| old.min(at)));
                }
            }
        }
        for key in &keys {
            if key.kind == Kind::Keyring {
                let removed = {
                    let mut st = key.st();
                    let (live, removed) = std::mem::take(&mut st.links)
                        .into_iter()
                        .partition::<Vec<_>, _>(|k| !dead.contains(&k.serial));
                    st.links = live;
                    removed
                };
                drop(removed);
            }
        }
        next.map(|at| {
            self.host
                .monotonic()
                .saturating_add(Duration::from_secs(at.saturating_sub(now)))
        })
    }

    fn user(&self, uid: Kuid) -> Arc<KeyUser> {
        let mut users = self.users.lock_or_recover();
        users
            .entry(uid.0)
            .or_insert_with(|| {
                Arc::new(KeyUser {
                    uid,
                    limits: self.limits.clone(),
                    state: Mutex::default(),
                })
            })
            .clone()
    }

    /// `key_alloc` and `key_instantiate_and_link`'s accounting: a new key owned by `owner`,
    /// within its quota (`EDQUOT`) unless `exempt`.
    fn alloc(
        &self,
        kind: Kind,
        description: &[u8],
        owner: (Kuid, Kgid),
        perm: u32,
        payload: Vec<u8>,
        exempt: bool,
    ) -> Result<Arc<Key>, Errno> {
        let user = self.user(owner.0);
        let quotalen = (description.len() + payload.len()) as u32;
        {
            let mut st = user.state.lock_or_recover();
            let (maxkeys, maxbytes) = user.quota();
            if !exempt && (st.nkeys + 1 > maxkeys || st.qnbytes + quotalen > maxbytes) {
                return Err(Errno::EDQUOT);
            }
            st.nkeys += 1;
            st.nikeys += 1;
            st.qnbytes += quotalen;
        }
        let mut table = self.serials.lock_or_recover();
        // `key_alloc_serial`: serials are positive and at least 3.
        let mut serial = table.last;
        loop {
            serial = match serial.checked_add(1) {
                Some(next) if next >= 3 => next,
                _ => 3,
            };
            if table
                .keys
                .get(&serial)
                .is_none_or(|k| k.strong_count() == 0)
            {
                break;
            }
        }
        table.last = serial;
        let key = Arc::new(Key {
            serial,
            kind,
            description: description.to_vec(),
            user: Mutex::new(user),
            state: Mutex::new(KeyState {
                uid: owner.0,
                gid: owner.1,
                perm,
                payload,
                negative: None,
                links: Vec::new(),
                revoked: false,
                revoked_at: None,
                invalidated: false,
                expiry: None,
                quotalen,
            }),
        });
        table.keys.retain(|_, k| k.strong_count() > 0);
        table.keys.insert(serial, Arc::downgrade(&key));
        Ok(key)
    }

    /// `key_lookup`: the live key with `serial`.
    fn find(&self, serial: i32) -> Result<Arc<Key>, Errno> {
        let table = self.serials.lock_or_recover();
        let key = table.keys.get(&serial).and_then(Weak::upgrade);
        key.filter(|k| !k.is_dead()).ok_or(Errno::ENOKEY)
    }

    /// `look_up_user_keyrings`: the user keyring and user session keyring of `cred`'s real
    /// uid, made on first use; the session keyring links the user keyring.
    fn user_keyrings(&self, cred: &Cred) -> (Arc<Key>, Arc<Key>) {
        let uid = cred.user.real;
        let user = self.user(uid);
        {
            let st = user.state.lock_or_recover();
            if let (Some(k), Some(s)) = (&st.keyring, &st.session) {
                return (k.clone(), s.clone());
            }
        }
        let local = cred.user_ns.map_kuid(uid).unwrap_or(uid.0);
        let owner = (uid, cred.group.real);
        let perm = (POS_ALL & !KeyPerm::KEY_POS_SETATTR.0) | USR_ALL;
        let make = |name: String| {
            self.alloc(
                Kind::Keyring,
                name.as_bytes(),
                owner,
                perm,
                Vec::new(),
                true,
            )
            .expect("exempt from the quota")
        };
        let keyring = make(format!("_uid.{local}"));
        let session = make(format!("_uid_ses.{local}"));
        session.st().links.push(keyring.clone());
        let mut st = user.state.lock_or_recover();
        let keyring = st.keyring.get_or_insert(keyring).clone();
        let session = st.session.get_or_insert(session).clone();
        (keyring, session)
    }

    /// The keys `/proc/keys` shows the opener `ctx`: those it may view.
    pub(crate) fn proc_keys(&self, ctx: &Ctx, ns: &UserNs) -> Vec<u8> {
        let keys: Vec<Arc<Key>> = {
            let table = self.serials.lock_or_recover();
            table.keys.values().filter_map(Weak::upgrade).collect()
        };
        let mut out = String::new();
        for key in keys {
            if !ns.has_uid(key.st().uid) {
                continue;
            }
            let possessed =
                key.st().perm & KeyPerm::KEY_POS_VIEW.0 != 0 && ctx.possesses(self, &key);
            if !key.permits(&ctx.cred, possessed, Need::View) {
                continue;
            }
            let st = key.st();
            let timeout = match st.expiry {
                None => "perm".to_string(),
                Some(at) if ctx.now >= at => "expd".to_string(),
                Some(at) => {
                    let t = at - ctx.now;
                    match t {
                        0..60 => format!("{t}s"),
                        60..3600 => format!("{}m", t / 60),
                        3600..86400 => format!("{}h", t / 3600),
                        86400..604800 => format!("{}d", t / 86400),
                        _ => format!("{}w", t / 604800),
                    }
                }
            };
            let flag = |on: bool, c: char| if on { c } else { '-' };
            let usage = Arc::strong_count(&key) - 1;
            out.push_str(&format!(
                "{:08x} I{}-Q-{}{} {:5} {:>4} {:08x} {:5} {:5} {:<9.9} {}",
                key.serial,
                flag(st.revoked, 'R'),
                flag(st.negative.is_some(), 'N'),
                flag(st.invalidated, 'i'),
                usage,
                timeout,
                st.perm,
                ns.munged_uid(st.uid),
                ns.munged_gid(st.gid),
                key.kind.name(),
                String::from_utf8_lossy(&key.description),
            ));
            // `->describe`.
            match key.kind {
                Kind::Keyring => {
                    let n = st.links.iter().filter(|k| !k.is_dead()).count();
                    match n {
                        0 => out.push_str(": empty"),
                        n => out.push_str(&format!(": {n}")),
                    }
                }
                _ if !st.revoked && st.negative.is_none() => {
                    out.push_str(&format!(": {}", st.payload.len()))
                }
                _ => {}
            }
            out.push('\n');
        }
        out.into_bytes()
    }

    /// `/proc/key-users`: every user with keys that `ns` maps.
    pub(crate) fn proc_key_users(&self, ns: &UserNs) -> Vec<u8> {
        let users: Vec<Arc<KeyUser>> = self.users.lock_or_recover().values().cloned().collect();
        let mut users: Vec<_> = users.into_iter().filter(|u| ns.has_uid(u.uid)).collect();
        users.sort_by_key(|u| u.uid.0);
        let mut out = String::new();
        for user in users {
            let (maxkeys, maxbytes) = user.quota();
            let st = user.state.lock_or_recover();
            out.push_str(&format!(
                "{:5}: {:5} {}/{} {}/{} {}/{}\n",
                ns.munged_uid(user.uid),
                Arc::strong_count(&user) - 1,
                st.nkeys,
                st.nikeys,
                st.nkeys,
                maxkeys,
                st.qnbytes,
                maxbytes
            ));
        }
        out.into_bytes()
    }
}

/// The keyrings a task's credentials hold (`cred->thread_keyring`, `session_keyring`,
/// `process_keyring`, `jit_keyring`).
#[derive(Clone, Default)]
pub(crate) struct CredKeys {
    pub(crate) thread: Option<Arc<Key>>,
    pub(crate) session: Option<Arc<Key>>,
    pub(crate) process: Option<Arc<Key>>,
    /// `KEYCTL_SET_REQKEY_KEYRING`'s default (`KEY_REQKEY_DEFL_*`).
    pub(crate) reqkey: i32,
}

impl PartialEq for CredKeys {
    fn eq(&self, other: &Self) -> bool {
        let same = |a: &Option<Arc<Key>>, b: &Option<Arc<Key>>| match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        };
        same(&self.thread, &other.thread)
            && same(&self.session, &other.session)
            && same(&self.process, &other.process)
            && self.reqkey == other.reqkey
    }
}

impl CredKeys {
    /// kernel/cred.c `copy_creds`: fork inherits neither thread nor process keyrings.
    pub(crate) fn for_child(&self) -> Self {
        Self {
            thread: None,
            process: None,
            ..self.clone()
        }
    }
}

/// A key found for an operation, and whether the caller possesses it (`key_ref_t`).
struct Found {
    key: Arc<Key>,
    possessed: bool,
}

/// How a lookup may go (`KEY_LOOKUP_*`).
#[derive(Clone, Copy, Default)]
struct Lookup {
    /// `KEY_LOOKUP_CREATE`: make the special keyrings the caller lacks.
    create: bool,
    /// `KEY_LOOKUP_PARTIAL`: permit negative state, but still run key_validate.
    partial: bool,
}

/// The default permissions of a key `add_key` makes (`key_create_or_update`).
fn default_perm(kind: Kind) -> u32 {
    let mut perm = KeyPerm::KEY_POS_VIEW.0
        | KeyPerm::KEY_POS_SEARCH.0
        | KeyPerm::KEY_POS_LINK.0
        | KeyPerm::KEY_POS_SETATTR.0
        | KeyPerm::KEY_USR_VIEW.0;
    if kind.readable() {
        perm |= KeyPerm::KEY_POS_READ.0;
    }
    if kind != Kind::Logon {
        perm |= KeyPerm::KEY_POS_WRITE.0;
    }
    perm
}

/// The permissions of a session keyring (`join_session_keyring`).
const SESSION_PERM: u32 =
    POS_ALL | KeyPerm::KEY_USR_VIEW.0 | KeyPerm::KEY_USR_READ.0 | KeyPerm::KEY_USR_LINK.0;

impl Kernel {
    /// The caller's key context: its credentials, thread keyring and the time.
    fn key_ctx(&self, caller: &Caller<'_>) -> Ctx {
        Ctx {
            cred: self.cred(caller),
            now: self.host.realtime().as_secs(),
        }
    }

    /// Makes the caller's credentials hold `change`'s keyrings.
    fn set_cred_keys(&self, caller: &Caller<'_>, change: impl FnOnce(&mut CredKeys)) {
        let mut new = (*self.task_cred(caller.task())).clone();
        change(&mut new.keys);
        self.commit_cred(caller.task(), new);
    }

    /// A new keyring for the caller's credentials: `name`, with `perm`, owned by its fs ids.
    fn new_keyring(&self, ctx: &Ctx, name: &[u8], perm: u32) -> Result<Arc<Key>, Errno> {
        let owner = (ctx.cred.user.fs, ctx.cred.group.fs);
        self.keys
            .alloc(Kind::Keyring, name, owner, perm, Vec::new(), false)
    }

    /// `lookup_user_key`: the key `id` names for the caller, and whether it possesses it,
    /// if it has `need`.
    fn lookup_key(
        &self,
        caller: &Caller<'_>,
        ctx: &mut Ctx,
        id: i32,
        lookup: Lookup,
        need: Need,
    ) -> Result<Found, Errno> {
        let found = match KeySpec(id) {
            KeySpec::KEY_SPEC_THREAD_KEYRING => {
                if ctx.cred.keys.thread.is_none() {
                    if !lookup.create {
                        return Err(Errno::ENOKEY);
                    }
                    let ring = self.keys.new_thread_keyring(&ctx.cred)?;
                    self.set_cred_keys(caller, |keys| keys.thread = Some(ring.clone()));
                    Arc::make_mut(&mut ctx.cred).keys.thread = Some(ring);
                }
                Found {
                    key: ctx.cred.keys.thread.clone().expect("installed"),
                    possessed: true,
                }
            }
            KeySpec::KEY_SPEC_PROCESS_KEYRING => {
                if ctx.cred.keys.process.is_none() {
                    if !lookup.create {
                        return Err(Errno::ENOKEY);
                    }
                    let ring = self.new_keyring(ctx, b"_pid", POS_ALL | KeyPerm::KEY_USR_VIEW.0)?;
                    self.set_cred_keys(caller, |k| k.process = Some(ring.clone()));
                    Arc::make_mut(&mut ctx.cred).keys.process = Some(ring);
                }
                Found {
                    key: ctx.cred.keys.process.clone().expect("installed"),
                    possessed: true,
                }
            }
            KeySpec::KEY_SPEC_SESSION_KEYRING => {
                let (_, user_session) = self.keys.user_keyrings(&ctx.cred);
                let on_user = ctx
                    .cred
                    .keys
                    .session
                    .as_ref()
                    .is_none_or(|s| Arc::ptr_eq(s, &user_session));
                // Access always installs a session keyring: a new one when creating, else the
                // user's session keyring.
                if ctx.cred.keys.session.is_none() || (on_user && lookup.create) {
                    let ring = match lookup.create {
                        true => self.new_keyring(ctx, b"_ses", SESSION_PERM)?,
                        false => user_session,
                    };
                    self.set_cred_keys(caller, |k| k.session = Some(ring.clone()));
                    Arc::make_mut(&mut ctx.cred).keys.session = Some(ring);
                }
                Found {
                    key: ctx.cred.keys.session.clone().expect("installed"),
                    possessed: true,
                }
            }
            KeySpec::KEY_SPEC_USER_KEYRING => Found {
                key: self.keys.user_keyrings(&ctx.cred).0,
                possessed: true,
            },
            KeySpec::KEY_SPEC_USER_SESSION_KEYRING => Found {
                key: self.keys.user_keyrings(&ctx.cred).1,
                possessed: true,
            },
            KeySpec::KEY_SPEC_GROUP_KEYRING => return Err(Errno::EINVAL),
            // No request_key authorisation is ever under way.
            KeySpec::KEY_SPEC_REQKEY_AUTH_KEY | KeySpec::KEY_SPEC_REQUESTOR_KEYRING => {
                return Err(Errno::ENOKEY);
            }
            _ if id < 1 => return Err(Errno::EINVAL),
            _ => {
                let key = self.keys.find(id)?;
                let possessed = ctx.possesses(&self.keys, &key);
                Found { key, possessed }
            }
        };
        if need != Need::Unlink {
            if !lookup.partial {
                found.key.read_state()?;
            }
            found.key.validate(ctx.now)?;
        }
        if !found.key.permits(&ctx.cred, found.possessed, need) {
            return Err(Errno::EACCES);
        }
        Ok(found)
    }

    /// A keyring `id` names, with `need` (`ENOTDIR` for another key).
    fn lookup_keyring(
        &self,
        caller: &Caller<'_>,
        ctx: &mut Ctx,
        id: i32,
        need: Need,
    ) -> Result<Found, Errno> {
        let lookup = Lookup {
            create: true,
            partial: false,
        };
        let found = self.lookup_key(caller, ctx, id, lookup, need)?;
        if found.key.kind != Kind::Keyring {
            return Err(Errno::ENOTDIR);
        }
        Ok(found)
    }

    /// `add_key`.
    pub(crate) async fn sys_add_key(
        &self,
        caller: &Caller<'_>,
        kind: u32,
        description: u32,
        payload: u32,
        plen: u32,
        ringid: i32,
    ) -> SysResult<i32> {
        let kind = Kind::lookup(&read_type(caller, kind).await?)?;
        let description = read_description(caller, description).await?;
        if plen > 1024 * 1024 - 1 {
            return Err(Errno::EINVAL.into());
        }
        let mut data = vec![0; plen as usize];
        if plen > 0 {
            if payload == 0 {
                return Err(Errno::EFAULT.into());
            }
            caller.memory().read(payload, &mut data).await?;
        }
        let mut ctx = self.key_ctx(caller);
        let ring = self.lookup_keyring(caller, &mut ctx, ringid, Need::Write)?;
        kind.check_payload(&data)?;
        kind.vet_description(&description)?;
        let _construction = self.keys.construction.lock_or_recover();
        // `key_create_or_update`: a type that updates updates the key of that description the
        // keyring holds, if the caller may write it.
        if kind != Kind::Keyring
            && let Some(old) = ring.key.keys().into_iter().find(|k| {
                k.kind == kind && k.description == description && k.validate(ctx.now).is_ok()
            })
        {
            let possessed = ring.possessed;
            if !old.permits(&ctx.cred, possessed, Need::Write) {
                return Err(Errno::EACCES.into());
            }
            old.validate(ctx.now)?;
            old.charge(plen)?;
            let mut st = old.st();
            st.payload = data;
            st.negative = None;
            st.expiry = None;
            return Ok(old.serial);
        }
        let owner = (ctx.cred.user.fs, ctx.cred.group.fs);
        let key = self
            .keys
            .alloc(kind, &description, owner, default_perm(kind), data, false)?;
        link(&ring.key, &key)?;
        Ok(key.serial)
    }

    /// security/keys/request_key.c `request_key_and_link`: a lookup, or construction when
    /// callout_info is non-NULL. The unavailable usermode helper leaves a negative key linked
    /// for 60 seconds (`complete_request_key`, `construct_key_and_link`).
    pub(crate) async fn sys_request_key(
        &self,
        caller: &Caller<'_>,
        kind: u32,
        description: u32,
        callout: u32,
        dest: i32,
    ) -> SysResult<i32> {
        let kind = Kind::lookup(&read_type(caller, kind).await?)?;
        let description = read_description(caller, description).await?;
        if callout != 0 {
            // sys_request_key copies the callout even when the key is already cached.
            read_cstr(
                caller.memory(),
                wasixv2_abi::GuestPtr::new(callout),
                MAX_DESC,
            )
            .await
            .map_err(|error| match error {
                wasixv2_syscalls::CstrError::Fault => Errno::EFAULT,
                wasixv2_syscalls::CstrError::TooLong => Errno::EINVAL,
            })?;
        }
        let mut ctx = self.key_ctx(caller);
        let dest = match dest {
            0 => None,
            id => Some(self.lookup_keyring(caller, &mut ctx, id, Need::Write)?),
        };
        let roots = ctx.search_roots(&self.keys);
        let _construction = self.keys.construction.lock_or_recover();
        match search(&ctx, &roots, true, kind, &description, true) {
            Ok(key) => {
                if let Some(dest) = dest {
                    if !key.permits(&ctx.cred, true, Need::Link) {
                        return Err(Errno::EACCES.into());
                    }
                    link(&dest.key, &key)?;
                }
                Ok(key.serial)
            }
            Err(Errno::EAGAIN) if callout != 0 => {
                if kind == Kind::Keyring {
                    return Err(Errno::EPERM.into());
                }
                kind.vet_description(&description)?;
                let dest = match dest {
                    Some(dest) => dest.key,
                    None => self.request_key_destination(&ctx)?,
                };
                let key = self.keys.alloc(
                    kind,
                    &description,
                    (ctx.cred.user.fs, ctx.cred.group.fs),
                    default_perm(kind),
                    Vec::new(),
                    false,
                )?;
                // No helper is available: complete_request_key negates the allocated key.
                {
                    let mut st = key.st();
                    st.negative = Some(Errno::ENOKEY);
                    st.expiry = Some(ctx.now.saturating_add(60));
                }
                link(&dest, &key)?;
                self.keys.rearm_gc();
                Err(Errno::ENOENT.into())
            }
            Err(Errno::EAGAIN) => Err(Errno::ENOKEY.into()),
            Err(error) => Err(error.into()),
        }
    }

    /// security/keys/request_key.c `construct_get_dest_keyring`: defaults fall through to
    /// the next existing ring. A joined session still requires WRITE (CVE-2017-17807).
    fn request_key_destination(&self, ctx: &Ctx) -> Result<Arc<Key>, Errno> {
        let default = ctx.cred.keys.reqkey;
        let ring = match default {
            0 | 1 | 7 if ctx.cred.keys.thread.is_some() => {
                ctx.cred.keys.thread.clone().expect("checked")
            }
            0..=2 | 7 if ctx.cred.keys.process.is_some() => {
                ctx.cred.keys.process.clone().expect("checked")
            }
            0..=3 | 7 if ctx.cred.keys.session.is_some() => {
                ctx.cred.keys.session.clone().expect("checked")
            }
            4 => self.keys.user_keyrings(&ctx.cred).0,
            0..=3 | 5 | 7 => self.keys.user_keyrings(&ctx.cred).1,
            _ => return Err(Errno::EINVAL),
        };
        if !ring.permits(&ctx.cred, true, Need::Write) {
            return Err(Errno::EACCES);
        }
        ring.validate(ctx.now)?;
        Ok(ring)
    }

    /// `keyctl`.
    pub(crate) async fn sys_keyctl(
        &self,
        caller: &Caller<'_>,
        option: i32,
        arg2: Ulong,
        arg3: Ulong,
        arg4: Ulong,
        arg5: Ulong,
    ) -> SysResult<i32> {
        let mut ctx = self.key_ctx(caller);
        let id = arg2 as i32;
        let found = |me: &Self, ctx: &mut Ctx, lookup: Lookup, need: Need| {
            me.lookup_key(caller, ctx, id, lookup, need)
        };
        let partial = Lookup {
            create: false,
            partial: true,
        };
        match KeyctlOp(option) {
            KeyctlOp::KEYCTL_GET_KEYRING_ID => {
                let lookup = Lookup {
                    create: arg3 != 0,
                    partial: false,
                };
                let key = self
                    .lookup_key(caller, &mut ctx, id, lookup, Need::Search)?
                    .key;
                Ok(key.serial)
            }
            KeyctlOp::KEYCTL_JOIN_SESSION_KEYRING => self.join_session(caller, &ctx, arg2).await,
            KeyctlOp::KEYCTL_UPDATE => {
                let len = arg4 as usize;
                if len > 1024 * 1024 - 1 {
                    return Err(Errno::EINVAL.into());
                }
                let mut data = vec![0; len];
                if len > 0 {
                    caller.memory().read(arg3, &mut data).await?;
                }
                let key = found(self, &mut ctx, Lookup::default(), Need::Write)?.key;
                if key.kind == Kind::Keyring {
                    return Err(Errno::EOPNOTSUPP.into());
                }
                key.validate(ctx.now)?;
                key.kind.check_payload(&data)?;
                key.charge(len as u32)?;
                key.st().payload = data;
                Ok(0)
            }
            KeyctlOp::KEYCTL_REVOKE => {
                let key = match found(self, &mut ctx, Lookup::default(), Need::Write) {
                    Err(Errno::EACCES) => found(self, &mut ctx, Lookup::default(), Need::Setattr)?,
                    other => other?,
                }
                .key;
                let mut st = key.st();
                st.revoked = true;
                st.revoked_at = Some(ctx.now);
                // `keyring_revoke`: a revoked keyring holds nothing.
                let links = std::mem::take(&mut st.links);
                drop(st);
                drop(links);
                self.keys.rearm_gc();
                Ok(0)
            }
            KeyctlOp::KEYCTL_CHOWN => {
                let key = found(self, &mut ctx, partial, Need::Setattr)?.key;
                self.key_chown(&ctx, &key, arg3, arg4)?;
                Ok(0)
            }
            KeyctlOp::KEYCTL_SETPERM => {
                let perm = arg3;
                let all = POS_ALL | USR_ALL | KeyPerm::KEY_GRP_ALL.0 | KeyPerm::KEY_OTH_ALL.0;
                if perm & !all != 0 {
                    return Err(Errno::EINVAL.into());
                }
                let key = found(self, &mut ctx, partial, Need::Setattr)?.key;
                // Only the owner, or CAP_SYS_ADMIN, changes the permissions.
                let owner = key.st().uid == ctx.cred.user.fs;
                if !owner && !ctx.cred.capable(Cap::SysAdmin) {
                    return Err(Errno::EACCES.into());
                }
                key.st().perm = perm;
                Ok(0)
            }
            KeyctlOp::KEYCTL_DESCRIBE => {
                let key = found(self, &mut ctx, partial, Need::View)?.key;
                let (uid, gid, perm) = {
                    let st = key.st();
                    (st.uid, st.gid, st.perm)
                };
                let ns = &ctx.cred.user_ns;
                let (uid, gid) = (ns.munged_uid(uid), ns.munged_gid(gid));
                let mut text = format!("{};{uid};{gid};{perm:08x};", key.kind.name()).into_bytes();
                text.extend(&key.description);
                text.push(0);
                put_out(caller, arg3, arg4, &text, true).await
            }
            KeyctlOp::KEYCTL_CLEAR => {
                let ring = self.lookup_keyring(caller, &mut ctx, id, Need::Write)?;
                ring.key.st().links.clear();
                Ok(0)
            }
            KeyctlOp::KEYCTL_LINK => {
                let key = found(self, &mut ctx, Lookup::default(), Need::Link)?.key;
                let ring = self.lookup_keyring(caller, &mut ctx, arg3 as i32, Need::Write)?;
                link(&ring.key, &key)?;
                Ok(0)
            }
            KeyctlOp::KEYCTL_UNLINK => {
                let ring = self.lookup_keyring(caller, &mut ctx, arg3 as i32, Need::Write)?;
                let key = found(self, &mut ctx, Lookup::default(), Need::Unlink)?.key;
                let mut st = ring.key.st();
                let at = st.links.iter().position(|k| Arc::ptr_eq(k, &key));
                st.links.remove(at.ok_or(Errno::ENOENT)?);
                Ok(0)
            }
            KeyctlOp::KEYCTL_SEARCH => {
                let ring = self.lookup_keyring(caller, &mut ctx, id, Need::Search)?;
                let kind = Kind::lookup(&read_type(caller, arg3).await?)?;
                let description = read_description(caller, arg4).await?;
                let dest = match arg5 as i32 {
                    0 => None,
                    dest => Some(self.lookup_keyring(caller, &mut ctx, dest, Need::Write)?),
                };
                let key = search(&ctx, &[ring.key], ring.possessed, kind, &description, false)
                    .map_err(|error| {
                        if error == Errno::EAGAIN {
                            Errno::ENOKEY
                        } else {
                            error
                        }
                    })?;
                if let Some(dest) = dest {
                    let possessed = ctx.possesses(&self.keys, &key);
                    if !key.permits(&ctx.cred, possessed, Need::Link) {
                        return Err(Errno::EACCES.into());
                    }
                    link(&dest.key, &key)?;
                }
                Ok(key.serial)
            }
            KeyctlOp::KEYCTL_READ => {
                let found = match found(self, &mut ctx, Lookup::default(), Need::Unlink) {
                    Ok(found) => found,
                    Err(errno) => return Err(errno.into()),
                };
                found.key.read_state()?;
                // `keyctl_read_key`: with READ, or as a key the caller possesses and may
                // search.
                let readable = found.key.permits(&ctx.cred, found.possessed, Need::Read);
                let searchable =
                    found.possessed && found.key.permits(&ctx.cred, true, Need::Search);
                if !readable && !searchable {
                    return Err(Errno::EACCES.into());
                }
                if !found.key.kind.readable() {
                    return Err(Errno::EOPNOTSUPP.into());
                }
                found.key.validate(ctx.now)?;
                let data = match found.key.kind {
                    Kind::Keyring => found
                        .key
                        .keys()
                        .iter()
                        .flat_map(|k| k.serial.to_le_bytes())
                        .collect(),
                    _ => found.key.st().payload.clone(),
                };
                put_out(caller, arg3, arg4, &data, false).await
            }
            KeyctlOp::KEYCTL_SET_REQKEY_KEYRING => {
                let defl = arg2 as i32;
                let old = ctx.cred.keys.reqkey;
                match defl {
                    -1 => Ok(old),
                    0..=5 | 7 => {
                        let special = match defl {
                            1 => Some(KeySpec::KEY_SPEC_THREAD_KEYRING.0),
                            2 => Some(KeySpec::KEY_SPEC_PROCESS_KEYRING.0),
                            _ => None,
                        };
                        if let Some(id) = special {
                            self.lookup_keyring(caller, &mut ctx, id, Need::Write)?;
                        }
                        self.set_cred_keys(caller, |k| k.reqkey = defl);
                        Ok(old)
                    }
                    _ => Err(Errno::EINVAL.into()),
                }
            }
            KeyctlOp::KEYCTL_SET_TIMEOUT => {
                let key = found(self, &mut ctx, partial, Need::Setattr)?.key;
                key.st().expiry = match arg3 {
                    0 => None,
                    timeout => Some(ctx.now + u64::from(timeout)),
                };
                self.keys.rearm_gc();
                Ok(0)
            }
            KeyctlOp::KEYCTL_INVALIDATE => {
                let key = found(self, &mut ctx, Lookup::default(), Need::Search)?.key;
                key.st().invalidated = true;
                self.keys.rearm_gc();
                Ok(0)
            }
            KeyctlOp::KEYCTL_GET_SECURITY => {
                found(self, &mut ctx, partial, Need::View)?;
                // No LSM labels keys: an empty string.
                put_out(caller, arg3, arg4, b"\0", true).await
            }
            KeyctlOp::KEYCTL_SESSION_TO_PARENT => self.session_to_parent(caller, &mut ctx),
            KeyctlOp::KEYCTL_MOVE => {
                const EXCL: u32 = 1;
                if arg5 & !EXCL != 0 {
                    return Err(Errno::EINVAL.into());
                }
                let key = found(self, &mut ctx, Lookup::default(), Need::Link)?.key;
                let from = self.lookup_keyring(caller, &mut ctx, arg3 as i32, Need::Write)?;
                let to = self.lookup_keyring(caller, &mut ctx, arg4 as i32, Need::Write)?;
                let held = from
                    .key
                    .st()
                    .links
                    .iter()
                    .position(|k| Arc::ptr_eq(k, &key));
                let at = held.ok_or(Errno::ENOENT)?;
                if arg5 & EXCL != 0
                    && to
                        .key
                        .keys()
                        .iter()
                        .any(|k| k.kind == key.kind && k.description == key.description)
                {
                    return Err(Errno::EEXIST.into());
                }
                link(&to.key, &key)?;
                from.key.st().links.remove(at);
                Ok(0)
            }
            KeyctlOp::KEYCTL_CAPABILITIES => {
                let caps0 = 0x01 | 0x20 | 0x80;
                put_out(caller, arg2, arg3, &[caps0, 0], false).await
            }
            // security/keys/keyctl.c: no request-key token is ever installed without a
            // usermode helper, but argument validation still precedes the authority check.
            KeyctlOp::KEYCTL_INSTANTIATE => {
                if arg3 != 0 && arg4 > 1024 * 1024 - 1 {
                    return Err(Errno::EINVAL.into());
                }
                Err(Errno::EPERM.into())
            }
            KeyctlOp::KEYCTL_INSTANTIATE_IOV => {
                let iovecs = crate::fs::read_iovecs(
                    caller.memory(),
                    wasixv2_abi::GuestPtr::new(arg3),
                    if arg3 == 0 { 0 } else { arg4 },
                )
                .await?;
                if iovecs.iter().map(|iov| u64::from(iov.iov_len)).sum::<u64>() > 1024 * 1024 - 1 {
                    return Err(Errno::EINVAL.into());
                }
                Err(Errno::EPERM.into())
            }
            KeyctlOp::KEYCTL_NEGATE => Err(Errno::EPERM.into()),
            KeyctlOp::KEYCTL_REJECT => {
                if arg4 == 0 || arg4 >= 4095 || matches!(arg4, 512 | 513 | 514 | 516) {
                    return Err(Errno::EINVAL.into());
                }
                Err(Errno::EPERM.into())
            }
            KeyctlOp::KEYCTL_ASSUME_AUTHORITY => match id {
                0 => Ok(0),
                id if id < 0 => Err(Errno::EINVAL.into()),
                _ => Err(Errno::ENOKEY.into()),
            },
            // Persistent keyrings, Diffie-Hellman, public keys, restrictions and
            // notifications are not configured.
            _ => Err(Errno::EOPNOTSUPP.into()),
        }
    }

    /// `keyctl_chown_key`: `uid` (-1 keeps it) needs CAP_SYS_ADMIN; `gid` CAP_SYS_ADMIN or an
    /// owner in that group.
    fn key_chown(&self, ctx: &Ctx, key: &Arc<Key>, uid: u32, gid: u32) -> Result<(), Errno> {
        let ns = &ctx.cred.user_ns;
        let uid = match uid {
            u32::MAX => None,
            uid => Some(ns.make_kuid(uid).ok_or(Errno::EINVAL)?),
        };
        let gid = match gid {
            u32::MAX => None,
            gid => Some(ns.make_kgid(gid).ok_or(Errno::EINVAL)?),
        };
        let admin = ctx.cred.capable(Cap::SysAdmin);
        let mut st = key.st();
        if uid.is_some_and(|u| u != st.uid) && !admin {
            return Err(Errno::EACCES);
        }
        if let Some(gid) = gid
            && gid != st.gid
            && !admin
            && (st.uid != ctx.cred.user.fs || !ctx.cred.in_group(gid))
        {
            return Err(Errno::EACCES);
        }
        if let Some(uid) = uid
            && uid != st.uid
        {
            // The key moves to its new owner's quota (`EDQUOT` if it has no room).
            let to = self.keys.user(uid);
            let from = std::mem::replace(&mut *key.user.lock_or_recover(), to.clone());
            {
                let mut t = to.state.lock_or_recover();
                let (maxkeys, maxbytes) = to.quota();
                if t.nkeys + 1 > maxkeys || t.qnbytes + st.quotalen > maxbytes {
                    drop(t);
                    *key.user.lock_or_recover() = from;
                    return Err(Errno::EDQUOT);
                }
                t.nkeys += 1;
                t.nikeys += 1;
                t.qnbytes += st.quotalen;
            }
            let mut f = from.state.lock_or_recover();
            f.nkeys -= 1;
            f.nikeys -= 1;
            f.qnbytes -= st.quotalen;
            st.uid = uid;
        }
        if let Some(gid) = gid {
            st.gid = gid;
        }
        Ok(())
    }

    /// `join_session_keyring`: a new anonymous session keyring, or the caller's named one
    /// (made if it has none it may search), as the session keyring.
    async fn join_session(&self, caller: &Caller<'_>, ctx: &Ctx, name: u32) -> SysResult<i32> {
        let ring = match name {
            0 => self.new_keyring(ctx, b"_ses", SESSION_PERM)?,
            name => {
                let name = read_description(caller, name).await?;
                if ctx
                    .cred
                    .keys
                    .session
                    .as_ref()
                    .is_some_and(|s| s.description == name)
                {
                    return Ok(ctx.cred.keys.session.as_ref().expect("checked").serial);
                }
                let named = {
                    let table = self.keys.serials.lock_or_recover();
                    table.keys.values().filter_map(Weak::upgrade).find(|k| {
                        k.kind == Kind::Keyring
                            && k.description == name
                            && !k.is_dead()
                            && k.st().uid == ctx.cred.user.real
                    })
                };
                match named {
                    Some(ring) if ring.permits(&ctx.cred, false, Need::Search) => ring,
                    Some(_) => return Err(Errno::EACCES.into()),
                    None => self.new_keyring(ctx, &name, SESSION_PERM)?,
                }
            }
        };
        let serial = ring.serial;
        self.set_cred_keys(caller, |k| k.session = Some(ring));
        Ok(serial)
    }

    /// `keyctl_session_to_parent`: the parent gets the caller's session keyring, if it has
    /// the caller's ids and is not init (`EPERM`).
    fn session_to_parent(&self, caller: &Caller<'_>, ctx: &mut Ctx) -> SysResult<i32> {
        self.lookup_key(
            caller,
            ctx,
            KeySpec::KEY_SPEC_SESSION_KEYRING.0,
            Lookup::default(),
            Need::Link,
        )?;
        let session = ctx.cred.keys.session.clone().expect("session installed");
        let me = self.process(caller.task());
        let parent = self
            .procs
            .tree
            .lock_or_recover()
            .nodes
            .get(&me.pid.0)
            .map(|n| n.parent)
            .filter(|&pid| pid > 1)
            .ok_or(Errno::EPERM)?;
        let mut state = self.state.lock_or_recover();
        let parent = state
            .tasks
            .get_mut(&wasixv2_syscalls::TaskId(parent))
            .ok_or(Errno::EPERM)?;
        // security/keys/keyctl.c `keyctl_session_to_parent`: the parent must be
        // single threaded, with an mm, and have the child's effective ownership.
        if parent.thread.left_mm() || !parent.process.group.single_threaded() {
            return Err(Errno::EPERM.into());
        }
        let (mine, theirs) = (&ctx.cred, &parent.cred);
        if theirs
            .keys
            .session
            .as_ref()
            .is_some_and(|ring| Arc::ptr_eq(ring, &session))
        {
            return Ok(0);
        }
        if [theirs.user.real, theirs.user.effective, theirs.user.saved]
            .iter()
            .any(|&id| id != mine.user.effective)
            || [
                theirs.group.real,
                theirs.group.effective,
                theirs.group.saved,
            ]
            .iter()
            .any(|&id| id != mine.group.effective)
            || session.st().uid != mine.user.effective
            || theirs
                .keys
                .session
                .as_ref()
                .is_some_and(|ring| ring.st().uid != mine.user.effective)
        {
            return Err(Errno::EPERM.into());
        }
        parent.session_keyring_work = Some(session);
        parent.work.flag().raise();
        Ok(0)
    }

    /// security/keys/process_keys.c `key_change_session_keyring`: replace only the
    /// session ring in the parent's credentials at its own return to userspace.
    pub(crate) fn key_session_task_work(&self, task: wasixv2_syscalls::TaskId) {
        let mut state = self.state.lock_or_recover();
        let Some(task) = state.tasks.get_mut(&task) else {
            return;
        };
        let Some(ring) = task.session_keyring_work.take() else {
            return;
        };
        if task.thread.exited() || task.work.exit().is_some() {
            return;
        }
        assert!(task.cred_override.is_none(), "commit during override_creds");
        let mut cred = (*task.cred).clone();
        cred.keys.session = Some(ring);
        task.cred = Arc::new(cred);
        task.signals.publish_cred(task.cred.clone());
    }
}

/// `__key_link`: `key` into `ring`, displacing a key of the same type and description; a
/// keyring that would end up inside itself is `EDEADLK`; nested deeper than six keyrings,
/// `ELOOP`.
fn link(ring: &Arc<Key>, key: &Arc<Key>) -> Result<(), Errno> {
    if ring.st().revoked {
        return Err(Errno::EKEYREVOKED);
    }
    if key.kind == Kind::Keyring && (Arc::ptr_eq(ring, key) || reaches(key, ring, 0)?) {
        return Err(Errno::EDEADLK);
    }
    let mut st = ring.st();
    if st.links.iter().any(|k| Arc::ptr_eq(k, key)) {
        return Ok(());
    }
    st.links
        .retain(|k| !(k.kind == key.kind && k.description == key.description));
    st.links.push(key.clone());
    Ok(())
}

/// Whether `target` is `from` or a keyring `from` holds, at most six keyrings down.
fn reaches(from: &Arc<Key>, target: &Arc<Key>, depth: usize) -> Result<bool, Errno> {
    if depth > MAX_DEPTH {
        return Err(Errno::ELOOP);
    }
    for k in from.keys() {
        if Arc::ptr_eq(&k, target) {
            return Ok(true);
        }
        if k.kind == Kind::Keyring && reaches(&k, target, depth + 1)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `keyring_search_rcu` from `roots`: the first key of `kind` and `description` the caller may
/// search, through keyrings it may search. Revoked or expired matches are skipped and reported
/// if nothing better is found; a searchable miss is `EAGAIN`, distinct from a negative key.
fn search(
    ctx: &Ctx,
    roots: &[Arc<Key>],
    possessed: bool,
    kind: Kind,
    description: &[u8],
    skip_expired: bool,
) -> Result<Arc<Key>, Errno> {
    let mut queue: VecDeque<(Arc<Key>, usize)> = roots.iter().map(|k| (k.clone(), 0)).collect();
    let mut seen = HashSet::new();
    let mut skipped = Errno::EAGAIN;
    while let Some((ring, depth)) = queue.pop_front() {
        if !seen.insert(ring.serial) {
            continue;
        }
        // security/keys/keyring.c search_nested_keyrings: revoked containers are not
        // traversed, while expiry belongs to a matching candidate, not its container.
        if ring.st().revoked {
            if ring.kind == kind && ring.description == description {
                skipped = Errno::EKEYREVOKED;
            }
            continue;
        }
        if !ring.permits(&ctx.cred, possessed, Need::Search) {
            skipped = Errno::EACCES;
            continue;
        }
        for key in std::iter::once(ring.clone()).chain(ring.keys()) {
            if key.kind == kind && key.description == description {
                match key.validate(ctx.now) {
                    Err(Errno::EKEYEXPIRED) if skip_expired => continue,
                    Err(error) => {
                        skipped = error;
                        continue;
                    }
                    Ok(()) => {}
                }
                if !key.permits(&ctx.cred, possessed, Need::Search) {
                    skipped = Errno::EACCES;
                    continue;
                }
                match key.read_state() {
                    Ok(()) => return Ok(key),
                    Err(error) => skipped = error,
                }
            } else if key.kind == Kind::Keyring
                && !Arc::ptr_eq(&key, &ring)
                && depth + 1 < MAX_DEPTH
                && key.permits(&ctx.cred, possessed, Need::Search)
            {
                queue.push_back((key, depth + 1));
            }
        }
    }
    Err(skipped)
}

/// `key_get_type_from_user`: at most 31 bytes, not empty.
async fn read_type(caller: &Caller<'_>, ptr: u32) -> Result<Vec<u8>, Errno> {
    match read_cstr(caller.memory(), wasixv2_abi::GuestPtr::new(ptr), MAX_TYPE).await {
        Ok(name) if name.is_empty() => Err(Errno::EINVAL),
        Ok(name) => Ok(name),
        Err(wasixv2_syscalls::CstrError::Fault) => Err(Errno::EFAULT),
        Err(wasixv2_syscalls::CstrError::TooLong) => Err(Errno::EINVAL),
    }
}

/// `strndup_user(..., KEY_MAX_DESC_SIZE)` of a description, which may not be empty.
async fn read_description(caller: &Caller<'_>, ptr: u32) -> Result<Vec<u8>, Errno> {
    if ptr == 0 {
        return Err(Errno::EINVAL);
    }
    match read_cstr(caller.memory(), wasixv2_abi::GuestPtr::new(ptr), MAX_DESC).await {
        Ok(desc) if desc.is_empty() => Err(Errno::EINVAL),
        Ok(desc) => Ok(desc),
        Err(wasixv2_syscalls::CstrError::Fault) => Err(Errno::EFAULT),
        Err(wasixv2_syscalls::CstrError::TooLong) => Err(Errno::EINVAL),
    }
}

/// Copies `data` to the caller's `buf` of `len` bytes and returns its full length: as much as
/// fits, or with `whole` only if all of it does (`keyctl_describe_key`).
async fn put_out(
    caller: &Caller<'_>,
    buf: u32,
    len: u32,
    data: &[u8],
    whole: bool,
) -> SysResult<i32> {
    let len = len as usize;
    if buf != 0 && len > 0 && (!whole || len >= data.len()) {
        let n = len.min(data.len());
        caller.memory().write(buf, &data[..n]).await?;
    }
    Ok(data.len() as i32)
}
