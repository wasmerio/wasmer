//! User namespaces (`kernel/user_namespace.c`): how the ids a guest names map to the kernel's
//! ids ([`Kuid`], [`Kgid`]), and which user namespace a process's capabilities count in.
//!
//! A user namespace maps ranges of its ids onto ranges of its parent's (`uid_map`, `gid_map`),
//! and so, through each ancestor, onto kernel ids. A map is written once; until then nothing is
//! mapped. Every id is a kernel id inside the kernel and translated where it crosses the
//! syscall or `/proc` boundary: in with [`UserNs::make_kuid`], out with
//! [`UserNs::munged_uid`], which shows an id the namespace does not map as
//! [`OVERFLOW_ID`].

use std::sync::{Arc, OnceLock};

use crate::{
    ns::{Ns, NsKind, UserNs},
    uidgid::{Kgid, Kuid, OVERFLOW_ID},
};

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

/// What a user namespace holds of its own (`struct user_namespace`); its parent is its owner
/// ([`Ns::owner_ns`]).
pub(crate) struct User {
    /// How deep it is: 0 for the initial namespace.
    level: u32,
    /// The effective uid of its creator (`owner`).
    owner: Kuid,
    uid_map: OnceLock<IdMap>,
    gid_map: OnceLock<IdMap>,
}

impl User {
    /// The initial user namespace's: every id maps to itself, and root made it.
    pub(crate) fn initial() -> Self {
        let user = Self {
            level: 0,
            owner: Kuid::GLOBAL_ROOT,
            uid_map: OnceLock::new(),
            gid_map: OnceLock::new(),
        };
        for map in [&user.uid_map, &user.gid_map] {
            let _ = map.set(IdMap::identity());
        }
        user
    }
}

/// An unwritten map maps nothing.
fn map_of(map: &OnceLock<IdMap>) -> Option<&IdMap> {
    map.get()
}

impl Ns<User> {
    /// The initial user namespace.
    pub(crate) fn initial_user() -> Arc<UserNs> {
        Ns::initial(NsKind::User, None, User::initial())
    }

    pub(crate) fn level(&self) -> u32 {
        self.level
    }

    /// The effective uid of the namespace's creator (`owner`).
    pub(crate) fn owner(&self) -> Kuid {
        self.owner
    }

    pub(crate) fn uid_map(&self) -> Option<&IdMap> {
        map_of(&self.uid_map)
    }

    pub(crate) fn gid_map(&self) -> Option<&IdMap> {
        map_of(&self.gid_map)
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

    /// The kernel id of the namespace's root (`make_kuid(ns, 0)`), if it maps one.
    pub(crate) fn root_uid(&self) -> Option<Kuid> {
        self.make_kuid(0)
    }

    /// `make_kgid(ns, 0)`.
    pub(crate) fn root_gid(&self) -> Option<Kgid> {
        self.make_kgid(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(init.level() == 0 && init.owner_ns().is_none());
    }
}
