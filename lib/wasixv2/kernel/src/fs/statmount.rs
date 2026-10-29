//! `statmount` and `listmount` (`fs/namespace.c`): a mount's attributes and the mounts of a
//! namespace by their unique ids (`STATX_MNT_ID_UNIQUE`).

use std::sync::Arc;

use wasixv2_abi::{
    ConstMntIdReqPtr, GuestLayout, ListmountFlags, ListmountMntId, MntIdReq, MntIdReqSize,
    MountAttrFlags, MountFlags as MsFlags, Statmount, StatmountFlags, StatmountMask, U64Array,
    Usize, VoidPtr,
};
use wasixv2_syscalls::{Caller, Errno, Fd, SysResult};
use wasixv2_vfs::{Cap, MNT_UNIQUE_ID_OFFSET, MntNs, Mount, MountInfo, Path};

use super::Env;
use crate::{Kernel, ns::Namespace, userns::kernel_user_ns};

/// `STATMOUNT_SUPPORTED`: what this kernel can report.
const SUPPORTED: u64 = StatmountMask::STATMOUNT_SB_BASIC.0
    | StatmountMask::STATMOUNT_MNT_BASIC.0
    | StatmountMask::STATMOUNT_PROPAGATE_FROM.0
    | StatmountMask::STATMOUNT_MNT_ROOT.0
    | StatmountMask::STATMOUNT_MNT_POINT.0
    | StatmountMask::STATMOUNT_FS_TYPE.0
    | StatmountMask::STATMOUNT_MNT_NS_ID.0
    | StatmountMask::STATMOUNT_MNT_OPTS.0
    | StatmountMask::STATMOUNT_FS_SUBTYPE.0
    | StatmountMask::STATMOUNT_SB_SOURCE.0
    | StatmountMask::STATMOUNT_OPT_ARRAY.0
    | StatmountMask::STATMOUNT_OPT_SEC_ARRAY.0
    | StatmountMask::STATMOUNT_SUPPORTED_MASK.0
    | StatmountMask::STATMOUNT_MNT_UIDMAP.0
    | StatmountMask::STATMOUNT_MNT_GIDMAP.0;

/// `STATMOUNT_STRING_REQ`: the parts that are strings after the fixed record.
const STRINGS: u64 = StatmountMask::STATMOUNT_MNT_ROOT.0
    | StatmountMask::STATMOUNT_MNT_POINT.0
    | StatmountMask::STATMOUNT_FS_TYPE.0
    | StatmountMask::STATMOUNT_MNT_OPTS.0
    | StatmountMask::STATMOUNT_FS_SUBTYPE.0
    | StatmountMask::STATMOUNT_SB_SOURCE.0
    | StatmountMask::STATMOUNT_OPT_ARRAY.0
    | StatmountMask::STATMOUNT_OPT_SEC_ARRAY.0
    | StatmountMask::STATMOUNT_MNT_UIDMAP.0
    | StatmountMask::STATMOUNT_MNT_GIDMAP.0;

/// `SB_RDONLY`.
const SB_RDONLY: u32 = 1;

/// `listmount`'s limit on ids per call.
const LISTMOUNT_MAX: u32 = 1_000_000;

/// The mount `statmount` describes, and the namespace it was found in.
struct Target {
    mount: Arc<Mount>,
    ns: Option<Arc<MntNs>>,
}

/// `kstatmount`: the fixed record and the strings after it, `\0` first so that an offset of
/// 0 names an empty string.
struct Builder {
    sm: Statmount,
    strings: Vec<u8>,
    bufsize: usize,
}

impl Builder {
    /// `statmount_string`: appends `value` and points `field` at it; nothing for an empty
    /// value. `EOVERFLOW` if the buffer cannot hold it.
    fn string(&mut self, flag: StatmountMask, value: &[u8], field: fn(&mut Statmount) -> &mut u32) {
        if value.is_empty() {
            return;
        }
        if self.strings.is_empty() {
            self.strings.push(0);
        }
        let start = self.strings.len();
        self.strings.extend_from_slice(value);
        self.strings.push(0);
        self.sm.mask |= flag.0;
        *field(&mut self.sm) = start as u32;
    }

    fn fits(&self) -> Result<(), Errno> {
        // `kbufsize >= s->bufsize`: the strings and the record must fit with room to spare.
        match Statmount::SIZE + self.strings.len() >= self.bufsize && !self.strings.is_empty() {
            true => Err(Errno::EOVERFLOW),
            false => Ok(()),
        }
    }
}

/// `mnt_to_attr_flags`.
fn attr_flags(mount: &Mount) -> u64 {
    let flags = mount.flags();
    let mut attr = 0;
    for (set, flag) in [
        (flags.read_only, MountAttrFlags::MOUNT_ATTR_RDONLY),
        (flags.nosuid, MountAttrFlags::MOUNT_ATTR_NOSUID),
        (flags.nodev, MountAttrFlags::MOUNT_ATTR_NODEV),
        (flags.noexec, MountAttrFlags::MOUNT_ATTR_NOEXEC),
        (flags.nodiratime, MountAttrFlags::MOUNT_ATTR_NODIRATIME),
        (flags.nosymfollow, MountAttrFlags::MOUNT_ATTR_NOSYMFOLLOW),
    ] {
        if set {
            attr |= flag.0;
        }
    }
    attr | match (flags.noatime, flags.strictatime) {
        (true, _) => MountAttrFlags::MOUNT_ATTR_NOATIME.0,
        (false, false) => MountAttrFlags::MOUNT_ATTR_RELATIME.0,
        (false, true) => MountAttrFlags::MOUNT_ATTR_STRICTATIME.0,
    }
}

/// `mnt_to_propagation_flags`.
fn propagation(info: &MountInfo) -> u64 {
    let mut flags = 0;
    if info.shared.is_some() {
        flags |= MsFlags::MS_SHARED.0;
    }
    if info.master.is_some() {
        flags |= MsFlags::MS_SLAVE.0;
    }
    if info.unbindable {
        flags |= MsFlags::MS_UNBINDABLE.0;
    }
    u64::from(match flags {
        0 => MsFlags::MS_PRIVATE.0,
        flags => flags,
    })
}

impl Kernel {
    /// `copy_mnt_id_req`.
    async fn mnt_id_req(
        &self,
        caller: &Caller<'_>,
        req: ConstMntIdReqPtr,
        by_fd: bool,
    ) -> Result<MntIdReq, Errno> {
        // `get_user` faults below `mmap_min_addr`, which Linux never maps; reads of the low
        // guard go through (DESIGN.md, "Guard page at 0"), so a null request is checked here.
        if req.addr() < caller.memory().low_guard().await {
            return Err(Errno::EFAULT);
        }
        let mut size = [0; 4];
        caller
            .memory()
            .read(req.addr(), &mut size)
            .await
            .map_err(|_| Errno::EFAULT)?;
        let size = u32::from_le_bytes(size);
        if size > crate::mm::PAGE_SIZE as u32 {
            return Err(Errno::E2BIG);
        }
        if size < MntIdReqSize::MNT_ID_REQ_SIZE_VER0.0 {
            return Err(Errno::EINVAL);
        }
        // `copy_struct_from_user`: what is past the known record must be zero.
        let mut bytes = vec![0; (size as usize).max(MntIdReq::SIZE)];
        caller
            .memory()
            .read(req.addr(), &mut bytes[..size as usize])
            .await
            .map_err(|_| Errno::EFAULT)?;
        if bytes[MntIdReq::SIZE..].iter().any(|&b| b != 0) {
            return Err(Errno::E2BIG);
        }
        let req = MntIdReq::read_from(&bytes).expect("long enough");
        if by_fd {
            if req.mnt_id != 0 || req.mnt_ns_id != 0 {
                return Err(Errno::EINVAL);
            }
        } else {
            if req.mnt_ns_fd != 0 && req.mnt_ns_id != 0 {
                return Err(Errno::EINVAL);
            }
            if req.mnt_id <= MNT_UNIQUE_ID_OFFSET {
                return Err(Errno::EINVAL);
            }
        }
        Ok(req)
    }

    /// `grab_requested_mnt_ns`: the namespace by id, by an nsfs file, or the caller's.
    fn requested_ns(&self, caller: &Caller<'_>, req: &MntIdReq) -> Result<Arc<MntNs>, Errno> {
        if req.mnt_ns_id != 0 {
            return self.vfs.lookup_ns(req.mnt_ns_id).ok_or(Errno::ENOENT);
        }
        if req.mnt_ns_fd != 0 {
            let file = self.file(caller.task(), Fd(req.mnt_ns_fd as i32))?;
            return match crate::nsfs::namespace(file.path()) {
                Some(Namespace::Mnt(ns)) => Ok(ns.tree.clone()),
                _ => Err(Errno::EINVAL),
            };
        }
        Ok(self.ns(caller).mnt.tree.clone())
    }

    /// `ns_capable_noaudit(ns->user_ns, CAP_SYS_ADMIN)`: the caller may see all of the mount
    /// namespace `ns`.
    fn mnt_ns_admin(&self, caller: &Caller<'_>, ns: &MntNs) -> bool {
        let owner = kernel_user_ns(ns.owner(), &self.init_user_ns);
        self.cred(caller).ns_capable(&owner, Cap::SysAdmin)
    }

    /// `grab_requested_root`: the caller's root in its own namespace, the root of another.
    fn requested_root(&self, caller: &Caller<'_>, env: &Env, ns: &MntNs) -> Path {
        match std::ptr::eq(ns, &*self.ns(caller).mnt.tree) {
            true => env.fs.root.clone(),
            false => ns.root(),
        }
    }

    /// `statmount`.
    pub(crate) async fn sys_statmount(
        &self,
        caller: &Caller<'_>,
        req: ConstMntIdReqPtr,
        buf: VoidPtr,
        bufsize: Usize,
        flags: u32,
    ) -> SysResult<i32> {
        let by_fd = StatmountFlags::STATMOUNT_BY_FD.0;
        if flags & !by_fd != 0 {
            return Err(Errno::EINVAL.into());
        }
        let req = self.mnt_id_req(caller, req, flags & by_fd != 0).await?;
        let env = self.env(caller);
        let mut mask = req.param;
        let target = match flags & by_fd != 0 {
            true => {
                let file = self.file(caller.task(), Fd(req.mnt_ns_fd as i32))?;
                let mount = file.path().mnt.clone();
                let ns = self.vfs.ns_of(&mount).and_then(|id| self.vfs.lookup_ns(id));
                if ns.is_none() {
                    // A mount in no namespace has no mountpoint to show.
                    mask &= !(StatmountMask::STATMOUNT_MNT_POINT.0
                        | StatmountMask::STATMOUNT_MNT_NS_ID.0);
                }
                Target { mount, ns }
            }
            false => {
                let ns = self.requested_ns(caller, &req)?;
                let own = std::ptr::eq(&*ns, &*self.ns(caller).mnt.tree);
                if req.mnt_ns_id != 0 && !own && !self.mnt_ns_admin(caller, &ns) {
                    return Err(Errno::EPERM.into());
                }
                let mount = ns.mount_by_unique_id(req.mnt_id).ok_or(Errno::ENOENT)?;
                Target {
                    mount,
                    ns: Some(ns),
                }
            }
        };
        if mask & STRINGS != 0 && bufsize as usize == Statmount::SIZE {
            return Err(Errno::EOVERFLOW.into());
        }
        let root = match &target.ns {
            Some(ns) => self.requested_root(caller, &env, ns),
            None => env.fs.root.clone(),
        };
        if flags & by_fd == 0
            && let Some(ns) = &target.ns
        {
            let mount = &target.mount;
            if !self.vfs.is_path_reachable(mount, &mount.root, &root)
                && !self.mnt_ns_admin(caller, ns)
            {
                return Err(Errno::EPERM.into());
            }
        }
        let builder = self.statmount(&target, &root, mask, bufsize as usize)?;
        let Builder {
            mut sm, strings, ..
        } = builder;
        let copy = (bufsize as usize).min(Statmount::SIZE);
        sm.size = (copy + strings.len()) as u32;
        let memory = caller.memory();
        if !strings.is_empty() {
            let at = buf
                .addr()
                .checked_add(Statmount::SIZE as u32)
                .ok_or(Errno::EFAULT)?;
            memory.write(at, &strings).await?;
        }
        let mut record = vec![0; Statmount::SIZE];
        sm.write_to(&mut record);
        memory.write(buf.addr(), &record[..copy]).await?;
        Ok(0)
    }

    /// `do_statmount` from the mount on.
    fn statmount(
        &self,
        target: &Target,
        root: &Path,
        mask: u64,
        bufsize: usize,
    ) -> Result<Builder, Errno> {
        let mount = &target.mount;
        let info = self.vfs.stat_mount(mount, root);
        let sb = mount.sb();
        let mut b = Builder {
            sm: Statmount::ZERO,
            strings: Vec::new(),
            bufsize,
        };
        let want = |flag: StatmountMask| mask & flag.0 != 0;
        if want(StatmountMask::STATMOUNT_MNT_BASIC) {
            b.sm.mask |= StatmountMask::STATMOUNT_MNT_BASIC.0;
            b.sm.mnt_id = mount.unique_id();
            b.sm.mnt_id_old = mount.id as u32;
            b.sm.mnt_parent_id = MNT_UNIQUE_ID_OFFSET + info.parent;
            b.sm.mnt_parent_id_old = info.parent as u32;
            b.sm.mnt_attr = attr_flags(mount);
            b.sm.mnt_propagation = propagation(&info);
            b.sm.mnt_peer_group = info.shared.map_or(0, u64::from);
            b.sm.mnt_master = info.master.map_or(0, u64::from);
        }
        if want(StatmountMask::STATMOUNT_SB_BASIC) {
            b.sm.mask |= StatmountMask::STATMOUNT_SB_BASIC.0;
            b.sm.sb_dev_major = sb.dev.major;
            b.sm.sb_dev_minor = sb.dev.minor;
            b.sm.sb_magic = u64::from(sb.info.magic);
            b.sm.sb_flags = match sb.is_read_only() {
                true => SB_RDONLY,
                false => 0,
            };
        }
        if want(StatmountMask::STATMOUNT_PROPAGATE_FROM) {
            b.sm.mask |= StatmountMask::STATMOUNT_PROPAGATE_FROM.0;
            if info.master.is_some() {
                b.sm.propagate_from = info.dominating.map_or(0, u64::from);
            }
        }
        if want(StatmountMask::STATMOUNT_FS_TYPE) {
            b.string(StatmountMask::STATMOUNT_FS_TYPE, &sb.fs.type_name(), |sm| {
                &mut sm.fs_type
            });
            b.fits()?;
        }
        if want(StatmountMask::STATMOUNT_MNT_ROOT) {
            let in_fs = crate::procfs::root_in_fs(mount);
            b.string(StatmountMask::STATMOUNT_MNT_ROOT, &in_fs, |sm| {
                &mut sm.mnt_root
            });
            b.fits()?;
        }
        if want(StatmountMask::STATMOUNT_MNT_POINT) {
            // `seq_path_root`: nothing for a mount outside the root.
            let point = self.vfs.d_path(&mount.root_path(), root);
            if !point.starts_with(b"(unreachable)") {
                b.string(StatmountMask::STATMOUNT_MNT_POINT, &point, |sm| {
                    &mut sm.mnt_point
                });
                b.fits()?;
            }
        }
        if want(StatmountMask::STATMOUNT_MNT_OPTS) {
            let mut options = Vec::new();
            sb.fs.show_options(&mut options);
            // Without the leading comma.
            let options = options.get(1..).unwrap_or_default();
            b.string(StatmountMask::STATMOUNT_MNT_OPTS, options, |sm| {
                &mut sm.mnt_opts
            });
            b.fits()?;
        }
        if want(StatmountMask::STATMOUNT_OPT_ARRAY) {
            let mut options = Vec::new();
            sb.fs.show_options(&mut options);
            // `statmount_opt_array`: each option NUL-terminated, the count beside them.
            let list: Vec<&[u8]> = options
                .split(|&b| b == b',')
                .filter(|o| !o.is_empty())
                .collect();
            if !list.is_empty() {
                b.sm.opt_num = list.len() as u32;
                let joined = list.join(&0);
                b.string(StatmountMask::STATMOUNT_OPT_ARRAY, &joined, |sm| {
                    &mut sm.opt_array
                });
                b.fits()?;
            }
        }
        if want(StatmountMask::STATMOUNT_SB_SOURCE) {
            b.string(StatmountMask::STATMOUNT_SB_SOURCE, mount.devname(), |sm| {
                &mut sm.sb_source
            });
            b.fits()?;
        }
        if want(StatmountMask::STATMOUNT_MNT_NS_ID)
            && let Some(ns) = &target.ns
        {
            b.sm.mask |= StatmountMask::STATMOUNT_MNT_NS_ID.0;
            b.sm.mnt_ns_id = ns.id();
        }
        if want(StatmountMask::STATMOUNT_SUPPORTED_MASK) {
            b.sm.mask |= StatmountMask::STATMOUNT_SUPPORTED_MASK.0;
            b.sm.supported_mask = SUPPORTED;
        }
        Ok(b)
    }

    /// `listmount`.
    pub(crate) async fn sys_listmount(
        &self,
        caller: &Caller<'_>,
        req: ConstMntIdReqPtr,
        mnt_ids: U64Array,
        nr_mnt_ids: Usize,
        flags: u32,
    ) -> SysResult<i32> {
        let reverse = ListmountFlags::LISTMOUNT_REVERSE.0;
        if flags & !reverse != 0 {
            return Err(Errno::EINVAL.into());
        }
        if nr_mnt_ids > LISTMOUNT_MAX {
            return Err(Errno::EOVERFLOW.into());
        }
        let req = self.mnt_id_req(caller, req, false).await?;
        let last = req.param;
        if last != 0 && last <= MNT_UNIQUE_ID_OFFSET {
            return Err(Errno::EINVAL.into());
        }
        let env = self.env(caller);
        let ns = self.requested_ns(caller, &req)?;
        let admin = self.mnt_ns_admin(caller, &ns);
        let own = std::ptr::eq(&*ns, &*self.ns(caller).mnt.tree);
        if req.mnt_ns_id != 0 && !own && !admin {
            return Err(Errno::ENOENT.into());
        }
        let root = self.requested_root(caller, &env, &ns);
        let under = match req.mnt_id == ListmountMntId::LSMT_ROOT.0 {
            true => root.clone(),
            false => ns
                .mount_by_unique_id(req.mnt_id)
                .ok_or(Errno::ENOENT)?
                .root_path(),
        };
        if !self.vfs.is_path_reachable(&under.mnt, &under.dentry, &root) && !admin {
            return Err(Errno::EPERM.into());
        }
        let ids = self.vfs.list_mounts(
            &ns,
            &under,
            req.mnt_id,
            last,
            flags & reverse != 0,
            nr_mnt_ids as usize,
        );
        let bytes: Vec<u8> = ids.iter().flat_map(|id| id.to_le_bytes()).collect();
        if !bytes.is_empty() {
            caller.memory().write(mnt_ids.addr(), &bytes).await?;
        }
        Ok(ids.len() as i32)
    }
}
