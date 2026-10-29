//! Filesystem contexts (`fs/fsopen.c`, `fs/fs_context.c`): `fsopen` makes a context for a new
//! instance of a filesystem type, `fspick` one for reconfiguring a mounted filesystem,
//! `fsconfig` gives it parameters and creates or reconfigures, and `fsmount` turns a created
//! instance into a detached mount that `move_mount` attaches.
//!
//! A context is an anonymous file (`anon_inode:[fscontext]`). The filesystem types take their
//! options as `mount`'s data does, so parameters are checked one by one as they come
//! ([`FilesystemType::parse_param`]) and the instance is made from all of them at
//! `FSCONFIG_CMD_CREATE`. No messages are logged: reading the file is `ENODATA`.

use std::sync::{Arc, Mutex};

use wasixv2_abi::{
    AtFd, ConstVoidPtr, Cstr, FsconfigCommand, FsmountFlags, FsopenFlags, FspickFlags,
    MountAttrFlags, OpenFlags,
};
use wasixv2_syscalls::{Caller, CstrError, Errno, Fd, LocalFuture, SysResult, TaskId, read_cstr};
use wasixv2_vfs::{Cap, Cred, Filesystem, LookupFlags, OpenHow, Path};
use wasixv2_wait::{PollMask, PollTable, Pollable};

use super::mount::{FsParam, FsType};
use crate::ns::NetNs;
use crate::{
    Kernel,
    file::{Backing, FileObject, Io, OpenFile},
    sync::MutexExt,
};

/// The longest key and string value `fsconfig` copies (`strndup_user(.., 256)`).
const PARAM_MAX: usize = 256;
/// The largest binary value `fsconfig` takes.
const BLOB_MAX: i32 = 1024 * 1024;

/// Where a context is (`enum fs_context_phase`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Taking parameters for a new instance.
    CreateParams,
    /// Created, waiting for `fsmount`.
    AwaitingMount,
    /// Mounted or reconfigured: the next parameter starts a reconfiguration.
    AwaitingReconf,
    /// Taking parameters for reconfiguring the instance.
    ReconfParams,
    /// A creation or reconfiguration failed; the context takes nothing more.
    Failed,
}

/// The state of a context (`struct fs_context`).
struct Context {
    ty: FsType,
    phase: Phase,
    /// `source=`, or `None`.
    source: Option<Vec<u8>>,
    /// The filesystem's own parameters, as `key[=value]`.
    params: Vec<(Vec<u8>, Option<Vec<u8>>)>,
    /// `ro` or `rw` as given (`sb_flags` within `sb_flags_mask`).
    read_only: Option<bool>,
    /// The instance, once created or picked (`fc->root`).
    fs: Option<Arc<dyn Filesystem>>,
    /// The mount `fspick` picked, whose filesystem a reconfiguration changes.
    picked: Option<Path>,
    /// The credentials of the opener (`fc->cred`).
    cred: Cred,
    /// The opener's pid namespace, which a proc instance shows (`proc_init_fs_context`).
    pid_ns: Arc<crate::ns::PidNs>,
    /// The opener's network namespace (`fc->net_ns`), which a sysfs instance shows.
    net: Arc<NetNs>,
}

/// An open filesystem context (`fscontext_fops`).
pub(crate) struct FsContextFile(Mutex<Context>);

impl Pollable for FsContextFile {
    fn poll(&self, _: Option<&mut PollTable<'_>>) -> PollMask {
        PollMask::READABLE | PollMask::WRITABLE
    }
}

impl FileObject for FsContextFile {
    /// `fscontext_read`: there are never messages.
    fn read<'a>(&'a self, _: Io<'a>, _: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async { Err(Errno::ENODATA.into()) })
    }

    fn write<'a>(&'a self, _: Io<'a>, _: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async { Err(Errno::EINVAL.into()) })
    }

    fn can_poll(&self) -> bool {
        false
    }
}

/// A parameter as `fsconfig` passes it (`struct fs_parameter`).
enum Value {
    Flag,
    String(Vec<u8>),
    /// A binary blob, a path or a file: no filesystem here takes one.
    Other,
}

impl Context {
    fn new(
        ty: FsType,
        phase: Phase,
        cred: Cred,
        pid_ns: Arc<crate::ns::PidNs>,
        net: Arc<NetNs>,
    ) -> Self {
        Self {
            net,
            ty,
            phase,
            source: None,
            params: Vec::new(),
            read_only: None,
            fs: None,
            picked: None,
            cred,
            pid_ns,
        }
    }

    /// `finish_clean_context`: after a mount or reconfiguration, the next use starts a new
    /// reconfiguration.
    fn finish_clean(&mut self) {
        if self.phase == Phase::AwaitingReconf {
            self.phase = Phase::ReconfParams;
        }
    }

    /// `vfs_clean_context`.
    fn clean(&mut self) {
        self.source = None;
        self.params.clear();
        self.read_only = None;
        self.phase = Phase::AwaitingReconf;
    }

    /// `vfs_parse_fs_param`: `ro` and `rw`, the filesystem's own parameters, then `source`.
    fn parse_param(&mut self, key: Vec<u8>, value: Value) -> Result<(), Errno> {
        if !matches!(self.phase, Phase::CreateParams | Phase::ReconfParams) {
            return Err(Errno::EBUSY);
        }
        // `vfs_parse_sb_flag`.
        match key.as_slice() {
            b"ro" => {
                self.read_only = Some(true);
                return Ok(());
            }
            b"rw" => {
                self.read_only = Some(false);
                return Ok(());
            }
            b"sync" | b"async" | b"dirsync" | b"lazytime" | b"nolazytime" | b"mand" | b"nomand" => {
                return Ok(());
            }
            _ => {}
        }
        let string = match value {
            Value::Flag => None,
            Value::String(string) => Some(string),
            Value::Other => return Err(Errno::EINVAL),
        };
        if key == b"source" {
            // `vfs_parse_fs_param_source`.
            let source = string.ok_or(Errno::EINVAL)?;
            if self.source.is_some() {
                return Err(Errno::EINVAL);
            }
            self.source = Some(source);
            return Ok(());
        }
        let param = FsParam {
            key: &key,
            value: string.as_deref(),
        };
        match &self.ty {
            FsType::Proc | FsType::Sys => return Err(Errno::EINVAL),
            FsType::Other(ty) => ty.parse_param(&param)?,
        }
        self.params.push((key, string));
        Ok(())
    }

    fn params(&self) -> Vec<FsParam<'_>> {
        self.params
            .iter()
            .map(|(key, value)| FsParam {
                key,
                value: value.as_deref(),
            })
            .collect()
    }
}

impl Kernel {
    /// `fsopen`.
    pub(crate) async fn sys_fsopen(
        &self,
        caller: &Caller<'_>,
        fs_name: Cstr,
        flags: u32,
    ) -> SysResult<i32> {
        let env = self.env(caller);
        Self::may_mount(&env)?;
        if flags & !FsopenFlags::FSOPEN_CLOEXEC.0 != 0 {
            return Err(Errno::EINVAL.into());
        }
        let name = copy_string(caller, fs_name, crate::mm::PAGE_SIZE as usize).await?;
        let ty = self.filesystems.get(&name).ok_or(Errno::ENODEV)?;
        let pid_ns = self.active_pid_ns(TaskId(env.task));
        let net = self.task_ref(caller.task(), |t| t.ns.net.clone());
        let context = Context::new(ty, Phase::CreateParams, env.cred, pid_ns, net);
        let cloexec = flags & FsopenFlags::FSOPEN_CLOEXEC.0 != 0;
        self.install_context(caller, context, cloexec)
    }

    /// `fspick`: a context for reconfiguring the filesystem mounted at `path`.
    pub(crate) async fn sys_fspick(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        path: Cstr,
        flags: u32,
    ) -> SysResult<i32> {
        let env = self.env(caller);
        Self::may_mount(&env)?;
        let known = FspickFlags::FSPICK_CLOEXEC.0
            | FspickFlags::FSPICK_SYMLINK_NOFOLLOW.0
            | FspickFlags::FSPICK_NO_AUTOMOUNT.0
            | FspickFlags::FSPICK_EMPTY_PATH.0;
        if flags & !known != 0 {
            return Err(Errno::EINVAL.into());
        }
        let has = |flag: FspickFlags| flags & flag.0 != 0;
        let name = super::read_path(caller.memory(), path).await?;
        let mut at = wasixv2_abi::AtFlags(0);
        if has(FspickFlags::FSPICK_EMPTY_PATH) {
            at |= wasixv2_abi::AtFlags::AT_EMPTY_PATH;
        }
        if has(FspickFlags::FSPICK_SYMLINK_NOFOLLOW) {
            at |= wasixv2_abi::AtFlags::AT_SYMLINK_NOFOLLOW;
        }
        let target = self.resolve_at(caller, &env, dfd, &name, at, true).await?;
        if !target.is_mount_root() {
            return Err(Errno::EINVAL.into());
        }
        let sb = target.mnt.sb();
        let ty = self
            .filesystems
            .get(sb.info.name.as_bytes())
            .ok_or(Errno::EINVAL)?;
        let pid_ns = self.active_pid_ns(TaskId(env.task));
        let net = self.task_ref(caller.task(), |t| t.ns.net.clone());
        let mut context = Context::new(ty, Phase::ReconfParams, env.cred, pid_ns, net);
        context.fs = Some(sb.fs.clone());
        context.picked = Some(target);
        let cloexec = has(FspickFlags::FSPICK_CLOEXEC);
        self.install_context(caller, context, cloexec)
    }

    /// `fscontext_create_fd`.
    fn install_context(
        &self,
        caller: &Caller<'_>,
        context: Context,
        cloexec: bool,
    ) -> SysResult<i32> {
        let object = FsContextFile(Mutex::new(context));
        let file = OpenFile::new(
            Backing::Stream(Arc::new(object), self.pseudo.anon("fscontext")),
            OpenFlags::O_RDWR,
        );
        let [fd] = self.install_files(caller, [file], cloexec)?;
        Ok(fd.0)
    }

    /// `fsconfig`.
    pub(crate) async fn sys_fsconfig(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        cmd: u32,
        key: Cstr,
        value: ConstVoidPtr,
        aux: i32,
    ) -> SysResult<i32> {
        if fd.0 < 0 {
            return Err(Errno::EINVAL.into());
        }
        let (has_key, has_value) = (!key.is_null(), !value.is_null());
        let valid = match FsconfigCommand(cmd) {
            FsconfigCommand::FSCONFIG_SET_FLAG => has_key && !has_value && aux == 0,
            FsconfigCommand::FSCONFIG_SET_STRING => has_key && has_value && aux == 0,
            FsconfigCommand::FSCONFIG_SET_BINARY => {
                has_key && has_value && aux > 0 && aux <= BLOB_MAX
            }
            FsconfigCommand::FSCONFIG_SET_PATH | FsconfigCommand::FSCONFIG_SET_PATH_EMPTY => {
                has_key && has_value && (aux == AtFd::AT_FDCWD.0 || aux >= 0)
            }
            FsconfigCommand::FSCONFIG_SET_FD => has_key && !has_value && aux >= 0,
            FsconfigCommand::FSCONFIG_CMD_CREATE
            | FsconfigCommand::FSCONFIG_CMD_CREATE_EXCL
            | FsconfigCommand::FSCONFIG_CMD_RECONFIGURE => !has_key && !has_value && aux == 0,
            _ => return Err(Errno::EOPNOTSUPP.into()),
        };
        if !valid {
            return Err(Errno::EINVAL.into());
        }
        let file = self.file(caller.task(), fd)?;
        let context = file.downcast::<FsContextFile>().ok_or(Errno::EINVAL)?;
        let key = match has_key {
            true => Some(copy_string(caller, Cstr::new(key.addr()), PARAM_MAX).await?),
            false => None,
        };
        let value = match FsconfigCommand(cmd) {
            FsconfigCommand::FSCONFIG_SET_FLAG => Value::Flag,
            FsconfigCommand::FSCONFIG_SET_STRING => {
                Value::String(copy_string(caller, Cstr::new(value.addr()), PARAM_MAX).await?)
            }
            FsconfigCommand::FSCONFIG_SET_BINARY => {
                let mut blob = vec![0; aux as usize];
                caller.memory().read(value.addr(), &mut blob).await?;
                Value::Other
            }
            FsconfigCommand::FSCONFIG_SET_PATH | FsconfigCommand::FSCONFIG_SET_PATH_EMPTY => {
                super::read_path(caller.memory(), Cstr::new(value.addr())).await?;
                Value::Other
            }
            FsconfigCommand::FSCONFIG_SET_FD => {
                self.file(caller.task(), Fd(aux))?;
                Value::Other
            }
            _ => Value::Flag,
        };
        let env = self.env(caller);
        let mut context = context.0.lock_or_recover();
        context.finish_clean();
        let result = match FsconfigCommand(cmd) {
            FsconfigCommand::FSCONFIG_CMD_CREATE | FsconfigCommand::FSCONFIG_CMD_CREATE_EXCL => {
                self.fs_create(&mut context)
            }
            FsconfigCommand::FSCONFIG_CMD_RECONFIGURE => self.fs_reconfigure(&mut context, &env),
            _ => context.parse_param(key.expect("checked above"), value),
        };
        result?;
        Ok(0)
    }

    /// `vfs_cmd_create`: the instance, from the parameters given.
    fn fs_create(&self, context: &mut Context) -> Result<(), Errno> {
        if context.phase != Phase::CreateParams {
            return Err(Errno::EBUSY);
        }
        // `mount_capable`.
        if !context.cred.capable(Cap::SysAdmin) {
            return Err(Errno::EPERM);
        }
        let made = match &context.ty {
            FsType::Proc => Ok(self.procfs_in(context.pid_ns.clone())),
            FsType::Sys => Ok(self.sysfs_of(context.net.clone())),
            FsType::Other(ty) => {
                let source = context.source.clone().unwrap_or_default();
                ty.mount(&source, &context.params(), &context.cred)
            }
        };
        match made {
            Ok(fs) => {
                context.fs = Some(fs);
                context.phase = Phase::AwaitingMount;
                Ok(())
            }
            Err(errno) => {
                context.phase = Phase::Failed;
                Err(errno)
            }
        }
    }

    /// `vfs_cmd_reconfigure`: the parameters given applied to the instance
    /// (`reconfigure_super`).
    fn fs_reconfigure(&self, context: &mut Context, env: &super::Env) -> Result<(), Errno> {
        if context.phase != Phase::ReconfParams {
            return Err(Errno::EBUSY);
        }
        if !env.cred.capable(Cap::SysAdmin) {
            context.phase = Phase::Failed;
            return Err(Errno::EPERM);
        }
        let fs = context.fs.clone().ok_or(Errno::EINVAL)?;
        let result = (|| {
            if let FsType::Other(ty) = &context.ty {
                ty.reconfigure(&fs, &context.params())?;
            } else if !context.params.is_empty() {
                return Err(Errno::EINVAL);
            }
            if let (Some(read_only), Some(picked)) = (context.read_only, &context.picked) {
                self.vfs.reconfigure_read_only(picked.mnt.sb(), read_only)?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                context.clean();
                Ok(())
            }
            Err(errno) => {
                context.phase = Phase::Failed;
                Err(errno)
            }
        }
    }

    /// `fsmount`: the instance the context created, as a detached mount.
    pub(crate) async fn sys_fsmount(
        &self,
        caller: &Caller<'_>,
        fs_fd: Fd,
        flags: u32,
        attr_flags: u32,
    ) -> SysResult<i32> {
        // `FSMOUNT_NAMESPACE`, a new mount namespace around the mount, is not supported.
        if flags & !FsmountFlags::FSMOUNT_CLOEXEC.0 != 0 {
            return Err(Errno::EINVAL.into());
        }
        let env = self.env(caller);
        Self::may_mount(&env)?;
        let attr = u64::from(attr_flags);
        let valid = MountAttrFlags::MOUNT_ATTR_RDONLY.0
            | MountAttrFlags::MOUNT_ATTR_NOSUID.0
            | MountAttrFlags::MOUNT_ATTR_NODEV.0
            | MountAttrFlags::MOUNT_ATTR_NOEXEC.0
            | MountAttrFlags::MOUNT_ATTR__ATIME.0
            | MountAttrFlags::MOUNT_ATTR_NODIRATIME.0
            | MountAttrFlags::MOUNT_ATTR_NOSYMFOLLOW.0;
        if attr & !valid != 0 {
            return Err(Errno::EINVAL.into());
        }
        let mut mount_flags = super::mount::attr_flags(attr);
        match attr & MountAttrFlags::MOUNT_ATTR__ATIME.0 {
            a if a == MountAttrFlags::MOUNT_ATTR_STRICTATIME.0 => mount_flags.strictatime = true,
            a if a == MountAttrFlags::MOUNT_ATTR_NOATIME.0 => mount_flags.noatime = true,
            a if a == MountAttrFlags::MOUNT_ATTR_RELATIME.0 => {}
            _ => return Err(Errno::EINVAL.into()),
        }
        let file = self.file(caller.task(), fs_fd)?;
        let context = file.downcast::<FsContextFile>().ok_or(Errno::EINVAL)?;
        let mounted = {
            let mut context = context.0.lock_or_recover();
            let fs = context.fs.clone().ok_or(Errno::EINVAL)?;
            if context.phase != Phase::AwaitingMount {
                return Err(Errno::EBUSY.into());
            }
            let source = context.source.clone().unwrap_or_else(|| b"none".to_vec());
            let read_only = context.read_only.unwrap_or(false);
            let mounted = self.vfs.create_mount(fs, source, mount_flags, read_only);
            context.picked = Some(mounted.path.clone());
            context.clean();
            mounted
        };
        let cloexec = flags & FsmountFlags::FSMOUNT_CLOEXEC.0 != 0;
        let file = OpenFile::new(Backing::Inode(mounted), OpenFlags::O_PATH);
        let [fd] = self.install_files(caller, [file], cloexec)?;
        Ok(fd.0)
    }

    /// An `O_PATH` file at `path` (`dentry_open(path, O_PATH)`).
    pub(crate) async fn open_path(&self, env: &super::Env, path: &Path) -> SysResult<OpenFile> {
        let how = OpenHow {
            flags: OpenFlags::O_PATH,
            mode: 0,
            lookup: LookupFlags {
                empty: true,
                ..LookupFlags::default()
            },
        };
        let file = self.vfs.open(&env.cx(), path, b"", &how).await?;
        Ok(OpenFile::new(Backing::Inode(file), OpenFlags::O_PATH))
    }
}

/// A string argument of at most `max` bytes: `EFAULT` if unreadable, `EINVAL` if longer.
async fn copy_string(caller: &Caller<'_>, ptr: Cstr, max: usize) -> Result<Vec<u8>, Errno> {
    if ptr.is_null() {
        return Err(Errno::EFAULT);
    }
    match read_cstr(caller.memory(), ptr, max).await {
        Ok(string) => Ok(string),
        Err(CstrError::Fault) => Err(Errno::EFAULT),
        Err(CstrError::TooLong) => Err(Errno::EINVAL),
    }
}
