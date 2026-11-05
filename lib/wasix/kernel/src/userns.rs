//! User namespaces (`kernel/user_namespace.c`, `kernel/ucount.c`): how the ids a guest names
//! map to the kernel's ids ([`Kuid`], [`Kgid`]), and which user namespace a process's
//! capabilities count in.
//!
//! A user namespace maps ranges of its ids onto ranges of its parent's (`uid_map`, `gid_map`),
//! and so, through each ancestor, onto kernel ids. A map is written once, through
//! `/proc/<pid>/uid_map` and `gid_map` ([`UserNs::write_map`]); until then nothing is mapped.
//! Every id is a kernel id inside the kernel and translated where it crosses the syscall or
//! `/proc` boundary: in with [`UserNs::make_kuid`], out with [`UserNs::munged_uid`], which
//! shows an id the namespace does not map as [`OVERFLOW_ID`].
//!
//! Whoever makes a user namespace holds every capability in it, and so over every namespace
//! it owns, but none outside ([`Cred::ns_capable`]). How many namespaces of each kind a user
//! may make in a user namespace is bounded per namespace up the chain
//! (`/proc/sys/user/max_*_namespaces`, [`Ucount`]).

use std::{
    collections::HashMap,
    fmt::Write as _,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

use wasixv2_syscalls::Errno;
use wasixv2_vfs::Cap;

use crate::{
    cred::Cred,
    ns::{Inums, Ns, NsKind, UserNs},
    sync::MutexExt,
    uidgid::{Kgid, Kuid, OVERFLOW_ID},
};

/// How deep user namespaces nest: one more below a namespace at this level is `ENOSPC`
/// (`create_user_ns`'s `parent_ns->level > 32`).
const MAX_PARENT_LEVEL: u32 = 32;
/// `UID_GID_MAP_MAX_EXTENTS`.
const MAX_EXTENTS: usize = 340;
/// Writes to a map file are shorter than a page (`map_write`).
const PAGE_SIZE: usize = 4096;
/// `/proc/sys/user/max_*_namespaces` of the initial namespace: Linux's `max_threads / 2`
/// (`fork_init`), here half the initial pid namespace's `pid_max`.
const INIT_UCOUNT_MAX: u32 = crate::pid::PID_MAX_DEFAULT / 2;
/// Their most, and that of a namespace made after boot (`INT_MAX`).
pub(crate) const UCOUNT_LIMIT: u32 = i32::MAX as u32;

/// One range of a map (`struct uid_gid_extent`): ids `first..first + count` of the namespace
/// are `lower_first..lower_first + count` of its parent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Extent {
    pub(crate) first: u32,
    pub(crate) lower_first: u32,
    pub(crate) count: u32,
}

impl Extent {
    /// Whether `id..id + count` lies in this extent's range that starts at `first`.
    fn holds(first: u32, extent_count: u32, id: u32, count: u32) -> bool {
        let (first, id) = (u64::from(first), u64::from(id));
        id >= first && id + u64::from(count) <= first + u64::from(extent_count)
    }

    /// The last id of each range.
    fn lasts(&self) -> (u32, u32) {
        (
            self.first + (self.count - 1),
            self.lower_first + (self.count - 1),
        )
    }

    /// `mappings_overlap` for two extents: either range meets the other's.
    fn overlaps(&self, other: &Extent) -> bool {
        let (last, lower_last) = self.lasts();
        let (other_last, other_lower_last) = other.lasts();
        (other.first <= last && other_last >= self.first)
            || (other.lower_first <= lower_last && other_lower_last >= self.lower_first)
    }
}

/// `struct uid_gid_map`: the extents, sorted by `first` (`forward`), and their indices sorted
/// by `lower_first` (`reverse`), so both directions are a binary search. Linux searches up to
/// five extents linearly and sorts only larger maps; one order for all is the same lookup.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct IdMap {
    forward: Vec<Extent>,
    reverse: Vec<usize>,
}

impl IdMap {
    /// A map of `extents`, which must not overlap in either range.
    pub(crate) fn new(mut extents: Vec<Extent>) -> Self {
        extents.sort_unstable_by_key(|e| e.first);
        let mut reverse: Vec<usize> = (0..extents.len()).collect();
        reverse.sort_unstable_by_key(|&i| extents[i].lower_first);
        Self {
            forward: extents,
            reverse,
        }
    }

    /// The map of the initial user namespace: every id is itself (`0 0 4294967295`).
    fn identity() -> Self {
        Self::new(vec![Extent {
            first: 0,
            lower_first: 0,
            count: u32::MAX,
        }])
    }

    /// The extents, by `first`.
    pub(crate) fn extents(&self) -> &[Extent] {
        &self.forward
    }

    /// `map_id_range_down`: the parent's ids for `id..id + count`, which must lie in one
    /// extent.
    pub(crate) fn range_down(&self, id: u32, count: u32) -> Option<u32> {
        let at = self
            .forward
            .partition_point(|e| e.first <= id)
            .checked_sub(1)?;
        let e = &self.forward[at];
        Extent::holds(e.first, e.count, id, count).then(|| id - e.first + e.lower_first)
    }

    /// `map_id_down`.
    pub(crate) fn down(&self, id: u32) -> Option<u32> {
        self.range_down(id, 1)
    }

    /// `map_id_up`: the namespace's id for the parent's `id`.
    pub(crate) fn up(&self, id: u32) -> Option<u32> {
        let at = self
            .reverse
            .partition_point(|&i| self.forward[i].lower_first <= id)
            .checked_sub(1)?;
        let e = &self.forward[self.reverse[at]];
        Extent::holds(e.lower_first, e.count, id, 1).then(|| id - e.lower_first + e.first)
    }
}

/// Which map of a user namespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MapKind {
    Uid,
    Gid,
    Projid,
}

impl MapKind {
    /// The capability writing the map needs over the parent (`cap_setid`); none for project
    /// ids, which anyone may map.
    fn cap(self) -> Option<Cap> {
        match self {
            MapKind::Uid => Some(Cap::Setuid),
            MapKind::Gid => Some(Cap::Setgid),
            MapKind::Projid => None,
        }
    }
}

/// What a user may have a bounded number of in a user namespace (`enum ucount_type`):
/// namespaces of a kind, inotify instances and inotify watches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UcountKind {
    Ns(NsKind),
    InotifyInstances,
    InotifyWatches,
}

impl From<NsKind> for UcountKind {
    fn from(kind: NsKind) -> Self {
        UcountKind::Ns(kind)
    }
}

/// The counted kinds, in `/proc/sys/user`'s order.
pub(crate) const UCOUNT_KINDS: [UcountKind; 10] = [
    UcountKind::Ns(NsKind::User),
    UcountKind::Ns(NsKind::Pid),
    UcountKind::Ns(NsKind::Uts),
    UcountKind::Ns(NsKind::Ipc),
    UcountKind::Ns(NsKind::Net),
    UcountKind::Ns(NsKind::Mnt),
    UcountKind::Ns(NsKind::Cgroup),
    UcountKind::Ns(NsKind::Time),
    UcountKind::InotifyInstances,
    UcountKind::InotifyWatches,
];

fn ucount_index(kind: impl Into<UcountKind>) -> usize {
    let kind = kind.into();
    UCOUNT_KINDS
        .iter()
        .position(|&k| k == kind)
        .expect("every kind is counted")
}

/// `INOTIFY_WATCH_COST` of a 32-bit kernel: an `inotify_inode_mark` and two inodes' worth.
const INOTIFY_WATCH_COST: u64 = 64 + 2 * 384;

/// The initial namespace's limit of `kind` (`inotify_user_setup`): 128 inotify instances,
/// and watches for a hundredth of memory, between 8192 and 1048576.
fn init_ucount_max(kind: UcountKind) -> u32 {
    match kind {
        UcountKind::Ns(_) => INIT_UCOUNT_MAX,
        UcountKind::InotifyInstances => 128,
        UcountKind::InotifyWatches => {
            let watches = crate::resource::ADDRESS_SPACE / 100 / INOTIFY_WATCH_COST;
            watches.clamp(8192, 1_048_576) as u32
        }
    }
}

/// What a user namespace holds of its own (`struct user_namespace`); its parent is its owner
/// ([`Ns::owner_ns`]).
pub(crate) struct User {
    /// How deep it is: 0 for the initial namespace.
    level: u32,
    /// The effective uid of its creator (`owner`).
    owner: Kuid,
    /// The creator held `CAP_SETFCAP`, so it may map its parent's root (`verify_root_map`).
    parent_could_setfcap: bool,
    uid_map: OnceLock<IdMap>,
    gid_map: OnceLock<IdMap>,
    projid_map: OnceLock<IdMap>,
    /// `USERNS_SETGROUPS_ALLOWED`.
    setgroups_allowed: AtomicBool,
    /// Serialises the writes of the maps and of `setgroups` (`userns_state_mutex`).
    writes: Mutex<()>,
    /// `ucount_max`: how many of each kind of [`UCOUNT_KINDS`] one user may have in this
    /// namespace and below.
    ucount_max: [AtomicU32; UCOUNT_KINDS.len()],
    /// How many each user has here and below, by kernel uid (`struct ucounts`).
    ucounts: Mutex<HashMap<Kuid, [u32; UCOUNT_KINDS.len()]>>,
}

impl User {
    /// The initial user namespace's: every id maps to itself, and root made it.
    pub(crate) fn initial() -> Self {
        let user = Self::new(0, Kuid::GLOBAL_ROOT, true, true, INIT_UCOUNT_MAX);
        for (max, kind) in user.ucount_max.iter().zip(UCOUNT_KINDS) {
            max.store(init_ucount_max(kind), Ordering::Relaxed);
        }
        for map in [&user.uid_map, &user.gid_map, &user.projid_map] {
            let _ = map.set(IdMap::identity());
        }
        user
    }

    fn new(
        level: u32,
        owner: Kuid,
        parent_could_setfcap: bool,
        setgroups_allowed: bool,
        ucount_max: u32,
    ) -> Self {
        Self {
            level,
            owner,
            parent_could_setfcap,
            uid_map: OnceLock::new(),
            gid_map: OnceLock::new(),
            projid_map: OnceLock::new(),
            setgroups_allowed: AtomicBool::new(setgroups_allowed),
            writes: Mutex::new(()),
            ucount_max: std::array::from_fn(|_| AtomicU32::new(ucount_max)),
            ucounts: Mutex::default(),
        }
    }

    fn map(&self, kind: MapKind) -> &OnceLock<IdMap> {
        match kind {
            MapKind::Uid => &self.uid_map,
            MapKind::Gid => &self.gid_map,
            MapKind::Projid => &self.projid_map,
        }
    }
}

/// One thing a user has, a namespace it made or an inotify instance or watch, counted against
/// its limit in the user namespace that owns it and in each above, up to the initial one
/// (`inc_ucount`); dropped with what it counts (`dec_ucount`).
pub(crate) struct Ucount {
    /// Where the count starts, and whose.
    ns: Arc<UserNs>,
    uid: Kuid,
    kind: usize,
}

impl Ucount {
    /// `inc_ucount`: counts a namespace of `kind` that `uid` makes, owned by `ns`: in `ns` for
    /// `uid`, then in each parent for the creator of the namespace below it. `ENOSPC` when a
    /// count would pass its namespace's limit.
    pub(crate) fn charge(
        ns: &Arc<UserNs>,
        uid: Kuid,
        kind: impl Into<UcountKind>,
    ) -> Result<Self, Errno> {
        let kind = ucount_index(kind);
        let mut at = Some((ns.clone(), uid));
        let mut charged: Vec<(Arc<UserNs>, Kuid)> = Vec::new();
        while let Some((ns, uid)) = at {
            let max = ns.ucount_max[kind].load(Ordering::Relaxed);
            let mut counts = ns.ucounts.lock_or_recover();
            let count = &mut counts.entry(uid).or_default()[kind];
            if *count >= max {
                drop(counts);
                ns.uncharge(uid, kind, false);
                for (ns, uid) in charged {
                    ns.uncharge(uid, kind, true);
                }
                return Err(Errno::ENOSPC);
            }
            *count += 1;
            drop(counts);
            at = ns.owner_ns().map(|parent| (parent.clone(), ns.owner));
            charged.push((ns, uid));
        }
        Ok(Self {
            ns: ns.clone(),
            uid,
            kind,
        })
    }
}

impl Ucount {
    /// Another count of `kind` for the same user in the same namespace.
    pub(crate) fn charge_another(&self, kind: UcountKind) -> Result<Ucount, Errno> {
        Ucount::charge(&self.ns, self.uid, kind)
    }
}

impl Drop for Ucount {
    fn drop(&mut self) {
        let mut at = Some((self.ns.clone(), self.uid));
        while let Some((ns, uid)) = at {
            ns.uncharge(uid, self.kind, true);
            at = ns.owner_ns().map(|parent| (parent.clone(), ns.owner));
        }
    }
}

/// The ids of a guest's map file, one extent a line: `first lower count`, blanks between and
/// around (`map_write`'s parsing). `EINVAL` for anything else, an empty map, a range that
/// wraps, ranges that overlap, or more than [`MAX_EXTENTS`].
fn parse_map(data: &[u8]) -> Result<Vec<Extent>, Errno> {
    // `memdup_user_nul`: the text ends at its first NUL.
    let data = data.split(|&b| b == 0).next().unwrap_or_default();
    let text = std::str::from_utf8(data).map_err(|_| Errno::EINVAL)?;
    let mut lines: Vec<&str> = text.split('\n').collect();
    // A newline that ends the text starts no line.
    if lines.len() > 1 && lines.last() == Some(&"") {
        lines.pop();
    }
    // `isspace`.
    let is_space = |c: char| c.is_ascii_whitespace() || c == '\u{b}';
    let mut extents: Vec<Extent> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let fields: Vec<&str> = line.split(is_space).filter(|f| !f.is_empty()).collect();
        let [first, lower_first, count] = fields[..] else {
            return Err(Errno::EINVAL);
        };
        // `kstrtou32` in base 10, which takes a leading `+` as `str::parse` does.
        let parse = |field: &str| field.parse::<u32>().map_err(|_| Errno::EINVAL);
        let extent = Extent {
            first: parse(first)?,
            lower_first: parse(lower_first)?,
            count: parse(count)?,
        };
        if extent.first == u32::MAX || extent.lower_first == u32::MAX {
            return Err(Errno::EINVAL);
        }
        // A count of 0, or one that wraps either range.
        let wraps = |start: u32| {
            start
                .checked_add(extent.count)
                .is_none_or(|end| end <= start)
        };
        if wraps(extent.first) || wraps(extent.lower_first) {
            return Err(Errno::EINVAL);
        }
        if extents.iter().any(|e| e.overlaps(&extent)) {
            return Err(Errno::EINVAL);
        }
        if extents.len() + 1 == MAX_EXTENTS && index + 1 < lines.len() {
            return Err(Errno::EINVAL);
        }
        extents.push(extent);
    }
    match extents.is_empty() {
        true => Err(Errno::EINVAL),
        false => Ok(extents),
    }
}

impl Ns<User> {
    /// The initial user namespace.
    pub(crate) fn initial_user() -> Arc<UserNs> {
        Ns::initial(NsKind::User, None, User::initial())
    }

    /// `create_user_ns` and `set_cred_user_ns`: a new user namespace below `cred`'s, which
    /// `cred`'s effective ids own, and the credentials in it, with every capability. `ENOSPC`
    /// past the deepest level or the creator's `max_user_namespaces`; `EPERM` for a creator
    /// that is `chrooted` (whose root is not its mount namespace's) or whose ids the parent
    /// does not map.
    pub(crate) fn create(cred: &Cred, chrooted: bool, inums: &Arc<Inums>) -> Result<Cred, Errno> {
        let parent = &cred.user_ns;
        if parent.level > MAX_PARENT_LEVEL {
            return Err(Errno::ENOSPC);
        }
        let (owner, group) = (cred.user.effective, cred.group.effective);
        let charge = Ucount::charge(parent, owner, NsKind::User)?;
        if chrooted || !parent.has_uid(owner) || !parent.has_gid(group) {
            return Err(Errno::EPERM);
        }
        let user = User::new(
            parent.level + 1,
            owner,
            cred.effective & Cap::Setfcap.bit() != 0,
            parent.setgroups_allowed(),
            UCOUNT_LIMIT,
        );
        let ns = Ns::new(NsKind::User, parent.clone(), user, inums, charge)?;
        Ok(cred.for_user_ns(ns))
    }

    /// Takes one back from `uid`'s count of namespaces of the kind at `kind`, if `counted`,
    /// and forgets a user with nothing counted.
    fn uncharge(&self, uid: Kuid, kind: usize, counted: bool) {
        let mut counts = self.ucounts.lock_or_recover();
        if let Some(count) = counts.get_mut(&uid) {
            if counted {
                count[kind] = count[kind].saturating_sub(1);
            }
            if count.iter().all(|&n| n == 0) {
                counts.remove(&uid);
            }
        }
    }

    /// The limit of `kind` (`max_*_namespaces`, `max_inotify_*`).
    pub(crate) fn ucount_max(&self, kind: impl Into<UcountKind>) -> u32 {
        self.ucount_max[ucount_index(kind)].load(Ordering::Relaxed)
    }

    /// Sets `max_*_namespaces` for `kind`: 0 up to `INT_MAX` (`EINVAL` beyond).
    pub(crate) fn set_ucount_max(
        &self,
        kind: impl Into<UcountKind>,
        max: u32,
    ) -> Result<(), Errno> {
        if max > UCOUNT_LIMIT {
            return Err(Errno::EINVAL);
        }
        self.ucount_max[ucount_index(kind)].store(max, Ordering::Relaxed);
        Ok(())
    }

    /// The namespace this one is in (`parent`); none for the initial one.
    pub(crate) fn parent(&self) -> Option<&Arc<UserNs>> {
        self.owner_ns()
    }

    pub(crate) fn level(&self) -> u32 {
        self.level
    }

    pub(crate) fn is_initial(&self) -> bool {
        self.level == 0
    }

    /// The effective uid of the namespace's creator (`owner`).
    pub(crate) fn owner(&self) -> Kuid {
        self.owner
    }

    pub(crate) fn uid_map(&self) -> Option<&IdMap> {
        self.uid_map.get()
    }

    pub(crate) fn gid_map(&self) -> Option<&IdMap> {
        self.gid_map.get()
    }

    pub(crate) fn setgroups_allowed(&self) -> bool {
        self.setgroups_allowed.load(Ordering::Acquire)
    }

    /// `in_userns`: whether this namespace is `ancestor` or below it.
    pub(crate) fn descends_from(self: &Arc<Self>, ancestor: &Arc<UserNs>) -> bool {
        let mut at = self;
        while at.level > ancestor.level {
            at = at.parent().expect("a namespace below another has a parent");
        }
        Arc::ptr_eq(at, ancestor)
    }

    /// `make_kuid`: the kernel id of the namespace's `uid`, if it maps it.
    pub(crate) fn make_kuid(&self, uid: u32) -> Option<Kuid> {
        self.uid_map()?.down(uid).map(Kuid)
    }

    /// `make_kgid`.
    pub(crate) fn make_kgid(&self, gid: u32) -> Option<Kgid> {
        self.gid_map()?.down(gid).map(Kgid)
    }

    /// `from_kuid`: the namespace's uid for `kuid`, if it maps it.
    pub(crate) fn map_kuid(&self, kuid: Kuid) -> Option<u32> {
        self.uid_map()?.up(kuid.0)
    }

    /// `from_kgid`.
    pub(crate) fn map_kgid(&self, kgid: Kgid) -> Option<u32> {
        self.gid_map()?.up(kgid.0)
    }

    /// `from_kuid_munged`: the namespace's uid for `kuid`, or the overflow uid.
    pub(crate) fn munged_uid(&self, kuid: Kuid) -> u32 {
        self.map_kuid(kuid).unwrap_or(OVERFLOW_ID)
    }

    /// `from_kgid_munged`.
    pub(crate) fn munged_gid(&self, kgid: Kgid) -> u32 {
        self.map_kgid(kgid).unwrap_or(OVERFLOW_ID)
    }

    /// `kuid_has_mapping`.
    pub(crate) fn has_uid(&self, kuid: Kuid) -> bool {
        self.map_kuid(kuid).is_some()
    }

    /// `kgid_has_mapping`.
    pub(crate) fn has_gid(&self, kgid: Kgid) -> bool {
        self.map_kgid(kgid).is_some()
    }

    /// The kernel id of the namespace's root (`make_kuid(ns, 0)`), if it maps one.
    pub(crate) fn root_uid(&self) -> Option<Kuid> {
        self.make_kuid(0)
    }

    /// `make_kgid(ns, 0)`.
    pub(crate) fn root_gid(&self) -> Option<Kgid> {
        self.make_kgid(0)
    }

    /// `uid_m_show` and its twins: the map as a reader in the user namespace `reader` sees
    /// it, the parent's ids as `reader`'s, or as the parent's when the reader is in this
    /// namespace (`4294967295` for one `reader` does not map).
    pub(crate) fn show_map(self: &Arc<Self>, kind: MapKind, reader: &Arc<UserNs>) -> Vec<u8> {
        let lower_ns = match (Arc::ptr_eq(self, reader), self.parent()) {
            (true, Some(parent)) => parent,
            _ => reader,
        };
        let mut out = String::new();
        for e in self.map(kind).get().map_or(&[][..], IdMap::extents) {
            let lower = lower_ns
                .map(kind)
                .get()
                .and_then(|map| map.up(e.lower_first))
                .unwrap_or(u32::MAX);
            let _ = writeln!(out, "{:10} {:10} {:10}", e.first, lower, e.count);
        }
        out.into_bytes()
    }

    /// `proc_uid_map_write`, `proc_gid_map_write`, `proc_projid_map_write` and `map_write`:
    /// sets the map, once, from `data` written at `offset` through a file opened with
    /// `opener`'s credentials. The opener must be in this namespace or its parent, and hold
    /// `CAP_SYS_ADMIN` over it (but for project ids); the ids it maps must be its own or it
    /// must hold `CAP_SETUID`/`CAP_SETGID` over the parent (`new_idmap_permitted`); each range
    /// must map in the parent. `EPERM` for each of these, `EINVAL` for a bad write.
    pub(crate) fn write_map(
        self: &Arc<Self>,
        kind: MapKind,
        opener: &Cred,
        offset: u64,
        data: &[u8],
    ) -> Result<usize, Errno> {
        let parent = self.parent().ok_or(Errno::EPERM)?;
        if !Arc::ptr_eq(&opener.user_ns, self) && !Arc::ptr_eq(&opener.user_ns, parent) {
            return Err(Errno::EPERM);
        }
        if offset != 0 || data.len() >= PAGE_SIZE {
            return Err(Errno::EINVAL);
        }
        let _writes = self.writes.lock_or_recover();
        let map = self.map(kind);
        if map.get().is_some() {
            return Err(Errno::EPERM);
        }
        if kind.cap().is_some() && !opener.ns_capable(self, Cap::SysAdmin) {
            return Err(Errno::EPERM);
        }
        let mut extents = parse_map(data)?;
        if !self.idmap_permitted(kind, opener, &extents) {
            return Err(Errno::EPERM);
        }
        // The parent's ids become kernel ids.
        let parent_map = parent.map(kind).get().ok_or(Errno::EPERM)?;
        for e in &mut extents {
            e.lower_first = parent_map
                .range_down(e.lower_first, e.count)
                .ok_or(Errno::EPERM)?;
        }
        let _ = map.set(IdMap::new(extents));
        Ok(data.len())
    }

    /// `new_idmap_permitted`. Linux checks the capability over the parent for both the writer
    /// and the opener; the file's credentials are the opener's, which is the writer unless a
    /// descriptor was handed on.
    fn idmap_permitted(self: &Arc<Self>, kind: MapKind, opener: &Cred, extents: &[Extent]) -> bool {
        let parent = self.parent().expect("checked by the caller");
        if kind == MapKind::Uid && !self.root_map_permitted(opener, extents) {
            return false;
        }
        // Mapping the opener's own id needs nothing it does not already have.
        if let [only] = extents
            && only.count == 1
            && self.owner == opener.user.effective
        {
            let own = match kind {
                MapKind::Uid => parent.make_kuid(only.lower_first) == Some(opener.user.effective),
                MapKind::Gid => {
                    !self.setgroups_allowed()
                        && parent.make_kgid(only.lower_first) == Some(opener.group.effective)
                }
                MapKind::Projid => false,
            };
            if own {
                return true;
            }
        }
        match kind.cap() {
            None => true,
            Some(cap) => opener.ns_capable(parent, cap),
        }
    }

    /// `verify_root_map`: mapping the parent's root needs `CAP_SETFCAP`: its creator's when
    /// it was made, for an opener inside the namespace, else the opener's over the parent.
    fn root_map_permitted(self: &Arc<Self>, opener: &Cred, extents: &[Extent]) -> bool {
        if !extents.iter().any(|e| e.lower_first == 0) {
            return true;
        }
        let parent = self.parent().expect("checked by the caller");
        match Arc::ptr_eq(&opener.user_ns, self) {
            true => self.parent_could_setfcap,
            false => opener.ns_capable(parent, Cap::Setfcap),
        }
    }

    /// `proc_setgroups_show`.
    pub(crate) fn show_setgroups(&self) -> Vec<u8> {
        match self.setgroups_allowed() {
            true => b"allow\n".to_vec(),
            false => b"deny\n".to_vec(),
        }
    }

    /// `proc_setgroups_write`: `allow` (which a denied namespace refuses) or `deny` (which a
    /// namespace whose gid map is written refuses), with blanks after; `EINVAL` for anything
    /// else and `EPERM` for a refusal. Opening the file to write needs `CAP_SYS_ADMIN` over the
    /// namespace (`proc_setgroups_open`).
    pub(crate) fn write_setgroups(&self, offset: u64, data: &[u8]) -> Result<usize, Errno> {
        // `char kbuf[8]`.
        if offset != 0 || data.len() >= 8 {
            return Err(Errno::EINVAL);
        }
        let text = data.split(|&b| b == 0).next().unwrap_or_default();
        let (allow, rest) = if let Some(rest) = text.strip_prefix(b"allow") {
            (true, rest)
        } else if let Some(rest) = text.strip_prefix(b"deny") {
            (false, rest)
        } else {
            return Err(Errno::EINVAL);
        };
        if !rest.iter().all(|&b| b.is_ascii_whitespace() || b == 0x0b) {
            return Err(Errno::EINVAL);
        }
        let _writes = self.writes.lock_or_recover();
        match allow {
            true if !self.setgroups_allowed() => return Err(Errno::EPERM),
            true => {}
            false if self.gid_map.get().is_some() => return Err(Errno::EPERM),
            false => self.setgroups_allowed.store(false, Ordering::Release),
        }
        Ok(data.len())
    }

    /// `userns_may_setgroups`: only once the gid map is written, and while `setgroups` is
    /// allowed.
    pub(crate) fn may_setgroups(&self) -> bool {
        let _writes = self.writes.lock_or_recover();
        self.gid_map.get().is_some() && self.setgroups_allowed()
    }
}

/// The VFS's view: kernel ids, as the superblocks and credentials it checks hold them.
impl wasixv2_vfs::UserNs for Ns<User> {
    fn has_uid(&self, kuid: u32) -> bool {
        self.map_kuid(Kuid(kuid)).is_some()
    }

    fn has_gid(&self, kgid: u32) -> bool {
        self.map_kgid(Kgid(kgid)).is_some()
    }

    fn make_kuid(&self, uid: u32) -> Option<u32> {
        Ns::make_kuid(self, uid).map(|kuid| kuid.0)
    }

    fn make_kgid(&self, gid: u32) -> Option<u32> {
        Ns::make_kgid(self, gid).map(|kgid| kgid.0)
    }

    fn ns_uid(&self, kuid: u32) -> Option<u32> {
        self.map_kuid(Kuid(kuid))
    }

    fn ns_gid(&self, kgid: u32) -> Option<u32> {
        self.map_kgid(Kgid(kgid))
    }

    fn is_initial(&self) -> bool {
        self.level == 0
    }
}

/// The kernel's user namespace that `owner`, a superblock's or credential's as the VFS holds
/// it, is: the initial one `init` for what the VFS made before the kernel knew.
pub(crate) fn kernel_user_ns(
    owner: &Arc<dyn wasixv2_vfs::UserNs>,
    init: &Arc<UserNs>,
) -> Arc<UserNs> {
    let any: Arc<dyn std::any::Any + Send + Sync> = owner.clone();
    any.downcast::<UserNs>().unwrap_or_else(|_| init.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cred::{Credentials, Ids};

    fn all<T: Copy>(id: T) -> Ids<T> {
        Ids {
            real: id,
            effective: id,
            saved: id,
            fs: id,
        }
    }

    fn map(extents: &[(u32, u32, u32)]) -> IdMap {
        IdMap::new(
            extents
                .iter()
                .map(|&(first, lower_first, count)| Extent {
                    first,
                    lower_first,
                    count,
                })
                .collect(),
        )
    }

    #[test]
    fn a_map_translates_both_ways_within_its_extents() {
        let m = map(&[(1000, 0, 10), (0, 100_000, 1000)]);

        assert_eq!(m.down(0), Some(100_000));
        assert_eq!(m.down(999), Some(100_999));
        assert_eq!(m.down(1005), Some(5));
        assert_eq!(m.down(1010), None);
        assert_eq!(m.up(5), Some(1005));
        assert_eq!(m.up(100_500), Some(500));
        assert_eq!(m.up(10), None);
        assert_eq!(m.up(99_999), None);
    }

    #[test]
    fn a_range_maps_down_only_within_one_extent() {
        let m = map(&[(0, 10, 5), (5, 15, 5)]);

        assert_eq!(m.range_down(1, 4), Some(11));
        assert_eq!(
            m.range_down(1, 5),
            None,
            "contiguous below, but two extents"
        );
    }

    #[test]
    fn the_initial_namespace_maps_every_id_but_minus_one() {
        let init = UserNs::initial_user();

        assert_eq!(init.make_kuid(0), Some(Kuid(0)));
        assert_eq!(init.make_kuid(u32::MAX - 1), Some(Kuid(u32::MAX - 1)));
        assert_eq!(init.make_kuid(u32::MAX), None);
        assert_eq!(init.munged_gid(Kgid(42)), 42);
        assert!(init.is_initial() && init.parent().is_none());
    }

    #[test]
    fn map_text_is_three_numbers_a_line() {
        assert_eq!(
            parse_map(b"0 1000 1\n"),
            Ok(vec![Extent {
                first: 0,
                lower_first: 1000,
                count: 1
            }])
        );
        assert_eq!(
            parse_map(b"  0\t1000   5  \n10 2000 1").map(|e| e.len()),
            Ok(2)
        );
        for bad in [
            &b""[..],
            b"\n",
            b"0 1000",
            b"0 1000 1 4",
            b"0 1000 0",
            b"-1 1000 1",
            b"4294967295 0 1",
            b"0 4294967295 1",
            b"4294967290 0 10",
            b"0 1000 1\n\n",
            b"0 1000 5\n3 2000 1",
            b"0 1000 5\n10 1003 1",
            b"0x10 0 1",
        ] {
            assert_eq!(parse_map(bad), Err(Errno::EINVAL), "{bad:?}");
        }
    }

    #[test]
    fn a_map_has_at_most_340_extents() {
        let text = |n: u32| -> Vec<u8> {
            (0..n)
                .map(|i| format!("{} {} 1\n", i * 2, i * 2))
                .collect::<String>()
                .into_bytes()
        };

        assert_eq!(parse_map(&text(340)).map(|e| e.len()), Ok(340));
        assert_eq!(parse_map(&text(341)), Err(Errno::EINVAL));
    }

    #[test]
    fn an_unprivileged_user_maps_only_itself_once() {
        let inums = Arc::default();
        let init = UserNs::initial_user();
        let user = Cred::new(&Credentials::new(1000, 1000), init.clone());
        let inside = UserNs::create(&user, false, &inums).unwrap();
        let ns = inside.user_ns.clone();

        assert_eq!(ns.make_kuid(0), None, "nothing is mapped yet");
        assert_eq!(
            ns.write_map(MapKind::Uid, &inside, 0, b"0 1001 1"),
            Err(Errno::EPERM),
            "another user's id"
        );
        assert_eq!(
            ns.write_map(MapKind::Uid, &inside, 0, b"0 1000 2"),
            Err(Errno::EPERM),
            "more than its own id"
        );
        assert_eq!(ns.write_map(MapKind::Uid, &inside, 0, b"0 1000 1"), Ok(8));
        assert_eq!(
            ns.write_map(MapKind::Uid, &inside, 0, b"0 1000 1"),
            Err(Errno::EPERM),
            "a map is written once"
        );
        assert_eq!(ns.make_kuid(0), Some(Kuid(1000)));
        assert_eq!(ns.munged_uid(Kuid(0)), OVERFLOW_ID);
        assert_eq!(
            ns.write_map(MapKind::Gid, &inside, 0, b"0 1000 1"),
            Err(Errno::EPERM),
            "its own gid only once setgroups is denied"
        );
        assert_eq!(ns.write_setgroups(0, b"deny\n"), Ok(5));
        assert_eq!(ns.write_setgroups(0, b"allow"), Err(Errno::EPERM));
        assert_eq!(ns.write_map(MapKind::Gid, &inside, 0, b"0 1000 1"), Ok(8));
        assert!(!ns.may_setgroups());
        let line = b"         0       1000          1\n";
        assert_eq!(ns.show_map(MapKind::Uid, &ns), line);
        assert_eq!(ns.show_map(MapKind::Uid, &init), line);
    }

    #[test]
    fn root_maps_a_range_and_its_child_sees_its_own_ids() {
        let inums = Arc::default();
        let init = UserNs::initial_user();
        let root = Cred::new(&Credentials::new(0, 0), init.clone());
        let first = UserNs::create(&root, false, &inums).unwrap();
        let ns1 = first.user_ns.clone();
        // Root writes the child's map from outside, as newuidmap does.
        let range = b"0 100000 65536\n";
        assert_eq!(ns1.write_map(MapKind::Uid, &root, 0, range), Ok(15));
        assert_eq!(ns1.make_kuid(5), Some(Kuid(100_005)));
        // The gid map too, so the child's root may make a namespace of its own.
        assert_eq!(ns1.write_map(MapKind::Gid, &root, 0, range), Ok(15));
        let mut root1 = first.clone();
        root1.user = all(Kuid(100_000));
        root1.group = all(Kgid(100_000));
        let second = UserNs::create(&root1, false, &inums).unwrap();
        let ns2 = second.user_ns.clone();
        assert_eq!(ns2.write_map(MapKind::Uid, &root1, 0, b"0 1000 10"), Ok(9));
        assert_eq!(ns2.make_kuid(3), Some(Kuid(101_003)));
        let seen = |reader: &Arc<UserNs>| ns2.show_map(MapKind::Uid, reader);
        assert_eq!(seen(&ns1), b"         0       1000         10\n");
        assert_eq!(seen(&init), b"         0     101000         10\n");
        assert_eq!(seen(&ns2), b"         0       1000         10\n");
        assert_eq!(
            ns2.write_map(MapKind::Gid, &root, 0, b"0 0 1"),
            Err(Errno::EPERM),
            "an opener two levels up may not write"
        );
    }

    #[test]
    fn max_user_namespaces_bounds_each_level() {
        let inums = Arc::default();
        let init = UserNs::initial_user();
        let user = Cred::new(&Credentials::new(1000, 1000), init.clone());
        init.set_ucount_max(NsKind::User, 1).unwrap();

        let made = UserNs::create(&user, false, &inums).unwrap();
        assert_eq!(
            UserNs::create(&user, false, &inums).err(),
            Some(Errno::ENOSPC)
        );
        drop(made);
        let again = UserNs::create(&user, false, &inums).unwrap();
        // The child's root is unmapped, so it cannot be the owner of a grandchild.
        let nested = UserNs::create(&again, false, &inums);
        assert_eq!(
            nested.err(),
            Some(Errno::ENOSPC),
            "counted up the chain first"
        );
        init.set_ucount_max(NsKind::User, 0).unwrap();
        assert_eq!(
            UserNs::create(&user, false, &inums).err(),
            Some(Errno::ENOSPC)
        );
        assert_eq!(
            init.set_ucount_max(NsKind::User, u32::MAX),
            Err(Errno::EINVAL)
        );
    }
}
