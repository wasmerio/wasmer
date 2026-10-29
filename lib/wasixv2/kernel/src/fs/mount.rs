//! The `mount` family of syscalls (`fs/namespace.c`): `mount`, `umount2`, `pivot_root` and
//! `mount_setattr`, and the filesystem types `mount` makes instances of (`fs/filesystems.c`,
//! with the option parsing of `fs/fs_context.c`).
//!
//! The kernel resolves paths in the caller's context, checks that it may mount (`may_mount`:
//! `CAP_SYS_ADMIN` over the user namespace that owns its mount namespace) at the point Linux
//! does, and leaves the change itself to the VFS, which checks it against the caller's mount
//! namespace. A new instance of a type that a user namespace may mount (`FS_USERNS_MOUNT`)
//! takes `CAP_SYS_ADMIN` over the user namespace it is for, of any other type over the initial
//! one (`mount_capable`), and belongs to that user namespace (`s_user_ns`).

use std::sync::{Arc, Mutex};

use wasixv2_abi::{
    AtFlags, ConstMountAttrPtr, ConstVoidPtr, Cstr, MountAttrFlags, MountFlags as MsFlags,
    MoveMountFlags, OpenFlags, OpenTreeFlags, UmountFlags as UmountFlagBits, Usize,
};
use wasixv2_syscalls::{Caller, CstrError, Errno, Fd, SysResult, TaskId, read_cstr};
use wasixv2_vfs::{
    Cap, Cred, Filesystem, LookupFlags, MountAttr, MountFlags, MountOp, PATH_MAX, Path,
    Propagation, UmountFlags,
};

use super::{Env, read_path};
use crate::{
    Kernel,
    cred::Cred as TaskCred,
    file::{Backing, OpenFile},
    ns::UserNs,
    nsfs::NsFile,
    sync::MutexExt,
    userns::kernel_user_ns,
};

/// One `key[=value]` of a mount's options (`struct fs_parameter`), after the options every
/// filesystem takes are taken out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FsParam<'a> {
    pub key: &'a [u8],
    pub value: Option<&'a [u8]>,
}

/// A kind of filesystem `mount` can make (`struct file_system_type` with its `fs_context`
/// operations).
pub trait FilesystemType: Send + Sync {
    /// An instance made from `source` with the options `params`, for a caller with `cred`
    /// (`parse_param` then `get_tree`). An option the type does not take is `EINVAL`.
    fn mount(
        &self,
        source: &[u8],
        params: &[FsParam<'_>],
        cred: &Cred,
    ) -> Result<Arc<dyn Filesystem>, Errno>;

    /// Checks one parameter as `fsconfig` gives it (`parse_param`), before the instance is
    /// made from all of them. By default every parameter is taken, and [`Self::mount`]
    /// decides.
    fn parse_param(&self, _param: &FsParam<'_>) -> Result<(), Errno> {
        Ok(())
    }

    /// Whether the root of a user namespace may make instances (`FS_USERNS_MOUNT`): only for a
    /// type whose instances show nothing beyond what its mounter may see already, as an empty
    /// memory filesystem. By default not, as for a filesystem over the host's files.
    fn userns_mount(&self) -> bool {
        false
    }

    /// Changes `fs`, an instance of this type, as a remount's `params` say (`reconfigure`).
    /// By default a type takes no options on remount.
    fn reconfigure(&self, _fs: &Arc<dyn Filesystem>, params: &[FsParam<'_>]) -> Result<(), Errno> {
        match params.is_empty() {
            true => Ok(()),
            false => Err(Errno::EINVAL),
        }
    }
}

/// A registered filesystem type: one of the kernel's own, or one the embedder provides.
#[derive(Clone)]
pub(super) enum FsType {
    /// procfs: every mount is a new instance over the same kernel.
    Proc,
    Other(Arc<dyn FilesystemType>),
}

impl FsType {
    /// `FS_USERNS_MOUNT`: procfs is.
    pub(super) fn userns_mount(&self) -> bool {
        match self {
            FsType::Proc => true,
            FsType::Other(ty) => ty.userns_mount(),
        }
    }
}

/// The filesystem types `mount` knows (`file_systems`), in the order they were registered.
pub(crate) struct Registry {
    types: Mutex<Vec<(String, FsType)>>,
}

impl Registry {
    /// The kernel's own mountable types.
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            types: Mutex::new(vec![("proc".into(), FsType::Proc)]),
        })
    }

    pub(super) fn get(&self, name: &[u8]) -> Option<FsType> {
        let types = self.types.lock_or_recover();
        types
            .iter()
            .find(|(known, _)| known.as_bytes() == name)
            .map(|(_, ty)| ty.clone())
    }

    /// The names, for `/proc/filesystems`.
    pub(crate) fn names(&self) -> Vec<String> {
        let types = self.types.lock_or_recover();
        types.iter().map(|(name, _)| name.clone()).collect()
    }
}

/// `copy_mount_string`: a string argument, or `None` for a null pointer.
async fn copy_mount_string(caller: &Caller<'_>, ptr: Cstr) -> Result<Option<Vec<u8>>, Errno> {
    if ptr.is_null() {
        return Ok(None);
    }
    match read_cstr(caller.memory(), ptr, PATH_MAX).await {
        Ok(string) => Ok(Some(string)),
        Err(CstrError::Fault) => Err(Errno::EFAULT),
        Err(CstrError::TooLong) => Err(Errno::EINVAL),
    }
}

/// `copy_mount_options`: a page of options, or as much of it as is readable; `EFAULT` if none
/// is. The page's last byte is always a NUL (`path_mount`'s sanity check), and the options end
/// at the first NUL.
async fn copy_mount_options(caller: &Caller<'_>, ptr: ConstVoidPtr) -> Result<Vec<u8>, Errno> {
    const PAGE: u32 = crate::mm::PAGE_SIZE as u32;
    if ptr.is_null() {
        return Ok(Vec::new());
    }
    let memory = caller.memory();
    let mut page = Vec::with_capacity(PAGE as usize);
    let mut addr = ptr.addr();
    while page.len() < PAGE as usize - 1 {
        // Up to the next page boundary, as `copy_from_user` faults per page.
        let len = (PAGE - addr % PAGE).min(PAGE - 1 - page.len() as u32);
        let mut chunk = vec![0; len as usize];
        if memory.read(addr, &mut chunk).await.is_err() {
            break;
        }
        page.extend_from_slice(&chunk);
        let Some(next) = addr.checked_add(len) else {
            break;
        };
        addr = next;
    }
    if page.is_empty() {
        return Err(Errno::EFAULT);
    }
    if let Some(nul) = page.iter().position(|&b| b == 0) {
        page.truncate(nul);
    }
    Ok(page)
}

/// `vfs_parse_monolithic_sep` with commas, and `vfs_parse_sb_flag`: the options every
/// filesystem takes change `read_only` (`SB_RDONLY`; `sync`, `dirsync`, `lazytime` and
/// `mand` and their opposites change nothing here), the rest are the filesystem's. A
/// `source=` option names the source when the call gave none (`vfs_parse_fs_param_source`).
fn parse_options<'a>(
    data: &'a [u8],
    read_only: &mut bool,
    source: &mut Option<Vec<u8>>,
) -> Result<Vec<FsParam<'a>>, Errno> {
    let mut params = Vec::new();
    for option in data.split(|&b| b == b',') {
        if option.is_empty() {
            continue;
        }
        let (key, value) = match option.iter().position(|&b| b == b'=') {
            Some(0) => continue,
            Some(at) => (&option[..at], Some(&option[at + 1..])),
            None => (option, None),
        };
        match key {
            b"ro" => *read_only = true,
            b"rw" => *read_only = false,
            b"sync" | b"async" | b"dirsync" | b"lazytime" | b"nolazytime" | b"mand" | b"nomand" => {
            }
            b"source" => {
                if source.is_some() {
                    return Err(Errno::EINVAL);
                }
                *source = Some(value.ok_or(Errno::EINVAL)?.to_vec());
            }
            _ => params.push(FsParam { key, value }),
        }
    }
    Ok(params)
}

/// `path_mount`'s split of `MS_*` into per-mount flags. A remount that names no access-time
/// flag keeps the mount's.
fn mount_flags(flags: u32, remount_of: Option<&Path>) -> MountFlags {
    let has = |flag: MsFlags| flags & flag.0 != 0;
    let mut out = MountFlags {
        read_only: has(MsFlags::MS_RDONLY),
        nosuid: has(MsFlags::MS_NOSUID),
        nodev: has(MsFlags::MS_NODEV),
        noexec: has(MsFlags::MS_NOEXEC),
        noatime: has(MsFlags::MS_NOATIME),
        nodiratime: has(MsFlags::MS_NODIRATIME),
        strictatime: false,
        nosymfollow: has(MsFlags::MS_NOSYMFOLLOW),
    };
    if has(MsFlags::MS_STRICTATIME) {
        out.noatime = false;
        out.strictatime = true;
    }
    let atime = MsFlags::MS_NOATIME.0
        | MsFlags::MS_NODIRATIME.0
        | MsFlags::MS_RELATIME.0
        | MsFlags::MS_STRICTATIME.0;
    if let Some(target) = remount_of
        && flags & atime == 0
    {
        let old = target.mnt.flags();
        out.noatime = old.noatime;
        out.nodiratime = old.nodiratime;
        out.strictatime = old.strictatime;
    }
    out
}

/// `flags_to_propagation_type`: exactly one of the propagation flags, besides `MS_REC` and
/// `MS_SILENT`.
fn propagation_type(flags: u32) -> Option<Propagation> {
    let kind = flags & !(MsFlags::MS_REC.0 | MsFlags::MS_SILENT.0);
    [
        (MsFlags::MS_SHARED, Propagation::Shared),
        (MsFlags::MS_SLAVE, Propagation::Slave),
        (MsFlags::MS_PRIVATE, Propagation::Private),
        (MsFlags::MS_UNBINDABLE, Propagation::Unbindable),
    ]
    .into_iter()
    .find(|(flag, _)| kind == flag.0)
    .map(|(_, propagation)| propagation)
}

/// `MOUNT_SETATTR_VALID_FLAGS`.
const SETATTR_VALID: u64 = MountAttrFlags::MOUNT_ATTR_RDONLY.0
    | MountAttrFlags::MOUNT_ATTR_NOSUID.0
    | MountAttrFlags::MOUNT_ATTR_NODEV.0
    | MountAttrFlags::MOUNT_ATTR_NOEXEC.0
    | MountAttrFlags::MOUNT_ATTR__ATIME.0
    | MountAttrFlags::MOUNT_ATTR_NODIRATIME.0
    | MountAttrFlags::MOUNT_ATTR_IDMAP.0
    | MountAttrFlags::MOUNT_ATTR_NOSYMFOLLOW.0;

/// `attr_flags_to_mnt_flags`.
pub(super) fn attr_flags(attr: u64) -> MountFlags {
    let has = |flag: MountAttrFlags| attr & flag.0 != 0;
    MountFlags {
        read_only: has(MountAttrFlags::MOUNT_ATTR_RDONLY),
        nosuid: has(MountAttrFlags::MOUNT_ATTR_NOSUID),
        nodev: has(MountAttrFlags::MOUNT_ATTR_NODEV),
        noexec: has(MountAttrFlags::MOUNT_ATTR_NOEXEC),
        nodiratime: has(MountAttrFlags::MOUNT_ATTR_NODIRATIME),
        nosymfollow: has(MountAttrFlags::MOUNT_ATTR_NOSYMFOLLOW),
        ..MountFlags::default()
    }
}

impl Kernel {
    /// Makes `name` a filesystem type `mount` can make instances of (`register_filesystem`);
    /// `EBUSY` if the name is taken.
    pub fn register_filesystem(
        &self,
        name: &str,
        ty: Arc<dyn FilesystemType>,
    ) -> Result<(), Errno> {
        let mut types = self.filesystems.types.lock_or_recover();
        if types.iter().any(|(known, _)| known == name) {
            return Err(Errno::EBUSY);
        }
        types.push((name.into(), FsType::Other(ty)));
        Ok(())
    }

    /// `may_mount`: `CAP_SYS_ADMIN` over the user namespace that owns the caller's mount
    /// namespace.
    pub(super) fn may_mount(&self, env: &Env) -> Result<(), Errno> {
        let task = TaskId(env.task);
        let owner = self.task_ns(task).mnt.user_ns().clone();
        match self.process(task).cred().ns_capable(&owner, Cap::SysAdmin) {
            true => Ok(()),
            false => Err(Errno::EPERM),
        }
    }

    /// `mount_capable`: whether `cred` may make an instance of `ty` for the user namespace
    /// `user_ns` (`fc->user_ns`).
    pub(super) fn mount_capable(ty: &FsType, cred: &TaskCred, user_ns: &Arc<UserNs>) -> bool {
        match ty.userns_mount() {
            true => cred.ns_capable(user_ns, Cap::SysAdmin),
            false => cred.capable_init(Cap::SysAdmin),
        }
    }

    /// The user namespace a new instance of `ty` that the task `task` makes is for
    /// (`fc->user_ns`): the pid namespace's owner for procfs (`proc_init_fs_context`), the
    /// mounter's own for any other.
    pub(super) fn fc_user_ns(&self, ty: &FsType, task: TaskId) -> Arc<UserNs> {
        match ty {
            FsType::Proc => self.active_pid_ns(task).user_ns().clone(),
            FsType::Other(_) => self.process(task).cred().user_ns,
        }
    }

    /// The user namespace a superblock belongs to (`s_user_ns`).
    pub(crate) fn sb_owner(&self, sb: &wasixv2_vfs::Superblock) -> Arc<UserNs> {
        kernel_user_ns(&sb.owner, &self.init_user_ns)
    }

    /// `mount`.
    pub(crate) async fn sys_mount(
        &self,
        caller: &Caller<'_>,
        dev_name: Cstr,
        dir_name: Cstr,
        fstype: Cstr,
        flags: MsFlags,
        data: ConstVoidPtr,
    ) -> SysResult<i32> {
        let fstype = copy_mount_string(caller, fstype).await?;
        let dev_name = copy_mount_string(caller, dev_name).await?;
        let data = copy_mount_options(caller, data).await?;
        let dir = read_path(caller.memory(), dir_name).await?;
        let env = self.env(caller);
        let target = self
            .vfs
            .lookup(&env.cx(), &env.fs.cwd, &dir, LookupFlags::follow())
            .await?;
        let request = MountRequest {
            dev_name,
            fstype,
            flags: flags.0,
            data,
        };
        self.path_mount(caller, &env, &target, request).await?;
        Ok(0)
    }

    /// `path_mount`.
    async fn path_mount(
        &self,
        caller: &Caller<'_>,
        env: &Env,
        target: &Path,
        request: MountRequest,
    ) -> Result<(), Errno> {
        let MountRequest {
            dev_name,
            fstype,
            mut flags,
            data,
        } = request;
        if flags & MsFlags::MS_MGC_MSK.0 == MsFlags::MS_MGC_VAL.0 {
            flags &= !MsFlags::MS_MGC_MSK.0;
        }
        if flags & MsFlags::MS_NOUSER.0 != 0 {
            return Err(Errno::EINVAL);
        }
        self.may_mount(env)?;
        let ns = self.ns(caller).mnt.tree.clone();
        let has = |flag: MsFlags| flags & flag.0 != 0;
        let remount_bind = MsFlags::MS_REMOUNT.0 | MsFlags::MS_BIND.0;
        if flags & remount_bind == remount_bind {
            let flags = mount_flags(flags, Some(target));
            return self
                .vfs
                .do_mount(&ns, target, MountOp::Reconfigure { flags });
        }
        if has(MsFlags::MS_REMOUNT) {
            return self.remount(env, &ns, target, flags, &data);
        }
        if has(MsFlags::MS_BIND) {
            let source = self.old_path(env, dev_name).await?;
            let recursive = has(MsFlags::MS_REC);
            return self
                .vfs
                .do_mount(&ns, target, MountOp::Bind { source, recursive });
        }
        let propagation = MsFlags::MS_SHARED.0
            | MsFlags::MS_PRIVATE.0
            | MsFlags::MS_SLAVE.0
            | MsFlags::MS_UNBINDABLE.0;
        if flags & propagation != 0 {
            let kind = propagation_type(flags).ok_or(Errno::EINVAL)?;
            let recursive = has(MsFlags::MS_REC);
            let op = MountOp::Propagation { kind, recursive };
            return self.vfs.do_mount(&ns, target, op);
        }
        if has(MsFlags::MS_MOVE) {
            let source = self.old_path(env, dev_name).await?;
            return self.vfs.do_mount(&ns, target, MountOp::Move { source });
        }
        self.new_mount(env, &ns, target, fstype, dev_name, flags, &data)
    }

    /// The source of a bind or move: a non-empty path, resolved from the working directory
    /// (`kern_path` with `LOOKUP_FOLLOW`).
    async fn old_path(&self, env: &Env, name: Option<Vec<u8>>) -> Result<Path, Errno> {
        let name = name.filter(|name| !name.is_empty()).ok_or(Errno::EINVAL)?;
        self.vfs
            .lookup(&env.cx(), &env.fs.cwd, &name, LookupFlags::follow())
            .await
    }

    /// `do_new_mount`: an instance of the type `fstype` (`ENODEV` if there is none such)
    /// made from `dev_name` and the options in `data`.
    #[allow(clippy::too_many_arguments)]
    fn new_mount(
        &self,
        env: &Env,
        ns: &wasixv2_vfs::MntNs,
        target: &Path,
        fstype: Option<Vec<u8>>,
        dev_name: Option<Vec<u8>>,
        flags: u32,
        data: &[u8],
    ) -> Result<(), Errno> {
        let fstype = fstype.ok_or(Errno::EINVAL)?;
        let ty = self.filesystems.get(&fstype).ok_or(Errno::ENODEV)?;
        let mut read_only = flags & MsFlags::MS_RDONLY.0 != 0;
        let mut source = dev_name;
        let params = parse_options(data, &mut read_only, &mut source)?;
        let task = TaskId(env.task);
        let user_ns = self.fc_user_ns(&ty, task);
        if matches!(ty, FsType::Proc) && !params.is_empty() {
            return Err(Errno::EINVAL);
        }
        if !Self::mount_capable(&ty, &self.process(task).cred(), &user_ns) {
            return Err(Errno::EPERM);
        }
        let fs = match ty {
            // `proc_init_fs_context`: the instance shows the mounter's pid namespace.
            FsType::Proc => self.procfs_in(self.active_pid_ns(task)),
            FsType::Other(ty) => ty.mount(source.as_deref().unwrap_or(b""), &params, &env.cred)?,
        };
        let op = MountOp::New {
            fs,
            source: source.unwrap_or_else(|| b"none".to_vec()),
            flags: mount_flags(flags, None),
            sb_read_only: read_only,
            owner: user_ns,
        };
        self.vfs.do_mount(ns, target, op)
    }

    /// `do_remount`: the filesystem's options and read-only state, and the mount's flags, with
    /// `CAP_SYS_ADMIN` over the user namespace the filesystem belongs to.
    fn remount(
        &self,
        env: &Env,
        ns: &wasixv2_vfs::MntNs,
        target: &Path,
        flags: u32,
        data: &[u8],
    ) -> Result<(), Errno> {
        let mut read_only = flags & MsFlags::MS_RDONLY.0 != 0;
        let mut source = None;
        let params = parse_options(data, &mut read_only, &mut source)?;
        if source.is_some() {
            return Err(Errno::EINVAL);
        }
        // `check_mnt` and `path_mounted` come before the filesystem sees its options.
        if !ns.contains(&target.mnt) || !target.is_mount_root() {
            return Err(Errno::EINVAL);
        }
        let sb = target.mnt.sb();
        let cred = self.process(TaskId(env.task)).cred();
        if !cred.ns_capable(&self.sb_owner(sb), Cap::SysAdmin) {
            return Err(Errno::EPERM);
        }
        match self.filesystems.get(sb.info.name.as_bytes()) {
            Some(FsType::Other(ty)) => ty.reconfigure(&sb.fs, &params)?,
            _ if params.is_empty() => {}
            _ => return Err(Errno::EINVAL),
        }
        let op = MountOp::Remount {
            read_only,
            flags: mount_flags(flags, Some(target)),
        };
        self.vfs.do_mount(ns, target, op)
    }

    /// `umount2` (`ksys_umount`).
    pub(crate) async fn sys_umount2(
        &self,
        caller: &Caller<'_>,
        name: Cstr,
        flags: UmountFlagBits,
    ) -> SysResult<i32> {
        let known = UmountFlagBits::MNT_FORCE.0
            | UmountFlagBits::MNT_DETACH.0
            | UmountFlagBits::MNT_EXPIRE.0
            | UmountFlagBits::UMOUNT_NOFOLLOW.0;
        if flags.0 & !known != 0 {
            return Err(Errno::EINVAL.into());
        }
        let has = |flag: UmountFlagBits| flags.0 & flag.0 != 0;
        let name = read_path(caller.memory(), name).await?;
        let env = self.env(caller);
        let lookup = LookupFlags {
            follow: !has(UmountFlagBits::UMOUNT_NOFOLLOW),
            ..LookupFlags::default()
        };
        let path = self
            .vfs
            .lookup(&env.cx(), &env.fs.cwd, &name, lookup)
            .await?;
        // `can_umount`.
        self.may_mount(&env)?;
        let flags = UmountFlags {
            force: has(UmountFlagBits::MNT_FORCE),
            detach: has(UmountFlagBits::MNT_DETACH),
            expire: has(UmountFlagBits::MNT_EXPIRE),
        };
        let ns = self.ns(caller).mnt.tree.clone();
        self.vfs.do_umount(&ns, &env.fs.root, &path, flags)?;
        Ok(0)
    }

    /// `pivot_root`: the caller's root mount moves to `put_old` and the mount at `new_root`
    /// takes its place; every process whose root or working directory was the caller's root
    /// moves to `new_root` (`chroot_fs_refs`).
    pub(crate) async fn sys_pivot_root(
        &self,
        caller: &Caller<'_>,
        new_root: Cstr,
        put_old: Cstr,
    ) -> SysResult<i32> {
        let new_root = read_path(caller.memory(), new_root).await?;
        let put_old = read_path(caller.memory(), put_old).await?;
        let env = self.env(caller);
        let lookup = LookupFlags {
            follow: true,
            directory: true,
            ..LookupFlags::default()
        };
        let cx = env.cx();
        let new = self.vfs.lookup(&cx, &env.fs.cwd, &new_root, lookup).await?;
        let old = self.vfs.lookup(&cx, &env.fs.cwd, &put_old, lookup).await?;
        self.may_mount(&env)?;
        let ns = self.ns(caller).mnt.tree.clone();
        let root = env.fs.root.clone();
        self.vfs.pivot_root(&ns, &root, &new, &old)?;
        self.chroot_fs_refs(&root, &new);
        Ok(0)
    }

    /// `chroot_fs_refs`: every root and working directory at `old` moves to `new`.
    fn chroot_fs_refs(&self, old: &Path, new: &Path) {
        let state = self.state.read_or_recover();
        for task in state.tasks.values() {
            let mut fs = task.fs.lock_or_recover();
            if fs.root == *old {
                fs.root = new.clone();
            }
            if fs.cwd == *old {
                fs.cwd = new.clone();
            }
        }
    }

    /// `mount_setattr`.
    pub(crate) async fn sys_mount_setattr(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        path: Cstr,
        flags: AtFlags,
        uattr: ConstMountAttrPtr,
        usize: Usize,
    ) -> SysResult<i32> {
        let known = AtFlags::AT_EMPTY_PATH.0
            | AtFlags::AT_RECURSIVE.0
            | AtFlags::AT_SYMLINK_NOFOLLOW.0
            | AtFlags::AT_NO_AUTOMOUNT.0;
        if flags.0 & !known != 0 {
            return Err(Errno::EINVAL.into());
        }
        let env = self.env(caller);
        // `wants_mount_setattr`.
        const VER0: u32 = 32;
        if usize > crate::mm::PAGE_SIZE as u32 {
            return Err(Errno::E2BIG.into());
        }
        if usize < VER0 {
            return Err(Errno::EINVAL.into());
        }
        self.may_mount(&env)?;
        let mut bytes = vec![0; usize as usize];
        caller.memory().read(uattr.addr(), &mut bytes).await?;
        if bytes[VER0 as usize..].iter().any(|&b| b != 0) {
            return Err(Errno::E2BIG.into());
        }
        let field = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().expect("8 bytes"));
        let (set, clear, propagation, userns_fd) = (field(0), field(8), field(16), field(24));
        if set == 0 && clear == 0 && propagation == 0 {
            return Ok(0);
        }
        let mut attr = self.build_mount_kattr(caller, set, clear, propagation, userns_fd)?;
        attr.recursive = flags.contains(AtFlags::AT_RECURSIVE);
        let path = read_path(caller.memory(), path).await?;
        let mut at = flags;
        at.0 &= !AtFlags::AT_RECURSIVE.0;
        let target = self.resolve_at(caller, &env, dfd, &path, at, true).await?;
        let ns = self.ns(caller).mnt.tree.clone();
        self.vfs.mount_setattr(&ns, &target, &attr)?;
        Ok(0)
    }

    /// `build_mount_kattr`: the change `mount_attr` asks for. The access-time setting is a
    /// value, not a flag: setting one needs `MOUNT_ATTR__ATIME` in `attr_clr`.
    fn build_mount_kattr(
        &self,
        caller: &Caller<'_>,
        set: u64,
        clear: u64,
        propagation: u64,
        userns_fd: u64,
    ) -> Result<MountAttr, Errno> {
        let kinds = MsFlags::MS_UNBINDABLE.0
            | MsFlags::MS_PRIVATE.0
            | MsFlags::MS_SLAVE.0
            | MsFlags::MS_SHARED.0;
        if propagation & !u64::from(kinds) != 0 || propagation.count_ones() > 1 {
            return Err(Errno::EINVAL);
        }
        let propagation = match propagation {
            0 => None,
            kind => Some(propagation_type(kind as u32).ok_or(Errno::EINVAL)?),
        };
        if (set | clear) & !SETATTR_VALID != 0 {
            return Err(Errno::EINVAL);
        }
        let mut attr = MountAttr {
            set: attr_flags(set),
            clear: attr_flags(clear),
            propagation,
            recursive: false,
        };
        let atime = MountAttrFlags::MOUNT_ATTR__ATIME.0;
        if clear & atime != 0 {
            if clear & atime != atime {
                return Err(Errno::EINVAL);
            }
            attr.clear.noatime = true;
            attr.clear.strictatime = true;
            match set & atime {
                0 => {}
                a if a == MountAttrFlags::MOUNT_ATTR_NOATIME.0 => attr.set.noatime = true,
                a if a == MountAttrFlags::MOUNT_ATTR_STRICTATIME.0 => attr.set.strictatime = true,
                _ => return Err(Errno::EINVAL),
            }
        } else if set & atime != 0 {
            return Err(Errno::EINVAL);
        }
        self.build_mount_idmapped(caller, set, clear, userns_fd)?;
        Ok(attr)
    }

    /// `build_mount_idmapped`: an idmapped mount needs a user namespace other than the initial
    /// one, and there is no other.
    fn build_mount_idmapped(
        &self,
        caller: &Caller<'_>,
        set: u64,
        clear: u64,
        userns_fd: u64,
    ) -> Result<(), Errno> {
        let idmap = MountAttrFlags::MOUNT_ATTR_IDMAP.0;
        if (set | clear) & idmap == 0 {
            return Ok(());
        }
        // Only a mount never exposed may lose its idmapping (`MOUNT_KATTR_IDMAP_REPLACE`).
        if clear & idmap != 0 {
            return Err(Errno::EINVAL);
        }
        if userns_fd > i32::MAX as u64 {
            return Err(Errno::EINVAL);
        }
        let file = self.file(caller.task(), Fd(userns_fd as i32))?;
        let ns = file.downcast::<NsFile>().ok_or(Errno::EINVAL)?;
        if ns.namespace().kind() != crate::ns::NsKind::User {
            return Err(Errno::EINVAL);
        }
        Err(Errno::EPERM)
    }
}

impl Kernel {
    /// `open_tree`: an `O_PATH` file at `filename`, or with `OPEN_TREE_CLONE` at the root of a
    /// detached copy of the tree there (with `AT_RECURSIVE`, of what is mounted below too).
    pub(crate) async fn sys_open_tree(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        flags: u32,
    ) -> SysResult<i32> {
        let clone = OpenTreeFlags::OPEN_TREE_CLONE.0;
        let recursive = AtFlags::AT_RECURSIVE.0;
        // `OPEN_TREE_NAMESPACE` (a new mount namespace around the copy) is not supported.
        let known = AtFlags::AT_EMPTY_PATH.0
            | AtFlags::AT_NO_AUTOMOUNT.0
            | recursive
            | AtFlags::AT_SYMLINK_NOFOLLOW.0
            | clone
            | OpenFlags::O_CLOEXEC.0;
        if flags & !known != 0 || flags & (recursive | clone) == recursive {
            return Err(Errno::EINVAL.into());
        }
        let env = self.env(caller);
        if flags & clone != 0 {
            self.may_mount(&env)?;
        }
        let name = read_path(caller.memory(), filename).await?;
        let at = AtFlags(flags & (AtFlags::AT_EMPTY_PATH.0 | AtFlags::AT_SYMLINK_NOFOLLOW.0));
        let path = self.resolve_at(caller, &env, dfd, &name, at, true).await?;
        let file = match flags & clone != 0 {
            true => {
                let ns = self.ns(caller).mnt.tree.clone();
                let copy = self.vfs.open_tree(&ns, &path, flags & recursive != 0)?;
                OpenFile::new(Backing::Inode(copy), OpenFlags::O_PATH)
            }
            false => self.open_path(&env, &path).await?,
        };
        let cloexec = flags & OpenFlags::O_CLOEXEC.0 != 0;
        let [fd] = self.install_files(caller, [file], cloexec)?;
        Ok(fd.0)
    }

    /// `move_mount`: moves the mount at `from` (or `from_dfd`'s, with an empty path) onto
    /// `to`, or attaches the detached tree it is the root of; with `MOVE_MOUNT_BENEATH` under
    /// the topmost mount there, and with `MOVE_MOUNT_SET_GROUP` makes `to` a peer of `from`
    /// instead.
    pub(crate) async fn sys_move_mount(
        &self,
        caller: &Caller<'_>,
        from_dfd: Fd,
        from: Cstr,
        to_dfd: Fd,
        to: Cstr,
        flags: u32,
    ) -> SysResult<i32> {
        let env = self.env(caller);
        self.may_mount(&env)?;
        let has = |flag: MoveMountFlags| flags & flag.0 != 0;
        if flags & !MoveMountFlags::MOVE_MOUNT__MASK.0 != 0 {
            return Err(Errno::EINVAL.into());
        }
        if has(MoveMountFlags::MOVE_MOUNT_BENEATH) && has(MoveMountFlags::MOVE_MOUNT_SET_GROUP) {
            return Err(Errno::EINVAL.into());
        }
        let to = self
            .move_mount_path(
                caller,
                &env,
                to_dfd,
                to,
                has(MoveMountFlags::MOVE_MOUNT_T_EMPTY_PATH),
                has(MoveMountFlags::MOVE_MOUNT_T_SYMLINKS),
            )
            .await?;
        let from = self
            .move_mount_path(
                caller,
                &env,
                from_dfd,
                from,
                has(MoveMountFlags::MOVE_MOUNT_F_EMPTY_PATH),
                has(MoveMountFlags::MOVE_MOUNT_F_SYMLINKS),
            )
            .await?;
        if has(MoveMountFlags::MOVE_MOUNT_SET_GROUP) {
            self.vfs.set_group(&from, &to)?;
            return Ok(0);
        }
        let ns = self.ns(caller).mnt.tree.clone();
        let beneath = has(MoveMountFlags::MOVE_MOUNT_BENEATH);
        self.vfs.move_mount(&ns, &from, &to, beneath)?;
        Ok(0)
    }

    /// One end of `move_mount`: `dfd`'s file for an empty path with `*_EMPTY_PATH`
    /// (`filename_maybe_null`), else the path resolved from `dfd`, following a final symlink
    /// only with `*_SYMLINKS`.
    async fn move_mount_path(
        &self,
        caller: &Caller<'_>,
        env: &Env,
        dfd: Fd,
        name: Cstr,
        empty: bool,
        follow: bool,
    ) -> Result<Path, Errno> {
        let name = match empty && name.is_null() {
            true => Vec::new(),
            false => read_path_empty(caller, name, empty).await?,
        };
        if name.is_empty() && dfd.0 >= 0 {
            return self.fd_path(caller, dfd);
        }
        let mut at = AtFlags(0);
        if empty {
            at |= AtFlags::AT_EMPTY_PATH;
        }
        if !follow {
            at |= AtFlags::AT_SYMLINK_NOFOLLOW;
        }
        self.resolve_at(caller, env, dfd, &name, at, true).await
    }
}

/// A path argument that may be empty when `empty` allows it (`getname_flags` with
/// `LOOKUP_EMPTY`): `ENOENT` for an empty one otherwise.
async fn read_path_empty(caller: &Caller<'_>, name: Cstr, empty: bool) -> Result<Vec<u8>, Errno> {
    match read_cstr(caller.memory(), name, PATH_MAX).await {
        Ok(name) if name.is_empty() && !empty => Err(Errno::ENOENT),
        Ok(name) => Ok(name),
        Err(CstrError::Fault) => Err(Errno::EFAULT),
        Err(CstrError::TooLong) => Err(Errno::ENAMETOOLONG),
    }
}

/// What `mount` was called with.
struct MountRequest {
    dev_name: Option<Vec<u8>>,
    fstype: Option<Vec<u8>>,
    flags: u32,
    data: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_split_at_commas_and_the_common_ones_set_read_only() {
        let mut read_only = false;
        let mut source = None;
        let params = parse_options(
            b"size=1m,ro,,mode=0700,=x,huge",
            &mut read_only,
            &mut source,
        )
        .unwrap();
        assert!(read_only);
        assert_eq!(
            params,
            [
                FsParam {
                    key: b"size",
                    value: Some(b"1m")
                },
                FsParam {
                    key: b"mode",
                    value: Some(b"0700")
                },
                FsParam {
                    key: b"huge",
                    value: None
                },
            ]
        );
        parse_options(b"rw,source=dev", &mut read_only, &mut source).unwrap();
        assert!(!read_only);
        assert_eq!(source.as_deref(), Some(b"dev".as_slice()));
        assert_eq!(
            parse_options(b"source=again", &mut read_only, &mut source),
            Err(Errno::EINVAL)
        );
    }

    #[test]
    fn exactly_one_propagation_flag_is_a_type() {
        let rec = MsFlags::MS_REC.0;
        assert_eq!(
            propagation_type(MsFlags::MS_PRIVATE.0 | rec),
            Some(Propagation::Private)
        );
        assert_eq!(
            propagation_type(MsFlags::MS_SHARED.0 | MsFlags::MS_SLAVE.0),
            None
        );
        assert_eq!(
            propagation_type(MsFlags::MS_SHARED.0 | MsFlags::MS_RDONLY.0),
            None
        );
    }
}
