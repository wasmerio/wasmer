use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Error};
use tokio::runtime::Handle;
use virtual_fs::{
    ArcFileSystem, ExactMountConflictMode, FileSystem, MountFileSystem, OverlayFileSystem,
    RootFileSystemBuilder,
};
use webc::metadata::annotations::Wasi as WasiAnnotation;

use crate::{
    WasiEnvBuilder,
    bin_factory::{BinaryPackage, BinaryPackageMounts},
    capabilities::Capabilities,
    fs::WasiFsRoot,
    journal::{DynJournal, DynReadableJournal, SnapshotTrigger},
};

pub const MAPPED_CURRENT_DIR_DEFAULT_PATH: &str = "/home";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExistingMountConflictBehavior {
    Fail,
    #[default]
    Override,
}

#[derive(Debug, Clone)]
pub struct MappedCommand {
    /// The new alias.
    pub alias: String,
    /// The original command.
    pub target: String,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct CommonWasiOptions {
    pub(crate) entry_function: Option<String>,
    pub(crate) args: Vec<String>,
    pub(crate) env: HashMap<String, String>,
    pub(crate) forward_host_env: bool,
    pub(crate) mapped_host_commands: Vec<MappedCommand>,
    pub(crate) mounts: Vec<MountedDirectory>,
    pub(crate) is_home_mapped: bool,
    pub(crate) injected_packages: Vec<BinaryPackage>,
    pub(crate) capabilities: Capabilities,
    pub(crate) read_only_journals: Vec<Arc<DynReadableJournal>>,
    pub(crate) writable_journals: Vec<Arc<DynJournal>>,
    pub(crate) snapshot_on: Vec<SnapshotTrigger>,
    pub(crate) snapshot_interval: Option<std::time::Duration>,
    pub(crate) stop_running_after_snapshot: bool,
    pub(crate) skip_stdio_during_bootstrap: bool,
    pub(crate) current_dir: Option<PathBuf>,
    pub(crate) existing_mount_conflict_behavior: ExistingMountConflictBehavior,
}

impl CommonWasiOptions {
    pub(crate) fn prepare_webc_env(
        &self,
        builder: &mut WasiEnvBuilder,
        container_mounts: Option<&BinaryPackageMounts>,
        wasi: &WasiAnnotation,
        root_fs: Option<WasiFsRoot>,
    ) -> Result<(), anyhow::Error> {
        if let Some(ref entry_function) = self.entry_function {
            builder.set_entry_function(entry_function);
        }

        let root_fs = root_fs.unwrap_or_else(|| {
            let mapped_dirs = self
                .mounts
                .iter()
                .map(|d| d.guest.as_str())
                .collect::<Vec<_>>();
            WasiFsRoot::from_filesystem(Arc::new(
                RootFileSystemBuilder::default().build_tmp_ext(&mapped_dirs),
            ))
        });
        let fs = prepare_filesystem(
            root_fs
                .root()
                .filesystem_at(Path::new("/"))
                .context("root fs is missing a / mount")?,
            &self.mounts,
            container_mounts,
            self.existing_mount_conflict_behavior,
        )?;

        // TODO: What's a preopen for '.' supposed to mean anyway? Why do we need it?
        if self.mounts.iter().all(|m| m.guest != ".") {
            // The user hasn't mounted "." to anything, so let's map it to "/"
            let path = builder.get_current_dir().unwrap_or(PathBuf::from("/"));
            builder.add_map_dir(".", path)?;
        }

        builder.add_preopen_dir("/")?;

        builder.set_fs_root(fs);

        for pkg in &self.injected_packages {
            builder.add_webc(pkg.clone());
        }

        let mapped_cmds = self
            .mapped_host_commands
            .iter()
            .map(|c| (c.alias.as_str(), c.target.as_str()));
        builder.add_mapped_commands(mapped_cmds);

        self.populate_env(wasi, builder);
        self.populate_args(wasi, builder);

        *builder.capabilities_mut() = self.capabilities.clone();

        #[cfg(feature = "journal")]
        {
            for journal in &self.read_only_journals {
                builder.add_read_only_journal(journal.clone());
            }
            for journal in &self.writable_journals {
                builder.add_writable_journal(journal.clone());
            }
            for trigger in &self.snapshot_on {
                builder.add_snapshot_trigger(*trigger);
            }
            if let Some(interval) = self.snapshot_interval {
                builder.with_snapshot_interval(interval);
            }
            builder.with_stop_running_after_snapshot(self.stop_running_after_snapshot);
        }

        Ok(())
    }

    fn populate_env(&self, wasi: &WasiAnnotation, builder: &mut WasiEnvBuilder) {
        for item in wasi.env.as_deref().unwrap_or_default() {
            // TODO(Michael-F-Bryan): Convert "wasi.env" in the webc crate from an
            // Option<Vec<String>> to a HashMap<String, String> so we avoid this
            // string.split() business
            match item.split_once('=') {
                Some((k, v)) => {
                    builder.add_env(k, v);
                }
                None => {
                    builder.add_env(item, String::new());
                }
            }
        }

        if self.forward_host_env {
            builder.add_envs(std::env::vars());
        }

        builder.add_envs(self.env.clone());
    }

    fn populate_args(&self, wasi: &WasiAnnotation, builder: &mut WasiEnvBuilder) {
        if let Some(main_args) = &wasi.main_args {
            builder.add_args(main_args);
        }

        builder.add_args(&self.args);
    }
}

// type ContainerFs =
//     OverlayFileSystem<TmpFileSystem, [RelativeOrAbsolutePathHack<Arc<dyn FileSystem>>; 1]>;

fn normalized_mount_path(guest_path: &str) -> Result<PathBuf, Error> {
    let mut guest_path = PathBuf::from(guest_path);

    if guest_path.is_relative() {
        guest_path = apply_relative_path_mounting_hack(&guest_path);
    }

    let mut normalized = PathBuf::from("/");
    for component in guest_path.components() {
        match component {
            Component::RootDir => normalized = PathBuf::from("/"),
            Component::CurDir => {}
            Component::ParentDir => {
                if normalized.as_os_str() == "/" {
                    anyhow::bail!(
                        "Invalid guest mount path \"{}\": parent traversal escapes the virtual root",
                        guest_path.display()
                    );
                }
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
            Component::Prefix(_) => {
                anyhow::bail!(
                    "Invalid guest mount path \"{}\": platform-specific prefixes are not supported",
                    guest_path.display()
                );
            }
        }
    }

    Ok(normalized)
}

fn prepare_filesystem(
    base_root: Arc<dyn FileSystem + Send + Sync>,
    mounted_dirs: &[MountedDirectory],
    container_mounts: Option<&BinaryPackageMounts>,
    conflict_behavior: ExistingMountConflictBehavior,
) -> Result<WasiFsRoot, Error> {
    let mut root_layers: Vec<Arc<dyn FileSystem + Send + Sync>> = Vec::new();
    let mount_fs = MountFileSystem::new();

    for MountedDirectory { guest, fs } in mounted_dirs {
        let guest_path = normalized_mount_path(guest)?;
        tracing::debug!(guest=%guest_path.display(), "Mounting");

        if guest_path == Path::new("/") {
            root_layers.push(fs.clone());
        } else {
            match conflict_behavior {
                ExistingMountConflictBehavior::Fail => mount_fs
                    .mount(&guest_path, Box::new(fs.clone()))
                    .with_context(|| format!("Unable to mount \"{}\"", guest_path.display()))?,
                ExistingMountConflictBehavior::Override => mount_fs
                    .set_mount(&guest_path, Box::new(fs.clone()))
                    .with_context(|| format!("Unable to mount \"{}\"", guest_path.display()))?,
            }
        }
    }

    let Some(container) = container_mounts else {
        let root_mount: Box<dyn FileSystem + Send + Sync> = if root_layers.is_empty() {
            Box::new(ArcFileSystem::new(base_root))
        } else {
            Box::new(OverlayFileSystem::new(
                ArcFileSystem::new(base_root),
                root_layers,
            ))
        };
        mount_fs.mount(Path::new("/"), root_mount)?;

        return Ok(WasiFsRoot::from_mount_fs(mount_fs));
    };

    if let Some(container_root) = &container.root_layer {
        root_layers.push(container_root.clone());
    }

    let root_mount: Box<dyn FileSystem + Send + Sync> = if root_layers.is_empty() {
        Box::new(ArcFileSystem::new(base_root))
    } else {
        Box::new(OverlayFileSystem::new(
            ArcFileSystem::new(base_root),
            root_layers,
        ))
    };

    mount_fs.mount(Path::new("/"), root_mount)?;
    let import_mode = match conflict_behavior {
        ExistingMountConflictBehavior::Fail => ExactMountConflictMode::Fail,
        ExistingMountConflictBehavior::Override => ExactMountConflictMode::KeepExisting,
    };
    let mut skipped_subtree: Option<PathBuf> = None;
    for mount in &container.mounts {
        if skipped_subtree
            .as_ref()
            .is_some_and(|prefix| mount.guest_path.starts_with(prefix))
        {
            continue;
        }

        match import_mode {
            ExactMountConflictMode::Fail => {
                mount_fs
                    .mount(&mount.guest_path, Box::new(mount.fs.clone()))
                    .with_context(|| {
                        format!(
                            "Unable to merge container mount \"{}\" into the prepared filesystem",
                            mount.guest_path.display()
                        )
                    })?;
            }
            ExactMountConflictMode::KeepExisting => {
                if mount_fs.filesystem_at(&mount.guest_path).is_some() {
                    skipped_subtree = Some(mount.guest_path.clone());
                    continue;
                }

                mount_fs
                    .mount(&mount.guest_path, Box::new(mount.fs.clone()))
                    .with_context(|| {
                        format!(
                            "Unable to merge container mount \"{}\" into the prepared filesystem",
                            mount.guest_path.display()
                        )
                    })?;
            }
            ExactMountConflictMode::ReplaceExisting => unreachable!("not used here"),
        }
    }

    Ok(WasiFsRoot::from_mount_fs(mount_fs))
}

/// HACK: We need this so users can mount host directories at relative paths.
/// This assumes that the current directory when a runner starts will be "/", so
/// instead of mounting to a relative path, we just mount to "/$path".
///
/// This isn't really a long-term solution because there is no guarantee what
/// the current directory will be. The WASI spec also doesn't require the
/// current directory to be part of the "main" filesystem at all, we really
/// *should* be mounting to a relative directory but that isn't supported by our
/// virtual fs layer.
///
/// See <https://github.com/wasmerio/wasmer/issues/3794> for more.
fn apply_relative_path_mounting_hack(original: &Path) -> PathBuf {
    debug_assert!(original.is_relative());

    let root = Path::new("/");
    let mapped_path = if original == Path::new(".") {
        root.to_path_buf()
    } else {
        root.join(original)
    };

    tracing::debug!(
        original_path=%original.display(),
        remapped_path=%mapped_path.display(),
        "Remapping a relative path"
    );

    mapped_path
}

#[derive(Debug, Clone)]
pub struct MountedDirectory {
    pub guest: String,
    pub fs: Arc<dyn FileSystem + Send + Sync>,
}

/// A directory that should be mapped from the host filesystem into a WASI
/// instance (the "guest").
///
/// # Panics
///
/// Converting a [`MappedDirectory`] to a [`MountedDirectory`] requires enabling
/// the `host-fs` feature flag. Using the [`From`] implementation without
/// enabling this feature will result in a runtime panic.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MappedDirectory {
    /// The absolute path for a directory on the host filesystem.
    pub host: std::path::PathBuf,
    /// The absolute path specifying where the host directory should be mounted
    /// inside the guest.
    pub guest: String,
}

impl From<MappedDirectory> for MountedDirectory {
    fn from(value: MappedDirectory) -> Self {
        cfg_if::cfg_if! {
            if #[cfg(feature = "host-fs")] {
                let MappedDirectory { host, guest } = value;
                let fs: Arc<dyn FileSystem + Send + Sync> =
                    Arc::new(virtual_fs::host_fs::FileSystem::new(Handle::current(), host).unwrap());

                MountedDirectory { guest, fs }
            } else {
                unreachable!("The `host-fs` feature needs to be enabled to map {value:?}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use tempfile::TempDir;
    use virtual_fs::TmpFileSystem;
    use virtual_fs::{DirEntry, FileType, Metadata};

    use super::*;

    fn base_root(root_fs: &MountFileSystem) -> Arc<dyn FileSystem + Send + Sync> {
        root_fs.filesystem_at(Path::new("/")).unwrap()
    }

    fn package_mounts(fs: MountFileSystem) -> BinaryPackageMounts {
        BinaryPackageMounts::from_mount_fs(fs)
    }

    const PYTHON: &[u8] =
        include_bytes!("../../../../wasmer-test-files/examples/python-0.1.0.wasmer");

    /// Fixes <https://github.com/wasmerio/wasmer/issues/3789>
    #[tokio::test]
    async fn mix_args_from_the_webc_and_user() {
        let args = CommonWasiOptions {
            args: vec!["extra".to_string(), "args".to_string()],
            ..Default::default()
        };
        let mut builder = WasiEnvBuilder::new("program-name");
        let mut annotations = WasiAnnotation::new("some-atom");
        annotations.main_args = Some(vec![
            "hard".to_string(),
            "coded".to_string(),
            "args".to_string(),
        ]);

        args.prepare_webc_env(&mut builder, None, &annotations, None)
            .unwrap();

        assert_eq!(
            builder.get_args(),
            [
                // the program name from
                "program-name",
                // from the WEBC's annotations
                "hard",
                "coded",
                "args",
                // from the user
                "extra",
                "args",
            ]
        );
    }

    #[tokio::test]
    async fn mix_env_vars_from_the_webc_and_user() {
        let args = CommonWasiOptions {
            env: vec![("EXTRA".to_string(), "envs".to_string())]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let mut builder = WasiEnvBuilder::new("python");
        let mut annotations = WasiAnnotation::new("python");
        annotations.env = Some(vec!["HARD_CODED=env-vars".to_string()]);

        args.prepare_webc_env(&mut builder, None, &annotations, None)
            .unwrap();

        assert_eq!(
            builder.get_env(),
            [
                ("HARD_CODED".to_string(), b"env-vars".to_vec()),
                ("EXTRA".to_string(), b"envs".to_vec()),
            ]
        );
    }

    fn unix_timestamp_nanos(instant: SystemTime) -> Option<u64> {
        let duration = instant.duration_since(SystemTime::UNIX_EPOCH).ok()?;
        Some(duration.as_nanos() as u64)
    }

    #[tokio::test]
    #[cfg_attr(not(feature = "host-fs"), ignore)]
    async fn python_use_case() {
        let temp = TempDir::new().unwrap();
        let sub_dir = temp.path().join("path").join("to");
        std::fs::create_dir_all(&sub_dir).unwrap();
        std::fs::write(sub_dir.join("file.txt"), b"Hello, World!").unwrap();
        let mapping = [MountedDirectory::from(MappedDirectory {
            guest: "/home".to_string(),
            host: sub_dir,
        })];
        let container = wasmer_package::utils::from_bytes(PYTHON).unwrap();
        let webc_fs = virtual_fs::WebcVolumeFileSystem::mount_all(&container);
        let mount_fs = MountFileSystem::new();
        mount_fs
            .mount(Path::new("/"), Box::new(webc_fs))
            .unwrap();

        let root_fs = RootFileSystemBuilder::default().build();
        let fs = prepare_filesystem(
            base_root(&root_fs),
            &mapping,
            Some(&package_mounts(mount_fs)),
            ExistingMountConflictBehavior::Override,
        )
        .unwrap();

        use virtual_fs::FileSystem;
        assert!(fs.metadata("/home/file.txt".as_ref()).unwrap().is_file());
        assert!(fs.metadata("lib".as_ref()).unwrap().is_dir());
        assert!(
            fs.metadata("lib/python3.6/collections/__init__.py".as_ref())
                .unwrap()
                .is_file()
        );
        assert!(
            fs.metadata("lib/python3.6/encodings/__init__.py".as_ref())
                .unwrap()
                .is_file()
        );
    }

    #[tokio::test]
    async fn package_mount_paths_remain_writable() {
        use virtual_fs::FileSystem;

        let container = wasmer_package::utils::from_bytes(PYTHON).unwrap();
        let lower = virtual_fs::WebcVolumeFileSystem::mount_all(&container);
        let pkg_mount = virtual_fs::OverlayFileSystem::new(TmpFileSystem::new(), [lower]);

        let mount_fs = MountFileSystem::new();
        mount_fs
            .mount(Path::new("/python"), Box::new(pkg_mount))
            .unwrap();

        let root_fs = RootFileSystemBuilder::default().build();
        let fs = prepare_filesystem(
            base_root(&root_fs),
            &[],
            Some(&package_mounts(mount_fs)),
            ExistingMountConflictBehavior::Override,
        )
        .unwrap();

        fs.create_dir(Path::new("/python/custom")).unwrap();
        fs.new_open_options()
            .create(true)
            .write(true)
            .open(Path::new("/python/custom/sitecustomize.py"))
            .unwrap();

        assert!(
            fs.metadata(Path::new("/python/custom/sitecustomize.py"))
                .unwrap()
                .is_file()
        );
        assert!(
            fs.metadata(Path::new("/python/lib/python3.6/collections/__init__.py"))
                .unwrap()
                .is_file()
        );
    }

    #[tokio::test]
    async fn package_mount_symlinks_remain_writable() {
        use virtual_fs::FileSystem;

        let container = wasmer_package::utils::from_bytes(PYTHON).unwrap();
        let lower = virtual_fs::WebcVolumeFileSystem::mount_all(&container);
        let pkg_mount = virtual_fs::OverlayFileSystem::new(TmpFileSystem::new(), [lower]);

        let mount_fs = MountFileSystem::new();
        mount_fs
            .mount(Path::new("/python"), Box::new(pkg_mount))
            .unwrap();

        let root_fs = RootFileSystemBuilder::default().build();
        let fs = prepare_filesystem(
            base_root(&root_fs),
            &[],
            Some(&package_mounts(mount_fs)),
            ExistingMountConflictBehavior::Override,
        )
        .unwrap();

        fs.create_symlink(
            Path::new("lib/python3.6/collections"),
            Path::new("/python/collections-link"),
        )
        .unwrap();

        assert_eq!(
            fs.readlink(Path::new("/python/collections-link")).unwrap(),
            Path::new("lib/python3.6/collections")
        );
        assert!(
            fs.symlink_metadata(Path::new("/python/collections-link"))
                .unwrap()
                .ft
                .is_symlink()
        );
    }

    #[tokio::test]
    async fn user_mounts_override_package_mounts_when_configured() {
        use virtual_fs::FileSystem;

        let user_mount = TmpFileSystem::new();
        user_mount
            .new_open_options()
            .create(true)
            .write(true)
            .open(Path::new("/user.txt"))
            .unwrap();

        let package_mount = TmpFileSystem::new();
        package_mount
            .new_open_options()
            .create(true)
            .write(true)
            .open(Path::new("/pkg.txt"))
            .unwrap();

        let mounted_dirs = [MountedDirectory {
            guest: "/python".to_string(),
            fs: Arc::new(user_mount),
        }];

        let container_mounts = MountFileSystem::new();
        container_mounts
            .mount(Path::new("/python"), Box::new(package_mount))
            .unwrap();

        let root_fs = RootFileSystemBuilder::default().build();
        let fs = prepare_filesystem(
            base_root(&root_fs),
            &mounted_dirs,
            Some(&package_mounts(container_mounts)),
            ExistingMountConflictBehavior::Override,
        )
        .unwrap();

        assert!(
            fs.metadata(Path::new("/python/user.txt"))
                .unwrap()
                .is_file()
        );
        assert_eq!(
            fs.metadata(Path::new("/python/pkg.txt")),
            Err(virtual_fs::FsError::EntryNotFound)
        );
    }

    #[tokio::test]
    async fn conflicting_mounts_fail_when_configured() {
        let user_mount = TmpFileSystem::new();
        let package_mount = TmpFileSystem::new();

        let mounted_dirs = [MountedDirectory {
            guest: "/python".to_string(),
            fs: Arc::new(user_mount),
        }];

        let container_mounts = MountFileSystem::new();
        container_mounts
            .mount(Path::new("/python"), Box::new(package_mount))
            .unwrap();

        let root_fs = RootFileSystemBuilder::default().build();
        let error = prepare_filesystem(
            base_root(&root_fs),
            &mounted_dirs,
            Some(&package_mounts(container_mounts)),
            ExistingMountConflictBehavior::Fail,
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Unable to merge container mount \"/python\""),
            "{error:#}"
        );
    }

    #[tokio::test]
    async fn root_mounts_are_composed_even_in_fail_mode() {
        use virtual_fs::FileSystem;

        let root_mount = TmpFileSystem::new();
        root_mount
            .new_open_options()
            .create(true)
            .write(true)
            .open(Path::new("/user.txt"))
            .unwrap();

        let mounted_dirs = [MountedDirectory {
            guest: "/".to_string(),
            fs: Arc::new(root_mount),
        }];

        let container_mounts = MountFileSystem::new();
        let container_root = TmpFileSystem::new();
        container_root
            .new_open_options()
            .create(true)
            .write(true)
            .open(Path::new("/pkg.txt"))
            .unwrap();
        container_mounts
            .mount(Path::new("/"), Box::new(container_root))
            .unwrap();

        let root_fs = RootFileSystemBuilder::default().build();
        let fs = prepare_filesystem(
            base_root(&root_fs),
            &mounted_dirs,
            Some(&package_mounts(container_mounts)),
            ExistingMountConflictBehavior::Fail,
        )
        .unwrap();

        assert!(fs.metadata(Path::new("/user.txt")).unwrap().is_file());
        assert!(fs.metadata(Path::new("/pkg.txt")).unwrap().is_file());
    }

    #[tokio::test]
    async fn multiple_root_mounts_are_composed() {
        use virtual_fs::FileSystem;

        let first_root = TmpFileSystem::new();
        first_root
            .new_open_options()
            .create(true)
            .write(true)
            .open(Path::new("/first.txt"))
            .unwrap();

        let second_root = TmpFileSystem::new();
        second_root
            .new_open_options()
            .create(true)
            .write(true)
            .open(Path::new("/second.txt"))
            .unwrap();

        let mounted_dirs = [
            MountedDirectory {
                guest: "/".to_string(),
                fs: Arc::new(first_root),
            },
            MountedDirectory {
                guest: "/".to_string(),
                fs: Arc::new(second_root),
            },
        ];

        let root_fs = RootFileSystemBuilder::default().build();
        let fs = prepare_filesystem(
            base_root(&root_fs),
            &mounted_dirs,
            None,
            ExistingMountConflictBehavior::Fail,
        )
        .unwrap();

        assert!(fs.metadata(Path::new("/first.txt")).unwrap().is_file());
        assert!(fs.metadata(Path::new("/second.txt")).unwrap().is_file());
    }

    #[test]
    fn invalid_guest_mount_paths_are_rejected() {
        let error = normalized_mount_path("../../python").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("parent traversal escapes the virtual root"),
            "{error:#}"
        );
    }

    #[tokio::test]
    #[cfg_attr(not(feature = "host-fs"), ignore)]
    async fn convert_mapped_directory_to_mounted_directory() {
        let temp = TempDir::new().unwrap();
        let dir = MappedDirectory {
            guest: "/mnt/dir".to_string(),
            host: temp.path().to_path_buf(),
        };
        let contents = "Hello, World!";
        let file_txt = temp.path().join("file.txt");
        std::fs::write(&file_txt, contents).unwrap();
        let metadata = std::fs::metadata(&file_txt).unwrap();

        let got = MountedDirectory::from(dir);

        let directory_contents: Vec<_> = got
            .fs
            .read_dir("/".as_ref())
            .unwrap()
            .map(|entry| entry.unwrap())
            .collect();
        assert_eq!(
            directory_contents,
            vec![DirEntry {
                path: PathBuf::from("/file.txt"),
                metadata: Ok(Metadata {
                    ft: FileType::new_file(),
                    // Note: Some timestamps aren't available on MUSL and will
                    // default to zero.
                    accessed: metadata
                        .accessed()
                        .ok()
                        .and_then(unix_timestamp_nanos)
                        .unwrap_or(0),
                    created: metadata
                        .created()
                        .ok()
                        .and_then(unix_timestamp_nanos)
                        .unwrap_or(0),
                    modified: metadata
                        .modified()
                        .ok()
                        .and_then(unix_timestamp_nanos)
                        .unwrap_or(0),
                    len: contents.len() as u64,
                })
            }]
        );
    }
}
