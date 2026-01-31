use std::collections::HashSet;
use std::sync::{Arc, Mutex, RwLock};

use vfs_core::context::{VfsConfig, VfsContext, VfsCred};
use vfs_core::mount::MountTable;
use vfs_core::path_types::{VfsPath, VfsPathBuf};
use vfs_core::path_walker::{PathWalkerAsync, ResolutionRequestAsync, WalkFlags};
use vfs_core::provider::FsProviderRegistry;
use vfs_core::provider::MountFlags;
use vfs_core::{Vfs, VfsDirHandle, VfsDirHandleAsync};
use vfs_rt::TokioRuntime;
use wasmer_wasix_types::wasi::{Errno, Fd as WasiFd, Fdflags, Fdflagsext, Rights};

use crate::ALL_RIGHTS;
use crate::bin_factory::BinaryPackage;
use vfs_core::{VfsBaseDirAsync, VfsError, VfsErrorKind};
use vfs_webc::WebcFsConfig;

use super::fd_table::{FdEntry, FdInner, FdTable, Kind};

#[derive(Debug)]
pub struct WasiFs {
    pub registry: Arc<FsProviderRegistry>,
    pub mounts: Arc<MountTable>,
    pub vfs: Vfs,
    pub ctx: RwLock<VfsContext>,
    pub fd_table: RwLock<FdTable>,
    pub preopen_fds: RwLock<Vec<WasiFd>>,
    pub cwd_path: RwLock<VfsPathBuf>,
    pub has_unioned: Mutex<HashSet<wasmer_config::package::PackageId>>,
}

impl WasiFs {
    pub fn new(
        registry: Arc<FsProviderRegistry>,
        mounts: Arc<MountTable>,
        ctx: VfsContext,
    ) -> Self {
        Self {
            registry,
            mounts,
            vfs: Vfs::new(mounts.clone()),
            ctx: RwLock::new(ctx),
            fd_table: RwLock::new(FdTable::new()),
            preopen_fds: RwLock::new(Vec::new()),
            cwd_path: RwLock::new(VfsPathBuf::from_bytes(b"/".to_vec())),
            has_unioned: Mutex::new(HashSet::new()),
        }
    }

    pub fn fork(&self) -> Self {
        let ctx = self.ctx.read().unwrap().clone();
        Self {
            registry: self.registry.clone(),
            mounts: self.mounts.clone(),
            vfs: self.vfs.clone(),
            ctx: RwLock::new(ctx),
            fd_table: RwLock::new(self.fd_table.read().unwrap().clone()),
            preopen_fds: RwLock::new(self.preopen_fds.read().unwrap().clone()),
            cwd_path: RwLock::new(self.cwd_path.read().unwrap().clone()),
            has_unioned: Mutex::new(self.has_unioned.lock().unwrap().clone()),
        }
    }

    pub fn set_current_dir(&self, path: &str) {
        let mut guard = self.cwd_path.write().unwrap();
        *guard = VfsPathBuf::from_bytes(path.as_bytes().to_vec());
    }

    pub fn current_dir(&self) -> VfsPathBuf {
        self.cwd_path.read().unwrap().clone()
    }

    pub fn get_fd(&self, fd: WasiFd) -> Result<FdEntry, Errno> {
        self.fd_table
            .read()
            .unwrap()
            .get(fd)
            .cloned()
            .ok_or(Errno::Badf)
    }

    pub fn get_fd_mut(&self, fd: WasiFd) -> Result<FdInner, Errno> {
        self.fd_table
            .read()
            .unwrap()
            .get(fd)
            .map(|entry| entry.inner.clone())
            .ok_or(Errno::Badf)
    }

    pub fn with_fd(
        &self,
        rights: Rights,
        rights_inheriting: Rights,
        flags: Fdflags,
        fd_flags: Fdflagsext,
        kind: Kind,
        idx: WasiFd,
    ) -> Result<(), Errno> {
        let entry = FdEntry {
            inner: FdInner {
                rights,
                rights_inheriting,
                flags,
                fd_flags,
            },
            kind,
            is_stdio: matches!(idx, 0 | 1 | 2),
        };
        let mut guard = self.fd_table.write().unwrap();
        if guard.insert(true, idx, entry) {
            Ok(())
        } else {
            Err(Errno::Exist)
        }
    }

    pub fn create_fd(
        &self,
        rights: Rights,
        rights_inheriting: Rights,
        flags: Fdflags,
        fd_flags: Fdflagsext,
        kind: Kind,
    ) -> Result<WasiFd, Errno> {
        let entry = FdEntry {
            inner: FdInner {
                rights,
                rights_inheriting,
                flags,
                fd_flags,
            },
            kind,
            is_stdio: false,
        };
        Ok(self.fd_table.write().unwrap().insert_first_free(entry))
    }

    pub fn replace_fd_kind(&self, fd: WasiFd, kind: Kind) -> Result<(), Errno> {
        let mut guard = self.fd_table.write().unwrap();
        let entry = guard.get_entry_mut(fd).ok_or(Errno::Badf)?;
        entry.kind = kind;
        Ok(())
    }

    pub fn clone_fd(
        &self,
        fd: WasiFd,
        min_result_fd: WasiFd,
        cloexec: Option<bool>,
    ) -> Result<WasiFd, Errno> {
        let fd_entry = self.get_fd(fd)?;
        let mut inner = fd_entry.inner.clone();
        if let Some(cloexec) = cloexec {
            inner.fd_flags.set(Fdflagsext::CLOEXEC, cloexec);
        }
        let entry = FdEntry {
            inner,
            kind: fd_entry.kind.clone(),
            is_stdio: fd_entry.is_stdio,
        };
        Ok(self
            .fd_table
            .write()
            .unwrap()
            .insert_first_free_after(entry, min_result_fd))
    }

    pub fn close_fd(&self, fd: WasiFd) -> Result<(), Errno> {
        let mut guard = self.fd_table.write().unwrap();
        guard.remove(fd).ok_or(Errno::Badf)?;
        Ok(())
    }

    pub async fn close_cloexec_fds(&self) {
        let to_close: Vec<_> = self
            .fd_table
            .read()
            .unwrap()
            .iter()
            .filter_map(|(fd, entry)| {
                if entry.inner.fd_flags.contains(Fdflagsext::CLOEXEC) && !entry.is_stdio {
                    Some(fd)
                } else {
                    None
                }
            })
            .collect();

        for fd in to_close {
            let _ = self.close_fd(fd);
        }
    }

    pub async fn close_all(&self) {
        let fds: Vec<_> = self.fd_table.read().unwrap().keys().collect();
        for fd in fds {
            let _ = self.close_fd(fd);
        }
    }

    pub async fn conditional_union(&self, pkg: &BinaryPackage) -> Result<(), VfsError> {
        if pkg.webc_volumes.is_empty() {
            return Ok(());
        }

        let mut guard = self.has_unioned.lock().unwrap();
        if guard.contains(&pkg.id) {
            return Ok(());
        }
        guard.insert(pkg.id.clone());
        drop(guard);

        for mapping in &pkg.webc_volumes {
            self.mount_webc_layer(mapping).await?;
        }

        Ok(())
    }

    async fn mount_webc_layer(
        &self,
        mapping: &vfs_webc::WebcVolumeMapping,
    ) -> Result<(), VfsError> {
        let walker = PathWalkerAsync::new(self.mounts.clone());
        let mut flags = WalkFlags::new(&self.ctx.read().unwrap());
        flags.allow_empty_path = true;
        let path = mapping.mount_path.as_path();
        let ctx_guard = self.ctx.read().unwrap();
        let req = ResolutionRequestAsync {
            ctx: &ctx_guard,
            base: VfsBaseDirAsync::Cwd,
            path,
            flags,
        };

        let resolved = match walker.resolve(req).await {
            Ok(resolved) => resolved,
            Err(err) if err.kind() == VfsErrorKind::NotFound => {
                let parent = walker
                    .resolve_parent(ResolutionRequestAsync {
                        ctx: &ctx_guard,
                        base: VfsBaseDirAsync::Cwd,
                        path,
                        flags,
                    })
                    .await?;
                parent
                    .dir
                    .node
                    .mkdir(&parent.name, vfs_core::node::MkdirOptions::default())
                    .await?;
                walker
                    .resolve(ResolutionRequestAsync {
                        ctx: &ctx_guard,
                        base: VfsBaseDirAsync::Cwd,
                        path,
                        flags,
                    })
                    .await?
            }
            Err(err) => return Err(err),
        };

        if resolved.node.file_type() != vfs_core::VfsFileType::Directory {
            return Err(VfsError::new(VfsErrorKind::NotDir, "webc.mount.not_dir"));
        }

        let mut config = WebcFsConfig::new(mapping.volume.clone());
        if let Some(root) = mapping.root.clone() {
            config = config.with_root(root);
        }
        let fs_sync = self.registry.create_fs_with_provider(
            "webc",
            &config,
            VfsPath::new(path.as_bytes()),
            MountFlags::READ_ONLY,
        )?;
        let runtime = default_runtime();
        let fs_async = vfs_core::provider::AsyncFsFromSync::new(fs_sync.clone(), runtime);
        self.mounts.mount(
            resolved.mount,
            resolved.inode,
            fs_sync.clone(),
            Arc::new(fs_async),
            fs_sync.root().inode(),
            MountFlags::READ_ONLY,
        )?;

        Ok(())
    }
}

pub fn default_ctx(cwd: VfsDirHandle, cwd_async: VfsDirHandleAsync) -> VfsContext {
    VfsContext::new(
        VfsCred::root(),
        cwd,
        Arc::new(VfsConfig::default()),
        Arc::new(vfs_core::policy::AllowAllPolicy),
    )
    .with_async_cwd(cwd_async)
}

pub fn default_runtime() -> Arc<dyn vfs_core::provider::VfsRuntime> {
    Arc::new(TokioRuntime::new(tokio::runtime::Handle::current()))
}

pub fn make_root_handles(
    mounts: &Arc<MountTable>,
) -> Result<(VfsDirHandle, VfsDirHandleAsync), Errno> {
    let root = mounts
        .snapshot()
        .mounts
        .get(0)
        .and_then(|m| m.as_ref())
        .ok_or(Errno::Io)?;
    let sync_root = root.fs_sync.root();
    let async_root = futures::executor::block_on(root.fs_async.root()).map_err(|_| Errno::Io)?;
    let sync_handle = VfsDirHandle::new(
        vfs_core::VfsHandleId(1),
        mounts.guard(root.id).map_err(|_| Errno::Io)?,
        root.root_inode,
        sync_root,
        None,
    );
    let async_handle = VfsDirHandleAsync::new(
        vfs_core::VfsHandleId(2),
        mounts.guard(root.id).map_err(|_| Errno::Io)?,
        root.root_inode,
        async_root,
        None,
    );
    Ok((sync_handle, async_handle))
}
