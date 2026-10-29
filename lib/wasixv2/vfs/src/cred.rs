//! Credentials and the mode-bit permission checks of Linux's `fs/namei.c`.
//!
//! Ids are kernel ids: what the initial user namespace sees. A credential's capabilities count
//! in its user namespace ([`UserNs`]), and over an inode only if that namespace maps the
//! inode's owner (`capable_wrt_inode_uidgid`); what reaches beyond every namespace, as making
//! a device node, takes the capability in the initial one ([`Cred::capable_init`]).

use std::{
    any::Any,
    fmt,
    sync::{Arc, LazyLock},
};

use wasixv2_abi::Errno;

use crate::types::{Attr, FileType, FsResult, S_IXUGO};

/// `MAY_*` masks of `inode_permission`.
pub const MAY_EXEC: u32 = 1;
pub const MAY_WRITE: u32 = 2;
pub const MAY_READ: u32 = 4;

/// Linux's capabilities, as bit numbers of its capability sets
/// (`include/uapi/linux/capability.h`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cap {
    Chown = 0,
    DacOverride = 1,
    DacReadSearch = 2,
    Fowner = 3,
    Fsetid = 4,
    Kill = 5,
    Setgid = 6,
    Setuid = 7,
    Setpcap = 8,
    LinuxImmutable = 9,
    NetBindService = 10,
    NetBroadcast = 11,
    NetAdmin = 12,
    NetRaw = 13,
    IpcLock = 14,
    IpcOwner = 15,
    SysModule = 16,
    SysRawio = 17,
    SysChroot = 18,
    SysPtrace = 19,
    SysPacct = 20,
    SysAdmin = 21,
    SysBoot = 22,
    SysNice = 23,
    SysResource = 24,
    SysTime = 25,
    SysTtyConfig = 26,
    Mknod = 27,
    Lease = 28,
    AuditWrite = 29,
    AuditControl = 30,
    Setfcap = 31,
    MacOverride = 32,
    MacAdmin = 33,
    Syslog = 34,
    WakeAlarm = 35,
    BlockSuspend = 36,
    AuditRead = 37,
    Perfmon = 38,
    Bpf = 39,
    CheckpointRestore = 40,
}

impl Cap {
    /// `CAP_LAST_CAP`.
    pub const LAST: u32 = Cap::CheckpointRestore as u32;

    /// The capability's bit in a capability set.
    pub const fn bit(self) -> u64 {
        1 << self as u32
    }
}

/// What the VFS needs of a user namespace (`struct user_namespace`): which kernel ids it maps,
/// and whether it is the initial one. The kernel's namespaces implement it.
pub trait UserNs: Any + Send + Sync {
    /// `kuid_has_mapping`: the namespace has a uid for the kernel uid `kuid`.
    fn has_uid(&self, kuid: u32) -> bool;
    /// `kgid_has_mapping`.
    fn has_gid(&self, kgid: u32) -> bool;
    /// `make_kuid`: the kernel uid of the namespace's `uid`, if it maps one.
    fn make_kuid(&self, uid: u32) -> Option<u32>;
    /// `make_kgid`.
    fn make_kgid(&self, gid: u32) -> Option<u32>;
    /// Whether this is the initial user namespace (`init_user_ns`), which maps every id.
    fn is_initial(&self) -> bool;
}

/// The initial user namespace as the VFS knows it without a kernel: every id but `-1` is
/// itself. What the embedder mounts belongs to it.
pub struct InitialUserNs;

impl UserNs for InitialUserNs {
    fn has_uid(&self, kuid: u32) -> bool {
        kuid != u32::MAX
    }

    fn has_gid(&self, kgid: u32) -> bool {
        kgid != u32::MAX
    }

    fn make_kuid(&self, uid: u32) -> Option<u32> {
        (uid != u32::MAX).then_some(uid)
    }

    fn make_kgid(&self, gid: u32) -> Option<u32> {
        (gid != u32::MAX).then_some(gid)
    }

    fn is_initial(&self) -> bool {
        true
    }
}

/// The [`InitialUserNs`].
pub fn initial_user_ns() -> Arc<dyn UserNs> {
    static INITIAL: LazyLock<Arc<dyn UserNs>> = LazyLock::new(|| Arc::new(InitialUserNs));
    INITIAL.clone()
}

/// Whether `a` and `b` are the same user namespace. There is one initial user namespace: the
/// [`InitialUserNs`] the VFS gives what it makes before a kernel exists, and the kernel's own,
/// are both it.
pub fn same_user_ns(a: &Arc<dyn UserNs>, b: &Arc<dyn UserNs>) -> bool {
    std::ptr::addr_eq(Arc::as_ptr(a), Arc::as_ptr(b)) || a.is_initial() && b.is_initial()
}

/// The identity a filesystem operation runs as.
#[derive(Clone)]
pub struct Cred {
    /// Real ids, which `access` checks against.
    pub uid: u32,
    pub gid: u32,
    /// Filesystem ids (normally the effective ids), which everything else checks against.
    pub fsuid: u32,
    pub fsgid: u32,
    pub groups: Vec<u32>,
    /// Effective capabilities, one bit per `Cap`, in `user_ns`.
    pub caps: u64,
    /// The user namespace the capabilities count in (`cred->user_ns`).
    pub user_ns: Arc<dyn UserNs>,
    /// What a security module checks for these credentials (`cred->security`); `None` when
    /// none restricts them.
    pub security: Option<Arc<dyn crate::Security>>,
}

impl PartialEq for Cred {
    fn eq(&self, other: &Self) -> bool {
        (self.uid, self.gid, self.fsuid, self.fsgid, self.caps)
            == (other.uid, other.gid, other.fsuid, other.fsgid, other.caps)
            && self.groups == other.groups
            && same_user_ns(&self.user_ns, &other.user_ns)
            && match (&self.security, &other.security) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }
    }
}

impl Eq for Cred {}

impl fmt::Debug for Cred {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cred")
            .field("uid", &self.uid)
            .field("gid", &self.gid)
            .field("fsuid", &self.fsuid)
            .field("fsgid", &self.fsgid)
            .field("groups", &self.groups)
            .field("caps", &self.caps)
            .field("initial_user_ns", &self.user_ns.is_initial())
            .finish()
    }
}

impl Cred {
    /// An unprivileged user of the initial user namespace with no supplementary groups.
    pub fn user(uid: u32, gid: u32) -> Self {
        Self {
            uid,
            gid,
            fsuid: uid,
            fsgid: gid,
            groups: Vec::new(),
            caps: 0,
            user_ns: initial_user_ns(),
            security: None,
        }
    }

    /// uid 0 of the initial user namespace with every capability.
    pub fn root() -> Self {
        Self {
            caps: u64::MAX,
            ..Self::user(0, 0)
        }
    }

    /// `ns_capable(current_user_ns(), cap)`: the capability in the credentials' own user
    /// namespace.
    pub fn capable(&self, cap: Cap) -> bool {
        self.caps & cap.bit() != 0
    }

    /// `capable`: the capability in the initial user namespace, for what no user namespace
    /// owns (devices, `trusted.*` attributes).
    pub fn capable_init(&self, cap: Cap) -> bool {
        self.capable(cap) && self.user_ns.is_initial()
    }

    /// `capable_wrt_inode_uidgid`: the capability, over an inode whose owner and group the
    /// credentials' user namespace maps (`privileged_wrt_inode_uidgid`).
    pub fn capable_wrt(&self, attr: &Attr, cap: Cap) -> bool {
        self.capable(cap) && self.user_ns.has_uid(attr.uid) && self.user_ns.has_gid(attr.gid)
    }

    /// `in_group_p`: the filesystem gid or a supplementary group.
    pub fn in_group(&self, gid: u32) -> bool {
        self.fsgid == gid || self.groups.contains(&gid)
    }

    /// `inode_owner_or_capable`: the owner, or `CAP_FOWNER` with the owner mapped.
    pub fn owns(&self, attr: &Attr) -> bool {
        self.fsuid == attr.uid || (self.capable(Cap::Fowner) && self.user_ns.has_uid(attr.uid))
    }

    /// `in_group_or_capable`: a member of `attr`'s group `gid`, or `CAP_FSETID` over `attr`,
    /// who keeps a set-group-ID bit.
    pub fn in_group_or_capable(&self, attr: &Attr, gid: u32) -> bool {
        self.in_group(gid) || self.capable_wrt(attr, Cap::Fsetid)
    }
}

/// `generic_permission`: owner, group or other bits, then the DAC-override capabilities.
pub fn generic_permission(cred: &Cred, attr: &Attr, mask: u32) -> FsResult<()> {
    let mask = mask & (MAY_READ | MAY_WRITE | MAY_EXEC);
    let bits = if cred.fsuid == attr.uid {
        attr.mode >> 6
    } else if cred.in_group(attr.gid) {
        attr.mode >> 3
    } else {
        attr.mode
    } & 7;
    if mask & !bits == 0 {
        return Ok(());
    }
    if attr.file_type == FileType::Directory {
        if mask & MAY_WRITE == 0 && cred.capable_wrt(attr, Cap::DacReadSearch) {
            return Ok(());
        }
        if cred.capable_wrt(attr, Cap::DacOverride) {
            return Ok(());
        }
        return Err(Errno::EACCES);
    }
    if mask == MAY_READ && cred.capable_wrt(attr, Cap::DacReadSearch) {
        return Ok(());
    }
    // Executing needs at least one execute bit, even with CAP_DAC_OVERRIDE.
    if (mask & MAY_EXEC == 0 || attr.mode & S_IXUGO != 0)
        && cred.capable_wrt(attr, Cap::DacOverride)
    {
        return Ok(());
    }
    Err(Errno::EACCES)
}

/// `check_sticky`: whether removing or renaming `victim` out of the sticky directory `dir` is
/// denied.
pub fn sticky_denies(cred: &Cred, dir: &Attr, victim: &Attr) -> bool {
    dir.mode & crate::types::S_ISVTX != 0
        && cred.fsuid != victim.uid
        && cred.fsuid != dir.uid
        && !cred.capable_wrt(victim, Cap::Fowner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Device, Timestamp};

    fn attr(file_type: FileType, mode: u32, uid: u32, gid: u32) -> Attr {
        Attr {
            ino: 1,
            file_type,
            mode,
            nlink: 1,
            uid,
            gid,
            rdev: Device::default(),
            size: 0,
            blocks: 0,
            blksize: 4096,
            atime: Timestamp::default(),
            mtime: Timestamp::default(),
            ctime: Timestamp::default(),
            btime: None,
            dev: None,
        }
    }

    #[test]
    fn owner_bits_win_even_when_group_or_other_would_allow() {
        let user = Cred::user(1, 1);
        let file = attr(FileType::Regular, 0o077, 1, 1);

        assert_eq!(
            generic_permission(&user, &file, MAY_READ),
            Err(Errno::EACCES)
        );
    }

    #[test]
    fn group_and_supplementary_groups_use_group_bits() {
        let mut user = Cred::user(1, 1);
        let file = attr(FileType::Regular, 0o640, 2, 7);

        assert_eq!(
            generic_permission(&user, &file, MAY_READ),
            Err(Errno::EACCES)
        );
        user.groups.push(7);
        assert_eq!(generic_permission(&user, &file, MAY_READ), Ok(()));
        assert_eq!(
            generic_permission(&user, &file, MAY_WRITE),
            Err(Errno::EACCES)
        );
    }

    #[test]
    fn root_overrides_except_exec_without_any_x_bit() {
        let root = Cred::root();
        let file = attr(FileType::Regular, 0o600, 5, 5);
        let dir = attr(FileType::Directory, 0, 5, 5);

        assert_eq!(generic_permission(&root, &file, MAY_WRITE), Ok(()));
        assert_eq!(
            generic_permission(&root, &file, MAY_EXEC),
            Err(Errno::EACCES)
        );
        assert_eq!(generic_permission(&root, &dir, MAY_EXEC), Ok(()));
    }
}
