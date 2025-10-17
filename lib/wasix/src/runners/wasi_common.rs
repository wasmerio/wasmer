use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Error};
use tokio::runtime::Handle;
use virtual_fs::{
    FileSystem, OverlayFileSystem, RootFileSystemBuilder, TmpFileSystem, UnionFileSystem,
};
use webc::metadata::annotations::Wasi as WasiAnnotation;

use crate::{
    WasiEnvBuilder,
    bin_factory::BinaryPackage,
    capabilities::Capabilities,
    fs::{WasiFsRoot, relative_path_hack::RelativeOrAbsolutePathHack},
    journal::{DynJournal, DynReadableJournal, SnapshotTrigger},
};

pub const MAPPED_CURRENT_DIR_DEFAULT_PATH: &str = "/home";

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
}

impl CommonWasiOptions {
    pub(crate) fn prepare_webc_env(
        &self,
        builder: &mut WasiEnvBuilder,
        container_fs: Option<UnionFileSystem>,
        wasi: &WasiAnnotation,
        root_fs: Option<TmpFileSystem>,
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
            RootFileSystemBuilder::default().build_ext(&mapped_dirs)
        });
        let fs = prepare_filesystem(root_fs, &self.mounts, container_fs)?;

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

fn build_directory_mappings(
    root_fs: &mut TmpFileSystem,
    mounted_dirs: &[MountedDirectory],
) -> Result<(), anyhow::Error> {
    for dir in mounted_dirs {
        let MountedDirectory {
            guest: guest_path,
            fs,
        } = dir;
        let mut guest_path = PathBuf::from(guest_path);
        tracing::debug!(
            guest=%guest_path.display(),
            "Mounting",
        );

        if guest_path.is_relative() {
            guest_path = apply_relative_path_mounting_hack(&guest_path);
        }

        let guest_path = root_fs
            .canonicalize_unchecked(&guest_path)
            .with_context(|| {
                format!(
                    "Unable to canonicalize guest path '{}'",
                    guest_path.display()
                )
            })?;

        if guest_path == Path::new("/") {
            root_fs
                .mount_directory_entries(&guest_path, fs, "/".as_ref())
                .context("Unable to mount to root")?;
        } else {
            if let Some(parent) = guest_path.parent() {
                create_dir_all(&*root_fs, parent).with_context(|| {
                    format!("Unable to create the \"{}\" directory", parent.display())
                })?;
            }

            TmpFileSystem::mount(root_fs, guest_path.clone(), fs, "/".into())
                .with_context(|| format!("Unable to mount \"{}\"", guest_path.display()))?;
        }
    }

    Ok(())
}

fn prepare_filesystem(
    mut root_fs: TmpFileSystem,
    mounted_dirs: &[MountedDirectory],
    container_fs: Option<UnionFileSystem>,
) -> Result<WasiFsRoot, Error> {
    if !mounted_dirs.is_empty() {
        build_directory_mappings(&mut root_fs, mounted_dirs)?;
    }

    // HACK(Michael-F-Bryan): The WebcVolumeFileSystem only accepts relative
    // paths, but our Python executable will try to access its standard library
    // with relative paths assuming that it is being run from the root
    // directory (i.e. it does `open("lib/python3.6/io.py")` instead of
    // `open("/lib/python3.6/io.py")`).
    // Until the FileSystem trait figures out whether relative paths should be
    // supported or not, we'll add an adapter that automatically retries
    // operations using an absolute path if it failed using a relative path.

    let fs = if let Some(container) = container_fs {
        let container = RelativeOrAbsolutePathHack(container);
        let fs = OverlayFileSystem::new(root_fs, [container]);
        WasiFsRoot::Overlay(Arc::new(fs))
    } else {
        WasiFsRoot::Sandbox(RelativeOrAbsolutePathHack(root_fs))
    };

    Ok(fs)
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

fn create_dir_all(fs: &dyn FileSystem, path: &Path) -> Result<(), Error> {
    if fs.metadata(path).is_ok() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        create_dir_all(fs, parent)?;
    }

    fs.create_dir(path)?;

    Ok(())
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
    use virtual_fs::{DirEntry, FileType, Metadata};

    use super::*;

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

    /// Tests that RelativeOrAbsolutePathHack works correctly
    ///
    /// This test verifies that the filesystem wrapper correctly handles both:
    /// 1. Absolute paths (like "/home/file.txt")
    /// 2. Relative paths (like "lib/test.txt") by converting them to absolute paths
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

        // Create a simple UnionFileSystem with some test directories
        let union_fs = UnionFileSystem::new();
        let lib_fs = virtual_fs::mem_fs::FileSystem::default();
        lib_fs.create_dir(Path::new("/python3.6")).unwrap();
        lib_fs.create_dir(Path::new("/python3.6/collections")).unwrap();
        lib_fs.create_dir(Path::new("/python3.6/encodings")).unwrap();
        lib_fs.new_open_options()
            .write(true)
            .create(true)
            .open(Path::new("/python3.6/collections/__init__.py"))
            .unwrap();
        lib_fs.new_open_options()
            .write(true)
            .create(true)
            .open(Path::new("/python3.6/encodings/__init__.py"))
            .unwrap();
        union_fs.mount("lib".to_string(), Path::new("/lib"), Box::new(lib_fs)).unwrap();

        let root_fs = RootFileSystemBuilder::default().build();
        let fs = prepare_filesystem(root_fs, &mapping, Some(union_fs)).unwrap();

        // Verify the filesystem was created correctly
        assert!(matches!(fs, WasiFsRoot::Overlay(_)));
        if let WasiFsRoot::Overlay(overlay_fs) = &fs {
            // Check that host-mounted file is accessible via absolute path
            assert!(
                overlay_fs
                    .metadata("/home/file.txt".as_ref())
                    .unwrap()
                    .is_file()
            );

            // Check that relative paths work (this is what RelativeOrAbsolutePathHack enables)
            // These should be converted to absolute paths like "/lib" by the hack
            assert!(overlay_fs.metadata("lib".as_ref()).unwrap().is_dir());
            assert!(
                overlay_fs
                    .metadata("lib/python3.6/collections/__init__.py".as_ref())
                    .unwrap()
                    .is_file()
            );
            assert!(
                overlay_fs
                    .metadata("lib/python3.6/encodings/__init__.py".as_ref())
                    .unwrap()
                    .is_file()
            );
        }
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
