
use std::path::PathBuf;
use std::sync::Arc;

use vfs_core::context::{VfsConfig, VfsContext, VfsCred};
use vfs_core::mount::MountTable;
use vfs_core::node::MkdirOptions;
use vfs_core::path_types::{VfsPath, VfsPathBuf};
use vfs_core::path_walker::{PathWalkerAsync, ResolutionRequestAsync, WalkFlags};
use vfs_core::provider::{FsProviderRegistry, MountFlags, MountRequest};
use vfs_core::{VfsErrorKind, VfsResult};

use vfs_host::config::HostFsConfig;
use vfs_host::provider::HostFsProvider;
use vfs_mem::config::MemFsConfig;
use vfs_mem::provider::MemFsProvider;
use vfs_overlay::config::{FsSpec, OverlayConfig, OverlayOptions};
use vfs_overlay::provider::OverlayProvider;
use vfs_webc::{WebcFsConfig, WebcFsProvider, WebcVolumeMapping};

use crate::fs::vfs::{WasiFs, default_ctx, make_root_handles};
use crate::state::builder::PreopenedDir;

#[derive(Debug, Clone)]
pub struct HostMount {
    pub guest: String,
    pub host: PathBuf,
}

pub fn build_default_fs(
    preopens: &[PreopenedDir],
    vfs_preopens: &[String],
    host_mounts: &[HostMount],
    webc_layers: &[WebcVolumeMapping],
) -> Result<WasiFs, String> {
    let registry = Arc::new(FsProviderRegistry::new());
    registry
        .register_sync(Arc::new(MemFsProvider))
        .map_err(|e| e.to_string())?;
    registry
        .register_sync(Arc::new(HostFsProvider))
        .map_err(|e| e.to_string())?;
    registry
        .register_sync(Arc::new(OverlayProvider::new(registry.clone())))
        .map_err(|e| e.to_string())?;
    registry
        .register_sync(Arc::new(WebcFsProvider))
        .map_err(|e| e.to_string())?;

    let upper = FsSpec {
        provider: "mem".to_string(),
        config: Box::new(MemFsConfig::default()),
    };
    let lowers = webc_layers
        .iter()
        .map(|layer| {
            let mut config = WebcFsConfig::new(layer.volume.clone());
            if let Some(root) = layer.root.clone() {
                config = config.with_root(root);
            }
            FsSpec {
                provider: "webc".to_string(),
                config: Box::new(config),
            }
        })
        .collect::<Vec<_>>();

    let root_fs = if lowers.is_empty() {
        registry
            .create_fs_with_provider(
                "mem",
                &MemFsConfig::default(),
                VfsPath::new(b"/"),
                MountFlags::empty(),
            )
            .map_err(|e| e.to_string())?
    } else {
        registry
            .create_fs_with_provider(
                "overlay",
                &OverlayConfig {
                    upper,
                    lowers,
                    options: OverlayOptions::default(),
                },
                VfsPath::new(b"/"),
                MountFlags::empty(),
            )
            .map_err(|e| e.to_string())?
    };

    let runtime = crate::fs::vfs::default_runtime();
    let root_async = vfs_core::provider::AsyncFsFromSync::new(root_fs.clone(), runtime);
    let mounts =
        Arc::new(MountTable::new(root_fs, Arc::new(root_async)).map_err(|e| e.to_string())?);

    let (cwd, cwd_async) =
        make_root_handles(&mounts).map_err(|e| format!("root handles: {e:?}"))?;
    let ctx = VfsContext::new(
        VfsCred::root(),
        cwd,
        Arc::new(VfsConfig::default()),
        Arc::new(vfs_core::policy::AllowAllPolicy),
    )
    .with_async_cwd(cwd_async);

    let wasi_fs = WasiFs::new(registry, mounts.clone(), ctx);
    let walker = PathWalkerAsync::new(mounts.clone());

    let create_mountpoint = |path: &VfsPathBuf| async {
        let mut flags = WalkFlags::new(&wasi_fs.ctx.read().unwrap());
        flags.allow_empty_path = true;
        let req = ResolutionRequestAsync {
            ctx: &wasi_fs.ctx.read().unwrap(),
            base: vfs_core::VfsBaseDirAsync::Cwd,
            path: path.as_path(),
            flags,
        };
        match walker.resolve(req).await {
            Ok(_) => Ok(()),
            Err(err) if err.kind() == VfsErrorKind::NotFound => {
                let parent = walker
                    .resolve_parent(ResolutionRequestAsync {
                        ctx: &wasi_fs.ctx.read().unwrap(),
                        base: vfs_core::VfsBaseDirAsync::Cwd,
                        path: path.as_path(),
                        flags,
                    })
                    .await?;
                parent
                    .dir
                    .node
                    .mkdir(&parent.name, MkdirOptions::default())
                    .await?;
                Ok(())
            }
            Err(err) => Err(err),
        }
    };

    for mount in host_mounts {
        let guest = if mount.guest.starts_with('/') {
            mount.guest.clone()
        } else {
            format!("/{}", mount.guest.trim_start_matches('/'))
        };
        let guest_path = VfsPathBuf::from_bytes(guest.as_bytes().to_vec());
        futures::executor::block_on(create_mountpoint(&guest_path)).map_err(|e| e.to_string())?;

        let fs = registry
            .create_fs_with_provider(
                "host",
                &HostFsConfig {
                    root: mount.host.clone(),
                    strict: false,
                },
                guest_path.as_path(),
                MountFlags::empty(),
            )
            .map_err(|e| e.to_string())?;
        let fs_async = vfs_core::provider::AsyncFsFromSync::new(fs.clone(), runtime.clone());
        mounts
            .mount(
                vfs_core::MountId::from_index(0),
                vfs_core::inode::make_vfs_inode(
                    vfs_core::MountId::from_index(0),
                    fs.root().inode(),
                ),
                fs,
                Arc::new(fs_async),
                fs.root().inode(),
                MountFlags::empty(),
            )
            .map_err(|e| e.to_string())?;
    }

    let _ = preopens;
    let _ = vfs_preopens;

    Ok(wasi_fs)
}
