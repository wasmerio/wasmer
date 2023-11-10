use std::path::PathBuf;
use std::{collections::HashMap, path::Path, sync::Arc};

use anyhow::Error;
use futures::future::BoxFuture;
use virtual_fs::{FileSystem, FsError, OverlayFileSystem, RootFileSystemBuilder, TmpFileSystem};
use webc::metadata::annotations::Wasi as WasiAnnotation;

use crate::{
    bin_factory::BinaryPackage, capabilities::Capabilities, runners::MappedDirectory,
    WasiEnvBuilder,
};

#[derive(Debug, Clone)]
pub struct MappedCommand {
    /// The new alias.
    pub alias: String,
    /// The original command.
    pub target: String,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct CommonWasiOptions {
    pub(crate) args: Vec<String>,
    pub(crate) env: HashMap<String, String>,
    pub(crate) forward_host_env: bool,
    pub(crate) mapped_dirs: Vec<MappedDirectory>,
    pub(crate) mapped_host_commands: Vec<MappedCommand>,
    pub(crate) injected_packages: Vec<BinaryPackage>,
    pub(crate) capabilities: Capabilities,
    pub(crate) fs: Option<TmpFileSystem>,
    pub(crate) current_dir: Option<PathBuf>,
}

impl CommonWasiOptions {
    pub(crate) fn prepare_webc_env(
        &self,
        builder: &mut WasiEnvBuilder,
        wasi: &WasiAnnotation,
    ) -> Result<(), anyhow::Error> {
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

    pub(crate) fn set_filesystem(
        &self,
        builder: &mut WasiEnvBuilder,
        root_fs: TmpFileSystem,
        container_fs: Option<Arc<dyn FileSystem + Send + Sync>>,
    ) -> Result<(), Error> {
        let root_fs = RootFileSystemBuilder::default().build_into(root_fs);
        if let Some(container_fs) = container_fs {
            let fs = Box::new(prepare_filesystem(root_fs, container_fs.clone())?);
            builder.set_fs(fs); // sandbox_fs / set_fs
        } else {
            let fs = Box::new(virtual_fs::TraceFileSystem(root_fs));
            builder.set_fs(fs);
        };
        builder.add_preopen_dir("/")?;
        Ok(())
    }
}

type ContainerFs = virtual_fs::TraceFileSystem<
    OverlayFileSystem<
        TmpFileSystem,
        [RelativeOrAbsolutePathHack<Arc<dyn FileSystem + Send + Sync>>; 1],
    >,
>;

fn prepare_filesystem(
    root_fs: TmpFileSystem,
    container_fs: Arc<dyn FileSystem + Send + Sync>,
) -> Result<ContainerFs, Error> {
    // HACK(Michael-F-Bryan): The WebcVolumeFileSystem only accepts relative
    // paths, but our Python executable will try to access its standard library
    // with relative paths assuming that it is being run from the root
    // directory (i.e. it does `open("lib/python3.6/io.py")` instead of
    // `open("/lib/python3.6/io.py")`).
    // Until the FileSystem trait figures out whether relative paths should be
    // supported or not, we'll add an adapter that automatically retries
    // operations using an absolute path if it failed using a relative path.
    let container_fs = RelativeOrAbsolutePathHack(container_fs);
    let fs = virtual_fs::TraceFileSystem::new(OverlayFileSystem::new(root_fs, [container_fs]));

    Ok(fs)
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

    fn rename<'a>(&'a self, from: &Path, to: &Path) -> BoxFuture<'a, virtual_fs::Result<()>> {
        let from = from.to_owned();
        let to = to.to_owned();
        Box::pin(async move { self.0.rename(&from, &to).await })
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
    #[tokio::test]
    async fn mix_args_from_the_webc_and_user() {
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

    #[tokio::test]
    async fn mix_env_vars_from_the_webc_and_user() {
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

    #[tokio::test]
    async fn python_use_case() {
        let temp = TempDir::new().unwrap();
        let sub_dir = temp.path().join("path").join("to");
        std::fs::create_dir_all(&sub_dir).unwrap();
        std::fs::write(sub_dir.join("file.txt"), b"Hello, World!").unwrap();
        let container = Container::from_bytes(PYTHON).unwrap();
        let webc_fs = WebcVolumeFileSystem::mount_all(&container);
        let mut builder = WasiEnvBuilder::new("");

        let mut root_fs = RootFileSystemBuilder::default().build();
        let home = virtual_fs::fs::native::FileSystem(sub_dir);
        root_fs.mount(PathBuf::from("/home"), home, PathBuf::new());
        let fs = prepare_filesystem(root_fs, Arc::new(webc_fs)).unwrap();

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
