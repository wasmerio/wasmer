use std::sync::Arc;

use vfs_core::flags::{OpenFlags, OpenOptions, ResolveFlags};
use vfs_core::node::{CreateFile, FsNode, MkdirOptions, UnlinkOptions};
use vfs_core::path_types::{VfsName, VfsPath};
use vfs_core::provider::{FsProvider, MountFlags, config_downcast_ref};
use vfs_core::{Fs, FsProviderRegistry, VfsError, VfsErrorKind, VfsResult};

use vfs_mem::{MemFs, MemFsConfig, MemFsProvider};
use vfs_overlay::{FsSpec, OverlayConfig, OverlayOptions, OverlayProvider};

fn ensure_dir(root: Arc<dyn FsNode>, path: &str) -> VfsResult<Arc<dyn FsNode>> {
    let mut cur = root;
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        let name = VfsName::new(seg.as_bytes())?;
        match cur.lookup(&name) {
            Ok(node) => {
                cur = node;
            }
            Err(err) if err.kind() == VfsErrorKind::NotFound => {
                cur = cur.mkdir(&name, MkdirOptions { mode: None })?;
            }
            Err(err) => return Err(err),
        }
    }
    Ok(cur)
}

fn create_file_with_contents(root: Arc<dyn FsNode>, path: &str, data: &[u8]) -> VfsResult<()> {
    let (parent_path, name) = match path.rsplit_once('/') {
        Some((parent, name)) => (parent, name),
        None => ("", path),
    };
    let parent = ensure_dir(root, parent_path)?;
    let name = VfsName::new(name.as_bytes())?;
    let node = parent.create_file(
        &name,
        CreateFile {
            mode: None,
            truncate: true,
            exclusive: false,
        },
    )?;
    let handle = node.open(OpenOptions {
        flags: OpenFlags::WRITE | OpenFlags::TRUNC,
        mode: None,
        resolve: ResolveFlags::empty(),
    })?;
    handle.write_at(0, data)?;
    Ok(())
}

fn read_file(root: Arc<dyn FsNode>, path: &str) -> VfsResult<Vec<u8>> {
    let mut cur = root;
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        let name = VfsName::new(seg.as_bytes())?;
        cur = cur.lookup(&name)?;
    }
    let meta = cur.metadata()?;
    let handle = cur.open(OpenOptions {
        flags: OpenFlags::READ,
        mode: None,
        resolve: ResolveFlags::empty(),
    })?;
    let mut buf = vec![0u8; meta.size as usize];
    let read = handle.read_at(0, &mut buf)?;
    buf.truncate(read);
    Ok(buf)
}

#[derive(Debug)]
struct SeededMemConfig {
    entries: Vec<(String, Vec<u8>)>,
}

#[derive(Debug, Clone, Copy)]
struct SeededMemProvider;

impl FsProvider for SeededMemProvider {
    fn name(&self) -> &'static str {
        "seeded"
    }

    fn capabilities(&self) -> vfs_core::provider::FsProviderCapabilities {
        vfs_core::provider::FsProviderCapabilities::empty()
    }

    fn validate_config(&self, config: &dyn vfs_core::provider::ProviderConfig) -> VfsResult<()> {
        config_downcast_ref::<SeededMemConfig>(config)
            .map(|_| ())
            .ok_or(VfsError::new(VfsErrorKind::InvalidInput, "seeded.config"))
    }

    fn mount(&self, req: vfs_core::provider::MountRequest<'_>) -> VfsResult<Arc<dyn vfs_core::Fs>> {
        let config = config_downcast_ref::<SeededMemConfig>(req.config).ok_or(VfsError::new(
            VfsErrorKind::InvalidInput,
            "seeded.mount.config",
        ))?;
        let fs = MemFs::new_with(MemFsConfig::default(), req.flags);
        let root = fs.root();
        for (path, data) in &config.entries {
            create_file_with_contents(root.clone(), path, data)?;
        }
        Ok(Arc::new(fs))
    }
}

#[derive(Debug, Clone, Copy)]
struct WritableMemProvider;

impl FsProvider for WritableMemProvider {
    fn name(&self) -> &'static str {
        "writable_mem"
    }

    fn capabilities(&self) -> vfs_core::provider::FsProviderCapabilities {
        vfs_core::provider::FsProviderCapabilities::empty()
    }

    fn validate_config(&self, config: &dyn vfs_core::provider::ProviderConfig) -> VfsResult<()> {
        config_downcast_ref::<MemFsConfig>(config)
            .map(|_| ())
            .ok_or(VfsError::new(
                VfsErrorKind::InvalidInput,
                "writable_mem.config",
            ))
    }

    fn mount(&self, req: vfs_core::provider::MountRequest<'_>) -> VfsResult<Arc<dyn vfs_core::Fs>> {
        let config = config_downcast_ref::<MemFsConfig>(req.config).ok_or(VfsError::new(
            VfsErrorKind::InvalidInput,
            "writable_mem.mount.config",
        ))?;
        let fs = MemFs::new_with(config.clone(), MountFlags::empty());
        Ok(Arc::new(fs))
    }
}

#[test]
fn registry_mount_exposes_overlay_behavior() {
    let registry = Arc::new(FsProviderRegistry::new());
    registry
        .register(Arc::new(MemFsProvider))
        .expect("register mem");
    registry
        .register(Arc::new(SeededMemProvider))
        .expect("register seeded");
    registry
        .register(Arc::new(OverlayProvider::new(registry.clone())))
        .expect("register overlay");

    let config = OverlayConfig {
        upper: FsSpec {
            provider: "mem".to_string(),
            config: Box::new(MemFsConfig::default()),
        },
        lowers: vec![FsSpec {
            provider: "seeded".to_string(),
            config: Box::new(SeededMemConfig {
                entries: vec![
                    ("/lower-only".to_string(), b"lower".to_vec()),
                    ("/shadow".to_string(), b"lower".to_vec()),
                ],
            }),
        }],
        options: OverlayOptions::default(),
    };

    let overlay = registry
        .mount_with_provider("overlay", &config, VfsPath::new(b"/"), MountFlags::empty())
        .expect("mount overlay");
    let root = overlay.root();

    let data = read_file(root.clone(), "/lower-only").unwrap();
    assert_eq!(data, b"lower");

    create_file_with_contents(root.clone(), "/shadow", b"upper").unwrap();
    let data = read_file(root, "/shadow").unwrap();
    assert_eq!(data, b"upper");
}

#[test]
fn read_only_mount_blocks_mutations() {
    let registry = Arc::new(FsProviderRegistry::new());
    registry
        .register(Arc::new(WritableMemProvider))
        .expect("register writable_mem");
    registry
        .register(Arc::new(OverlayProvider::new(registry.clone())))
        .expect("register overlay");

    let config = OverlayConfig {
        upper: FsSpec {
            provider: "writable_mem".to_string(),
            config: Box::new(MemFsConfig::default()),
        },
        lowers: vec![FsSpec {
            provider: "writable_mem".to_string(),
            config: Box::new(MemFsConfig::default()),
        }],
        options: OverlayOptions::default(),
    };

    let overlay = registry
        .mount_with_provider(
            "overlay",
            &config,
            VfsPath::new(b"/"),
            MountFlags::READ_ONLY,
        )
        .expect("mount overlay read-only");
    let root = overlay.root();

    let err = root
        .create_file(
            &VfsName::new(b"file").unwrap(),
            CreateFile {
                mode: None,
                truncate: true,
                exclusive: false,
            },
        )
        .err()
        .expect("expected error");
    assert_eq!(err.kind(), VfsErrorKind::ReadOnlyFs);

    let err = root
        .mkdir(&VfsName::new(b"dir").unwrap(), MkdirOptions { mode: None })
        .err()
        .expect("expected error");
    assert_eq!(err.kind(), VfsErrorKind::ReadOnlyFs);

    let err = root
        .unlink(
            &VfsName::new(b"missing").unwrap(),
            UnlinkOptions { must_be_dir: false },
        )
        .err()
        .expect("expected error");
    assert_eq!(err.kind(), VfsErrorKind::ReadOnlyFs);
}
