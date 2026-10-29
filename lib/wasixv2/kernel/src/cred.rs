//! Credentials (`struct cred`): the real, effective, saved and filesystem ids, the supplementary
//! groups, the capability sets and the securebits, and how the set*id calls, `capset`, `prctl`
//! and `execve` change them (`kernel/sys.c`, `security/commoncap.c`).
//!
//! The ids are kernel ids ([`Kuid`], [`Kgid`]); the syscalls translate them through the
//! process's user namespace ([`Cred::user_ns`]), whose root is privileged the way Linux makes
//! it: a process whose effective uid is that root holds every capability, and the set*id
//! transitions drop and regain them as on Linux. Credentials are per process; Linux keeps them
//! per thread, but libc applies every set*id call to all threads, so a program cannot tell.

use std::{fmt, sync::Arc};

use wasixv2_syscalls::Errno;
use wasixv2_vfs::{Cap, Cred as FsCred};

use crate::{
    ns::UserNs,
    uidgid::{Kgid, Kuid, NO_ID},
};

/// `CAP_VALID_MASK`: every capability.
pub(crate) const CAP_FULL: u64 = (1 << (Cap::LAST + 1)) - 1;
/// `CAP_FS_MASK`: the capabilities a filesystem uid of 0 historically meant.
const CAP_FS_MASK: u64 = Cap::Chown.bit()
    | Cap::Mknod.bit()
    | Cap::DacOverride.bit()
    | Cap::DacReadSearch.bit()
    | Cap::Fowner.bit()
    | Cap::Fsetid.bit()
    | Cap::MacOverride.bit();
/// `NGROUPS_MAX`.
pub(crate) const NGROUPS_MAX: usize = 65536;

/// The securebits (`include/uapi/linux/securebits.h`).
pub(crate) mod secure {
    pub(crate) const NOROOT: u32 = 1 << 0;
    pub(crate) const NO_SETUID_FIXUP: u32 = 1 << 2;
    pub(crate) const KEEP_CAPS: u32 = 1 << 4;
    pub(crate) const KEEP_CAPS_LOCKED: u32 = 1 << 5;
    pub(crate) const NO_CAP_AMBIENT_RAISE: u32 = 1 << 6;
    const EXEC_RESTRICT_FILE: u32 = 1 << 8;
    const EXEC_DENY_INTERACTIVE: u32 = 1 << 10;
    pub(crate) const ALL_BITS: u32 = NOROOT
        | NO_SETUID_FIXUP
        | KEEP_CAPS
        | NO_CAP_AMBIENT_RAISE
        | EXEC_RESTRICT_FILE
        | EXEC_DENY_INTERACTIVE;
    pub(crate) const ALL_LOCKS: u32 = ALL_BITS << 1;
    /// Restrictions user space enforces on itself, which need no privilege.
    pub(crate) const ALL_UNPRIVILEGED: u32 = EXEC_RESTRICT_FILE | EXEC_DENY_INTERACTIVE;
}

/// The identity a machine's first process runs as, in the initial user namespace. Its children
/// inherit it, and from there it changes only as the guest's set*id calls and `execve`s change
/// it.
///
/// The default is uid and gid 1 with gid 1 as the only supplementary group. uid 0 is root and
/// holds every capability, so permission checks pass that would fail for anyone else.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Credentials {
    uid: u32,
    gid: u32,
    groups: Vec<u32>,
}

impl Default for Credentials {
    fn default() -> Self {
        Self::new(1, 1)
    }
}

impl Credentials {
    /// `uid` and `gid`, with `gid` as the only supplementary group.
    ///
    /// # Panics
    ///
    /// If `uid` or `gid` is `u32::MAX`, which is `(uid_t)-1` and not an id.
    pub fn new(uid: u32, gid: u32) -> Self {
        assert!(uid != NO_ID && gid != NO_ID, "(uid_t)-1 is not an id");
        Self {
            uid,
            gid,
            groups: vec![gid],
        }
    }

    /// Replaces the supplementary groups.
    ///
    /// # Panics
    ///
    /// If there are more than 65536 (`NGROUPS_MAX`), or one is `u32::MAX`.
    pub fn with_groups(mut self, groups: impl IntoIterator<Item = u32>) -> Self {
        let mut groups: Vec<u32> = groups.into_iter().collect();
        assert!(groups.len() <= NGROUPS_MAX, "more than NGROUPS_MAX groups");
        assert!(!groups.contains(&NO_ID), "(gid_t)-1 is not a group");
        groups.sort_unstable();
        self.groups = groups;
        self
    }

    pub fn uid(&self) -> u32 {
        self.uid
    }

    pub fn gid(&self) -> u32 {
        self.gid
    }

    /// The supplementary groups, in ascending order.
    pub fn groups(&self) -> &[u32] {
        &self.groups
    }
}

/// One kind of id: the real, effective, saved and filesystem uid, or the same four gids.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Ids<T> {
    pub(crate) real: T,
    pub(crate) effective: T,
    pub(crate) saved: T,
    pub(crate) fs: T,
}

impl<T: Copy + Eq> Ids<T> {
    fn all(id: T) -> Self {
        Self {
            real: id,
            effective: id,
            saved: id,
            fs: id,
        }
    }

    /// Whether an unprivileged process may take `id`: it is its real, effective or saved id.
    pub(crate) fn holds(&self, id: T) -> bool {
        id == self.real || id == self.effective || id == self.saved
    }

    /// `__sys_setuid`: with the privilege every id but the filesystem one follows, otherwise
    /// only the effective one, to the real or saved id.
    fn set(&mut self, id: T, privileged: bool) -> Result<(), Errno> {
        if privileged {
            self.real = id;
            self.saved = id;
        } else if id != self.real && id != self.saved {
            return Err(Errno::EPERM);
        }
        self.effective = id;
        self.fs = id;
        Ok(())
    }

    /// `__sys_setreuid`: the real id may become the effective one and the effective id any of
    /// the three. The saved id follows the effective one when the real id is set or the
    /// effective one moves away from the real id. `None` leaves an id as it is.
    fn set_re(
        &mut self,
        real: Option<T>,
        effective: Option<T>,
        privileged: bool,
    ) -> Result<(), Errno> {
        let old = *self;
        if let Some(real) = real {
            if real != old.real && real != old.effective && !privileged {
                return Err(Errno::EPERM);
            }
            self.real = real;
        }
        if let Some(effective) = effective {
            if !old.holds(effective) && !privileged {
                return Err(Errno::EPERM);
            }
            self.effective = effective;
        }
        if real.is_some() || effective.is_some_and(|id| id != old.real) {
            self.saved = self.effective;
        }
        self.fs = self.effective;
        Ok(())
    }

    /// `__sys_setresuid`: without the privilege, each new id is one of the current three.
    fn set_res(&mut self, ids: [Option<T>; 3], privileged: bool) -> Result<(), Errno> {
        if !privileged && !ids.iter().flatten().all(|&id| self.holds(id)) {
            return Err(Errno::EPERM);
        }
        let [real, effective, saved] = ids;
        for (slot, id) in [
            (&mut self.real, real),
            (&mut self.effective, effective),
            (&mut self.saved, saved),
        ] {
            if let Some(id) = id {
                *slot = id;
            }
        }
        self.fs = self.effective;
        Ok(())
    }

    /// One of the set*id calls but `setfsuid`.
    fn apply(&mut self, change: IdChange<T>, privileged: bool) -> Result<(), Errno> {
        match change {
            IdChange::Id(id) => self.set(id, privileged),
            IdChange::Re(real, effective) => self.set_re(real, effective, privileged),
            IdChange::Res(real, effective, saved) => {
                self.set_res([real, effective, saved], privileged)
            }
        }
    }

    /// `__sys_setfsuid`: any of the four ids, or anything with the privilege; `None`, an id
    /// the caller's namespace does not map, changes nothing. Returns whether the filesystem id
    /// changed.
    fn set_fs(&mut self, id: Option<T>, privileged: bool) -> bool {
        match id {
            Some(id) if id != self.fs && (self.holds(id) || privileged) => {
                self.fs = id;
                true
            }
            _ => false,
        }
    }
}

/// Which ids a set*id call changes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum IdKind {
    User,
    Group,
}

/// A set*id call other than `setfsuid`/`setfsgid`, in the guest's ids (`u32`) or in kernel
/// ids. `None` leaves an id unchanged, which `-1` asks for ([`given`]).
#[derive(Clone, Copy, Debug)]
pub(crate) enum IdChange<T> {
    /// `setuid`
    Id(T),
    /// `setreuid`
    Re(Option<T>, Option<T>),
    /// `setresuid`
    Res(Option<T>, Option<T>, Option<T>),
}

/// A guest's id for a set*id call: `None` for `-1`, which leaves the id unchanged.
pub(crate) fn given(id: u32) -> Option<u32> {
    (id != NO_ID).then_some(id)
}

impl IdChange<u32> {
    /// The change in kernel ids, each of the guest's ids made one with `make`: `EINVAL` for
    /// one the caller's user namespace does not map, `setuid(-1)` included (`__sys_setuid`).
    pub(crate) fn to_kernel<T>(
        self,
        make: impl Fn(u32) -> Option<T>,
    ) -> Result<IdChange<T>, Errno> {
        let id = |id: u32| make(id).ok_or(Errno::EINVAL);
        let maybe = |slot: Option<u32>| slot.map(id).transpose();
        Ok(match self {
            IdChange::Id(uid) => IdChange::Id(id(uid)?),
            IdChange::Re(real, effective) => IdChange::Re(maybe(real)?, maybe(effective)?),
            IdChange::Res(real, effective, saved) => {
                IdChange::Res(maybe(real)?, maybe(effective)?, maybe(saved)?)
            }
        })
    }
}

/// A process's credentials.
#[derive(Clone)]
pub(crate) struct Cred {
    pub(crate) user: Ids<Kuid>,
    pub(crate) group: Ids<Kgid>,
    /// Supplementary groups, in ascending order (`groups_sort`).
    pub(crate) groups: Vec<Kgid>,
    pub(crate) inheritable: u64,
    pub(crate) permitted: u64,
    pub(crate) effective: u64,
    pub(crate) bounding: u64,
    pub(crate) ambient: u64,
    pub(crate) securebits: u32,
    /// The user namespace the capabilities count in, and the ids are seen through
    /// (`cred->user_ns`).
    pub(crate) user_ns: Arc<UserNs>,
}

impl PartialEq for Cred {
    fn eq(&self, other: &Self) -> bool {
        self.user == other.user
            && self.group == other.group
            && self.groups == other.groups
            && self.inheritable == other.inheritable
            && self.permitted == other.permitted
            && self.effective == other.effective
            && self.bounding == other.bounding
            && self.ambient == other.ambient
            && self.securebits == other.securebits
            && Arc::ptr_eq(&self.user_ns, &other.user_ns)
    }
}

impl Eq for Cred {}

impl fmt::Debug for Cred {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cred")
            .field("user", &self.user)
            .field("group", &self.group)
            .field("groups", &self.groups)
            .field("inheritable", &self.inheritable)
            .field("permitted", &self.permitted)
            .field("effective", &self.effective)
            .field("bounding", &self.bounding)
            .field("ambient", &self.ambient)
            .field("securebits", &self.securebits)
            .field("user_ns_level", &self.user_ns.level())
            .finish()
    }
}

impl Cred {
    /// The first process's credentials, in the initial user namespace `user_ns`: root holds
    /// every capability, as init does; anyone else none, as after a login program's `setuid`.
    pub(crate) fn new(identity: &Credentials, user_ns: Arc<UserNs>) -> Self {
        let caps = match identity.uid {
            0 => CAP_FULL,
            _ => 0,
        };
        Self {
            user: Ids::all(Kuid(identity.uid)),
            group: Ids::all(Kgid(identity.gid)),
            groups: identity.groups.iter().copied().map(Kgid).collect(),
            inheritable: 0,
            permitted: caps,
            effective: caps,
            bounding: CAP_FULL,
            ambient: 0,
            securebits: 0,
            user_ns,
        }
    }

    pub(crate) fn capable(&self, cap: Cap) -> bool {
        self.effective & cap.bit() != 0
    }

    pub(crate) fn secure(&self, bit: u32) -> bool {
        self.securebits & bit != 0
    }

    /// `in_group_p`: the filesystem gid or a supplementary group.
    pub(crate) fn in_group(&self, gid: Kgid) -> bool {
        self.group.fs == gid || self.groups.binary_search(&gid).is_ok()
    }

    /// Who owns what the process creates: its filesystem uid and gid, as the kernel ids the
    /// filesystems store.
    pub(crate) fn fs_owner(&self) -> (u32, u32) {
        (self.user.fs.0, self.group.fs.0)
    }

    /// The root of the process's user namespace, if it maps one (`make_kuid(user_ns, 0)`).
    fn root_uid(&self) -> Option<Kuid> {
        self.user_ns.root_uid()
    }

    /// Whether `id` is the root of the process's user namespace.
    fn is_root(&self, id: Kuid) -> bool {
        self.root_uid() == Some(id)
    }

    /// What the filesystem checks against: kernel ids.
    pub(crate) fn fs(&self) -> FsCred {
        FsCred {
            uid: self.user.real.0,
            gid: self.group.real.0,
            fsuid: self.user.fs.0,
            fsgid: self.group.fs.0,
            groups: self.groups.iter().map(|gid| gid.0).collect(),
            caps: self.effective,
        }
    }

    /// `access_override_creds`: `access` checks with the real ids, and with the permitted
    /// capabilities only if the real uid is root.
    pub(crate) fn fs_for_access(&self) -> FsCred {
        let mut cred = self.fs();
        cred.fsuid = self.user.real.0;
        cred.fsgid = self.group.real.0;
        if !self.secure(secure::NO_SETUID_FIXUP) {
            cred.caps = match self.is_root(self.user.real) {
                true => self.permitted,
                false => 0,
            };
        }
        cred
    }

    /// The ids `SO_PEERCRED` and `SCM_CREDENTIALS` report (`cred_to_ucred`).
    pub(crate) fn ucred_ids(&self) -> (Kuid, Kgid) {
        (self.user.effective, self.group.effective)
    }

    /// `kill_ok_by_cred`: whether this sender may signal a process with `target`'s ids.
    pub(crate) fn may_signal(&self, target: &Cred) -> bool {
        let (sender, target) = (&self.user, &target.user);
        [sender.effective, sender.real]
            .iter()
            .any(|&id| id == target.saved || id == target.real)
            || self.capable(Cap::Kill)
    }

    /// `check_same_owner`: the caller's effective uid is the target's real or effective uid.
    pub(crate) fn same_owner(&self, target: &Cred) -> bool {
        let euid = self.user.effective;
        euid == target.user.effective || euid == target.user.real
    }

    /// `check_prlimit_permission`: the target's uids and gids all match the caller's real id,
    /// or the caller has `CAP_SYS_RESOURCE`.
    pub(crate) fn may_change_limits_of(&self, target: &Cred) -> bool {
        fn same<T: Copy + Eq>(caller: &Ids<T>, target: &Ids<T>) -> bool {
            [target.real, target.effective, target.saved]
                .iter()
                .all(|&id| id == caller.real)
        }
        (same(&self.user, &target.user) && same(&self.group, &target.group))
            || self.capable(Cap::SysResource)
    }

    /// `__ptrace_may_access` in a read mode, for a process outside the caller's thread group
    /// with `target`'s ids: its real, effective and saved uids and gids all are the caller's
    /// (its filesystem ids with `fscreds`, else its real ids), and it is `dumpable`; or the
    /// caller has `CAP_SYS_PTRACE`.
    pub(crate) fn may_read_task(&self, target: &Cred, dumpable: bool, fscreds: bool) -> bool {
        let (uid, gid) = match fscreds {
            true => (self.user.fs, self.group.fs),
            false => (self.user.real, self.group.real),
        };
        fn all<T: Copy + Eq>(ids: &Ids<T>, id: T) -> bool {
            [ids.real, ids.effective, ids.saved]
                .iter()
                .all(|&x| x == id)
        }
        let ptrace = self.capable(Cap::SysPtrace);
        ptrace || (all(&target.user, uid) && all(&target.group, gid) && dumpable)
    }

    /// One of `setuid`, `setreuid` and `setresuid`, with kernel ids.
    pub(crate) fn change_uids(&self, change: IdChange<Kuid>) -> Result<Self, Errno> {
        let mut new = self.clone();
        new.user.apply(change, self.capable(Cap::Setuid))?;
        if !self.secure(secure::NO_SETUID_FIXUP) {
            new.emulate_setxuid(self);
        }
        Ok(new)
    }

    /// One of `setgid`, `setregid` and `setresgid`, with kernel ids.
    pub(crate) fn change_gids(&self, change: IdChange<Kgid>) -> Result<Self, Errno> {
        let mut new = self.clone();
        new.group.apply(change, self.capable(Cap::Setgid))?;
        Ok(new)
    }

    /// `setfsuid`: returns the old filesystem uid, which the call returns whether or not it
    /// changed anything, and the new credentials. `None` is a uid the caller's namespace does
    /// not map.
    pub(crate) fn change_fsuid(&self, id: Option<Kuid>) -> (Kuid, Self) {
        let mut new = self.clone();
        let old = self.user.fs;
        let changed = new.user.set_fs(id, self.capable(Cap::Setuid));
        if changed && !self.secure(secure::NO_SETUID_FIXUP) {
            // The filesystem capabilities follow a filesystem uid of root.
            if self.is_root(old) && !self.is_root(new.user.fs) {
                new.effective &= !CAP_FS_MASK;
            } else if !self.is_root(old) && self.is_root(new.user.fs) {
                new.effective |= new.permitted & CAP_FS_MASK;
            }
        }
        (old, new)
    }

    /// `setfsgid`, as [`Cred::change_fsuid`].
    pub(crate) fn change_fsgid(&self, id: Option<Kgid>) -> (Kgid, Self) {
        let mut new = self.clone();
        let old = self.group.fs;
        new.group.set_fs(id, self.capable(Cap::Setgid));
        (old, new)
    }

    /// `cap_emulate_setxuid`: leaving root in all three ids drops the capabilities (unless
    /// `SECBIT_KEEP_CAPS`), and the effective set follows an effective uid of root.
    fn emulate_setxuid(&mut self, old: &Self) {
        let root = old.root_uid();
        let holds_root = |ids: &Ids<Kuid>| root.is_some_and(|root| ids.holds(root));
        if holds_root(&old.user) && !holds_root(&self.user) {
            if !old.secure(secure::KEEP_CAPS) {
                self.permitted = 0;
                self.effective = 0;
            }
            self.ambient = 0;
        }
        if old.is_root(old.user.effective) && !old.is_root(self.user.effective) {
            self.effective = 0;
        }
        if !old.is_root(old.user.effective) && old.is_root(self.user.effective) {
            self.effective = self.permitted;
        }
    }

    /// `setgroups`, with kernel ids: needs `CAP_SETGID`.
    pub(crate) fn set_groups(&self, mut groups: Vec<Kgid>) -> Result<Self, Errno> {
        if !self.capable(Cap::Setgid) {
            return Err(Errno::EPERM);
        }
        groups.sort_unstable();
        Ok(Self {
            groups,
            ..self.clone()
        })
    }

    /// `cap_capset`: the new sets are within what the process already has.
    pub(crate) fn capset(
        &self,
        effective: u64,
        permitted: u64,
        inheritable: u64,
    ) -> Result<Self, Errno> {
        let subset = |a: u64, b: u64| a & !b == 0;
        if !self.capable(Cap::Setpcap) && !subset(inheritable, self.inheritable | self.permitted) {
            return Err(Errno::EPERM);
        }
        if !subset(inheritable, self.inheritable | self.bounding)
            || !subset(permitted, self.permitted)
            || !subset(effective, permitted)
        {
            return Err(Errno::EPERM);
        }
        Ok(Self {
            effective,
            permitted,
            inheritable,
            ambient: self.ambient & permitted & inheritable,
            ..self.clone()
        })
    }

    /// The credentials after `execve` of a file with mode `mode` owned by `owner`
    /// (`bprm_fill_uid` and `cap_bprm_creds_from_file`, for files without file capabilities),
    /// and whether the exec is privilege-elevating (`AT_SECURE`). `setid` says whether the
    /// file's set-user-ID and set-group-ID bits may apply: not on a `nosuid` mount or under
    /// `no_new_privs`.
    pub(crate) fn exec(
        &self,
        mode: u32,
        owner: (Kuid, Kgid),
        setid: bool,
        no_new_privs: bool,
    ) -> (Self, bool) {
        const S_ISUID: u32 = 0o4000;
        const S_ISGID: u32 = 0o2000;
        const S_IXGRP: u32 = 0o010;
        let mut new = self.clone();
        if setid && mode & S_ISUID != 0 {
            new.user.effective = owner.0;
        }
        if setid && mode & (S_ISGID | S_IXGRP) == S_ISGID | S_IXGRP {
            new.group.effective = owner.1;
        }
        // `handle_privileged_root`: root gets its bounding set back on every exec.
        let mut raise_effective = false;
        if !self.secure(secure::NOROOT) {
            if self.is_root(new.user.effective) || self.is_root(new.user.real) {
                new.permitted = self.bounding | self.inheritable;
            }
            raise_effective = self.is_root(new.user.effective);
        }
        let id_changed =
            new.user.effective != self.user.effective || !self.in_group(new.group.effective);
        let gained = new.permitted & !self.permitted != 0;
        if (id_changed || gained) && no_new_privs {
            new.user.effective = new.user.real;
            new.group.effective = new.group.real;
            new.permitted &= self.permitted;
        }
        new.user.saved = new.user.effective;
        new.user.fs = new.user.effective;
        new.group.saved = new.group.effective;
        new.group.fs = new.group.effective;
        if id_changed {
            new.ambient = 0;
        }
        new.permitted |= new.ambient;
        new.effective = match raise_effective {
            true => new.permitted,
            false => new.ambient,
        };
        new.securebits &= !secure::KEEP_CAPS;
        let secure = id_changed
            || new.user.effective != self.user.real
            || new.group.effective != self.group.real
            || (!self.is_root(new.user.real)
                && (raise_effective || new.permitted & !new.ambient != 0));
        (new, secure)
    }

    /// Whether committing `self` over `old` makes the process non-dumpable (`commit_creds`).
    pub(crate) fn drops_dumpable(&self, old: &Self) -> bool {
        self.user.effective != old.user.effective
            || self.user.fs != old.user.fs
            || self.group.effective != old.group.effective
            || self.group.fs != old.group.fs
            || self.permitted & !old.permitted != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SETUID_ROOT: u32 = 0o4755;

    fn root() -> Cred {
        Cred::new(&Credentials::new(0, 0), UserNs::initial_user())
    }

    fn user(uid: u32) -> Cred {
        Cred::new(&Credentials::new(uid, uid), UserNs::initial_user())
    }

    fn uids(cred: &Cred) -> [u32; 4] {
        let ids = cred.user;
        [ids.real.0, ids.effective.0, ids.saved.0, ids.fs.0]
    }

    fn u(id: u32) -> Kuid {
        Kuid(id)
    }

    #[test]
    fn root_starts_with_every_capability_and_others_with_none() {
        let root = root();
        let user = user(1000);

        assert_eq!((root.permitted, root.effective), (CAP_FULL, CAP_FULL));
        assert!(root.capable(Cap::DacOverride) && root.capable(Cap::CheckpointRestore));
        assert_eq!((user.permitted, user.effective), (0, 0));
        assert_eq!(user.bounding, CAP_FULL);
        assert_eq!(user.groups, [Kgid(1000)]);
        assert!(root.fs().capable(Cap::Chown) && !user.fs().capable(Cap::Chown));
    }

    #[test]
    fn setuid_away_from_root_drops_every_capability_for_good() {
        let dropped = root().change_uids(IdChange::Id(u(1000))).unwrap();

        assert_eq!(uids(&dropped), [1000; 4]);
        assert_eq!((dropped.permitted, dropped.effective), (0, 0));
        assert_eq!(dropped.change_uids(IdChange::Id(u(0))), Err(Errno::EPERM));
    }

    #[test]
    fn a_saved_root_uid_brings_the_capabilities_back() {
        let seteuid = IdChange::Res(None, Some(u(1000)), None);
        let lowered = root().change_uids(seteuid).unwrap();

        assert_eq!(uids(&lowered), [0, 1000, 0, 1000]);
        assert_eq!((lowered.permitted, lowered.effective), (CAP_FULL, 0));
        let restored = lowered
            .change_uids(IdChange::Res(None, Some(u(0)), None))
            .unwrap();
        assert_eq!(restored.effective, CAP_FULL);
    }

    #[test]
    fn keep_caps_keeps_the_permitted_set_across_setuid() {
        let mut cred = root();
        cred.securebits |= secure::KEEP_CAPS;

        let dropped = cred.change_uids(IdChange::Id(u(1000))).unwrap();

        assert_eq!((dropped.permitted, dropped.effective), (CAP_FULL, 0));
    }

    #[test]
    fn unprivileged_id_changes_only_move_between_held_ids() {
        let cred = user(1000);

        assert_eq!(cred.change_uids(IdChange::Id(u(0))), Err(Errno::EPERM));
        assert_eq!(
            cred.change_gids(IdChange::Re(None, Some(Kgid(5)))),
            Err(Errno::EPERM)
        );
        assert_eq!(
            IdChange::Id(NO_ID)
                .to_kernel(|id| cred.user_ns.make_kuid(id))
                .err(),
            Some(Errno::EINVAL)
        );
        assert_eq!(cred.set_groups(vec![Kgid(5)]), Err(Errno::EPERM));
        let (old, same) = cred.change_fsuid(Some(u(0)));
        assert_eq!((old, same.user.fs), (u(1000), u(1000)));
    }

    #[test]
    fn setreuid_moves_the_saved_uid_with_the_effective_one() {
        let cred = Cred {
            user: Ids {
                real: u(1),
                effective: u(2),
                saved: u(3),
                fs: u(2),
            },
            ..user(1)
        };

        let swapped = cred
            .change_uids(IdChange::Re(Some(u(2)), Some(u(1))))
            .unwrap();

        assert_eq!(uids(&swapped), [2, 1, 1, 1]);
        let to_saved = cred.change_uids(IdChange::Re(None, Some(u(3)))).unwrap();
        assert_eq!(uids(&to_saved), [1, 3, 3, 3]);
    }

    #[test]
    fn a_filesystem_uid_of_zero_carries_the_filesystem_capabilities() {
        let (old, fs_user) = root().change_fsuid(Some(u(1000)));

        assert_eq!(old, u(0));
        assert!(!fs_user.capable(Cap::DacOverride) && fs_user.capable(Cap::Kill));
        let (_, back) = fs_user.change_fsuid(Some(u(0)));
        assert_eq!(back.effective, CAP_FULL);
    }

    #[test]
    fn root_sets_groups_sorted() {
        let cred = root()
            .set_groups(vec![Kgid(30), Kgid(10), Kgid(20)])
            .unwrap();

        assert_eq!(cred.groups, [Kgid(10), Kgid(20), Kgid(30)]);
        assert!(cred.in_group(Kgid(20)) && !cred.in_group(Kgid(40)));
    }

    #[test]
    fn capset_only_narrows() {
        let root = root();
        let narrowed = root
            .capset(Cap::Kill.bit(), Cap::Kill.bit() | Cap::Chown.bit(), 0)
            .unwrap();

        assert!(narrowed.capable(Cap::Kill) && !narrowed.capable(Cap::Chown));
        assert_eq!(
            narrowed.capset(Cap::Chown.bit(), Cap::Kill.bit(), 0),
            Err(Errno::EPERM)
        );
        assert_eq!(narrowed.capset(0, CAP_FULL, 0), Err(Errno::EPERM));
        assert_eq!(user(1).capset(0, 0, Cap::Kill.bit()), Err(Errno::EPERM));
    }

    #[test]
    fn exec_gives_root_its_capabilities_and_nobody_else_any() {
        let (root_after, root_secure) = root().exec(0o755, (u(5), Kgid(5)), true, false);
        let (user_after, user_secure) = user(1000).exec(0o755, (u(5), Kgid(5)), true, false);

        assert_eq!(root_after.effective, CAP_FULL);
        assert!(!root_secure);
        assert_eq!(user_after.effective, 0);
        assert!(!user_secure);
    }

    #[test]
    fn exec_of_a_setuid_root_file_makes_the_caller_root() {
        let (cred, secure) = user(1000).exec(SETUID_ROOT, (u(0), Kgid(0)), true, false);

        assert_eq!(uids(&cred), [1000, 0, 0, 0]);
        assert_eq!(cred.effective, CAP_FULL);
        assert!(secure);
    }

    #[test]
    fn nosuid_and_no_new_privs_ignore_the_setuid_bit() {
        let user = user(1000);
        let (nosuid, _) = user.exec(SETUID_ROOT, (u(0), Kgid(0)), false, false);
        let (no_new_privs, _) = user.exec(SETUID_ROOT, (u(0), Kgid(0)), false, true);

        assert_eq!(uids(&nosuid), [1000; 4]);
        assert_eq!(nosuid.effective, 0);
        assert_eq!(no_new_privs, user);
    }

    #[test]
    fn kill_needs_a_shared_uid_or_cap_kill() {
        let (a, b) = (user(1000), user(2000));

        assert!(a.may_signal(&a));
        assert!(!a.may_signal(&b));
        assert!(root().may_signal(&b));
    }
}
