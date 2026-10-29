//! POSIX ACLs (`fs/posix_acl.c`): the `system.posix_acl_access` and
//! `system.posix_acl_default` extended attributes, their checks in `permission`, inheritance
//! of a directory's default ACL by what is made in it, and `chmod` keeping the access ACL in
//! step with the mode.
//!
//! A driver stores an ACL as an ordinary extended attribute in the UAPI format (`struct
//! posix_acl_xattr_header` and entries, little-endian) with kernel ids, as filesystems keep
//! them on disk; a driver without extended attributes has no ACLs (`EOPNOTSUPP`). The VFS
//! translates ids from and to the caller's user namespace, and caches each inode's ACLs.

use std::sync::Arc;

use wasixv2_abi::{Errno, PosixAclMisc};

use crate::{
    Vfs,
    cred::{Cred, MAY_EXEC, MAY_READ, MAY_WRITE, UserNs},
    dentry::Dentry,
    mount::Path,
    notify::FsEvent,
    ops::Ctx,
    types::{Attr, FileType, FsResult, S_ISGID, SetAttr, TimeSet, XattrSet},
};

pub const XATTR_NAME_POSIX_ACL_ACCESS: &[u8] = b"system.posix_acl_access";
pub const XATTR_NAME_POSIX_ACL_DEFAULT: &[u8] = b"system.posix_acl_default";

const ACL_USER_OBJ: u16 = PosixAclMisc::ACL_USER_OBJ.0 as u16;
const ACL_USER: u16 = PosixAclMisc::ACL_USER.0 as u16;
const ACL_GROUP_OBJ: u16 = PosixAclMisc::ACL_GROUP_OBJ.0 as u16;
const ACL_GROUP: u16 = PosixAclMisc::ACL_GROUP.0 as u16;
const ACL_MASK: u16 = PosixAclMisc::ACL_MASK.0 as u16;
const ACL_OTHER: u16 = PosixAclMisc::ACL_OTHER.0 as u16;
const ACL_PERMS: u16 = (MAY_READ | MAY_WRITE | MAY_EXEC) as u16;
const VERSION: u32 = PosixAclMisc::POSIX_ACL_XATTR_VERSION.0 as u32;
const UNDEFINED_ID: u32 = PosixAclMisc::ACL_UNDEFINED_ID.0 as u32;
const S_IRWXU: u32 = 0o700;
const S_IRWXG: u32 = 0o070;
const S_IRWXO: u32 = 0o007;

/// Which of an inode's two ACLs (`ACL_TYPE_ACCESS`, `ACL_TYPE_DEFAULT`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AclKind {
    Access,
    Default,
}

impl AclKind {
    /// `posix_acl_type`: the ACL an extended attribute name stands for.
    pub fn of_xattr(name: &[u8]) -> Option<Self> {
        match name {
            XATTR_NAME_POSIX_ACL_ACCESS => Some(Self::Access),
            XATTR_NAME_POSIX_ACL_DEFAULT => Some(Self::Default),
            _ => None,
        }
    }

    fn xattr(self) -> &'static [u8] {
        match self {
            Self::Access => XATTR_NAME_POSIX_ACL_ACCESS,
            Self::Default => XATTR_NAME_POSIX_ACL_DEFAULT,
        }
    }

    pub(crate) fn index(self) -> usize {
        self as usize
    }
}

/// One entry (`struct posix_acl_entry`); `id` is a kernel id for `ACL_USER` and `ACL_GROUP`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AclEntry {
    pub tag: u16,
    pub perm: u16,
    pub id: u32,
}

/// An ACL (`struct posix_acl`), its entries in the order `posix_acl_valid` demands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PosixAcl(pub Vec<AclEntry>);

impl PosixAcl {
    /// `posix_acl_from_xattr`: the ACL of an extended attribute value, its ids mapped to
    /// kernel ids by `uid` and `gid`; `None` for a value with no entries. A wrong version is
    /// `EOPNOTSUPP`, anything else malformed `EINVAL`.
    pub fn from_xattr(
        value: &[u8],
        uid: impl Fn(u32) -> Option<u32>,
        gid: impl Fn(u32) -> Option<u32>,
    ) -> FsResult<Option<Self>> {
        if value.len() < 4 {
            return Err(Errno::EINVAL);
        }
        if u32::from_le_bytes(value[..4].try_into().unwrap()) != VERSION {
            return Err(Errno::EOPNOTSUPP);
        }
        let body = &value[4..];
        if !body.len().is_multiple_of(8) {
            return Err(Errno::EINVAL);
        }
        if body.is_empty() {
            return Ok(None);
        }
        let mut entries = Vec::with_capacity(body.len() / 8);
        for raw in body.chunks_exact(8) {
            let tag = u16::from_le_bytes([raw[0], raw[1]]);
            let perm = u16::from_le_bytes([raw[2], raw[3]]);
            let id = u32::from_le_bytes(raw[4..8].try_into().unwrap());
            let id = match tag {
                ACL_USER_OBJ | ACL_GROUP_OBJ | ACL_MASK | ACL_OTHER => UNDEFINED_ID,
                ACL_USER => uid(id).ok_or(Errno::EINVAL)?,
                ACL_GROUP => gid(id).ok_or(Errno::EINVAL)?,
                _ => return Err(Errno::EINVAL),
            };
            entries.push(AclEntry { tag, perm, id });
        }
        Ok(Some(Self(entries)))
    }

    /// `posix_acl_to_xattr`: the extended attribute value, ids mapped by `uid` and `gid`
    /// (`ACL_UNDEFINED_ID` for an id the caller cannot name).
    pub fn to_xattr(
        &self,
        uid: impl Fn(u32) -> Option<u32>,
        gid: impl Fn(u32) -> Option<u32>,
    ) -> Vec<u8> {
        let mut out = VERSION.to_le_bytes().to_vec();
        for entry in &self.0 {
            let id = match entry.tag {
                ACL_USER => uid(entry.id).unwrap_or(UNDEFINED_ID),
                ACL_GROUP => gid(entry.id).unwrap_or(UNDEFINED_ID),
                _ => UNDEFINED_ID,
            };
            out.extend_from_slice(&entry.tag.to_le_bytes());
            out.extend_from_slice(&entry.perm.to_le_bytes());
            out.extend_from_slice(&id.to_le_bytes());
        }
        out
    }

    /// The form drivers store: kernel ids as they are.
    pub fn to_stored(&self) -> Vec<u8> {
        self.to_xattr(Some, Some)
    }

    /// The ACL a driver stored.
    pub fn from_stored(value: &[u8]) -> FsResult<Option<Self>> {
        Self::from_xattr(value, Some, Some)
    }

    /// `posix_acl_valid`: one owner, group and other entry, users and groups in between in
    /// order and only with a mask, ids the filesystem's user namespace `owner` maps.
    pub fn valid(&self, owner: &dyn UserNs) -> FsResult<()> {
        let mut state = ACL_USER_OBJ;
        let mut needs_mask = false;
        for entry in &self.0 {
            if entry.perm & !ACL_PERMS != 0 {
                return Err(Errno::EINVAL);
            }
            state = match (entry.tag, state) {
                (ACL_USER_OBJ, ACL_USER_OBJ) => ACL_USER,
                (ACL_USER, ACL_USER) if owner.has_uid(entry.id) => {
                    needs_mask = true;
                    ACL_USER
                }
                (ACL_GROUP_OBJ, ACL_USER) => ACL_GROUP,
                (ACL_GROUP, ACL_GROUP) if owner.has_gid(entry.id) => {
                    needs_mask = true;
                    ACL_GROUP
                }
                (ACL_MASK, ACL_GROUP) => ACL_OTHER,
                (ACL_OTHER, ACL_OTHER) => 0,
                (ACL_OTHER, ACL_GROUP) if !needs_mask => 0,
                _ => return Err(Errno::EINVAL),
            };
        }
        match state {
            0 => Ok(()),
            _ => Err(Errno::EINVAL),
        }
    }

    /// `posix_acl_equiv_mode`: the permission bits the ACL makes `mode`; whether it says more
    /// than they can (`true`: users, groups or a mask).
    pub fn equiv_mode(&self, mode: &mut u32) -> bool {
        let mut bits = 0;
        let mut not_equiv = false;
        for entry in &self.0 {
            let perm = u32::from(entry.perm) & S_IRWXO;
            match entry.tag {
                ACL_USER_OBJ => bits |= perm << 6,
                ACL_GROUP_OBJ => bits |= perm << 3,
                ACL_OTHER => bits |= perm,
                ACL_MASK => {
                    bits = (bits & !S_IRWXG) | (perm << 3);
                    not_equiv = true;
                }
                _ => not_equiv = true,
            }
        }
        *mode = (*mode & !0o777) | bits;
        not_equiv
    }

    /// `posix_acl_permission`: whether `cred` gets `want` from the ACL of an object owned as
    /// `attr` says.
    pub fn permission(&self, cred: &Cred, attr: &Attr, want: u32) -> FsResult<()> {
        let want = (want & (MAY_READ | MAY_WRITE | MAY_EXEC)) as u16;
        let mask = self.0.iter().find(|e| e.tag == ACL_MASK).map(|e| e.perm);
        let masked = |perm: u16| perm & mask.unwrap_or(ACL_PERMS);
        let granted = |perm: u16| match perm & want == want {
            true => Ok(()),
            false => Err(Errno::EACCES),
        };
        let mut found = false;
        for entry in &self.0 {
            match entry.tag {
                ACL_USER_OBJ if cred.fsuid == attr.uid => return granted(entry.perm),
                ACL_USER if cred.fsuid == entry.id => return granted(masked(entry.perm)),
                ACL_GROUP_OBJ | ACL_GROUP => {
                    let gid = match entry.tag {
                        ACL_GROUP_OBJ => attr.gid,
                        _ => entry.id,
                    };
                    if cred.in_group(gid) {
                        found = true;
                        if entry.perm & want == want {
                            return granted(masked(entry.perm));
                        }
                    }
                }
                ACL_OTHER if found => return Err(Errno::EACCES),
                ACL_OTHER => return granted(entry.perm),
                _ => {}
            }
        }
        Err(Errno::EIO)
    }

    /// `posix_acl_create_masq`: the ACL and `mode` of a new object, each limited by the
    /// other; whether the ACL says more than the mode (`true`).
    fn create_masq(&mut self, mode: &mut u32) -> FsResult<bool> {
        let mut not_equiv = false;
        let (mut group_obj, mut mask_obj) = (None, None);
        for (i, entry) in self.0.iter_mut().enumerate() {
            let perm = u32::from(entry.perm);
            match entry.tag {
                ACL_USER_OBJ => {
                    entry.perm = (perm & ((*mode >> 6) | !S_IRWXO)) as u16;
                    *mode &= (u32::from(entry.perm) << 6) | !S_IRWXU;
                }
                ACL_USER | ACL_GROUP => not_equiv = true,
                ACL_GROUP_OBJ => group_obj = Some(i),
                ACL_OTHER => {
                    entry.perm = (perm & (*mode | !S_IRWXO)) as u16;
                    *mode &= u32::from(entry.perm) | !S_IRWXO;
                }
                ACL_MASK => {
                    mask_obj = Some(i);
                    not_equiv = true;
                }
                _ => return Err(Errno::EIO),
            }
        }
        let i = mask_obj.or(group_obj).ok_or(Errno::EIO)?;
        let entry = &mut self.0[i];
        entry.perm = (u32::from(entry.perm) & ((*mode >> 3) | !S_IRWXO)) as u16;
        *mode &= (u32::from(entry.perm) << 3) | !S_IRWXG;
        Ok(not_equiv)
    }

    /// `__posix_acl_chmod_masq`: the owner, mask (or group) and other entries from `mode`.
    fn chmod_masq(&mut self, mode: u32) -> FsResult<()> {
        let has_mask = self.0.iter().any(|e| e.tag == ACL_MASK);
        let mut group = false;
        for entry in &mut self.0 {
            match entry.tag {
                ACL_USER_OBJ => entry.perm = ((mode & S_IRWXU) >> 6) as u16,
                ACL_OTHER => entry.perm = (mode & S_IRWXO) as u16,
                ACL_MASK => entry.perm = ((mode & S_IRWXG) >> 3) as u16,
                ACL_GROUP_OBJ if !has_mask => {
                    entry.perm = ((mode & S_IRWXG) >> 3) as u16;
                    group = true;
                }
                ACL_USER | ACL_GROUP | ACL_GROUP_OBJ => {}
                _ => return Err(Errno::EIO),
            }
        }
        match has_mask || group {
            true => Ok(()),
            false => Err(Errno::EIO),
        }
    }
}

/// An inode's knowledge of one of its ACLs (`i_acl`, `i_default_acl`).
#[derive(Clone, Debug, Default)]
pub(crate) enum CachedAcl {
    /// Not read from the driver yet (`ACL_NOT_CACHED`).
    #[default]
    Unknown,
    /// The filesystem keeps no ACLs (`!IS_POSIXACL`).
    Unsupported,
    Absent,
    Present(Arc<PosixAcl>),
}

/// What a new object gets from its directory's default ACL (`posix_acl_create`).
pub(crate) struct InitAcls {
    access: Option<PosixAcl>,
    default: Option<Arc<PosixAcl>>,
}

impl Vfs {
    /// `get_inode_acl`: the ACL of `dentry`'s inode, read from the driver once and kept until
    /// the VFS changes it. `EOPNOTSUPP` where the filesystem has no ACLs.
    pub(crate) async fn get_acl(
        &self,
        dentry: &Dentry,
        kind: AclKind,
    ) -> FsResult<Option<Arc<PosixAcl>>> {
        let inode = dentry.inode();
        let cached = inode.cached_acl(kind);
        let state = match cached {
            CachedAcl::Unknown => {
                let state = match dentry.node().getxattr(kind.xattr()).await {
                    Ok(value) => match PosixAcl::from_stored(&value)? {
                        Some(acl) => CachedAcl::Present(Arc::new(acl)),
                        None => CachedAcl::Absent,
                    },
                    Err(Errno::ENODATA) => CachedAcl::Absent,
                    Err(Errno::EOPNOTSUPP) => CachedAcl::Unsupported,
                    Err(errno) => return Err(errno),
                };
                inode.cache_acl(kind, state.clone());
                state
            }
            state => state,
        };
        match state {
            CachedAcl::Present(acl) => Ok(Some(acl)),
            CachedAcl::Absent => Ok(None),
            _ => Err(Errno::EOPNOTSUPP),
        }
    }

    /// The access ACL `permission` consults, if any (`check_acl`).
    pub(crate) async fn access_acl(&self, dentry: &Dentry) -> FsResult<Option<Arc<PosixAcl>>> {
        match self.get_acl(dentry, AclKind::Access).await {
            Err(Errno::EOPNOTSUPP) => Ok(None),
            result => result,
        }
    }

    /// Stores `acl` (or removes it, `None`) through the driver and keeps it cached
    /// (`set_cached_acl`).
    async fn store_acl(
        &self,
        dentry: &Dentry,
        kind: AclKind,
        acl: Option<&PosixAcl>,
    ) -> FsResult<()> {
        let node = dentry.node();
        let inode = dentry.inode();
        match acl {
            Some(acl) => {
                let value = acl.to_stored();
                node.setxattr(kind.xattr(), &value, XattrSet::Any).await?;
                inode.cache_acl(kind, CachedAcl::Present(Arc::new(acl.clone())));
            }
            None => {
                match node.removexattr(kind.xattr()).await {
                    Ok(()) | Err(Errno::ENODATA) => {}
                    Err(errno) => return Err(errno),
                }
                inode.cache_acl(kind, CachedAcl::Absent);
            }
        }
        Ok(())
    }

    /// `do_get_acl`: the value of `system.posix_acl_*` with ids as the caller names them.
    pub(crate) async fn getxattr_acl(
        &self,
        cx: &Ctx<'_>,
        path: &Path,
        kind: AclKind,
    ) -> FsResult<Vec<u8>> {
        let acl = self
            .get_acl(&path.dentry, kind)
            .await?
            .ok_or(Errno::ENODATA)?;
        let ns = &cx.cred.user_ns;
        Ok(acl.to_xattr(|id| ns.ns_uid(id), |id| ns.ns_gid(id)))
    }

    /// `do_set_acl` and `vfs_set_acl`, or `vfs_remove_acl` with `None`: the owner (or
    /// `CAP_FOWNER`) sets either ACL; a default ACL is only for a directory; an access ACL
    /// equivalent to a mode becomes that mode, and any access ACL changes the mode
    /// (`posix_acl_update_mode`).
    pub(crate) async fn setxattr_acl(
        &self,
        cx: &Ctx<'_>,
        path: &Path,
        kind: AclKind,
        value: Option<&[u8]>,
    ) -> FsResult<()> {
        let ns = &cx.cred.user_ns;
        let mut acl = match value {
            Some(value) => {
                PosixAcl::from_xattr(value, |id| ns.make_kuid(id), |id| ns.make_kgid(id))?
            }
            None => None,
        };
        if path.mnt.is_read_only() {
            return Err(Errno::EROFS);
        }
        let dentry = &path.dentry;
        let _lock = dentry.inode().lock().await;
        let attr = dentry.node().getattr().await?;
        if crate::ops::immutable(dentry) || crate::ops::append_only(dentry) {
            return Err(Errno::EPERM);
        }
        // `set_posix_acl`.
        match self.get_acl(dentry, kind).await {
            Err(Errno::EOPNOTSUPP) => return Err(Errno::EOPNOTSUPP),
            Err(errno) => return Err(errno),
            Ok(_) => {}
        }
        if kind == AclKind::Default && attr.file_type != FileType::Directory {
            return match acl {
                Some(_) => Err(Errno::EACCES),
                None => Ok(()),
            };
        }
        if !cx.cred.owns(&attr) {
            return Err(Errno::EPERM);
        }
        if let Some(acl) = &acl {
            acl.valid(dentry.sb().owner.as_ref())?;
        }
        if kind == AclKind::Access {
            // `posix_acl_update_mode`.
            let mut mode = attr.mode;
            if let Some(given) = &acl
                && !given.equiv_mode(&mut mode)
            {
                acl = None;
            }
            if !cx.cred.in_group_or_capable(&attr, attr.gid) {
                mode &= !S_ISGID;
            }
            if mode != attr.mode {
                let set = SetAttr {
                    mode: Some(mode),
                    ctime: Some(TimeSet::Now),
                    ..SetAttr::default()
                };
                dentry.node().setattr(&set).await?;
            }
        }
        self.store_acl(dentry, kind, acl.as_ref()).await?;
        let object = Self::object(dentry);
        self.notify_object(dentry, FsEvent::Attrib { object });
        Ok(())
    }

    /// `posix_acl_chmod`: the access ACL follows a new mode.
    pub(crate) async fn acl_chmod(&self, dentry: &Dentry, mode: u32) -> FsResult<()> {
        if !dentry.node().acl_permissions() {
            return Ok(());
        }
        let Ok(Some(acl)) = self.get_acl(dentry, AclKind::Access).await else {
            return Ok(());
        };
        let mut acl = PosixAcl::clone(&acl);
        acl.chmod_masq(mode)?;
        self.store_acl(dentry, AclKind::Access, Some(&acl)).await
    }

    /// `posix_acl_create`: the mode of a new object in `dir` made with `mode` (permission bits
    /// and, for the type, `kind`), and the ACLs it gets. Without a default ACL the umask
    /// applies; with one, the ACL does instead.
    pub(crate) async fn posix_acl_create(
        &self,
        dir: &Dentry,
        file_type: FileType,
        mode: &mut u32,
        umask: u32,
    ) -> FsResult<Option<InitAcls>> {
        if !dir.node().acl_permissions() {
            *mode &= !umask;
            return Ok(None);
        }
        let parent = match file_type {
            FileType::Symlink => return Ok(None),
            _ => self.get_acl(dir, AclKind::Default).await,
        };
        let default = match parent {
            Ok(Some(default)) => default,
            Ok(None) | Err(Errno::EOPNOTSUPP) => {
                *mode &= !umask;
                return Ok(None);
            }
            Err(errno) => return Err(errno),
        };
        let mut access = PosixAcl::clone(&default);
        let not_equiv = access.create_masq(mode)?;
        Ok(Some(InitAcls {
            access: not_equiv.then_some(access),
            default: (file_type == FileType::Directory).then_some(default),
        }))
    }

    /// Gives a new object the ACLs `posix_acl_create` decided.
    pub(crate) async fn init_acls(&self, dentry: &Dentry, acls: Option<InitAcls>) -> FsResult<()> {
        let Some(acls) = acls else {
            return Ok(());
        };
        if let Some(default) = &acls.default {
            self.store_acl(dentry, AclKind::Default, Some(default))
                .await?;
        }
        if let Some(access) = &acls.access {
            self.store_acl(dentry, AclKind::Access, Some(access))
                .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Device, Timestamp};

    fn acl(entries: &[(u16, u16, u32)]) -> PosixAcl {
        PosixAcl(
            entries
                .iter()
                .map(|&(tag, perm, id)| AclEntry { tag, perm, id })
                .collect(),
        )
    }

    fn attr(mode: u32, uid: u32, gid: u32) -> Attr {
        Attr {
            ino: 1,
            file_type: FileType::Regular,
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

    const U: u32 = UNDEFINED_ID;

    #[test]
    fn named_entries_are_limited_by_the_mask() {
        // u::rw-, u:7:rwx, g::r--, m::r--, o::---
        let acl = acl(&[
            (ACL_USER_OBJ, 6, U),
            (ACL_USER, 7, 7),
            (ACL_GROUP_OBJ, 4, U),
            (ACL_MASK, 4, U),
            (ACL_OTHER, 0, U),
        ]);
        let file = attr(0o640, 1, 1);
        let user7 = Cred::user(7, 100);
        assert_eq!(acl.permission(&user7, &file, MAY_READ), Ok(()));
        assert_eq!(acl.permission(&user7, &file, MAY_WRITE), Err(Errno::EACCES));
        // A group member that no entry grants falls to nothing, not to other.
        let member = Cred::user(9, 1);
        assert_eq!(acl.permission(&member, &file, MAY_READ), Ok(()));
        assert_eq!(acl.permission(&member, &file, MAY_EXEC), Err(Errno::EACCES));
        assert_eq!(
            acl.permission(&Cred::user(9, 9), &file, MAY_READ),
            Err(Errno::EACCES)
        );
    }

    #[test]
    fn validity_order_and_mask_rules() {
        let ns = crate::cred::InitialUserNs;
        let minimal = acl(&[
            (ACL_USER_OBJ, 6, U),
            (ACL_GROUP_OBJ, 4, U),
            (ACL_OTHER, 4, U),
        ]);
        assert_eq!(minimal.valid(&ns), Ok(()));
        let no_mask = acl(&[
            (ACL_USER_OBJ, 6, U),
            (ACL_USER, 6, 5),
            (ACL_GROUP_OBJ, 4, U),
            (ACL_OTHER, 4, U),
        ]);
        assert_eq!(no_mask.valid(&ns), Err(Errno::EINVAL));
        let out_of_order = acl(&[
            (ACL_GROUP_OBJ, 4, U),
            (ACL_USER_OBJ, 6, U),
            (ACL_OTHER, 4, U),
        ]);
        assert_eq!(out_of_order.valid(&ns), Err(Errno::EINVAL));
        let mut mode = 0o7000;
        assert!(!minimal.equiv_mode(&mut mode));
        assert_eq!(mode, 0o7644);
    }

    #[test]
    fn create_and_chmod_keep_mode_and_acl_in_step() {
        let mut default = acl(&[
            (ACL_USER_OBJ, 7, U),
            (ACL_USER, 7, 5),
            (ACL_GROUP_OBJ, 5, U),
            (ACL_MASK, 7, U),
            (ACL_OTHER, 5, U),
        ]);
        let mut mode = 0o666;
        assert_eq!(default.create_masq(&mut mode), Ok(true));
        assert_eq!(mode, 0o664);
        assert_eq!(default.0[3].perm, 6);
        default.chmod_masq(0o750).unwrap();
        assert_eq!(
            default.0.iter().map(|e| e.perm).collect::<Vec<_>>(),
            [7, 7, 5, 5, 0]
        );
        let value = default.to_stored();
        assert_eq!(PosixAcl::from_stored(&value), Ok(Some(default)));
        assert_eq!(PosixAcl::from_stored(&[3, 0, 0, 0]), Err(Errno::EOPNOTSUPP));
        assert_eq!(PosixAcl::from_stored(&[2, 0, 0, 0, 1]), Err(Errno::EINVAL));
    }
}
