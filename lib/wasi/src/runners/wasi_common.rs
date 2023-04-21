use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Error};
use virtual_fs::{FileSystem, FsError, OverlayFileSystem, RootFileSystemBuilder};
use webc::metadata::annotations::Wasi as WasiAnnotation;

use crate::{runners::MappedDirectory, WasiEnvBuilder};

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct CommonWasiOptions {
    pub(crate) args: Vec<String>,
    pub(crate) env: HashMap<String, String>,
    pub(crate) forward_host_env: bool,
    pub(crate) mapped_dirs: Vec<MappedDirectory>,
}

impl CommonWasiOptions {
    pub(crate) fn prepare_webc_env(
        &self,
        builder: &mut WasiEnvBuilder,
        container_fs: Arc<dyn FileSystem>,
        wasi: &WasiAnnotation,
    ) -> Result<(), anyhow::Error> {
        let fs = prepare_filesystem(&self.mapped_dirs, container_fs, |path| {
            builder.add_preopen_dir(path).map_err(Error::from)
        })?;

        builder.add_preopen_dir("/")?;
        if fs.read_dir(".".as_ref()).is_ok() {
            // Sometimes "." won't be mounted so preopening will fail.
            builder.add_preopen_dir(".")?;
        }

        builder.set_fs(fs);

        self.populate_env(wasi, builder);
        self.populate_args(wasi, builder);

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

fn prepare_filesystem(
    mapped_dirs: &[MappedDirectory],
    container_fs: Arc<dyn FileSystem>,
    mut preopen: impl FnMut(&Path) -> Result<(), Error>,
) -> Result<Box<dyn FileSystem + Send + Sync>, Error> {
    let root_fs = RootFileSystemBuilder::default().build();

    if !mapped_dirs.is_empty() {
        let host_fs: Arc<dyn FileSystem + Send + Sync> = Arc::new(crate::default_fs_backing());

        for dir in mapped_dirs {
            let MappedDirectory { host, guest } = dir;
            let guest = PathBuf::from(guest);
            tracing::debug!(
                guest=%guest.display(),
                host=%host.display(),
                "Mounting host folder",
            );

            if let Some(parent) = guest.parent() {
                create_dir_all(&root_fs, parent).with_context(|| {
                    format!("Unable to create the \"{}\" directory", parent.display())
                })?;
            }

            root_fs
                .mount(guest.clone(), &host_fs, host.clone())
                .with_context(|| {
                    format!(
                        "Unable to mount \"{}\" to \"{}\"",
                        host.display(),
                        guest.display()
                    )
                })?;

            preopen(&guest)
                .with_context(|| format!("Unable to preopen \"{}\"", guest.display()))?;
        }
    }

    // HACK(Michael-F-Bryan): The WebcVolumeFileSystem only accepts relative
    // paths, but our Python executable will try to access its standard library
    // with relative paths assuming that it is being run from the root
    // directory (i.e. it does `open("lib/python3.6/io.py")` instead of
    // `open("/lib/python3.6/io.py")`).
    // Until the FileSystem trait figures out whether relative paths should be
    // supported or not, we'll add an adapter that automatically retries
    // operations using an absolute path if it failed using a relative path.
    let container_fs = RelativeOrAbsolutePathHack(container_fs);

    Ok(Box::new(OverlayFileSystem::new(root_fs, [container_fs])))
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

#[derive(Debug)]
struct RelativeOrAbsolutePathHack<F>(F);

impl<F: FileSystem> RelativeOrAbsolutePathHack<F> {
    fn execute<Func, Ret>(&self, path: &Path, operation: Func) -> Result<Ret, FsError>
    where
        Func: Fn(&F, &Path) -> Result<Ret, FsError>,
    {
        // First, try it with the path we were given
        let result = operation(&self.0, path);

        if result.is_err() && !path.is_absolute() {
            // we were given a relative path, but maybe the operation will work
            // using absolute paths instead.
            let path = Path::new("/").join(path);
            operation(&self.0, &path)
        } else {
            result
        }
    }
}

impl<F: FileSystem> virtual_fs::FileSystem for RelativeOrAbsolutePathHack<F> {
    fn read_dir(&self, path: &Path) -> virtual_fs::Result<virtual_fs::ReadDir> {
        self.execute(path, |fs, p| fs.read_dir(p))
    }

    fn create_dir(&self, path: &Path) -> virtual_fs::Result<()> {
        self.execute(path, |fs, p| fs.create_dir(p))
    }

    fn remove_dir(&self, path: &Path) -> virtual_fs::Result<()> {
        self.execute(path, |fs, p| fs.remove_dir(p))
    }

    fn rename(&self, from: &Path, to: &Path) -> virtual_fs::Result<()> {
        self.execute(from, |fs, p| fs.rename(p, to))
    }

    fn metadata(&self, path: &Path) -> virtual_fs::Result<virtual_fs::Metadata> {
        self.execute(path, |fs, p| fs.metadata(p))
    }

    fn remove_file(&self, path: &Path) -> virtual_fs::Result<()> {
        self.execute(path, |fs, p| fs.remove_file(p))
    }

    fn new_open_options(&self) -> virtual_fs::OpenOptions {
        virtual_fs::OpenOptions::new(self)
    }
}

impl<F: FileSystem> virtual_fs::FileOpener for RelativeOrAbsolutePathHack<F> {
    fn open(
        &self,
        path: &Path,
        conf: &virtual_fs::OpenOptionsConfig,
    ) -> virtual_fs::Result<Box<dyn virtual_fs::VirtualFile + Send + Sync + 'static>> {
        self.execute(path, |fs, p| {
            fs.new_open_options().options(conf.clone()).open(p)
        })
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use virtual_fs::WebcVolumeFileSystem;
    use webc::Container;

    use super::*;

    const PYTHON: &[u8] = include_bytes!("../../../c-api/examples/assets/python-0.1.0.wasmer");

    /// Fixes <https://github.com/wasmerio/wasmer/issues/3789>
    #[test]
    fn mix_args_from_the_webc_and_user() {
        let args = CommonWasiOptions {
            args: vec!["extra".to_string(), "args".to_string()],
            ..Default::default()
        };
        let mut builder = WasiEnvBuilder::new("program-name");
        let fs = Arc::new(virtual_fs::EmptyFileSystem::default());
        let mut annotations = WasiAnnotation::new("some-atom");
        annotations.main_args = Some(vec![
            "hard".to_string(),
            "coded".to_string(),
            "args".to_string(),
        ]);

        args.prepare_webc_env(&mut builder, fs, &annotations)
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

    #[test]
    fn mix_env_vars_from_the_webc_and_user() {
        let args = CommonWasiOptions {
            env: vec![("EXTRA".to_string(), "envs".to_string())]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let mut builder = WasiEnvBuilder::new("python");
        let fs = Arc::new(virtual_fs::EmptyFileSystem::default());
        let mut annotations = WasiAnnotation::new("python");
        annotations.env = Some(vec!["HARD_CODED=env-vars".to_string()]);

        args.prepare_webc_env(&mut builder, fs, &annotations)
            .unwrap();

        assert_eq!(
            builder.get_env(),
            [
                ("HARD_CODED".to_string(), b"env-vars".to_vec()),
                ("EXTRA".to_string(), b"envs".to_vec()),
            ]
        );
    }

    #[test]
    fn python_use_case() {
        let temp = TempDir::new().unwrap();
        let sub_dir = temp.path().join("path").join("to");
        std::fs::create_dir_all(&sub_dir).unwrap();
        std::fs::write(sub_dir.join("file.txt"), b"Hello, World!").unwrap();
        let mapping = [MappedDirectory {
            guest: "/home".to_string(),
            host: sub_dir,
        }];
        let container = Container::from_bytes(PYTHON).unwrap();
        let webc_fs = WebcVolumeFileSystem::mount_all(&container);

        let fs = prepare_filesystem(&mapping, Arc::new(webc_fs), |_| Ok(())).unwrap();

        assert!(fs.metadata("/home/file.txt".as_ref()).unwrap().is_file());
        assert!(fs.metadata("lib".as_ref()).unwrap().is_dir());
        assert!(fs
            .metadata("lib/python3.6/collections/__init__.py".as_ref())
            .unwrap()
            .is_file());
        assert!(fs
            .metadata("lib/python3.6/encodings/__init__.py".as_ref())
            .unwrap()
            .is_file());
    }
}
