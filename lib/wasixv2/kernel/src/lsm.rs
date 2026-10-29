//! The Linux security modules this kernel has, as `lsm_list_modules`, `lsm_get_self_attr`,
//! `lsm_set_self_attr` (`security/lsm_syscalls.c`, `security/security.c`) and securityfs's
//! `lsm` file (`security/inode.c`) show them: the capabilities, Landlock and Yama. None keeps
//! process attributes, so the attribute calls find no module to answer.

use std::{
    any::Any,
    sync::{Arc, OnceLock},
};

use async_trait::async_trait;
use wasixv2_abi::{Errno, GuestLayout, GuestPtr, GuestPtrMut, LsmAttr, LsmCtx, LsmFlags, LsmId};
use wasixv2_syscalls::{Caller, SysResult, read_value, write_value};
use wasixv2_vfs::{
    Attr, Cred, DirEntry, DirSink, FileType, Filesystem, FsInfo, FsResult, Handle, NAME_MAX, Node,
    OpenOptions, SetAttr, StatFs, Timestamp,
};

use crate::{Kernel, fs::mount::FilesystemType, fs::mount::FsParam};

/// The active modules, in the order they stack (`lsm_idlist`, `lsm_names`): the capabilities
/// first (`LSM_ORDER_FIRST`), then as `CONFIG_LSM`'s default orders them.
const MODULES: &[(LsmId, &str)] = &[
    (LsmId::LSM_ID_CAPABILITY, "capability"),
    (LsmId::LSM_ID_LANDLOCK, "landlock"),
    (LsmId::LSM_ID_YAMA, "yama"),
];

/// `SECURITYFS_MAGIC`.
const SECURITYFS_MAGIC: u32 = 0x7363_6673;

impl Kernel {
    /// The securityfs instance, to mount at `/sys/kernel/security`.
    pub fn securityfs(&self) -> Arc<dyn Filesystem> {
        self.filesystems
            .mount_option_less(b"securityfs")
            .expect("securityfs is one of the kernel's types")
    }

    /// `lsm_list_modules`: the ids of the active modules, if `size` says there is room for
    /// them (`E2BIG` otherwise, with the room needed in `size`).
    pub(crate) async fn sys_lsm_list_modules(
        &self,
        caller: &Caller<'_>,
        ids: u32,
        size: GuestPtrMut<u32>,
        flags: u32,
    ) -> SysResult<i32> {
        if flags != 0 {
            return Err(Errno::EINVAL.into());
        }
        let memory = caller.memory();
        let room: u32 = read_value(memory, GuestPtr::new(size.addr())).await?;
        let total = (MODULES.len() * 8) as u32;
        write_value(memory, size, &total).await?;
        if room < total {
            return Err(Errno::E2BIG.into());
        }
        for (i, (id, _)) in MODULES.iter().enumerate() {
            let at = GuestPtrMut::<u64>::new(ids + 8 * i as u32);
            write_value(memory, at, &id.0).await?;
        }
        Ok(MODULES.len() as i32)
    }

    /// `security_getselfattr`: no module keeps process attributes, so after the checks the
    /// size is 0 and the answer `EOPNOTSUPP`.
    pub(crate) async fn sys_lsm_get_self_attr(
        &self,
        caller: &Caller<'_>,
        attr: u32,
        ctx: u32,
        size: GuestPtrMut<u32>,
        flags: u32,
    ) -> SysResult<i32> {
        if attr == LsmAttr::LSM_ATTR_UNDEF.0 {
            return Err(Errno::EINVAL.into());
        }
        if flags & !LsmFlags::LSM_FLAG_SINGLE.0 != 0 {
            return Err(Errno::EINVAL.into());
        }
        let memory = caller.memory();
        let _room: u32 = read_value(memory, GuestPtr::new(size.addr())).await?;
        if flags != 0 {
            // The module asked for, in `ctx->id`.
            let _: u64 = read_value(memory, GuestPtr::new(ctx)).await?;
        }
        write_value(memory, size, &0u32).await?;
        Err(Errno::EOPNOTSUPP.into())
    }

    /// `security_setselfattr`: the context must be well formed, and no module takes one.
    pub(crate) async fn sys_lsm_set_self_attr(
        &self,
        caller: &Caller<'_>,
        _attr: u32,
        ctx: u32,
        size: u32,
        flags: u32,
    ) -> SysResult<i32> {
        if flags != 0 || (size as usize) < LsmCtx::SIZE {
            return Err(Errno::EINVAL.into());
        }
        if size > 4096 {
            return Err(Errno::E2BIG.into());
        }
        let mut raw = vec![0; size as usize];
        caller.memory().read(ctx, &mut raw).await?;
        let header = LsmCtx::read_from(&raw).expect("checked size");
        let fixed = LsmCtx::SIZE as u64;
        if u64::from(size) < header.len
            || u64::from(size) < header.ctx_len + fixed
            || header.len < header.ctx_len + fixed
        {
            return Err(Errno::EINVAL.into());
        }
        Err(Errno::EOPNOTSUPP.into())
    }
}

/// The `securityfs` type: one instance (`get_tree_single`), mounted at `/sys/kernel/security`.
#[derive(Default)]
pub(crate) struct SecurityFsType {
    instance: OnceLock<Arc<dyn Filesystem>>,
}

impl FilesystemType for SecurityFsType {
    fn mount(
        &self,
        _source: &[u8],
        params: &[FsParam<'_>],
        _cred: &Cred,
    ) -> Result<Arc<dyn Filesystem>, Errno> {
        if !params.is_empty() {
            return Err(Errno::EINVAL);
        }
        let fs = self.instance.get_or_init(|| Arc::new(SecurityFs));
        Ok(fs.clone())
    }
}

/// securityfs: a root directory and the `lsm` file.
#[derive(Default)]
struct SecurityFs;

#[async_trait(?Send)]
impl Filesystem for SecurityFs {
    fn info(&self) -> FsInfo {
        FsInfo {
            name: "securityfs",
            magic: SECURITYFS_MAGIC,
            name_max: NAME_MAX,
            read_only: false,
            dev: None,
            has_btime: false,
            stable: true,
        }
    }

    fn root(&self) -> Arc<dyn Node> {
        Arc::new(SecNode::Root)
    }

    async fn statfs(&self) -> FsResult<StatFs> {
        Ok(StatFs {
            block_size: 4096,
            name_max: NAME_MAX as u32,
            fragment_size: 4096,
            ..StatFs::default()
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SecNode {
    Root,
    /// `lsm`: the active modules, comma-separated (`lsm_read`).
    Lsm,
}

/// What `lsm` holds: the module names, with no newline.
fn lsm_names() -> String {
    let names: Vec<&str> = MODULES.iter().map(|(_, name)| *name).collect();
    names.join(",")
}

#[async_trait(?Send)]
impl Node for SecNode {
    fn ino(&self) -> u64 {
        match self {
            Self::Root => 1,
            Self::Lsm => 2,
        }
    }

    fn file_type(&self) -> FileType {
        match self {
            Self::Root => FileType::Directory,
            Self::Lsm => FileType::Regular,
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn getattr(&self) -> FsResult<Attr> {
        let (mode, nlink) = match self {
            Self::Root => (0o755, 2),
            Self::Lsm => (0o444, 1),
        };
        Ok(Attr {
            ino: self.ino(),
            file_type: self.file_type(),
            mode,
            nlink,
            uid: 0,
            gid: 0,
            rdev: Default::default(),
            size: 0,
            blocks: 0,
            blksize: 4096,
            atime: Timestamp::default(),
            mtime: Timestamp::default(),
            ctime: Timestamp::default(),
            btime: None,
            dev: None,
        })
    }

    async fn setattr(&self, _: &SetAttr) -> FsResult<Attr> {
        Err(Errno::EPERM)
    }

    async fn lookup(&self, name: &[u8]) -> FsResult<Arc<dyn Node>> {
        match (self, name) {
            (Self::Root, b"lsm") => Ok(Arc::new(Self::Lsm)),
            (Self::Root, _) => Err(Errno::ENOENT),
            _ => Err(Errno::ENOTDIR),
        }
    }

    async fn open(&self, options: OpenOptions) -> FsResult<Box<dyn Handle>> {
        match self {
            Self::Root => Ok(Box::new(RootHandle)),
            Self::Lsm if options.write => Err(Errno::EACCES),
            Self::Lsm => Ok(Box::new(LsmHandle(lsm_names()))),
        }
    }
}

struct RootHandle;

#[async_trait(?Send)]
impl Handle for RootHandle {
    async fn readdir(&self, cookie: u64, sink: &mut dyn DirSink) -> FsResult<()> {
        if cookie == 0 {
            let entry = DirEntry {
                name: b"lsm",
                ino: 2,
                file_type: FileType::Regular,
                next: 1,
            };
            sink.push(entry);
        }
        Ok(())
    }
}

struct LsmHandle(String);

#[async_trait(?Send)]
impl Handle for LsmHandle {
    async fn read_at(&self, buf: &mut [u8], offset: u64) -> FsResult<usize> {
        let data = self.0.as_bytes();
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(data.len());
        let n = buf.len().min(data.len() - start);
        buf[..n].copy_from_slice(&data[start..start + n]);
        Ok(n)
    }
}
