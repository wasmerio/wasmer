//! Credentials and the mode-bit permission checks of Linux's `fs/namei.c`.

use wasixv2_abi::Errno;

use crate::types::{Attr, FileType, FsResult, S_IXUGO};

/// `MAY_*` masks of `inode_permission`.
pub const MAY_EXEC: u32 = 1;
pub const MAY_WRITE: u32 = 2;
pub const MAY_READ: u32 = 4;

/// Capabilities the filesystem code checks, as bit numbers of Linux's capability sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cap {
    Chown = 0,
    DacOverride = 1,
    DacReadSearch = 2,
    Fowner = 3,
    Fsetid = 4,
    SysAdmin = 21,
    Mknod = 27,
}

/// The identity a filesystem operation runs as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cred {
    /// Real ids, which `access` checks against.
    pub uid: u32,
    pub gid: u32,
    /// Filesystem ids (normally the effective ids), which everything else checks against.
    pub fsuid: u32,
    pub fsgid: u32,
    pub groups: Vec<u32>,
    /// Effective capabilities, one bit per `Cap`.
    pub caps: u64,
}

impl Cred {
    /// An unprivileged user with no supplementary groups.
    pub fn user(uid: u32, gid: u32) -> Self {
        Self {
            uid,
            gid,
            fsuid: uid,
            fsgid: gid,
            groups: Vec::new(),
            caps: 0,
        }
    }

    /// uid 0 with every capability.
    pub fn root() -> Self {
        Self {
            caps: u64::MAX,
            ..Self::user(0, 0)
        }
    }

    pub fn capable(&self, cap: Cap) -> bool {
        self.caps & (1 << cap as u32) != 0
    }

    /// `in_group_p`: the filesystem gid or a supplementary group.
    pub fn in_group(&self, gid: u32) -> bool {
        self.fsgid == gid || self.groups.contains(&gid)
    }

    /// The credentials `access`/`faccessat` without `AT_EACCESS` check with: the real ids as
    /// filesystem ids, and no capabilities unless the real uid is root (`access_override_creds`).
    pub fn for_access(&self) -> Self {
        let caps = if self.uid == 0 { self.caps } else { 0 };
        Self {
            fsuid: self.uid,
            fsgid: self.gid,
            caps,
            ..self.clone()
        }
    }

    /// `inode_owner_or_capable`.
    pub fn owns(&self, attr: &Attr) -> bool {
        self.fsuid == attr.uid || self.capable(Cap::Fowner)
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
        if mask & MAY_WRITE == 0 && cred.capable(Cap::DacReadSearch) {
            return Ok(());
        }
        if cred.capable(Cap::DacOverride) {
            return Ok(());
        }
        return Err(Errno::EACCES);
    }
    if mask == MAY_READ && cred.capable(Cap::DacReadSearch) {
        return Ok(());
    }
    // Executing needs at least one execute bit, even with CAP_DAC_OVERRIDE.
    if (mask & MAY_EXEC == 0 || attr.mode & S_IXUGO != 0) && cred.capable(Cap::DacOverride) {
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
        && !cred.capable(Cap::Fowner)
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
